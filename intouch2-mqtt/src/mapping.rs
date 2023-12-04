use std::collections::HashMap;

use mqttrs::{Packet, Publish, QosPid};
use serde::Deserialize;
use tokio::task::JoinSet;

use crate::{
    home_assistant,
    mqtt_session::{MqttError, Session as MqttSession, Topic},
    spa::{SpaConnection, SpaError},
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
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Tokio channel error: {0}")]
    BroadcastRecv(#[from] tokio::sync::broadcast::error::RecvError),
    #[error("Runtime error: {0}")]
    Runtime(#[from] tokio::task::JoinError),
}

pub struct Mapping {
    device: home_assistant::ConfigureDevice,
    jobs: JoinSet<Result<(), MappingError>>,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, untagged)]
pub enum MappingType {
    U8 { u8_addr: u16 },
    U16 { u16_addr: u16 },
    Array { addr: u16, len: u16 },
    Static(serde_json::Value),
}

impl MappingType {
    pub fn is_static(&self) -> bool {
        matches!(self, Self::Static(_))
    }
    pub fn range(&self) -> Option<std::ops::Range<usize>> {
        let start = match self {
            Self::U8 { u8_addr: start }
            | Self::U16 { u16_addr: start }
            | Self::Array { addr: start, .. } => usize::from(*start),
            Self::Static(_) => return None,
        };
        let len = match self {
            Self::U8 { .. } => 1,
            Self::U16 { .. } => 2,
            Self::Array { len, .. } => usize::from(*len),
            Self::Static(_) => unreachable!(),
        };
        let end = start + len;
        Some(start..end)
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields, untagged)]
pub enum MqttType {
    State { state: MappingType },
    Value(serde_json::Value),
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct GenericMapping {
    #[serde(rename = "type")]
    pub mqtt_type: &'static str,
    pub name: &'static str,
    pub unique_id: &'static str,
    // pub topics: HashMap<&'a str, MappingType<'a>>,
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
}

impl GenericMapping {
    pub fn config_is_static(&self) -> bool {
        true
        // self.mqtt_values.values().find(|x| !x.is_static()).is_none()
    }
}

impl Mapping {
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
        } = mapping;
        let mut next_state_topic = || {
            counter += 1;
            topics.topic(&mqtt_type, &format!("{unique_id}/{counter}"), Topic::State)
        };

        let device = self.device.clone();
        let mut new_jobs = vec![];
        let json_config = {
            let mut config = home_assistant::ConfigureGeneric {
                base: home_assistant::ConfigureBase {
                    name: &mqtt_name,
                    unique_id: &unique_id,
                    device: &device,
                },
                args: Default::default(),
            };
            for (key, value) in &mqtt_values {
                match value {
                    MqttType::State { state } => {
                        let topic = next_state_topic();
                        let mut sender = mqtt.sender();
                        {
                            let topic = topic.clone();
                            let state = state.clone();
                            let mut subscription = if let Some(range) = state.range() {
                                Some(spa.subscribe(range).await)
                            } else {
                                None
                            };
                            new_jobs.push(async move {
                                loop {
                                    let reported_value = match (
                                        &state,
                                        subscription.as_mut().map(|x| x.borrow_and_update()),
                                    ) {
                                        (MappingType::Static(value), None) => value.clone(),
                                        (MappingType::Static(_), Some(_)) => unreachable!(),
                                        (MappingType::U8 { .. }, Some(data)) => {
                                            let new_value: &[u8; 1] = data
                                                .as_ref()
                                                .try_into()
                                                .expect("This will always be 1 byte");
                                            serde_json::Value::Number(new_value[0].into())
                                        }
                                        (MappingType::U16 { .. }, Some(data)) => {
                                            let new_value: &[u8; 2] = data
                                                .as_ref()
                                                .try_into()
                                                .expect("This will always be 2 bytes");
                                            serde_json::Value::Number(
                                                u16::from_be_bytes(*new_value).into(),
                                            )
                                        }
                                        (MappingType::Array { .. }, Some(data)) => {
                                            serde_json::Value::Array(
                                                data.iter()
                                                    .map(|x| serde_json::Value::Number((*x).into()))
                                                    .collect(),
                                            )
                                        }
                                        (_, None) => unreachable!(),
                                    };
                                    let payload = serde_json::to_vec(&reported_value)?;
                                    let package = Packet::Publish(Publish {
                                        dup: false,
                                        qospid: QosPid::AtMostOnce,
                                        retain: false,
                                        topic_name: &topic,
                                        payload: &payload,
                                    });
                                    sender.send(&package).await?;
                                    if let Some(subscription) = &mut subscription {
                                        subscription.changed().await.unwrap();
                                    } else {
                                        return Ok(());
                                    }
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
        let config_packet = Packet::Publish(Publish {
            dup: false,
            qospid: QosPid::AtMostOnce,
            retain: false,
            topic_name: &config_topic,
            payload: &json_config,
        });
        mqtt.send(config_packet).await?;
        for job in new_jobs {
            self.jobs.spawn(job);
        }
        Ok(())
    }

    pub async fn tick(&mut self) -> Result<(), MappingError> {
        if let Some(join_result) = self.jobs.join_next().await {
            _ = join_result?;
        }
        Ok(())
    }
}

impl Mapping {
    pub fn new(device: home_assistant::ConfigureDevice) -> Result<Self, MappingError> {
        let jobs = JoinSet::new();
        Ok(Self { jobs, device })
    }
}
