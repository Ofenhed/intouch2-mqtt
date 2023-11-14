use std::{
  collections::HashMap,
  net::{Ipv4Addr, Ipv6Addr, SocketAddr},
  sync::Arc,
  time::Duration,
};
use tokio::{net::UdpSocket, task::JoinSet, time::timeout};

#[derive(thiserror::Error, Debug)]
pub enum PortForwardError {
  #[error("IO Error: {0}")]
  Io(#[from] std::io::Error),
  #[error("No DNS match: {0}")]
  NoDnsMatch(String),
  #[error("Tokio join error: {0}")]
  TokioJoin(#[from] tokio::task::JoinError),
}

fn source_for_taget(addr: impl ToOwned<Owned = SocketAddr>) -> SocketAddr {
  let mut target_ip = addr.to_owned();
  target_ip.set_port(0);
  match &mut target_ip {
    SocketAddr::V4(addr) => addr.set_ip(Ipv4Addr::UNSPECIFIED),
    SocketAddr::V6(addr) => addr.set_ip(Ipv6Addr::UNSPECIFIED),
  };
  target_ip
}

pub async fn start_port_forward(
  source_addr: SocketAddr,
  target_addr: SocketAddr,
  handshake_timeout: Duration,
  udp_timeout: Duration,
) -> Result<(), PortForwardError> {
  let target_bind_addr = source_for_taget(target_addr);
  let sock_forward = Arc::new(UdpSocket::bind(source_addr).await?);
  enum SocketData {
    Incoming(SocketAddr, Vec<u8>),
    Timeout(SocketAddr),
    SendCompleted,
  }
  let mut workers = JoinSet::<Result<SocketData, PortForwardError>>::new();
  let mut forwards: HashMap<SocketAddr, Arc<UdpSocket>> = HashMap::new();
  loop {
    let listener = sock_forward.clone();
    workers.spawn(async move {
      let mut buffer = Vec::from([0; 4096]);
      let (len, source) = listener.recv_from(buffer.as_mut()).await?;
      buffer.truncate(len);
      return Ok(SocketData::Incoming(source, buffer));
    });
    while let Some(job) = workers.join_next().await {
      use std::collections::hash_map::Entry;
      match job?? {
        SocketData::Incoming(source_addr, data) => {
          match forwards.entry(source_addr) {
            Entry::Occupied(entry) => {
              let socket = entry.get().clone();
              workers.spawn(async move {
                socket.send(&data).await?;
                Ok(SocketData::SendCompleted)
              });
            }
            Entry::Vacant(entry) => {
              let new_socket = Arc::new(UdpSocket::bind(target_bind_addr).await?);
              entry.insert(new_socket.clone());
              new_socket.connect(target_addr).await?;
              eprintln!("New client: {}", source_addr);
              let sock_reply = sock_forward.clone();
              workers.spawn(async move {
                new_socket.send(&data).await?;
                drop(data);
                let mut buf = Box::new([0; 4096]);
                let mut max_wait = handshake_timeout;
                loop {
                  let len = match timeout(max_wait, new_socket.recv(buf.as_mut())).await {
                    Ok(len) => len?,
                    Err(_timeout) => return Ok(SocketData::Timeout(source_addr)),
                  };
                  max_wait = udp_timeout;
                  sock_reply.send_to(&buf[0..len], source_addr).await?;
                }
              });
            }
          }
          break;
        }
        SocketData::SendCompleted => (),
        SocketData::Timeout(socket_addr) => {
          eprintln!("Timeout for {}", socket_addr);
          forwards.remove(&socket_addr);
        }
      }
    }
  }
}
