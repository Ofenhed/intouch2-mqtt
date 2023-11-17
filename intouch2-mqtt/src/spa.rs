use std::{
    borrow::Cow,
    net::SocketAddr,
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use intouch2::{
    composer::compose_network_data,
    generate_uuid,
    object::{package_data, NetworkPackage, NetworkPackageData},
    parser::{parse_network_data, ParseError},
};
use tokio::{net::UdpSocket, select, sync::Mutex, time};

use crate::{unspecified_source_for_taget, WithBuffer};

pub struct SpaConnection {
    socket: UdpSocket,
    src: Arc<[u8]>,
    dst: Arc<[u8]>,
    interval: Mutex<time::Interval>,
    pings_since_last_pong: AtomicU8,
}

#[derive(thiserror::Error, Debug)]
pub enum SpaError {
    #[error("Unexpected answer")]
    UnexpectedAnswer(NetworkPackage<'static>),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Lost connection to Spa")]
    SpaConnectionLost,
}

impl WithBuffer for SpaConnection {
    type Buffer = [u8; 4096];

    fn make_buffer() -> [u8; 4096] {
        [0; 4096]
    }
}

impl SpaConnection {
    pub async fn new(target: &SocketAddr) -> Result<Self, SpaError> {
        let mut buffer = Box::new([0; 4096]);
        let socket = UdpSocket::bind(unspecified_source_for_taget(*target)).await?;
        socket.connect(target).await?;
        socket
            .send(compose_network_data(&NetworkPackage::Hello(Cow::Borrowed(b"1"))).as_ref())
            .await?;

        let (len, remote) = socket.recv_from(buffer.as_mut()).await?;
        socket.set_broadcast(false)?;
        socket.connect(remote).await?;

        let receiver = match parse_network_data(&buffer[0..len])? {
            NetworkPackage::Hello(msg) => Ok(msg),
            msg => Err(SpaError::UnexpectedAnswer(msg.to_static())),
        }?;
        let (dst, name): (Arc<[u8]>, Box<[u8]>) = {
            let pos = receiver
                .iter()
                .position(|x| *x == '|' as u8)
                .unwrap_or(receiver.len());
            (receiver[0..pos].into(), receiver[pos + 1..].into())
        };
        let src: Arc<[u8]> = generate_uuid().into();
        socket
            .send(&compose_network_data(&NetworkPackage::Hello(
                Cow::Borrowed(&src),
            )))
            .await?;
        socket
            .send(&compose_network_data(&NetworkPackage::Addressed {
                src: Some(Cow::Borrowed(&src)),
                dst: Some(Cow::Borrowed(&dst)),
                data: package_data::GetVersion.into(),
            }))
            .await?;
        let len = socket.recv(buffer.as_mut()).await?;
        let spa_object = match parse_network_data(&buffer[0..len])?.to_static() {
            NetworkPackage::Addressed {
                src: _,
                dst: _,
                data: NetworkPackageData::Version(x),
            } => {
                println!(
                    "Connected to {}, got version {:?}",
                    String::from_utf8_lossy(&name),
                    x
                );
                Ok(Self {
                    socket,
                    src,
                    dst,
                    interval: time::interval_at(time::Instant::now(), Duration::from_secs(3))
                        .into(),
                    pings_since_last_pong: 0.into(),
                })
            }
            msg => Err(SpaError::UnexpectedAnswer(msg.into())),
        }?;
        Ok(spa_object)
    }

    fn make_package<'a>(&'a self, data: NetworkPackageData<'a>) -> NetworkPackage<'a> {
        NetworkPackage::Addressed {
            src: Some(Cow::Borrowed(&self.src)),
            dst: Some(Cow::Borrowed(&self.dst)),
            data,
        }
    }

    async fn tick(&self) {
        let mut interval = self.interval.lock().await;
        interval.tick().await;
    }

    pub async fn recv<'a>(
        &self,
        buffer: &'a mut <Self as WithBuffer>::Buffer,
    ) -> Result<Option<NetworkPackage<'a>>, SpaError> {
        select! {
            _ = self.tick() => {
                self.socket.send(&compose_network_data(&self.make_package(package_data::Ping.into()))).await?;
                let pings_sent = self.pings_since_last_pong.fetch_add(1, Ordering::Relaxed);
                if pings_sent > 5 {
                    Err(SpaError::SpaConnectionLost)
                } else {
                    Ok(None)
                }
            },
            len = self.socket.recv(buffer.as_mut()) => {
                let packet = parse_network_data(&buffer[0..len?])?;
                match packet {
                    NetworkPackage::Addressed { data: NetworkPackageData::Pong, .. } => self.pings_since_last_pong.store(0, Ordering::Relaxed),
                    _ => (),
                }
                return Ok(Some(packet));
            },
        }
    }
}
