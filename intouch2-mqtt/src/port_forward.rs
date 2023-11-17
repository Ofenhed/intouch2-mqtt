use intouch2::{
    composer::compose_network_data, object::NetworkPackage, parser::parse_network_data,
};
use std::{
    borrow::Cow,
    mem::{take, MaybeUninit},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{Mutex, RwLock},
    task::JoinSet,
    time::{timeout, timeout_at, Instant},
};

use crate::{
    port_forward_mapping::ForwardMapping, unspecified_source_for_taget, Buffers, NoClone, StaticBox,
};

#[derive(thiserror::Error, Debug)]
pub enum PortForwardError {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No DNS match: {0}")]
    NoDnsMatch(String),
    #[error("Tokio join error: {0}")]
    TokioJoin(#[from] tokio::task::JoinError),
    #[error("Channel error: {0}")]
    AddressedChannel(#[from] tokio::sync::mpsc::error::SendError<(SocketAddr, Box<[u8]>)>),
    #[error("Channel error: {0}")]
    Channel(#[from] tokio::sync::mpsc::error::SendError<Box<[u8]>>),
    #[error("Spa Hello Timeout")]
    SpaTimeout,
    #[error("Invalid spa name: {}", String::from_utf8_lossy(.0))]
    InvalidSpaName(Box<[u8]>),
}

const NET_BUFFER_SIZE: usize = 4096;

pub struct Pipes<R, W> {
    pub rx: tokio::sync::mpsc::Receiver<R>,
    pub tx: tokio::sync::mpsc::Sender<W>,
}

#[derive(Clone, Debug)]
pub struct PortForward {
    pub source_addr: SocketAddr,
    pub target_addr: SocketAddr,
    pub handshake_timeout: Duration,
    pub udp_timeout: Duration,
    pub verbose: bool,
    pub dump_traffic: bool,
}

fn transmute_uninit<T>(arr: &mut [MaybeUninit<T>]) -> &mut [T] {
    unsafe { std::mem::transmute(arr) }
}

struct SpaHello<'a> {
    id: &'a [u8],
    name: &'a [u8],
}

impl<'a> TryFrom<&'a [u8]> for SpaHello<'a> {
    type Error = PortForwardError;

    fn try_from(buf: &'a [u8]) -> Result<Self, Self::Error> {
        for i in 0..buf.len() {
            if buf[i] == b'|' {
                let (before, rest) = buf.split_at(i);
                let after = &rest[1..];
                return Ok(SpaHello {
                    id: before,
                    name: after,
                });
            }
        }
        Err(PortForwardError::InvalidSpaName(buf.into()))
    }
}

impl<'a> SpaHello<'a> {
    fn new(buf: &'a [u8]) -> Result<Self, PortForwardError> {
        buf.try_into()
    }
}

impl PortForward {
    pub async fn run(self) -> Result<(), PortForwardError> {
        let target_bind_addr = unspecified_source_for_taget(self.target_addr);
        let sock_clients = StaticBox::new(UdpSocket::bind(self.source_addr).await?);
        let send_clients = Arc::new(Mutex::new(sock_clients.to_no_clone()));
        let recv_clients = sock_clients.to_no_clone();
        drop(sock_clients);
        let sock_spa = UdpSocket::bind(target_bind_addr).await?;
        sock_spa.connect(self.target_addr).await?;

        let mut spa_id = {
            let mut tries: u8 = 5;
            let mut buf = Box::new([0; 512]);
            'retry: loop {
                tries -= 1;
                sock_spa
                    .send(&compose_network_data(&NetworkPackage::Hello(
                        Cow::Borrowed(b"1"),
                    )))
                    .await?;
                let timeout = Instant::now() + Duration::from_secs(1);

                'ignore_package: loop {
                    match timeout_at(timeout, sock_spa.recv(buf.as_mut())).await {
                        Err(_) => break 'ignore_package,
                        Ok(len) => match parse_network_data(&buf[0..len?]) {
                            Err(_) | Ok(NetworkPackage::Addressed { .. }) => {
                                continue 'ignore_package
                            }
                            Ok(NetworkPackage::Hello(spa_name)) => {
                                break 'retry Ok(spa_name.into_owned())
                            }
                        },
                    }
                }
                if tries == 0 {
                    break Err(PortForwardError::SpaTimeout);
                }
            }
        }?;
        let mut spa_hello = SpaHello::new(&spa_id)?;
        let mut hello_response = Arc::new(RwLock::new(compose_network_data(
            &NetworkPackage::Hello(Cow::Borrowed(&spa_hello.id)),
        )));

        let sock_spa = StaticBox::new(sock_spa);
        let send_spa = Arc::new(Mutex::new(sock_spa.to_no_clone()));
        let recv_spa = sock_spa.to_no_clone();
        drop(sock_spa);
        enum SocketData {
            FromClient {
                source_addr: SocketAddr,
                data: Vec<u8>,
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            FromSpa {
                data: Vec<u8>,
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            Timeout {
                client_addr: SocketAddr,
            },
            SpawnSpaListener {
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            SpawnClientListener {
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            SendCompleted {
                buf: Option<Vec<u8>>,
            },
        }
        let mut workers = JoinSet::<Result<SocketData, PortForwardError>>::new();
        workers.spawn(async {
            Ok(SocketData::SpawnSpaListener {
                recv_sock: Some(recv_spa),
            })
        });
        workers.spawn(async {
            Ok(SocketData::SpawnClientListener {
                recv_sock: Some(recv_clients),
            })
        });
        let mut forwards: ForwardMapping = Default::default();
        let mut buffers: Buffers<20, Vec<u8>> = Buffers::new();

        loop {
            while let Some(job) = workers.join_next().await {
                let mut job_result = job??;
                match &mut job_result {
                    SocketData::SendCompleted { buf } => {
                        if let Some(buf) = take(buf) {
                            buffers.release(buf);
                            continue;
                        }
                    }
                    SocketData::FromClient { recv_sock, .. }
                    | SocketData::SpawnClientListener { recv_sock } => {
                        let mut buf = buffers.take_or(|| Vec::with_capacity(NET_BUFFER_SIZE));
                        let Some(recv_sock) = std::mem::take(recv_sock) else {
                            unreachable!(
                "recv_sock will always be set when FromClient or SpawnClientListener is returned"
              )
                        };
                        workers.spawn(async {
                            buf.clear();
                            let (len, source_addr) = recv_sock
                                .recv_from(transmute_uninit(buf.spare_capacity_mut()))
                                .await?;
                            unsafe { buf.set_len(len) };
                            Ok(SocketData::FromClient {
                                recv_sock: Some(recv_sock),
                                source_addr,
                                data: buf,
                            })
                        });
                    }
                    SocketData::FromSpa { recv_sock, .. }
                    | SocketData::SpawnSpaListener { recv_sock } => {
                        let mut buf = buffers.take_or(|| Vec::with_capacity(NET_BUFFER_SIZE));
                        let Some(recv_sock) = std::mem::take(recv_sock) else {
                            unreachable!(
                "recv_sock will always be set when FromSpa or SpawnSpaListener is returned"
              )
                        };
                        workers.spawn(async {
                            buf.clear();
                            let len = recv_sock
                                .recv(transmute_uninit(buf.spare_capacity_mut()))
                                .await?;
                            unsafe { buf.set_len(len) };
                            Ok(SocketData::FromSpa {
                                recv_sock: Some(recv_sock),
                                data: buf,
                            })
                        });
                    }
                    _ => (),
                }
                match job_result {
                    SocketData::FromClient {
                        source_addr, data, ..
                    } => {
                        match parse_network_data(&data) {
                            Ok(NetworkPackage::Addressed {
                                src: Some(src),
                                dst: Some(dst),
                                data: content,
                                ..
                            }) if dst[..] == spa_hello.id[..] => {
                                if self.dump_traffic {
                                    eprintln!("{source_addr} -> {}", content.display());
                                }
                                let count_before = forwards.len();
                                forwards.insert(source_addr, &*src);
                                if self.verbose && count_before != forwards.len() {
                                    eprintln!(
                                        "New client {} at {}",
                                        String::from_utf8_lossy(&src),
                                        source_addr
                                    );
                                }
                                let send_spa = send_spa.clone();
                                workers.spawn(async move {
                                    send_spa.lock().await.send(&data).await?;
                                    Ok(SocketData::SendCompleted { buf: Some(data) })
                                });
                            }
                            Ok(NetworkPackage::Addressed { dst: Some(dst), .. }) => {
                                if self.verbose {
                                    eprintln!(
                                        "Received package addressed for unknown id {}",
                                        String::from_utf8_lossy(&dst)
                                    )
                                }
                            }
                            Ok(NetworkPackage::Addressed { dst: None, .. }) => {
                                if self.verbose {
                                    eprintln!("Received unaddressed packet from {source_addr}");
                                }
                            }
                            Err(package_error) => {
                                if self.verbose {
                                    eprintln!("Invalid package received from {source_addr}: {package_error}")
                                }
                            }
                            Ok(NetworkPackage::Hello(_)) => {
                                let send_clients = send_clients.clone();
                                let hello_response = hello_response.clone();
                                workers.spawn(async move {
                                    send_clients
                                        .lock()
                                        .await
                                        .send_to(&hello_response.read().await, source_addr)
                                        .await?;
                                    Ok(SocketData::SendCompleted { buf: Some(data) })
                                });
                            }
                        }
                    }
                    SocketData::FromSpa { data, .. } => match parse_network_data(&data) {
                        Ok(NetworkPackage::Addressed {
                            dst: Some(dst),
                            data: content,
                            ..
                        }) => {
                            if let Some(forward_info) = forwards.get_id(&dst) {
                                let addr = *forward_info.addr();
                                if self.dump_traffic {
                                    eprintln!("{addr} <- {}", content.display());
                                }
                                let send_clients = send_clients.clone();
                                workers.spawn(async move {
                                    send_clients
                                        .lock()
                                        .await
                                        .send_to(data.as_ref(), addr)
                                        .await?;
                                    Ok(SocketData::SendCompleted { buf: Some(data) })
                                });
                            }
                        }
                        Err(package_error) => {
                            if self.verbose {
                                eprintln!("Invalid package received from spa: {package_error}")
                            }
                        }
                        Ok(NetworkPackage::Hello(id)) => {
                            if id[..] != spa_id[..] {
                                if self.verbose {
                                    eprintln!(
                                        "Spa changed name to {}",
                                        String::from_utf8_lossy(&id)
                                    );
                                }
                                spa_id = id.into();
                                spa_hello = SpaHello::new(&spa_id)?;
                                *hello_response.write().await = compose_network_data(
                                    &NetworkPackage::Hello(Cow::Borrowed(&spa_id)),
                                );
                            }
                        }
                        Ok(NetworkPackage::Addressed { dst: None, .. }) => {
                            if self.verbose {
                                eprintln!("Got package without destination from Spa");
                            }
                        }
                    },
                    SocketData::Timeout { client_addr } => {
                        eprintln!("Timeout for {}", client_addr);
                        forwards.remove_addr(&client_addr);
                    }
                    SocketData::SpawnClientListener { .. }
                    | SocketData::SpawnSpaListener { .. } => (),
                    SocketData::SendCompleted { .. } => {
                        unreachable!("SendCompleted is filtered out above")
                    }
                }
            }
        }
        // let spa_pipe = {
        //  let sock = sock_spa.clone();
        //  let (pub_tx, mut rx) = tokio::sync::mpsc::channel::<Box<[u8]>>(100);
        //  workers.spawn(async move {
        //    while let Some(msg) = rx.recv().await {
        //      sock.send(&msg).await?;
        //    }
        //    Ok(())
        //  });
        //  let sock = sock_spa.clone();
        //  let (tx, mut pub_rx) = tokio::sync::mpsc::channel::<Box<[u8]>>(100);
        //  workers.spawn(async move {
        //    let mut buf = Box::new([0; NET_BUFFER_SIZE]);
        //    loop {
        //      let len = sock.recv(buf.as_mut()).await?;
        //      tx.send(buf[0..len].into()).await?;
        //    }
        //    Ok(())
        //  });
        //  Pipes { tx: pub_tx, rx: pub_rx }
        //};
        // let forwarder_pipe = {
        //  let sock = sock_forward.clone();
        //  let (pub_tx, mut rx) = tokio::sync::mpsc::channel::<(SocketAddr, Vec<u8>)>(100);
        //  workers.spawn(async move {
        //    while let Some((addr, msg)) = rx.recv().await {
        //      sock.send_to(&msg, addr).await?;
        //    }
        //    Ok(())
        //  });
        //  let sock = sock_forward.clone();
        //  let (tx, mut pub_rx) = tokio::sync::mpsc::channel::<(SocketAddr, Box<[u8]>)>(100);
        //  workers.spawn(async move {
        //    let mut buf = Box::new([0; NET_BUFFER_SIZE]);
        //    loop {
        //      let (len, addr) = sock.recv_from(buf.as_mut()).await?;
        //      tx.send((addr, buf[0..len].into())).await?;
        //    }
        //    Ok(())
        //  });
        //  Pipes { tx: pub_tx, rx: pub_rx }
        //};

        // enum Data {
        //  HandleSpa(Pipes<Box<[u8]>, Box<[u8]>>),
        //  HandleForward(Pipes<(SocketAddr, Box<[u8]>), (SocketAddr, Box<[u8]>)>),
        //  FromSpa(Box<[u8]>),
        //  FromForward(SocketAddr, Box<[u8]>),
        //}
        // workers.spawn(async move {
        //  let mut new_data = JoinSet::new();
        //  new_data.spawn(async move { Data::HandleSpa });
        //  new_data.spawn(async move { Data::HandleForward });
        //  while let Some(job) = new_data.join_next().await {
        //    let job = job?;
        //    match &job {
        //      Data::HandleSpa | Data::FromSpa(_) => {
        //        new_data.spawn(async move {

        //        });
        //      },
        //      Data::HandleForward | Data::FromForward(_, _) => {} // SPawn forward worker
        //    }
        //    match job {
        //      Data::HandleSpa | Data::HandleForward => continue,
        //      Data::FromSpa(spa_data) => (),
        //      Data::FromForward(addr, spa_data) => (),
        //    }
        //  }
        //  Ok(())
        //});
        // while let Some(job) = workers.join_next().await {
        //  job??;
        //}
        // Ok(())
    }
}
