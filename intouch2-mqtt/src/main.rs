use anyhow::Context;
use clap::Parser;
use intouch2::object::NetworkPackageData;
use intouch2_mqtt::{
    home_assistant,
    mapping::{self, Mapping},
    mqtt_session::{MqttAuth, SessionBuilder as MqttSession},
    port_forward::{FullPackagePipe, PortForwardBuilder, PortForwardError},
    spa::{SpaConnection, SpaError},
};
use std::{
    collections::VecDeque,
    net::IpAddr,
    path::PathBuf,
    sync::{Arc, OnceLock},
    time::Duration,
};

use serde::Deserialize;

use tokio::{
    net::{self},
    task::JoinSet,
    time::timeout,
};

mod default_values {
    use super::*;
    pub fn spa_name() -> Arc<str> {
        "spa_pool".into()
    }

    pub fn spa_port() -> u16 {
        10022
    }

    pub fn udp_timeout() -> u16 {
        300
    }

    pub fn handshake_timeout() -> u16 {
        10
    }

    pub fn discovery_topic() -> Arc<str> {
        "homeassistant".into()
    }

    pub fn r#false() -> bool {
        false
    }
}

#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum JsonValue<T: Deserialize<'static>> {
    #[serde(skip)]
    Parsed(T),
    #[serde(untagged)]
    Raw(String),
}

impl<T: Deserialize<'static>> JsonValue<T> {
    fn unwrap(&self) -> &T {
        let JsonValue::Parsed(value) = self else {
            panic!("Tried to unwrap a raw JsonValue")
        };
        value
    }

    fn leaking_parse(&mut self) -> Result<(), anyhow::Error> {
        let raw_value: &'static str = {
            let JsonValue::Raw(raw_value) = self else {
                panic!("leaking_parse can only be used on raw JsonValue")
            };
            Box::leak(Box::from(raw_value.as_ref()))
        };
        let parsed = serde_json::from_str(raw_value).context(raw_value)?;
        *self = JsonValue::Parsed(parsed);
        Ok(())
    }
}

#[derive(Parser, Deserialize, Debug)]
#[serde(deny_unknown_fields)]
struct Command {
    /// The IP and Port of the Spa system.
    #[arg(long)]
    spa_target: Arc<str>,

    /// The name which should be used for the spa in MQTT commands
    #[serde(default = "default_values::spa_name")]
    #[arg(default_value = "spa_pool", short = 'n', alias = "spa_name")]
    spa_id: Arc<str>,

    /// The memory size of your spa. This can be found by wiretapping your Spa app. This is
    /// required for anything else than wiretapping.
    #[arg(long)]
    spa_memory_size: Option<usize>,

    /// Timeout before the Spa is considered unaccessible after initial contact.
    #[serde(default = "default_values::udp_timeout")]
    #[arg(default_value = "300")]
    spa_udp_timeout: u16,

    /// Timeout for the first Hello packet to the Spa.
    #[serde(default = "default_values::handshake_timeout")]
    #[arg(default_value = "10", alias = "handshake-timeout")]
    spa_handshake_timeout: u16,
    #[serde(default = "default_values::r#false")]
    #[arg(short, long)]
    verbose: bool,
    #[serde(default = "default_values::r#false")]
    #[arg(short, long)]

    /// Dump all traffic to stdout
    dump_traffic: bool,

    /// Forward traffic from a local port to the Spa. This can be used to figure out
    /// spa_memory_size, or for general debugging.
    #[arg(alias = "forward-ip", required = false)]
    spa_forward_listen_ip: Option<IpAddr>,

    #[serde(skip, default = "default_values::spa_port")]
    #[arg(default_value = "10022", alias = "forward-port")]
    spa_forward_listen_port: u16,

    /// The MQTT server address and port number
    #[arg(long)]
    mqtt_target: Option<Arc<str>>,

    #[arg(
        short = 'u',
        requires("mqtt-password"),
        requires("mqtt-target"),
        env("MQTT_USER")
    )]
    mqtt_username: Option<Arc<str>>,

    #[arg(
        short = 'p',
        requires("mqtt-username"),
        requires("mqtt-target"),
        env("MQTT_PASSWORD")
    )]
    mqtt_password: Option<Arc<str>>,

    #[serde(default = "default_values::discovery_topic")]
    #[arg(default_value = "homeassistant")]
    mqtt_discovery_topic: Arc<str>,

    #[arg(long)]
    #[serde(default)]
    mqtt_availability_topic: Option<Arc<str>>,

    /// Set this to dump memory changes to the specified MQTT topic as
    /// "{package_dump_mqtt_topic}/{client_id}".
    #[arg(long)]
    package_dump_mqtt_topic: Option<Arc<str>>,

    /// Set this to dump memory changes to the specified MQTT topic as
    /// "{memory_changes_mqtt_topic}/{changed_address}".
    #[arg(long)]
    memory_changes_mqtt_topic: Option<Arc<str>>,

    #[arg(skip)]
    #[serde(rename = "entities_json", default)]
    entities: Vec<JsonValue<mapping::GenericMapping>>,
}

impl Command {
    fn get() -> &'static Command {
        static ARGS: OnceLock<Command> = OnceLock::new();
        ARGS.get_or_init(|| {
            let config_file = "/data/options.json";
            if std::env::args_os().len() <= 1 {
                if let Ok(config_file) = std::fs::read(config_file) {
                    let loaded_config = Box::new(config_file);
                    let json = loaded_config.leak();
                    match serde_json::from_slice::<Command>(json) {
                        Ok(mut config) => {
                            return {
                                for entity in config.entities.iter_mut() {
                                    if let Err(err) = entity.leaking_parse() {
                                        eprintln!("Could not parse entity json: {err}");
                                        if let Some(cause) = err.source() {
                                            eprintln!("{cause}");
                                        }
                                        std::process::exit(1);
                                    }
                                }
                                config
                            }
                        }
                        Err(err) => {
                            eprintln!("Could not read config: {err}");
                            std::process::exit(1);
                        }
                    }
                }
            }
            Command::parse()
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No DNS match: {0}")]
    NoDnsMatch(Arc<str>),
    #[error("No reply from Spa")]
    NoReplyFromSpa,
    #[error("Spa error: {0}")]
    Spa(#[from] SpaError),
    #[error("Port forward error: {0}")]
    PortForward(#[from] PortForwardError),
    #[error("Port forward closed unexpectedly")]
    PortForwardClosed,
    #[error("Runtime error: {0}")]
    TokioJoinSet(#[from] tokio::task::JoinError),
    #[error("Invalid arguments: {0}")]
    InvalidArguments(&'static str),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Command::get();
    let mut mqtt = if let Some(target) = &args.mqtt_target {
        let mut mqtt_addrs = net::lookup_host(target.as_ref()).await?;
        let mqtt_addr = if let Some(addr) = mqtt_addrs.next() {
            Ok(addr)
        } else {
            Err(Error::NoDnsMatch(target.clone()))
        }?;
        let auth = match (args.mqtt_username.as_deref(), args.mqtt_password.as_deref()) {
            (Some(username), Some(password)) => MqttAuth::Simple { username, password },
            (None, None) => MqttAuth::None,
            (None, Some(_)) | (Some(_), None) => {
                return Err(Error::InvalidArguments(
                    "mqtt_username or mqtt_password neds to be both set or both unset",
                ))?
            }
        };
        let session = MqttSession {
            discovery_topic: args.mqtt_discovery_topic.clone(),
            availability_topic: args.mqtt_availability_topic.clone(),
            target: mqtt_addr,
            auth,
            keep_alive: 30,
        };
        Some(session.connect().await?)
    } else {
        None
    };
    let mut spa_addrs = net::lookup_host(args.spa_target.as_ref()).await?;
    let spa_addr = if let Some(addr) = spa_addrs.next() {
        Ok(addr)
    } else {
        Err(Error::NoDnsMatch(args.spa_target.clone()))
    }?;
    println!("Spa addr: {spa_addr}");
    let spa_pipe = FullPackagePipe::new();
    let forward_addr = args
        .spa_forward_listen_ip
        .as_ref()
        .map(|x| std::net::SocketAddr::new(*x, args.spa_forward_listen_port));
    let mut forward_builder = PortForwardBuilder {
        listen_addr: forward_addr,
        target_addr: spa_addr,
        handshake_timeout: Duration::from_secs(args.spa_handshake_timeout.into()),
        udp_timeout: Duration::from_secs(args.spa_udp_timeout.into()),
        verbose: args.verbose,
        package_dump_pipe: None,
        dump_traffic: args.dump_traffic,
        local_connection: args.spa_memory_size.map(|_| spa_pipe.forwarder),
    };
    enum JoinResult {
        SpaConnected(Arc<SpaConnection>),
    }
    let mut join_set = JoinSet::<anyhow::Result<JoinResult>>::new();
    match (&mut mqtt, &args.package_dump_mqtt_topic) {
        (None, Some(_)) => {
            return Err(Error::InvalidArguments(
                "package_dump_mqtt_topic requires a MQTT connection",
            ))?
        }
        (_, None) => (),
        (Some(mqtt), Some(dump_topic)) => {
            let mut mqtt_sender = mqtt.sender();
            let topic = dump_topic.clone();
            let mut package_pipe = forward_builder.dump_packages();
            join_set.spawn(async move {
                let mut recent_packages = VecDeque::with_capacity(10);
                loop {
                    let (direction, package) = package_pipe.recv().await?;
                    match package {
                        NetworkPackageData::Ping | NetworkPackageData::Pong => continue,
                        _ => (),
                    }
                    if recent_packages.contains(&package) {
                        continue;
                    }
                    if recent_packages.len() == recent_packages.capacity() {
                        recent_packages.pop_back();
                    }
                    let serde_json::Value::Object(mut package_json) =
                        serde_json::to_value(&package)?
                    else {
                        unreachable!()
                    };
                    let serde_json::Value::Object(direction_json) =
                        serde_json::to_value(&direction)?
                    else {
                        unreachable!()
                    };
                    for (key, value) in direction_json.into_iter() {
                        let previous = package_json.insert(key, value);
                        debug_assert!(previous.is_none(), "Dumped package accidentally modified");
                    }
                    let key = serde_json::to_vec(&package_json)?;
                    recent_packages.push_front(package);
                    let package = mqttrs::Packet::Publish(mqttrs::Publish {
                        dup: false,
                        qospid: mqttrs::QosPid::AtMostOnce,
                        retain: false,
                        topic_name: &topic,
                        payload: &key,
                    });
                    mqtt_sender.send(&package).await?;
                }
            });
        }
    };
    let forward = forward_builder.build().await?;
    join_set.spawn(async move {
        println!("Forwarding");
        forward.run().await?;
        println!("Stopping forward");
        Err(Error::PortForwardClosed)?
    });
    let spa = if let Some(memory_size) = args.spa_memory_size {
        join_set.spawn(async move {
            Ok(JoinResult::SpaConnected(Arc::new(
                timeout(
                    Duration::from_secs(5),
                    SpaConnection::new(memory_size, spa_pipe.spa),
                )
                .await
                .map_err(|_| Error::NoReplyFromSpa)??,
            )))
        });
        let Some(reply) = join_set.join_next().await else {
            unreachable!("The function above will return")
        };
        let JoinResult::SpaConnected(spa) = reply?? else {
            unreachable!("SpaConnected is the only possible reply from threads spawned before here")
        };
        Some(spa)
    } else {
        None
    };
    if let Some(mqtt) = &mqtt {
        mqtt.notify_online().await?;
    }
    match (&mut mqtt, &spa, &args.package_dump_mqtt_topic) {
        (Some(mqtt), Some(spa), Some(memory_change_topic)) => {
            let mut mqtt_sender = mqtt.sender();
            let mut key_presses = spa.subscribe_keypress();
            join_set.spawn(async move {
                loop {
                    let key = format!("{}", key_presses.recv().await?);
                    let package = mqttrs::Packet::Publish(mqttrs::Publish {
                        dup: false,
                        qospid: mqttrs::QosPid::AtMostOnce,
                        retain: false,
                        topic_name: memory_change_topic,
                        payload: key.as_bytes(),
                    });
                    mqtt_sender.send(&package).await?;
                }
            });
        }
        (None, _, Some(_)) | (_, None, Some(_)) => {
            return Err(Error::InvalidArguments(
                "key_press_mqtt_topic requires both mqtt and spa_memory_size to be set",
            ))?
        }
        (_, _, None) => (),
    }
    match (mqtt, &spa, &args.memory_changes_mqtt_topic) {
        (Some(mut mqtt), Some(spa), memory_change_topic) => {
            if let Some(memory_change_topic) = memory_change_topic {
                let mut mqtt_sender = mqtt.sender();
                let len = spa.len().await;
                let mut spa_data = spa.subscribe(0..len).await;
                let memory_change_topic = PathBuf::from(memory_change_topic.as_ref());
                join_set.spawn(async move {
                    let mut previous: Box<[u8]> = spa_data.borrow_and_update().as_ref().into();
                    let mut differences = Vec::with_capacity(len);
                    loop {
                        differences.clear();
                        {
                            spa_data.changed().await?;
                            let data = spa_data.borrow_and_update();
                            for i in 0..len {
                                if previous[i] != data[i] {
                                    differences.push((i, data[i]));
                                }
                            }
                            previous = data.as_ref().into();
                        }
                        for (position, value) in differences.iter() {
                            let payload = format!("{value}");
                            let topic_name = memory_change_topic.join(format!("{position}"));
                            let package = mqttrs::Packet::Publish(mqttrs::Publish {
                                dup: false,
                                qospid: mqttrs::QosPid::AtMostOnce,
                                retain: false,
                                topic_name: topic_name
                                    .to_str()
                                    .expect("All paths will be valid UTF-8"),
                                payload: payload.as_bytes(),
                            });
                            mqtt_sender.send(&package).await?;
                        }
                        #[cfg(debug_assertions)]
                        if args.verbose {
                            let differences: String = differences
                                .iter()
                                .map(|(i, d)| format!("{i}: {d}, "))
                                .collect();
                            println!("Differences: {}", differences);
                        }
                    }
                });
            }
            let spa_name = String::from_utf8_lossy(spa.name());
            let mut mapping = Mapping::new(home_assistant::ConfigureDevice {
                identifiers: Box::from([args.spa_id.clone()]),
                name: spa_name.into(),
            })?;
            for entity in &args.entities {
                mapping
                    .add_generic(entity.unwrap().clone(), &spa, &mut mqtt)
                    .await?;
            }
            join_set.spawn(async move {
                loop {
                    mapping.tick().await?;
                }
            });
            join_set.spawn(async move {
                loop {
                    mqtt.tick().await?;
                }
            });
        }
        (None, _, Some(_)) | (_, None, Some(_)) => {
            return Err(Error::InvalidArguments(
                "mqtt_memory_changes_topic requires both mqtt and spa_memory_size to be set",
            ))?
        }
        (_, _, None) => (),
    }
    if let Some(spa) = spa {
        // let spa_worker = spa.clone();
        join_set.spawn(async move {
            loop {
                spa.recv().await?;
            }
        });
    }
    while let Some(job) = join_set.join_next().await {
        job??;
    }
    Ok(())
}
