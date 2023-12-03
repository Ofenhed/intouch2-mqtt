use clap::Parser;
use intouch2_mqtt::{
    home_assistant,
    mapping::{self, Mapping},
    mqtt_session::{MqttAuth, SessionBuilder as MqttSession},
    port_forward::{FullPackagePipe, PortForward, PortForwardError},
    spa::{SpaConnection, SpaError},
};
use std::{
    net::IpAddr,
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

    fn leaking_parse(&mut self) -> Result<(), serde_json::error::Error> {
        let raw_value = {
            let JsonValue::Raw(raw_value) = self else {
                panic!("leaking_parse can only be used on raw JsonValue")
            };
            Box::leak(Box::from(raw_value.as_ref()))
        };
        eprintln!("Parsing JSON {raw_value}");
        let parsed = serde_json::from_str(raw_value)?;
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

    /// Set this to dump memory changes to the specified MQTT topic as
    /// "{memory_changes_mqtt_target}/{changed_address}".
    #[arg(long)]
    memory_changes_mqtt_target: Option<Arc<str>>,

    /// A mapping of all lights which should be mapped from the Spa to MQTT.
    #[arg(skip)]
    #[serde(rename = "lights_json")]
    lights: Vec<JsonValue<mapping::LightMapping<'static>>>,

    /// A mapping of all pumps which should be mapped from the Spa to MQTT.
    #[arg(skip)]
    #[serde(rename = "pumps_json")]
    pumps: Vec<JsonValue<mapping::FanMapping<'static>>>,
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
                    println!("Loading config {}", String::from_utf8_lossy(json));
                    match serde_json::from_slice::<Command>(json) {
                        Ok(mut config) => {
                            return {
                                let lights_result: Result<(), serde_json::error::Error> = config
                                    .lights
                                    .iter_mut()
                                    .map(JsonValue::leaking_parse)
                                    .collect();
                                if let Err(err) = lights_result {
                                    eprintln!("Could not parse light: {err}");
                                    std::process::exit(1);
                                }
                                let pumps_result: Result<(), serde_json::error::Error> = config
                                    .pumps
                                    .iter_mut()
                                    .map(JsonValue::leaking_parse)
                                    .collect();
                                if let Err(err) = pumps_result {
                                    eprintln!("Could not parse pump: {err}");
                                    std::process::exit(1);
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
    eprintln!("Args: {args:?}");
    let mqtt = if let Some(target) = &args.mqtt_target {
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
    let forward = PortForward {
        listen_addr: forward_addr,
        target_addr: spa_addr,
        handshake_timeout: Duration::from_secs(args.spa_handshake_timeout.into()),
        udp_timeout: Duration::from_secs(args.spa_udp_timeout.into()),
        verbose: args.verbose,
        dump_traffic: args.dump_traffic,
        local_connection: args.spa_memory_size.map(|_| spa_pipe.forwarder),
    };
    enum JoinResult {
        SpaConnected(Arc<SpaConnection>),
    }
    let mut join_set = JoinSet::<anyhow::Result<JoinResult>>::new();
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
    match (mqtt, &spa, &args.memory_changes_mqtt_target) {
        (Some(mut mqtt), Some(spa), memory_change_topic) => {
            if let Some(memory_change_topic) = memory_change_topic {
                let mut mqtt_sender = mqtt.sender();
                let len = spa.len().await;
                let mut spa_data = spa.subscribe(0..len).await;
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
                            let topic_name = format!("{memory_change_topic}/{position}");
                            let package = mqttrs::Packet::Publish(mqttrs::Publish {
                                dup: false,
                                qospid: mqttrs::QosPid::AtMostOnce,
                                retain: false,
                                topic_name: &topic_name,
                                payload: payload.as_bytes(),
                            });
                            mqtt_sender.send(&package).await?;
                        }
                        #[cfg(debug_assertions)]
                        {
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
                identifiers: Box::from([&*args.spa_id]),
                name: spa_name.to_string(),
            })?;
            let mut counter = 0;
            for light in &args.lights {
                counter += 1;
                mapping
                    .add_light(
                        &format!("light{counter}"),
                        light.unwrap().clone(),
                        &spa,
                        &mut mqtt,
                    )
                    .await?;
            }
            counter = 0;
            for pump in &args.pumps {
                counter += 1;
                mapping
                    .add_pump(
                        &format!("pump{counter}"),
                        pump.unwrap().clone(),
                        &spa,
                        &mut mqtt,
                    )
                    .await?;
            }
            join_set.spawn(async move {
                loop {
                    mapping.tick().await?;
                }
            });
            // let mut mqtt_spy = mqtt.subscribe();
            // join_set.spawn(async move {
            //    loop {
            //        let response = mqtt_spy.recv().await?;
            //        eprintln!("Got data: {:?}", response.packet);
            //    }
            //});
            join_set.spawn(async move {
                loop {
                    mqtt.tick().await?;
                }
            });
        }
        (None, Some(_), Some(_)) => {
            return Err(Error::InvalidArguments(
                "mqtt_memory_changes_topic requres mqtt to be set",
            ))?
        }
        (Some(_), None, _) => {
            return Err(Error::InvalidArguments(
                "mqtt requres a known spa_memory_size",
            ))?
        }
        (_, None, Some(_)) => {
            return Err(Error::InvalidArguments(
                "mqtt_memory_changes_topic requres a known spa_memory_size",
            ))?
        }
        (None, _, None) => (),
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
