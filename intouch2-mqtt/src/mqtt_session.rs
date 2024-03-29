use mqttrs::*;
use std::{
    net::SocketAddr,
    path::Path,
    pin::{pin, Pin},
    sync::{
        atomic::{AtomicU16, Ordering},
        Arc,
    },
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpSocket, TcpStream},
    select,
    sync::{self, broadcast, mpsc},
    task::JoinSet,
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
    pub base_topic: Arc<str>,
    pub target: SocketAddr,
    pub auth: MqttAuth<'a>,
    pub keep_alive: u16,
    pub publish_retries: u8,
    pub publish_timeout: time::Duration,
}

#[derive(Debug)]
pub struct MqttPacket {
    _buf: Pin<Box<[u8]>>,
    packet: Packet<'static>,
}

impl MqttPacket {
    pub fn packet(&self) -> &Packet {
        unsafe { transmute_lifetime(&self.packet) }
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
        Ok(MqttPacket { _buf: data, packet })
    }
}

#[cfg(test)]
mod test {
    impl Drop for super::MqttPacket {
        fn drop(&mut self) {
            eprintln!("Dropping packet {self:?}");
        }
    }

    #[test]
    fn self_referential() -> anyhow::Result<()> {
        let mut buffer1 = [0; 512];
        let mut buffer2 = [0; 512];
        let packet1_original = mqttrs::Packet::Puback(mqttrs::Pid::try_from(10).unwrap());
        let packet2_original = mqttrs::Packet::Connack(mqttrs::Connack {
            session_present: true,
            code: mqttrs::ConnectReturnCode::Accepted,
        });
        let data1_len = mqttrs::encode_slice(&packet1_original, &mut buffer1)?;
        let data2_len = mqttrs::encode_slice(&packet2_original, &mut buffer2)?;
        let data1 = &buffer1[..data1_len];
        let data2 = &buffer2[..data2_len];
        let mut packet1 = super::MqttPacket::try_from(data1)?;
        let mut packet2 = super::MqttPacket::try_from(data2)?;
        assert_eq!(packet1.packet(), &packet1_original);
        assert_eq!(packet2.packet(), &packet2_original);
        std::mem::swap(&mut packet1, &mut packet2);
        assert_eq!(packet1.packet(), &packet2_original);
        assert_eq!(packet2.packet(), &packet1_original);
        Ok(())
    }
}

pub struct AtomicPid {
    pid: AtomicU16,
}

impl AtomicPid {
    pub fn next_pid(&self) -> Pid {
        loop {
            if let Ok(pid) = self.pid.fetch_add(1, Ordering::Relaxed).try_into() {
                return pid;
            }
        }
    }
}

impl Default for AtomicPid {
    fn default() -> Self {
        Self { pid: 1.into() }
    }
}

#[derive(Debug)]
pub struct PublishQueueEntry {
    topic: Arc<Path>,
    payload: Arc<[u8]>,
    qospid: QosPid,
    response: sync::oneshot::Sender<Result<(), MqttError>>,
}

pub struct Session {
    stream: TcpStream,
    jobs: JoinSet<Result<(), MqttError>>,
    buffer: Box<[u8; 4096]>,
    discovery_topic: Arc<Path>,
    availability_topic: Option<Arc<str>>,
    base_topic: Arc<Path>,
    pid: Arc<AtomicPid>,
    send_queue: mpsc::Receiver<Box<[u8]>>,
    send_queue_sender: mpsc::Sender<Box<[u8]>>,
    publish_queue: mpsc::Receiver<PublishQueueEntry>,
    publish_queue_sender: mpsc::Sender<PublishQueueEntry>,
    subscribers: broadcast::Sender<Arc<MqttPacket>>,
    publish_timeout: time::Duration,
    publish_retries: u8,
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
    #[error("Runtime error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Unexpected packet type: {0:?}")]
    UnexpectedPacketType(PacketType),
    #[error("Authentication failed: {0:?}")]
    AuthenticationFailed(ConnectReturnCode),
    #[error("JSON error: {0}")]
    JSON(#[from] serde_json::Error),
    #[error("MQTT Subscribe timed out: {0:?}")]
    MqttSubscribeTimeout(Box<[SubscribeTopic]>),
    #[error("MQTT Subscribe failed: {0:?}")]
    MqttSubscribeFailed(Box<[SubscribeTopic]>),
    #[error("MQTT Send pipe failed: {0}")]
    PipeSend(#[from] mpsc::error::SendError<Box<[u8]>>),
    #[error("Failed to receive data from MQTT: {0}")]
    MqttRecvError(#[from] broadcast::error::RecvError),
    #[error("MQTT publish send failed: {0}")]
    MqttPublishSend(#[from] mpsc::error::SendError<PublishQueueEntry>),
    #[error("MQTT publish recv failed: {0}")]
    MqttPublishRecv(#[from] sync::oneshot::error::RecvError),
    #[error("MQTT publish reply failed")]
    MqttPublishReply,
    #[error("Publish timeout")]
    PublishTimeout,
    #[error("Forwarding MQTT packages to subscribers failed")]
    ForwardToSubscribers,
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
    pid: Arc<AtomicPid>,
}

impl PacketSender {
    pub async fn send(&mut self, packet: &Packet<'_>) -> Result<(), MqttError> {
        let len = encode_slice(&packet, self.buffer.as_mut())?;
        self.sender.send(self.buffer[..len].into()).await?;
        Ok(())
    }
    pub fn next_pid(&self) -> Pid {
        self.pid.next_pid()
    }
}

#[derive(Clone)]
pub struct PacketPublisher {
    sender: mpsc::Sender<PublishQueueEntry>,
    pid: Arc<AtomicPid>,
}

impl PacketPublisher {
    pub async fn publish(
        &mut self,
        topic: impl Into<Arc<Path>>,
        qos: QosPid,
        payload: impl Into<Arc<[u8]>>,
    ) -> Result<(), MqttError> {
        let (tx, rx) = sync::oneshot::channel();
        let package = PublishQueueEntry {
            topic: topic.into(),
            payload: payload.into(),
            qospid: qos,
            response: tx,
        };
        self.sender.send(package).await?;
        Ok(rx.await??)
    }
    pub fn next_pid(&self) -> Pid {
        self.pid.next_pid()
    }
}

pub struct TopicGenerator {
    discovery_topic: Arc<Path>,
    base_topic: Arc<Path>,
}
impl TopicGenerator {
    #[inline(always)]
    pub fn topic(&self, r#type: &str, name: &str, topic: Topic) -> String {
        match topic {
            Topic::Config => self
                .discovery_topic
                .join(r#type)
                .join(name)
                .join(<&str as From<_>>::from(topic)),
            Topic::None => self.base_topic.join(r#type).join(name),
            topic => self
                .base_topic
                .join(r#type)
                .join(name)
                .join(<&str as From<_>>::from(topic)),
        }
        .to_string_lossy()
        .to_string()
    }
}

impl Session {
    pub fn topic_generator(&self) -> TopicGenerator {
        TopicGenerator {
            discovery_topic: self.discovery_topic.clone(),
            base_topic: self.base_topic.clone(),
        }
    }
    #[inline(always)]
    pub fn topic(&self, r#type: &str, name: &str, topic: Topic) -> String {
        self.topic_generator().topic(r#type, name, topic)
    }
    pub fn next_pid(&self) -> Pid {
        self.pid.next_pid()
    }

    pub fn subscribe(&mut self) -> broadcast::Receiver<Arc<MqttPacket>> {
        self.subscribers.subscribe()
    }

    pub fn sender(&self) -> PacketSender {
        PacketSender {
            sender: self.send_queue_sender.clone(),
            buffer: Box::new([0; 4096]),
            pid: self.pid.clone(),
        }
    }

    pub fn publisher(&self) -> PacketPublisher {
        PacketPublisher {
            sender: self.publish_queue_sender.clone(),
            pid: self.pid.clone(),
        }
    }

    pub async fn mqtt_subscribe(
        &mut self,
        topics: impl AsRef<[SubscribeTopic]>,
    ) -> Result<(), MqttError> {
        let subscribe_pid = self.next_pid();
        let packet = Packet::Subscribe(Subscribe {
            pid: subscribe_pid,
            topics: topics.as_ref().into(),
        });
        let encoded_len = encode_slice(&packet, self.buffer.as_mut())?;
        let sleep_duration = self.publish_timeout / self.publish_retries.into();
        for _ in 0..usize::from(self.publish_retries) {
            self.stream.write_all(&self.buffer[..encoded_len]).await?;
            'keep_waiting: loop {
                select! {
                    _ = tokio::time::sleep(sleep_duration) => {
                        break 'keep_waiting
                    }
                    received = self.recv() => {
                        match &received?.packet {
                            Packet::Suback(Suback { pid, return_codes }) if pid == &subscribe_pid => {
                                let failed: Box<_> = Vec::from(topics.as_ref())
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
            }
        }
        Err(MqttError::MqttSubscribeTimeout(topics.as_ref().into()))
    }

    pub async fn tick(&mut self) -> Result<(), MqttError> {
        _ = self.recv().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Arc<MqttPacket>, MqttError> {
        loop {
            select! {
                read = self.stream.read(self.buffer.as_mut()) => {
                    let response_len = read?;
                    let package = MqttPacket::try_from(&self.buffer[..response_len])?;
                    match package.packet {
                        Packet::Pingreq => {
                            let response = Packet::Pingresp;
                            let len = encode_slice(&response, self.buffer.as_mut())?;
                            self.stream.write_all(&self.buffer[..len]).await?;
                            continue;
                        },
                        Packet::Pingresp => continue,
                        _ => (),
                    }
                    let package = Arc::new(package);
                    let _orphan_subscriptions_are_ok = self.subscribers.send(package.clone());
                    return Ok(package)
                },
                _ = self.ping_interval.tick() => {
                    let response = Packet::Pingreq;
                    let len = encode_slice(&response, self.buffer.as_mut())?;
                    self.stream.write_all(&self.buffer[..len]).await?;
                },
                to_send = self.send_queue.recv() => {
                    if let Some(send) = to_send {
                        self.stream.write_all(send.as_ref()).await?;
                    }
                },
                job_result = self.jobs.join_next(), if !self.jobs.is_empty() => {
                    if let Some(job_result) = job_result {
                        let _: () = job_result??;
                    }
                }
                to_publish = self.publish_queue.recv() => {
                    if let Some(PublishQueueEntry { topic, payload, qospid: pid, response }) = to_publish {
                        let publish_retries = self.publish_retries;
                        let publish_timeout = self.publish_timeout;
                        let topic_name = topic.display().to_string();
                        if matches!(pid, QosPid::AtMostOnce) {
                            let packet = Packet::Publish(Publish { dup: false, qospid: pid, retain: false, topic_name: &topic_name, payload: &payload });
                            let len = encode_slice(&packet, self.buffer.as_mut())?;
                            response.send(self.stream.write_all(&self.buffer[..len]).await.map_err(Into::into)).map_err(|_| MqttError::MqttPublishReply)?;
                        } else {
                            let mut sender = self.sender();
                            let mut receiver = self.subscribe();
                            self.jobs.spawn(async move {
                                let timeout = match pid {
                                    QosPid::AtMostOnce => unreachable!(),
                                    QosPid::AtLeastOnce(_) => publish_timeout / publish_retries.into(),
                                    QosPid::ExactlyOnce(_) => publish_timeout,
                                };
                                let real_timeout = (std::time::Instant::now() + publish_timeout).into();
                                for attempt in 0 ..= usize::from(publish_retries) {
                                    let packet = Packet::Publish(Publish { dup: attempt != 0, qospid: pid, retain: false, topic_name: &topic_name, payload: &payload });
                                    if let Err(e) = sender.send(&packet).await {
                                        response.send(Err(e)).map_err(|_| MqttError::MqttPublishReply)?;
                                        return Ok(());
                                    }
                                    let timeout = (std::time::Instant::now() + timeout).into();
                                    'keep_waiting: loop {
                                        match pid {
                                            QosPid::AtMostOnce => unreachable!(),
                                            qos@QosPid::AtLeastOnce(pid) | qos@QosPid::ExactlyOnce(pid) => select! {
                                                _ = tokio::time::sleep_until(real_timeout) => {
                                                    response.send(Err(MqttError::PublishTimeout)).map_err(|_| MqttError::MqttPublishReply)?;
                                                    return Ok(());
                                                }
                                                _ = tokio::time::sleep_until(timeout) => {
                                                    match qos {
                                                        QosPid::AtLeastOnce(_) => break 'keep_waiting,
                                                        QosPid::ExactlyOnce(_) => {
                                                            response.send(Err(MqttError::PublishTimeout)).map_err(|_| MqttError::MqttPublishReply)?;
                                                            return Ok(());
                                                        },
                                                        QosPid::AtMostOnce => unreachable!(),
                                                    }
                                                }
                                                package = receiver.recv() => {
                                                    let package = match package {
                                                        Ok(package) => package,
                                                        Err(e) => {
                                                            response.send(Err(e.into())).map_err(|_| MqttError::MqttPublishReply)?;
                                                            return Ok(())
                                                        }
                                                    };
                                                    match package.packet {
                                                        Packet::Puback(ack_pid) if ack_pid == pid => {
                                                            response.send(Ok(())).map_err(|_| MqttError::MqttPublishReply)?;
                                                            return Ok(())
                                                        }
                                                        Packet::Pubrec(ack_pid) if ack_pid == pid => {
                                                            sender.send(&Packet::Pubrel(ack_pid)).await?;
                                                            response.send(Ok(())).map_err(|_| MqttError::MqttPublishReply)?;
                                                            return Ok(())
                                                        }
                                                        _ => (),
                                                    }
                                                }
                                            },
                                        }
                                    }
                                }
                                response.send(Err(MqttError::PublishTimeout)).map_err(|_| MqttError::MqttPublishReply)?;
                                Ok(())
                            });
                        }
                    }
                },
            }
        }
    }

    pub async fn notify_online(&mut self) -> Result<(), MqttError> {
        if let Some(availability_topic) = self
            .availability_topic
            .as_ref()
            .map(|path| Arc::from(Path::new(&**path)))
        {
            let mut publisher = self.publisher();
            let mut publish = pin!(publisher.publish(
                availability_topic,
                QosPid::AtLeastOnce(self.next_pid()),
                *b"online"
            ));
            loop {
                select! {
                    publish_result = &mut publish => {
                        publish_result?;
                        return Ok(())
                    },
                    tick_result = self.tick() => {
                        tick_result?;
                    },
                }
            }
        }
        Ok(())
    }

    pub async fn send(&mut self, packet: &Packet<'_>) -> Result<(), MqttError> {
        let encoded_len = encode_slice(&packet, self.buffer.as_mut())?;
        self.stream.write_all(&self.buffer[..encoded_len]).await?;
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
        stream.write_all(&buffer[..packet_len]).await?;
        let bytes_read = stream.read(buffer.as_mut()).await?;
        let Some(response) = decode_slice(&buffer[..bytes_read])? else {
            return Err(MqttError::NotEnoughData(buffer[..bytes_read].into()))?;
        };
        if let Packet::Connack(ack) = response {
            match ack.code {
                ConnectReturnCode::Accepted => {
                    let (send_queue_sender, send_queue) = mpsc::channel(10);
                    let (publish_queue_sender, publish_queue) = mpsc::channel(10);
                    let ping_interval = time::interval_at(
                        time::Instant::now(),
                        time::Duration::from_secs((self.keep_alive >> 1).into()),
                    );
                    Ok(Session {
                        stream,
                        buffer,
                        jobs: JoinSet::new(),
                        availability_topic: self.availability_topic,
                        base_topic: Arc::from(Path::new(&*self.base_topic)),
                        discovery_topic: Arc::from(Path::new(&*self.discovery_topic)),
                        pid: Default::default(),
                        publish_retries: self.publish_retries,
                        publish_timeout: self.publish_timeout,
                        subscribers: tokio::sync::broadcast::Sender::new(100),
                        send_queue,
                        send_queue_sender,
                        ping_interval,
                        publish_queue,
                        publish_queue_sender,
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
