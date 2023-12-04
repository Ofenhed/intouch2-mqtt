use mqttrs::*;
use std::{
    net::SocketAddr,
    ops::{Add, Deref},
    pin::Pin,
    sync::Arc,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::{broadcast, mpsc},
    time,
};

const CLIENT_ID: &str = "spa_client";

pub enum MqttAuth<'a> {
    Simple {
        username: &'a str,
        password: &'a str,
    },
    None,
}

pub struct SessionBuilder<'a> {
    pub discovery_topic: Arc<str>,
    pub availability_topic: Option<Arc<str>>,
    pub target: SocketAddr,
    pub auth: MqttAuth<'a>,
    pub keep_alive: u16,
}

#[derive(Debug)]
pub struct MqttPacket {
    pub buf: Pin<Box<[u8]>>,
    pub packet: Packet<'static>,
}

impl Deref for MqttPacket {
    type Target = Packet<'static>;

    fn deref(&self) -> &Self::Target {
        &self.packet
    }
}

unsafe fn transmute_lifetime<'a, 'b, T: ?Sized>(from: &'a T) -> &'b T {
    std::mem::transmute(from)
}

impl TryFrom<&'_ [u8]> for MqttPacket {
    type Error = MqttError;

    fn try_from(value: &'_ [u8]) -> Result<Self, Self::Error> {
        let data: Pin<Box<[u8]>> = Box::into_pin(Box::from(value));
        let box_ref: &'static [u8] = unsafe { transmute_lifetime(&data.as_ref()) };
        let packet = decode_slice(box_ref)?;
        let Some(packet) = packet else {
            return Err(MqttError::NotEnoughData(value.into()))?;
        };
        Ok(MqttPacket { buf: data, packet })
    }
}

pub struct Session {
    stream: TcpStream,
    buffer: Box<[u8; 4096]>,
    discovery_topic: Arc<str>,
    availability_topic: Option<Arc<str>>,
    pid: Pid,
    send_queue: mpsc::Receiver<Box<[u8]>>,
    send_queue_sender: mpsc::Sender<Box<[u8]>>,
    subscribers: broadcast::Sender<Arc<MqttPacket>>,
    ping_interval: time::Interval,
}

#[derive(thiserror::Error, Debug)]
pub enum MqttError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Encoding(#[from] mqttrs::Error),
    #[error("Not enough data received: {}", String::from_utf8_lossy(.0))]
    NotEnoughData(Box<[u8]>),
    #[error("Unexpected packet type: {0:?}")]
    UnexpectedPacketType(PacketType),
    #[error("Authentication failed: {0:?}")]
    AuthenticationFailed(ConnectReturnCode),
    #[error("JSON error: {0}")]
    JSON(#[from] serde_json::Error),
    #[error("MQTT Subscribe failed: {0:?}")]
    MqttSubscribeFailed(Box<[SubscribeTopic]>),
    #[error("MQTT Send pipe failed: {0}")]
    PipeSend(#[from] mpsc::error::SendError<Box<[u8]>>),
}

#[derive(strum::IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum Topic {
    State,
    Status,
    Switch,
    Config,
    Set,
    None,
}

#[derive(Clone)]
pub struct PacketSender {
    sender: mpsc::Sender<Box<[u8]>>,
    buffer: Box<[u8; 4096]>,
}

impl PacketSender {
    pub async fn send(&mut self, packet: &Packet<'_>) -> Result<(), MqttError> {
        let len = encode_slice(&packet, self.buffer.as_mut())?;
        self.sender.send(self.buffer[..len].into()).await?;
        Ok(())
    }
}

pub struct TopicGenerator {
    discovery_topic: Arc<str>,
}
impl TopicGenerator {
    #[inline(always)]
    pub fn topic(&self, r#type: &str, name: &str, topic: Topic) -> String {
        if matches!(topic, Topic::None) {
            format!(
                "{discovery}/{type}/{name}",
                discovery = self.discovery_topic
            )
        } else {
            let topic: &'static str = topic.into();
            format!(
                "{discovery}/{type}/{name}/{topic}",
                discovery = self.discovery_topic
            )
        }
    }
}

impl Session {
    pub fn topic_generator(&self) -> TopicGenerator {
        TopicGenerator {
            discovery_topic: self.discovery_topic.clone(),
        }
    }
    #[inline(always)]
    pub fn topic(&self, r#type: &str, name: &str, topic: Topic) -> String {
        self.topic_generator().topic(r#type, name, topic)
    }
    fn next_pid(&mut self) -> Pid {
        let mut new_pid = self.pid.add(1);
        std::mem::swap(&mut self.pid, &mut new_pid);
        new_pid
    }

    pub fn subscribe(&mut self) -> broadcast::Receiver<Arc<MqttPacket>> {
        self.subscribers.subscribe()
    }

    pub fn sender(&self) -> PacketSender {
        PacketSender {
            sender: self.send_queue_sender.clone(),
            buffer: Box::new([0; 4096]),
        }
    }

    pub async fn mqtt_subscribe(&mut self, topics: Vec<SubscribeTopic>) -> Result<(), MqttError> {
        let subscribe_pid = self.next_pid();
        let packet = Packet::Subscribe(Subscribe {
            pid: subscribe_pid,
            topics: topics.clone(),
        });
        let encoded_len = encode_slice(&packet, self.buffer.as_mut())?;
        self.stream.write(&self.buffer[..encoded_len]).await?;
        loop {
            match &self.tick().await?.packet {
                Packet::Suback(Suback { pid, return_codes }) if pid == &subscribe_pid => {
                    let failed: Box<_> = topics
                        .into_iter()
                        .zip(return_codes.into_iter())
                        .filter_map(|(topic, return_code)| {
                            if !matches!(return_code, SubscribeReturnCodes::Success(_)) {
                                Some(topic)
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !failed.is_empty() {
                        return Err(MqttError::MqttSubscribeFailed(failed))?;
                    } else {
                        return Ok(());
                    }
                }
                _ => (),
            }
        }
    }

    pub async fn tick(&mut self) -> Result<Arc<MqttPacket>, MqttError> {
        loop {
            select! {
                read = self.stream.read(self.buffer.as_mut()) => {
                    let response_len = read?;
                    let package = MqttPacket::try_from(&self.buffer[..response_len])?;
                    match package.packet {
                        Packet::Pingreq => {
                            let response = Packet::Pingresp;
                            let len = encode_slice(&response, self.buffer.as_mut())?;
                            eprintln!("Ping received");
                            self.stream.write(&self.buffer[..len]).await?;
                            continue;
                        },
                        Packet::Pingresp => continue,
                        _ => (),
                    }
                    let package = Arc::new(package);
                    _ = self.subscribers.send(package.clone());
                    return Ok(package)
                },
                _ = self.ping_interval.tick() => {
                    let response = Packet::Pingreq;
                    let len = encode_slice(&response, self.buffer.as_mut())?;
                    self.stream.write(&self.buffer[..len]).await?;
                },
                to_send = self.send_queue.recv() => {
                    if let Some(send) = to_send {
                        self.stream.write(send.as_ref()).await?;
                    }
                },
            }
        }
    }

    pub async fn notify_online(&self) -> Result<(), MqttError> {
        if let Some(availability_topic) = self.availability_topic.as_deref() {
            let packet = Packet::Publish(Publish {
                dup: false,
                qospid: QosPid::AtMostOnce,
                retain: false,
                topic_name: availability_topic,
                payload: b"online",
            });
            self.sender().send(&packet).await?;
        }
        Ok(())
    }

    pub async fn send(&mut self, packet: Packet<'_>) -> Result<(), MqttError> {
        let encoded_len = encode_slice(&packet, self.buffer.as_mut())?;
        self.stream.write(&self.buffer[..encoded_len]).await?;
        Ok(())
    }
}

impl SessionBuilder<'_> {
    pub async fn connect(self) -> Result<Session, MqttError> {
        let last_will = if let Some(topic) = self.availability_topic.as_deref() {
            Some(LastWill {
                topic,
                message: b"offline",
                qos: QoS::AtMostOnce,
                retain: false,
            })
        } else {
            None
        };
        let mut connect = Connect {
            protocol: Protocol::MQTT311,
            keep_alive: self.keep_alive,
            client_id: CLIENT_ID.into(),
            clean_session: true,
            last_will,
            username: None,
            password: None,
        };
        if let MqttAuth::Simple { username, password } = self.auth {
            connect.username = Some(username);
            connect.password = Some(password.as_bytes());
        }
        let mut buffer = Box::new([0; 4096]);
        let packet = Packet::Connect(connect);
        let packet_len = encode_slice(&packet, buffer.as_mut())?;
        let connection = match self.target {
            SocketAddr::V4(_) => TcpSocket::new_v4()?,
            SocketAddr::V6(_) => TcpSocket::new_v6()?,
        };
        let mut stream = connection.connect(self.target).await?;
        stream.write(&buffer[..packet_len]).await?;
        let bytes_read = stream.read(buffer.as_mut()).await?;
        let Some(response) = decode_slice(&buffer[..bytes_read])? else {
            return Err(MqttError::NotEnoughData(buffer[..bytes_read].into()))?;
        };
        if let Packet::Connack(ack) = response {
            match ack.code {
                ConnectReturnCode::Accepted => {
                    let (send_queue_sender, send_queue) = mpsc::channel(10);
                    let ping_interval = time::interval_at(
                        time::Instant::now(),
                        time::Duration::from_secs((self.keep_alive >> 1).into()),
                    );
                    Ok(Session {
                        stream,
                        buffer,
                        availability_topic: self.availability_topic,
                        discovery_topic: self.discovery_topic,
                        pid: Pid::new(),
                        subscribers: tokio::sync::broadcast::Sender::new(10),
                        send_queue,
                        send_queue_sender,
                        ping_interval,
                    })
                }
                failed => Err(MqttError::AuthenticationFailed(failed)),
            }
        } else {
            Err(MqttError::UnexpectedPacketType(response.get_type()))
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // todo!("Disconnect from server")
    }
}
