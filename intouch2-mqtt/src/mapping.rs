use std::{
    collections::HashMap,
    future::Future,
    mem,
    path::Path,
    pin::{pin, Pin},
    sync::Arc,
};

use mqttrs::{Packet, Publish, QoS, QosPid, SubscribeTopic};
use serde::Deserialize;
use tokio::{
    select,
    sync::{self, mpsc, watch, Mutex, OwnedMutexGuard},
    task::JoinSet,
};

use crate::{
    home_assistant,
    mqtt_session::{MqttError, Session as MqttSession, Topic},
    spa::{SpaCommand, SpaConnection, SpaError},
};

#[derive(Deserialize)]
pub struct Entity<T> {
    pub entity: T,
    pub id: String,
    pub name: String,
}

#[derive(Deserialize)]
pub enum Light {
    RGB {
        red: usize,
        green: usize,
        blue: usize,
    },
    Dimmer(Box<Light>),
}

#[derive(Deserialize)]
pub struct Pump {}

#[derive(Deserialize)]
pub struct Climate {}

#[derive(Deserialize)]
pub enum Entities {
    Light(Entity<Light>),
    Pump(Entity<Pump>),
    Climate(Entity<Climate>),
}

#[derive(Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub entities: Entities,
}

#[derive(Deserialize)]
pub struct Config {
    pub entities: Vec<Device>,
}

#[derive(thiserror::Error, Debug)]
pub enum MappingError {
    #[error(transparent)]
    Mqtt(#[from] MqttError),
    #[error(transparent)]
    Spa(#[from] SpaError),
    #[error("Could not communicate with Spa service: {0}")]
    SpaCommand(#[from] tokio::sync::mpsc::error::SendError<SpaCommand>),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Tokio channel error: {0}")]
    BroadcastRecv(#[from] tokio::sync::broadcast::error::RecvError),
    #[error("Runtime error: {0}")]
    Runtime(#[from] tokio::task::JoinError),
    #[error("Data channel failed: {0}")]
    WatchChanged(#[from] watch::error::RecvError),
    #[error("Data channel unexpectedly closed: {0}")]
    ChannelClosed(&'static str),
    #[error("No job can be performed, because initialization failed")]
    PublisherDeadlockedByFailedInitialization,
}

pub struct Mapping {
    device: home_assistant::ConfigureDevice,
    jobs: JoinSet<Result<(), MappingError>>,
    uninitialized: Vec<Arc<Mutex<()>>>,
    active: sync::watch::Sender<bool>,
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialMode<T> {
    WatercareMode,
    #[serde(untagged)]
    Multiple(Box<[T]>),
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum MappingType {
    U8 { u8_addr: u16 },
    U16 { u16_addr: u16 },
    Array { addr: u16, len: u16 },
    Special(SpecialMode<MappingType>),
}

pub struct WatchMap<W, I, T> {
    watch: W,
    map: Box<dyn FnMut(&I) -> T + Send + 'static>,
    value: Option<T>,
}

pub trait GenericWatchMap<T>: Send {
    fn changed<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), MappingError>> + 'a + Send>>;

    fn borrow_and_update(&mut self) -> &T;
}

impl<I: Sync + Send + 'static, T: Send> GenericWatchMap<T> for WatchMap<watch::Receiver<I>, I, T> {
    fn changed<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), MappingError>> + 'a + Send>> {
        Box::pin(async move { Ok(self.watch.changed().await?) })
    }

    fn borrow_and_update(&mut self) -> &T {
        self.value = Some((self.map)(&self.watch.borrow_and_update()));
        self.value.as_ref().expect("Value set to some right above")
    }
}

impl GenericWatchMap<serde_json::Value> for WatchMap<mpsc::Receiver<()>, (), serde_json::Value> {
    fn changed<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<(), MappingError>> + 'a + Send>> {
        Box::pin(async move {
            match self.watch.recv().await {
                Some(()) => {
                    while self.watch.try_recv().is_ok() {}
                    self.value = None;
                    Ok(())
                }
                None => Err(MappingError::ChannelClosed("WatchMap<mpsc::Receiver>")).into(),
            }
        })
    }

    fn borrow_and_update(&mut self) -> &serde_json::Value {
        if self.value.is_none() {
            self.value = Some((self.map)(&()));
        }
        self.value.as_ref().expect("Value set to Some right above")
    }
}

impl<W, I, T> WatchMap<W, I, T> {
    pub fn new<F: 'static + Send + FnMut(&I) -> T>(watch: W, map: F) -> Self {
        Self {
            watch,
            map: Box::new(map),
            value: None,
        }
    }
}

impl MappingType {
    pub fn subscribe<'a, T: Send + 'static>(
        &'a self,
        spa: &'a SpaConnection,
        jobs: &'a mut JoinSet<Result<T, MappingError>>,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<Box<dyn GenericWatchMap<serde_json::Value>>, MappingError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            fn to_return<T: GenericWatchMap<serde_json::Value> + 'static>(
                x: T,
            ) -> Box<dyn GenericWatchMap<serde_json::Value>> {
                Box::new(x)
            }
            match self {
                MappingType::Special(SpecialMode::Multiple(children)) => {
                    let (tx, rx) = mpsc::channel(children.len());
                    let mut subscriptions = Vec::with_capacity(children.len());
                    for child in children.iter() {
                        subscriptions.push(child.subscribe(spa, jobs).await?);
                    }
                    for sub in children.to_owned().into_iter() {
                        let mut subscriber = sub.subscribe(spa, jobs).await?;
                        let tx = tx.clone();
                        jobs.spawn(async move {
                            loop {
                                subscriber.changed().await?;
                                _ = tx.send(()).await;
                            }
                        });
                    }
                    let map = WatchMap::new(rx, move |_: &()| {
                        serde_json::Value::Array(
                            subscriptions
                                .iter_mut()
                                .map(|x| x.borrow_and_update().to_owned())
                                .collect(),
                        )
                    });
                    Ok(to_return(map))
                }
                MappingType::Special(SpecialMode::WatercareMode) => {
                    let subscribe = spa.subscribe_watercare_mode().await;
                    let map = WatchMap::new(subscribe, |x: &Option<u8>| {
                        x.map(|valid_data| serde_json::Value::Number(valid_data.into()))
                            .unwrap_or(serde_json::Value::Null)
                    });
                    Ok(to_return(map))
                }
                value @ MappingType::U8 { .. } => {
                    let subscribe = spa.subscribe(value.range().expect("U8 has a range")).await;
                    let map = WatchMap::new(subscribe, |valid_data: &Box<[u8]>| {
                        let array: &[u8; 1] = valid_data
                            .as_ref()
                            .try_into()
                            .expect("This value will always be 1 byte");
                        serde_json::Value::Number(array[0].into())
                    });
                    Ok(to_return(map))
                }
                value @ MappingType::U16 { .. } => {
                    let subscribe = spa.subscribe(value.range().expect("U16 has a range")).await;
                    let map = WatchMap::new(subscribe, |valid_data: &Box<[u8]>| {
                        let array: &[u8; 2] = valid_data
                            .as_ref()
                            .try_into()
                            .expect("This value will always be 2 bytes");
                        serde_json::Value::Number(u16::from_be_bytes(*array).into())
                    });
                    Ok(to_return(map))
                }
                value @ MappingType::Array { .. } => {
                    let subscribe = spa
                        .subscribe(value.range().expect("Array has a range"))
                        .await;
                    let map = WatchMap::new(subscribe, |valid_data: &Box<[u8]>| {
                        serde_json::Value::Array(
                            valid_data
                                .iter()
                                .map(|element| serde_json::Value::Number((*element).into()))
                                .collect(),
                        )
                    });
                    Ok(to_return(map))
                }
            }
        })
    }
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum CommandStatusType {
    U8 { u8_addr: u16 },
    U16 { u16_addr: u16 },
    Array { addr: u16, len: u16 },
}

impl CommandStatusType {
    pub fn parse(&self, payload: &[u8]) -> Result<Box<[u8]>, serde_json::error::Error> {
        match self {
            CommandStatusType::U8 { .. } => {
                Ok(Box::from(&[serde_json::from_slice::<u8>(payload)?][..]))
            }
            CommandStatusType::U16 { .. } => Ok(Box::from(
                serde_json::from_slice::<u16>(payload)?.to_be_bytes(),
            )),
            CommandStatusType::Array { .. } => Ok(serde_json::from_slice::<Box<[u8]>>(payload)?),
        }
    }

    pub fn range(&self) -> std::ops::Range<u16> {
        match self {
            CommandStatusType::U8 { u8_addr } => *u8_addr..u8_addr + 1,
            CommandStatusType::U16 { u16_addr } => *u16_addr..u16_addr + 2,
            CommandStatusType::Array { addr, len } => *addr..addr + len,
        }
    }
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum CommandMappingType {
    SetStatus {
        config_version: u8,
        log_version: u8,
        pack_type: u8,
        #[serde(flatten)]
        data: CommandStatusType,
    },
    Special(SpecialMode<CommandMappingType>),
}

impl MappingType {
    pub fn range(&self) -> Option<std::ops::Range<usize>> {
        let start = match self {
            Self::U8 { u8_addr: start }
            | Self::U16 { u16_addr: start }
            | Self::Array { addr: start, .. } => usize::from(*start),
            Self::Special(_) => return None,
        };
        let len = match self {
            Self::U8 { .. } => 1,
            Self::U16 { .. } => 2,
            Self::Array { len, .. } => usize::from(*len),
            Self::Special(_) => unreachable!(),
        };
        let end = start + len;
        Some(start..end)
    }
}

#[derive(serde::Deserialize, Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum MqttType {
    State { state: MappingType },
    Command { command: CommandMappingType },
    Value(serde_json::Value),
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct GenericMapping {
    #[serde(rename = "type")]
    pub mqtt_type: &'static str,
    pub name: &'static str,
    pub unique_id: &'static str,
    #[serde(default)]
    pub qos: u8,
    #[serde(flatten)]
    pub mqtt_values: HashMap<&'static str, MqttType>,
}

#[cfg(test)]
mod tests {
    #[test]
    fn barebone_generic() -> anyhow::Result<()> {
        let mapping: super::GenericMapping = serde_json::from_str(
            r#"{"type": "light", "name": "Some light", "unique_id": "light0001"}"#,
        )?;
        eprintln!("Mapping was {mapping:?}");
        Ok(())
    }
    #[test]
    fn with_custom_values() -> anyhow::Result<()> {
        let mapping: super::GenericMapping = serde_json::from_str(
            r#"{"type": "light", "name": "Some light", "unique_id": "light0001", "optimistic": false}"#,
        )?;
        eprintln!("Mapping was {mapping:?}");
        Ok(())
    }
    #[test]
    fn with_custom_values_early() -> anyhow::Result<()> {
        let mapping: super::GenericMapping = serde_json::from_str(
            r#"{"type": "light", "optimistic": false, "name": "Some light", "unique_id": "light0001"}"#,
        )?;
        eprintln!("Mapping was {mapping:?}");
        Ok(())
    }
    #[test]
    fn with_fetcher() -> anyhow::Result<()> {
        let mapping: super::GenericMapping = serde_json::from_str(
            r#"{"type": "light", "name": "Some light", "unique_id": "light0001", "state_topic": {"state": {"u8_addr": 100}}}"#,
        )?;
        eprintln!("Mapping was {mapping:?}");
        Ok(())
    }
    #[test]
    fn create_mqtt_type() -> anyhow::Result<()> {
        let to_serialize = super::MqttType::Command {
            command: super::CommandMappingType::SetStatus {
                config_version: 1,
                log_version: 2,
                pack_type: 3,
                data: super::CommandStatusType::U8 { u8_addr: 4 },
            },
        };
        let serialized = serde_json::to_string(&to_serialize)?;
        eprintln!("Serialized: {serialized}");
        let reparsed: super::MqttType = serde_json::from_str(&serialized)?;
        assert_eq!(to_serialize, reparsed);
        let parsed: super::MqttType = serde_json::from_str(
            r#"{"command":{"config_version":1,"log_version":2,"pack_type":3,"u16_addr":4}}"#,
        )?;
        assert!(matches!(parsed, super::MqttType::Command { .. }));
        Ok(())
    }
}

impl GenericMapping {
    pub fn config_is_static(&self) -> bool {
        true
    }
}

impl Mapping {
    pub async fn reset(&mut self) {
        self.jobs.shutdown().await;
        self.jobs = JoinSet::new();
        self.uninitialized = vec![];
        self.active.send_replace(false);
    }

    pub async fn start(&mut self, mqtt: &mut MqttSession) -> Result<(), MappingError> {
        self.active.send_replace(true);
        while let Some(lock) = self.uninitialized.last().map(<Arc<_> as Clone>::clone) {
            let mut acquire_lock = pin!(lock.lock_owned());
            loop {
                select! {
                    _ = &mut acquire_lock => {
                        self.uninitialized.pop();
                        break
                    }
                    tick_result = self.tick() => {
                        let _: () = tick_result?;
                        continue
                    }
                    mqtt_result = mqtt.tick() => {
                        let _: () = mqtt_result?;
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn add_generic(
        &mut self,
        mapping: GenericMapping,
        spa: &SpaConnection,
        mqtt: &mut MqttSession,
    ) -> Result<(), MappingError> {
        let config_topic = mqtt.topic(&mapping.mqtt_type, &mapping.unique_id, Topic::Config);
        let mut counter = 0;
        let topics = mqtt.topic_generator();
        let GenericMapping {
            mqtt_type,
            name: mqtt_name,
            unique_id,
            mqtt_values,
            qos,
        } = mapping;
        let mut next_topic = |topic: Topic| {
            counter += 1;
            topics.topic(&mqtt_type, &format!("{unique_id}/{counter}"), topic)
        };
        let next_qos = {
            let publisher = mqtt.publisher();
            move || match qos {
                1 => QosPid::AtLeastOnce(publisher.next_pid()),
                2 => QosPid::ExactlyOnce(publisher.next_pid()),
                _ => QosPid::AtMostOnce,
            }
        };

        let device = self.device.clone();
        let json_config = {
            let mut config = home_assistant::ConfigureGeneric {
                base: home_assistant::ConfigureBase {
                    name: &mqtt_name,
                    unique_id: &unique_id,
                    device: &device,
                    qos,
                },
                args: Default::default(),
            };
            for (key, value) in &mqtt_values {
                match value {
                    MqttType::State { state } => {
                        let topic = next_topic(Topic::State);
                        {
                            let topic = topic.clone();
                            let state = state.clone();
                            let mut sender = mqtt.publisher();
                            let mut data_subscription =
                                state.subscribe(&spa, &mut self.jobs).await?;
                            let mut initialized = self.active.subscribe();
                            let mutex = Arc::new(Mutex::new(())).try_lock_owned().expect(
                                "This mutex was just created, the lock should be guaranteed",
                            );
                            self.uninitialized
                                .push(OwnedMutexGuard::mutex(&mutex).clone());
                            let mut first_state_sent = Some(mutex);
                            let next_qos = next_qos.clone();
                            self.jobs.spawn(async move {
                                loop {
                                    if *initialized.borrow_and_update() {
                                        break
                                    }
                                    if initialized.changed().await.is_err() {
                                        if !*initialized.borrow_and_update() {
                                            return Err(MappingError::PublisherDeadlockedByFailedInitialization);
                                        }
                                    }
                                }
                                loop {
                                    let reported_value = data_subscription.borrow_and_update();
                                    let payload = serde_json::to_vec(&reported_value)?;
                                    sender
                                        .publish(Path::new(&topic), next_qos(), payload)
                                        .await?;
                                    let lock: Option<OwnedMutexGuard<()>> =
                                        mem::take(&mut first_state_sent);
                                    drop(lock);
                                    data_subscription.changed().await?;
                                }
                            });
                        }
                        config.args.insert(key.as_ref(), topic.into())
                    }
                    MqttType::Command { command } => {
                        let topic = next_topic(Topic::Set);
                        mqtt.mqtt_subscribe(vec![SubscribeTopic {
                            topic_path: topic.clone(),
                            qos: QoS::AtMostOnce,
                        }])
                        .await?;
                        let mut receiver = mqtt.subscribe();
                        let spa_sender = spa.sender();
                        {
                            let topic = topic.clone();
                            let command = command.clone();
                            self.jobs.spawn(async move {
                                loop {
                                    match (&command, &receiver.recv().await?.packet()) {
                                        (
                                            CommandMappingType::Special(SpecialMode::WatercareMode),
                                            Packet::Publish(Publish {
                                                dup: false,
                                                topic_name,
                                                payload,
                                                ..
                                            }),
                                        ) if topic_name == &&topic => {
                                            let Ok(valid_str) =
                                                String::from_utf8(Vec::from(*payload))
                                            else {
                                                eprintln!("Invalid payload from MQTT: {payload:?}");
                                                continue;
                                            };
                                            let Ok(mode) = valid_str.parse() else {
                                                eprintln!("Invalid payload from MQTT: {valid_str}");
                                                continue;
                                            };
                                            spa_sender.send(SpaCommand::SetWatercare(mode)).await?;
                                        }
                                        (
                                            CommandMappingType::SetStatus { config_version, log_version, pack_type, data },
                                            Packet::Publish(Publish {
                                                dup: false,
                                                topic_name,
                                                payload,
                                                ..
                                            }),
                                        ) if topic_name == &topic => {
                                            let range = data.range();
                                            let payload = match data.parse(payload) {
                                                Ok(data) => data,
                                                Err(e) => {
                                                    eprintln!("Invalid data from MQTT: {e}");
                                                    continue;
                                                }
                                            };
                                            if range.len() != payload.len() {
                                                eprintln!("Data does not match size constraint of {len}: {payload:?}", len = range.len());
                                                continue;
                                            }
                                            spa_sender.send(SpaCommand::SetStatus {
                                                config_version: *config_version, log_version: *log_version, pack_type: *pack_type, pos: range.start, data: (*payload).into(),
                                            }).await?;
                                        }
                                        _ => (),
                                    };
                                }
                            });
                        }
                        config.args.insert(key.as_ref(), topic.into())
                    }
                    MqttType::Value(value) => config.args.insert(key.as_ref(), value.clone()),
                };
            }
            serde_json::to_vec(&config)?
        };
        let mut publisher = mqtt.publisher();
        let mut publish =
            pin!(publisher.publish(Path::new(&config_topic), next_qos(), json_config,));
        loop {
            select! {
                publish_result = &mut publish => {
                    publish_result?;
                    break
                }
                mqtt_result = mqtt.tick() => {
                    mqtt_result?
                }
            }
        }
        Ok(())
    }

    pub async fn tick(&mut self) -> Result<(), MappingError> {
        select! {
            join_result = self.jobs.join_next() => {
                if let Some(join_result) = join_result {
                    join_result??;
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(1000)), if self.jobs.is_empty() => {},
        }
        Ok(())
    }
}

impl Mapping {
    pub fn new(device: home_assistant::ConfigureDevice) -> Result<Self, MappingError> {
        let jobs = JoinSet::new();
        Ok(Self {
            jobs,
            device,
            uninitialized: vec![],
            active: sync::watch::Sender::new(false),
        })
    }
}
