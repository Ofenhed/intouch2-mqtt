use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
};

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

pub struct Mapping<'a> {
    device: home_assistant::ConfigureDevice<'a>,
    jobs: JoinSet<Result<(), MappingError>>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct EnumMapping {
    pub address: u16,
    pub len: u16,
    pub bitmap: Option<String>,
    pub mapping: HashMap<String, String>,
}

fn apply_bitmap<'a>(bitmap: Option<&[u8]>, data: &'a [u8]) -> Cow<'a, [u8]> {
    if let Some(bitmap) = bitmap {
        Cow::Owned(
            data.iter()
                .zip(bitmap.iter())
                .map(|(data, map)| data & map)
                .collect(),
        )
    } else {
        Cow::Borrowed(data)
    }
}

impl EnumMapping {
    fn apply_bitmap<'a>(&self, data: &'a [u8]) -> Cow<'a, [u8]> {
        apply_bitmap(self.bitmap.as_ref().map(|x| x.as_bytes()), data)
    }
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PlainMapping {
    pub address: u16,
    pub len: u16,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct LightMapping<'a> {
    pub name: &'a str,
    pub onoff: EnumMapping,
    pub rgb: Option<PlainMapping>,
    pub effects: Option<EnumMapping>,
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FanMapping<'a> {
    pub name: &'a str,
    pub state_mapping: EnumMapping,
    pub percent_mapping: Option<EnumMapping>,
}

#[derive(serde::Deserialize, strum::IntoStaticStr, Debug, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub enum TemperatureUnit {
    C,
    F,
}

impl TemperatureUnit {
    pub fn translate(&self, data: &[u8; 2]) -> f32 {
        let new_value: f32 = u16::from_be_bytes(*data).into();
        let float = match self {
            Self::C => new_value / 18.0,
            Self::F => (new_value + 320.0) / 10.0,
        };
        float
    }
}

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ClimateMapping<'a> {
    pub name: &'a str,
    pub target_addr: u16,
    pub current_addr: Option<u16>,
    pub unit: TemperatureUnit,
}

impl Mapping<'_> {
    pub async fn add_light(
        &mut self,
        identifier: &str,
        mapping: LightMapping<'_>,
        spa: &SpaConnection,
        mqtt: &mut MqttSession,
    ) -> Result<(), MappingError> {
        let unique_id = format!("light{identifier}");
        let state_topic = mqtt.topic("light", &unique_id, Topic::State);
        let command_topic = mqtt.topic("light", &unique_id, Topic::Set);
        let config_topic = mqtt.topic("light", &unique_id, Topic::Config);
        let (effect_list, effects_state_topic, effects_command_topic): (
            Option<Vec<String>>,
            Option<String>,
            Option<String>,
        ) = if let Some(effects) = &mapping.effects {
            let values: HashSet<_> = effects.mapping.values().map(ToOwned::to_owned).collect();
            (
                Some(values.into_iter().collect()),
                Some(mqtt.topic("light", &format!("{unique_id}/effect"), Topic::State)),
                Some(mqtt.topic("light", &format!("{unique_id}/effect"), Topic::Set)),
            )
        } else {
            (None, None, None)
        };
        let (color_mode, rgb_state_topic, rgb_command_topic) = if let Some(_rgb) = &mapping.rgb {
            (
                Some("rgb"),
                Some(mqtt.topic("light", &format!("{unique_id}/rgb"), Topic::State)),
                Some(mqtt.topic("light", &format!("{unique_id}/rgb"), Topic::Set)),
            )
        } else {
            (None, None, None)
        };
        let effects = effect_list
            .as_ref()
            .map(|x| x.iter().map(AsRef::as_ref).collect());
        let payload = home_assistant::ConfigureLight {
            command_topic: &command_topic,
            rgb_state_topic: rgb_state_topic.as_deref(),
            rgb_command_topic: rgb_command_topic.as_deref(),
            state_topic: Some(&state_topic),
            effect_list: effects,
            effect_state_topic: effects_state_topic.as_deref(),
            effect_command_topic: effects_command_topic.as_deref(),
            color_mode,
            base: home_assistant::ConfigureBase {
                name: mapping.name,
                optimistic: false,
                unique_id: &unique_id,
                device: &self.device,
            },
        };
        let json_payload = serde_json::to_vec(&payload)?;
        let config_packet = Packet::Publish(Publish {
            dup: false,
            qospid: QosPid::AtMostOnce,
            retain: false,
            topic_name: &config_topic,
            payload: &json_payload,
        });
        mqtt.send(config_packet).await?;
        let onoff_start = mapping.onoff.address.into();
        let onoff_end = (mapping.onoff.address + mapping.onoff.len).into();
        let mut spa_data = spa.subscribe(onoff_start..onoff_end).await;
        let onoff = mapping.onoff;
        {
            let mut sender = mqtt.sender();
            self.jobs.spawn(async move {
                loop {
                    let empty = "".to_string();
                    let reported_value = {
                        let new_value = spa_data.borrow_and_update();
                        onoff
                            .mapping
                            .get(
                                String::from_utf8_lossy(
                                    &onoff.apply_bitmap(new_value.as_ref()).as_ref(),
                                )
                                .as_ref(),
                            )
                            .unwrap_or(&empty)
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: &state_topic,
                        payload: reported_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    spa_data.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        if let Some(effects) = mapping.effects {
            let mut sender = mqtt.sender();
            let start = effects.address.into();
            let end = (effects.address + effects.len).into();
            let mut spa_data = spa.subscribe(start..end).await;
            self.jobs.spawn(async move {
                loop {
                    let null = "".to_string();
                    let reported_value = {
                        let new_value = spa_data.borrow_and_update();
                        effects
                            .mapping
                            .get(
                                String::from_utf8_lossy(
                                    &effects.apply_bitmap(new_value.as_ref()).as_ref(),
                                )
                                .as_ref(),
                            )
                            .unwrap_or(&null)
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: effects_state_topic
                            .as_deref()
                            .expect("Can only get here if effects topic is Some"),
                        payload: reported_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    spa_data.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        if let Some(rgb) = mapping.rgb {
            let mut sender = mqtt.sender();
            let start = rgb.address.into();
            let end = (rgb.address + rgb.len).into();
            let mut spa_data = spa.subscribe(start..end).await;
            self.jobs.spawn(async move {
                loop {
                    let payload = {
                        let raw = spa_data.borrow_and_update();
                        format!(
                            "{red},{green},{blue}",
                            red = raw[0],
                            green = raw[1],
                            blue = raw[2]
                        )
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: rgb_state_topic
                            .as_deref()
                            .expect("Can only get here if rgb topic is Some"),
                        payload: payload.as_bytes(),
                    });
                    sender.send(&package).await?;
                    spa_data.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        Ok(())
    }

    pub async fn add_climate(
        &mut self,
        identifier: &str,
        mapping: ClimateMapping<'_>,
        spa: &SpaConnection,
        mqtt: &mut MqttSession,
    ) -> Result<(), MappingError> {
        let unique_id = format!("climate{identifier}");
        let target_temperature_state_topic = mqtt.topic("climate/target", &unique_id, Topic::State);
        let current_temperature_state_topic =
            mqtt.topic("climate/current", &unique_id, Topic::State);
        let config_topic = mqtt.topic("climate", &unique_id, Topic::Config);
        let payload = home_assistant::ConfigureClimate {
            temperature_state_topic: Some(&target_temperature_state_topic),
            temperature_unit: Some(mapping.unit.into()),
            current_temperature_topic: mapping
                .current_addr
                .map(|_| current_temperature_state_topic.as_str()),
            base: home_assistant::ConfigureBase {
                name: mapping.name,
                optimistic: false,
                unique_id: &unique_id,
                device: &self.device,
            },
        };
        let json_payload = serde_json::to_vec(&payload)?;
        mqtt.send(Packet::Publish(Publish {
            dup: false,
            qospid: QosPid::AtMostOnce,
            retain: false,
            topic_name: &config_topic,
            payload: &json_payload,
        }))
        .await?;
        if let Some(current_temp_addr) = mapping.current_addr {
            let mut current_temperature = spa
                .subscribe(current_temp_addr.into()..(current_temp_addr + 2).into())
                .await;
            let mut sender = mqtt.sender();
            self.jobs.spawn(async move {
                loop {
                    let new_value = {
                        let ptr = current_temperature.borrow_and_update();
                        let raw: &[u8; 2] = ptr
                            .as_ref()
                            .try_into()
                            .expect("This is always two bytes long, as written above");
                        format!("{}", mapping.unit.translate(raw))
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: &current_temperature_state_topic,
                        payload: new_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    current_temperature.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        {
            let mut temperature_target = spa
                .subscribe(mapping.target_addr.into()..(mapping.target_addr + 2).into())
                .await;
            let mut sender = mqtt.sender();
            self.jobs.spawn(async move {
                loop {
                    let new_value = {
                        let ptr = temperature_target.borrow_and_update();
                        let raw: &[u8; 2] = ptr
                            .as_ref()
                            .try_into()
                            .expect("This is always two bytes long, as written above");
                        format!("{}", mapping.unit.translate(raw))
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: &target_temperature_state_topic,
                        payload: new_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    temperature_target.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        Ok(())
    }

    pub async fn add_pump(
        &mut self,
        identifier: &str,
        mapping: FanMapping<'_>,
        spa: &SpaConnection,
        mqtt: &mut MqttSession,
    ) -> Result<(), MappingError> {
        let unique_id = format!("pump{identifier}");
        let state_topic = mqtt.topic("fan", &unique_id, Topic::State);
        let command_topic = mqtt.topic("fan", &unique_id, Topic::Set);
        let config_topic = mqtt.topic("fan", &unique_id, Topic::Config);
        let (percent_state_topic, percent_command_topic): (Option<String>, Option<String>) =
            if let Some(_) = &mapping.percent_mapping {
                (
                    Some(mqtt.topic("fan", &format!("{unique_id}/percent"), Topic::State)),
                    Some(mqtt.topic("fan", &format!("{unique_id}/percent"), Topic::Set)),
                )
            } else {
                (None, None)
            };
        let payload = home_assistant::ConfigureFan {
            command_topic: &command_topic,
            state_topic: Some(&state_topic),
            percentage_command_topic: percent_command_topic.as_deref(),
            percentage_state_topic: percent_state_topic.as_deref(),
            base: home_assistant::ConfigureBase {
                name: mapping.name,
                optimistic: false,
                unique_id: &unique_id,
                device: &self.device,
            },
        };
        let json_payload = serde_json::to_vec(&payload)?;
        let config_packet = Packet::Publish(Publish {
            dup: false,
            qospid: QosPid::AtMostOnce,
            retain: false,
            topic_name: &config_topic,
            payload: &json_payload,
        });
        mqtt.send(config_packet).await?;
        let state_start = mapping.state_mapping.address.into();
        let state_end = (mapping.state_mapping.address + mapping.state_mapping.len).into();
        let mut state = spa.subscribe(state_start..state_end).await;
        let state_mapping = mapping.state_mapping;
        {
            let mut sender = mqtt.sender();
            self.jobs.spawn(async move {
                loop {
                    let empty = "".to_string();
                    let reported_value = {
                        let new_value = state.borrow_and_update();
                        state_mapping
                            .mapping
                            .get(
                                String::from_utf8_lossy(
                                    &state_mapping.apply_bitmap(new_value.as_ref()).as_ref(),
                                )
                                .as_ref(),
                            )
                            .unwrap_or(&empty)
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: &state_topic,
                        payload: reported_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    state.changed().await.unwrap(); // TODO: Add error handling
                }
            });
        }
        if let Some(percent) = mapping.percent_mapping {
            let mut sender = mqtt.sender();
            let start = percent.address.into();
            let end = (percent.address + percent.len).into();
            let mut spa_data = spa.subscribe(start..end).await;
            self.jobs.spawn(async move {
                loop {
                    let null = "".to_string();
                    let reported_value = {
                        let new_value = spa_data.borrow_and_update();
                        percent
                            .mapping
                            .get(
                                String::from_utf8_lossy(
                                    &percent.apply_bitmap(new_value.as_ref()).as_ref(),
                                )
                                .as_ref(),
                            )
                            .unwrap_or(&null)
                    };
                    let package = Packet::Publish(Publish {
                        dup: false,
                        qospid: QosPid::AtMostOnce,
                        retain: false,
                        topic_name: percent_state_topic
                            .as_deref()
                            .expect("Can only get here if effects topic is Some"),
                        payload: reported_value.as_bytes(),
                    });
                    sender.send(&package).await?;
                    spa_data.changed().await.unwrap(); // TODO: Add error handling
                }
            });
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

impl<'a> Mapping<'a> {
    pub fn new(device: home_assistant::ConfigureDevice<'a>) -> Result<Self, MappingError> {
        let jobs = JoinSet::new();
        Ok(Self { jobs, device })
    }
}
