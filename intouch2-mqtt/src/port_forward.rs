use intouch2::{
    composer::compose_network_data,
    object::{NetworkPackage, NetworkPackageData},
    parser::parse_network_data,
    ToStatic,
};
use std::{
    borrow::Cow,
    cmp::min,
    mem::{take, MaybeUninit},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::{broadcast, mpsc, Mutex, RwLock},
    task::JoinSet,
    time::{self, timeout_at, Instant},
};

use crate::{
    port_forward_mapping::{ForwardAddr, ForwardMapping},
    unspecified_source_for_taget, Buffers, NoClone, StaticBox,
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
    #[error("Pipe send error: {0}")]
    PipeSendFailed(#[from] broadcast::error::SendError<NetworkPackage<'static>>),
    #[error("Invalid spa name: {}", String::from_utf8_lossy(.0))]
    InvalidSpaName(Box<[u8]>),
    #[error("Data dump failed: {0}")]
    DumpFailed(#[from] broadcast::error::SendError<DataDumpType>),
}

const NET_BUFFER_SIZE: usize = 4096;

#[derive(Debug)]
pub struct PackagePipe {
    pub rx: mpsc::Receiver<NetworkPackage<'static>>,
    pub tx: Arc<broadcast::Sender<NetworkPackage<'static>>>,
}

pub struct SpaPipe {
    broadcast_sender: Arc<broadcast::Sender<NetworkPackage<'static>>>,
    pub tx: mpsc::Sender<NetworkPackage<'static>>,
}

impl SpaPipe {
    pub fn subscribe(&self) -> broadcast::Receiver<NetworkPackage<'static>> {
        self.broadcast_sender.subscribe()
    }
}

pub struct FullPackagePipe {
    pub forwarder: PackagePipe,
    pub spa: SpaPipe,
}

impl FullPackagePipe {
    pub fn new() -> Self {
        let broadcast_sender = Arc::new(broadcast::Sender::new(30));
        let (mtx, mrx) = mpsc::channel(30);
        FullPackagePipe {
            spa: SpaPipe {
                broadcast_sender: broadcast_sender.clone(),
                tx: mtx,
            },
            forwarder: PackagePipe {
                tx: broadcast_sender,
                rx: mrx,
            },
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum Player {
    Local,
    #[serde(untagged)]
    Client(SocketAddr),
}

#[derive(Debug, Clone, serde::Serialize)]
pub enum DataSource {
    To(Player),
    From(Player),
}

pub type DataDumpType = (DataSource, NetworkPackageData<'static>);

#[derive(Debug)]
pub struct PortForward {
    send_clients: Option<Arc<Mutex<NoClone<UdpSocket>>>>,
    recv_clients: Option<NoClone<UdpSocket>>,
    send_pipe: Option<Arc<broadcast::Sender<NetworkPackage<'static>>>>,
    recv_pipe: Option<mpsc::Receiver<NetworkPackage<'static>>>,
    send_spa: Arc<Mutex<NoClone<UdpSocket>>>,
    recv_spa: NoClone<UdpSocket>,
    spa_hello: Vec<u8>,
    handshake_timeout: Duration,
    udp_timeout: Duration,
    forwards: ForwardMapping<()>,
    package_dump_pipe: Option<Arc<broadcast::Sender<DataDumpType>>>,
    verbose: bool,
    dump_traffic: bool,
}

pub struct PortForwardBuilder {
    pub listen_addr: Option<SocketAddr>,
    pub target_addr: SocketAddr,
    pub handshake_timeout: Duration,
    pub udp_timeout: Duration,
    pub local_connection: Option<PackagePipe>,
    pub package_dump_pipe: Option<broadcast::Sender<DataDumpType>>,
    pub verbose: bool,
    pub dump_traffic: bool,
}

fn transmute_uninit<T>(arr: &mut [MaybeUninit<T>]) -> &mut [T] {
    unsafe { std::mem::transmute(arr) }
}

struct SpaHello<'a> {
    id: &'a [u8],
    #[allow(dead_code)]
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

impl PortForwardBuilder {
    pub fn dump_packages(&mut self) -> broadcast::Receiver<DataDumpType> {
        self.package_dump_pipe
            .get_or_insert_with(|| broadcast::Sender::new(10))
            .subscribe()
    }

    pub async fn build(self) -> Result<PortForward, PortForwardError> {
        let PortForwardBuilder {
            listen_addr,
            target_addr,
            handshake_timeout,
            udp_timeout,
            local_connection,
            package_dump_pipe: package_dump,
            verbose,
            dump_traffic,
        } = self;

        let target_bind_addr = unspecified_source_for_taget(target_addr);
        let (send_clients, recv_clients) = if let Some(listen_addr) = listen_addr {
            if self.verbose {
                eprintln!("Listening on {listen_addr}");
            }
            let sock_clients = StaticBox::new(UdpSocket::bind(listen_addr).await?);
            let send_clients = Arc::new(Mutex::new(sock_clients.to_no_clone()));
            let recv_clients = sock_clients.to_no_clone();
            (Some(send_clients), Some(recv_clients))
        } else {
            (None, None)
        };
        let (send_pipe, recv_pipe) = if let Some(pipes) = local_connection {
            (Some(pipes.tx), Some(pipes.rx))
        } else {
            (None, None)
        };
        let sock_spa = UdpSocket::bind(target_bind_addr).await?;
        sock_spa.connect(self.target_addr).await?;

        let mut spa_hello = {
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
        let sock_spa = StaticBox::new(sock_spa);
        let send_spa = Arc::new(Mutex::new(sock_spa.to_no_clone()));
        let recv_spa = sock_spa.to_no_clone();
        drop(sock_spa);

        Ok(PortForward {
            forwards: Default::default(),
            spa_hello,
            send_clients,
            recv_clients,
            send_pipe,
            recv_pipe,
            send_spa,
            recv_spa,
            handshake_timeout,
            udp_timeout,
            package_dump_pipe: package_dump.map(Into::into),
            verbose,
            dump_traffic,
        })
    }
}

impl PortForward {
    pub async fn run(mut self) -> Result<(), PortForwardError> {
        let mut spa_hello = SpaHello::new(&self.spa_hello)?;
        let mut hello_response = Arc::new(RwLock::new(compose_network_data(
            &NetworkPackage::Hello(Cow::Borrowed(&spa_hello.id)),
        )));

        #[derive(Debug)]
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
            FromPipe {
                data: NetworkPackage<'static>,
                recv_pipe: Option<mpsc::Receiver<NetworkPackage<'static>>>,
            },
            PipeDied,
            Timeout,
            SpawnSpaListener {
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            SpawnClientListener {
                recv_sock: Option<NoClone<UdpSocket>>,
            },
            SpawnPipeListener {
                recv_pipe: Option<mpsc::Receiver<NetworkPackage<'static>>>,
            },
            SendCompleted {
                buf: Option<Vec<u8>>,
            },
        }
        let mut workers = JoinSet::<Result<SocketData, PortForwardError>>::new();
        workers.spawn(async { Ok(SocketData::Timeout) });
        workers.spawn(async {
            Ok(SocketData::SpawnSpaListener {
                recv_sock: Some(self.recv_spa),
            })
        });
        if let Some(recv_clients) = self.recv_clients {
            workers.spawn(async {
                Ok(SocketData::SpawnClientListener {
                    recv_sock: Some(recv_clients),
                })
            });
        }
        if let Some(recv_pipe) = self.recv_pipe {
            workers.spawn(async {
                Ok(SocketData::SpawnPipeListener {
                    recv_pipe: Some(recv_pipe),
                })
            });
        }
        let mut buffers: Buffers<20, Vec<u8>> = Buffers::new();

        loop {
            while let Some(job) = workers.join_next().await {
                let mut job_result = job??;
                match &mut job_result {
                    SocketData::SendCompleted { buf } => {
                        if let Some(buf) = take(buf) {
                            buffers.release(buf);
                        }
                        continue;
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
                    SocketData::FromPipe { recv_pipe, .. }
                    | SocketData::SpawnPipeListener { recv_pipe } => {
                        let Some(mut recv_pipe) = std::mem::take(recv_pipe) else {
                            unreachable!(
                                "recv_pipe will always be set when FromPipe or SpawnPipeListener is returned"
                                )
                        };
                        workers.spawn(async {
                            if let Some(data) = recv_pipe.recv().await {
                                Ok(SocketData::FromPipe {
                                    recv_pipe: Some(recv_pipe),
                                    data,
                                })
                            } else {
                                Ok(SocketData::PipeDied)
                            }
                        });
                    }
                    SocketData::Timeout => {
                        let (timeouts, next_timeout) = self
                            .forwards
                            .clear_timeouts(self.handshake_timeout, self.udp_timeout);
                        if self.verbose {
                            for client in timeouts.iter() {
                                eprintln!("Client {client:?} timed out")
                            }
                        }
                        workers.spawn(async move {
                            if let Some(next_timeout) = next_timeout {
                                time::sleep_until(next_timeout).await;
                            } else {
                                time::sleep(min(self.handshake_timeout, self.udp_timeout)).await;
                            }
                            Ok(SocketData::Timeout)
                        });
                        continue;
                    }
                    _ => (),
                }
                match job_result {
                    SocketData::FromPipe { data, .. } => match data {
                        NetworkPackage::Addressed {
                            src: Some(_),
                            data: ref package,
                            ..
                        } => {
                            if self.dump_traffic
                                && !matches!(
                                    package,
                                    NetworkPackageData::Ping | NetworkPackageData::Pong
                                )
                            {
                                eprintln!("Self -> {}", package.display());
                            }
                            if let Some(dump_pipe) = &mut self.package_dump_pipe {
                                dump_pipe
                                    .send((DataSource::From(Player::Local), package.to_static()))?;
                            }
                            let send_spa = self.send_spa.clone();
                            workers.spawn(async move {
                                send_spa
                                    .lock()
                                    .await
                                    .send(&compose_network_data(&data))
                                    .await?;
                                Ok(SocketData::SendCompleted { buf: None })
                            });
                        }
                        NetworkPackage::Hello(id) => {
                            self.forwards.insert(ForwardAddr::Pipe, id, ());
                            let Some(send_pipe) = &self.send_pipe else {
                                unreachable!("Pipe must be set to end up here")
                            };
                            send_pipe.send(NetworkPackage::Hello(self.spa_hello.clone().into()))?;
                        }
                        invalid_package => {
                            eprintln!("Invalid package from pipe: {invalid_package}")
                        }
                    },
                    SocketData::FromClient {
                        source_addr, data, ..
                    } => match parse_network_data(&data) {
                        Ok(
                            ref package @ NetworkPackage::Addressed {
                                src: Some(ref src),
                                dst: Some(ref dst),
                                data: ref content,
                                ..
                            },
                        ) if dst[..] == spa_hello.id[..] => {
                            if self.dump_traffic
                                && !matches!(
                                    content,
                                    NetworkPackageData::Ping | NetworkPackageData::Pong
                                )
                            {
                                eprintln!("{source_addr} -> {}", content.display());
                            }
                            if let Some(dump_pipe) = &mut self.package_dump_pipe {
                                dump_pipe.send((
                                    DataSource::From(Player::Client(source_addr)),
                                    content.to_static(),
                                ))?;
                            }
                            let count_before = self.forwards.len();
                            let info =
                                self.forwards
                                    .insert(ForwardAddr::Socket(source_addr), &**src, ());
                            info.did_forward();
                            if self.verbose && count_before != self.forwards.len() {
                                eprintln!(
                                    "New client {} at {}",
                                    String::from_utf8_lossy(&src),
                                    source_addr
                                );
                            }
                            let send_spa = self.send_spa.clone();
                            let send_pipe =
                                if let (Some(pipe), NetworkPackageData::SetStatus { .. }) =
                                    (&self.send_pipe, content)
                                {
                                    Some((pipe.clone(), package.to_static()))
                                } else {
                                    None
                                };
                            workers.spawn(async move {
                                send_spa.lock().await.send(&data).await?;
                                if let Some((send_pipe, content)) = send_pipe {
                                    eprintln!("Forwarding set command");
                                    send_pipe.send(content)?;
                                }
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
                                eprintln!(
                                    "Invalid package received from {source_addr}: {package_error}"
                                )
                            }
                        }
                        Ok(NetworkPackage::Hello(_)) => {
                            let Some(send_clients) = &self.send_clients else {
                                unreachable!("How can you get messages from clients if you don't have any clients?")
                            };
                            if self.verbose {
                                if self
                                    .forwards
                                    .get_addr(&ForwardAddr::Socket(source_addr))
                                    .is_none()
                                {
                                    eprintln!("New hello received from {source_addr}")
                                }
                            }
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
                    },
                    SocketData::FromSpa { data, .. } => match parse_network_data(&data) {
                        Ok(
                            ref package @ NetworkPackage::Addressed {
                                dst: Some(ref dst),
                                data: ref content,
                                ..
                            },
                        ) => {
                            if let Some(ref mut forward_info) = self.forwards.get_id_mut(&dst) {
                                forward_info.got_reply();
                                match *forward_info.addr() {
                                    ForwardAddr::Pipe => {
                                        let Some(pipe) = &self.send_pipe else {
                                            unreachable!()
                                        };
                                        let sender = pipe.clone();
                                        if self.dump_traffic
                                            && !matches!(
                                                content,
                                                NetworkPackageData::Ping | NetworkPackageData::Pong
                                            )
                                        {
                                            eprintln!("Self <- {}", content.display());
                                        }
                                        let package = package.to_static();
                                        if let (
                                            Some(dump_pipe),
                                            NetworkPackage::Addressed { data, .. },
                                        ) = (&mut self.package_dump_pipe, &package)
                                        {
                                            dump_pipe.send((
                                                DataSource::To(Player::Local),
                                                data.into(),
                                            ))?;
                                        }
                                        workers.spawn(async move {
                                            sender.send(package)?;
                                            Ok(SocketData::SendCompleted { buf: Some(data) })
                                        });
                                    }
                                    ForwardAddr::Socket(addr) => {
                                        let Some(send_clients) = &self.send_clients else {
                                            unreachable!("How can you send to clients if there are no clients?")
                                        };
                                        if self.dump_traffic
                                            && !matches!(
                                                content,
                                                NetworkPackageData::Ping | NetworkPackageData::Pong
                                            )
                                        {
                                            eprintln!("{addr} <- {}", content.display());
                                        }
                                        if let Some(dump_pipe) = &mut self.package_dump_pipe {
                                            dump_pipe.send((
                                                DataSource::To(Player::Client(addr)),
                                                content.to_static(),
                                            ))?;
                                        }
                                        let send_clients = send_clients.clone();
                                        let sender = if let (
                                            Some(sender),
                                            NetworkPackageData::PushStatus { .. },
                                        ) = (&self.send_pipe, content)
                                        {
                                            Some((sender.clone(), package.to_static()))
                                        } else {
                                            None
                                        };
                                        workers.spawn(async move {
                                            send_clients
                                                .lock()
                                                .await
                                                .send_to(data.as_ref(), addr)
                                                .await?;
                                            if let Some((sender, package)) = sender {
                                                sender.send(package)?;
                                            }
                                            Ok(SocketData::SendCompleted { buf: Some(data) })
                                        });
                                    }
                                }
                            }
                        }
                        Err(package_error) => {
                            if self.verbose {
                                eprintln!("Invalid package received from spa: {package_error}")
                            }
                        }
                        Ok(NetworkPackage::Hello(id)) => {
                            if id[..] != self.spa_hello[..] {
                                if self.verbose {
                                    eprintln!(
                                        "Spa changed name to {}",
                                        String::from_utf8_lossy(&id)
                                    );
                                }
                                self.spa_hello = id.into();
                                spa_hello = SpaHello::new(&self.spa_hello)?;
                                *hello_response.write().await = compose_network_data(
                                    &NetworkPackage::Hello(Cow::Borrowed(&self.spa_hello)),
                                );
                            }
                        }
                        Ok(NetworkPackage::Addressed { dst: None, .. }) => {
                            if self.verbose {
                                eprintln!("Got package without destination from Spa");
                            }
                        }
                    },
                    SocketData::SpawnClientListener { .. }
                    | SocketData::SpawnSpaListener { .. }
                    | SocketData::SpawnPipeListener { .. } => (),
                    SocketData::PipeDied => {
                        if self.verbose {
                            eprintln!("Internal Spa pipe disconnected")
                        }
                    }
                    filtered @ SocketData::SendCompleted { .. }
                    | filtered @ SocketData::Timeout => {
                        unreachable!("{filtered:?} is filtered out above")
                    }
                }
            }
        }
    }
}
