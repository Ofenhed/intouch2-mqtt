use clap::Parser;
use intouch2_mqtt::{
    home_assistant,
    mapping::{self, FanMapping, HardcodedMapping},
    mqtt_session::{MqttAuth, SessionBuilder as MqttSession},
    port_forward::{FullPackagePipe, PortForward, PortForwardError},
    spa::{SpaConnection, SpaError},
};
use mqttrs::Packet;

use std::{
    collections::HashMap,
    ffi::OsStr,
    net::IpAddr,
    sync::{Arc, OnceLock},
    time::Duration,
};

use serde::Deserialize;

use tokio::{
    net::{self},
    sync::RwLock,
    task::JoinSet,
    time::timeout,
};

// TODO: See https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
// Do NOT send a retained message, as it will be saved by mosquitto as a ghost device

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
}

#[derive(clap::Args, Deserialize)]
struct SpaForward {
    #[serde(rename = "forward_listen_ip")]
    #[arg(
        long = "spa-forward-listen-ip",
        id = "spa-forward-listen-ip",
        alias = "forward-ip",
        required = false
    )]
    listen_ip: IpAddr,
    #[serde(rename = "forward_listen_port", default = "default_values::spa_port")]
    #[arg(
        long = "spa-forward-listen-port",
        id = "spa-forward-listen-port",
        default_value = "10022",
        alias = "forward-port"
    )]
    listen_port: u16,
}

#[derive(clap::Args, Deserialize)]
struct SpaOptions {
    #[serde(rename = "spa_target")]
    #[arg(long = "spa-target", id = "spa-target")]
    target: Arc<str>,
    #[serde(rename = "spa_name", default = "default_values::spa_name")]
    #[arg(
        default_value = "spa_pool",
        short = 'n',
        long = "spa-name",
        id = "spa-name",
        alias = "name"
    )]
    name: Arc<str>,
    #[arg(long = "spa-memory-size", id = "spa-memory-size")]
    #[serde(rename = "spa_memory_size")]
    memory_size: usize,
    #[serde(flatten)]
    #[command(flatten)]
    forward: Option<SpaForward>,
    #[serde(rename = "udp_timeout", default = "default_values::udp_timeout")]
    #[arg(
        long = "spa-timeout",
        id = "spa-timeout",
        default_value = "300",
        alias = "forward-timeout"
    )]
    udp_timeout: u16,
    #[serde(
        rename = "handshake_timeout",
        default = "default_values::handshake_timeout"
    )]
    #[arg(
        long = "spa-handshake-timeout",
        id = "spa-handshake-timeout",
        default_value = "10",
        alias = "handshake-timeout"
    )]
    handshake_timeout: u16,
}

#[derive(Deserialize, Default)]
struct MqttDevices<'a> {
    #[serde(borrow = "'a")]
    lights: Vec<mapping::LightMapping<'a>>,
    #[serde(borrow = "'a")]
    pumps: Vec<FanMapping<'a>>,
}

#[derive(clap::Args, Deserialize)]
struct MqttOptions<'a> {
    #[serde(rename = "mqtt_target")]
    #[arg(long = "mqtt-target", id = "mqtt-target")]
    target: Option<Arc<str>>,
    #[serde(
        rename = "home_assistant_discovery_topic",
        default = "default_values::discovery_topic"
    )]
    #[arg(
        long = "discovery-topic",
        id = "discovery-topic",
        default_value = "homeassistant"
    )]
    prefix: Arc<str>,
    #[command(flatten)]
    #[serde(flatten)]
    auth: Option<MqttUser>,
    #[arg(skip)]
    #[serde(borrow = "'a")]
    devices: Option<MqttDevices<'a>>,
}

#[derive(clap::Args, Deserialize, Clone)]
struct MqttUser {
    #[serde(rename = "mqtt_username")]
    #[arg(
        short = 'u',
        long = "mqtt-username",
        id = "mqtt-username",
        required = false,
        requires("mqtt-password"),
        requires("mqtt-target"),
        env("MQTT_USER")
    )]
    username: Arc<str>,
    #[serde(rename = "mqtt_password")]
    #[arg(
        short = 'p',
        long = "mqtt-password",
        id = "mqtt-password",
        required = false,
        requires("mqtt-username"),
        requires("mqtt-target"),
        env("MQTT_PASSWORD")
    )]
    password: Arc<OsStr>,
}

#[derive(Parser, Deserialize)]
struct Command<'a> {
    #[serde(flatten)]
    #[command(flatten)]
    spa: SpaOptions,
    #[serde(flatten)]
    #[serde(borrow = "'a")]
    #[command(flatten)]
    mqtt: MqttOptions<'a>,
    #[arg(short, long)]
    verbose: bool,
    #[arg(short, long)]
    dump_traffic: bool,
    #[arg(long)]
    memory_changes_topic: Option<Arc<str>>,
}

impl Command<'_> {
    fn get() -> &'static Command<'static> {
        static ARGS: OnceLock<Command> = OnceLock::new();
        ARGS.get_or_init(|| {
            if std::env::args_os().len() <= 1 {
                todo!("Read args from /data/options.yaml");
            } else {
                Command::parse()
            }
        })
    }
}

#[derive(Eq, PartialEq, Debug)]
enum LostContactReason {
    MissingPong,
    RadioError,
}

fn lost_contact(reason: LostContactReason) {
    println!("Lost contact with spa. Reason: {:?}", reason);
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Command::get();
    let mut mqtt = if let Some(target) = &args.mqtt.target {
        let auth = if let Some(auth) = &args.mqtt.auth {
            MqttAuth::Simple {
                username: &auth.username,
                password: &auth.password,
            }
        } else {
            MqttAuth::None
        };
        let session = MqttSession {
            discovery_topic: args.mqtt.prefix.clone(),
            target: target.parse()?,
            auth,
            keep_alive: 30,
        };
        Some(session.connect().await?)
    } else {
        None
    };
    if let Some(ref mut mqtt) = mqtt {
        mqtt.add_test_device().await?;
    }
    let mut spa_addrs = net::lookup_host(args.spa.target.as_ref()).await?;
    let spa_addr = if let Some(addr) = spa_addrs.next() {
        Ok(addr)
    } else {
        Err(Error::NoDnsMatch(args.spa.target.clone()))
    }?;
    println!("Spa addr: {spa_addr}");
    let spa_pipe = FullPackagePipe::new();
    let forward_addr = args
        .spa
        .forward
        .as_ref()
        .map(|x| std::net::SocketAddr::new(x.listen_ip, x.listen_port));
    let forward = PortForward {
        listen_addr: forward_addr,
        target_addr: spa_addr,
        handshake_timeout: Duration::from_secs(args.spa.handshake_timeout.into()),
        udp_timeout: Duration::from_secs(args.spa.udp_timeout.into()),
        verbose: args.verbose,
        dump_traffic: args.dump_traffic,
        local_connection: Some(spa_pipe.forwarder),
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
    join_set.spawn(async move {
        Ok(JoinResult::SpaConnected(Arc::new(
            timeout(
                Duration::from_secs(5),
                SpaConnection::new(args.spa.memory_size, spa_pipe.spa),
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
    if let Some(mut mqtt) = mqtt {
        if let Some(memory_change_topic) = &args.memory_changes_topic {
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
        let mut mapping = HardcodedMapping::new(home_assistant::ConfigureDevice {
            identifiers: Box::new(["spa_unique_name_placeholder"]),
            name: &args.spa.name,
        })?;
        let primary = mapping::LightMapping {
            name: "Primary",
            onoff: mapping::EnumMapping {
                address: 601,
                len: 1,
                bitmap: None,
                mapping: HashMap::from([
                    (vec![0u8], b"OFF".into()),
                    (vec![1u8], b"ON".into()),
                    (vec![2u8], b"ON".into()),
                    (vec![5u8], b"ON".into()),
                ]),
            },
            rgb: Some(mapping::PlainMapping {
                address: 0x25c,
                len: 3,
            }),
            effects: Some(mapping::EnumMapping {
                address: 601,
                len: 1,
                bitmap: None,
                mapping: HashMap::from([
                    (vec![0u8], b"None".into()),
                    (vec![1u8], b"Slow Fade".into()),
                    (vec![2u8], b"Fast Fade".into()),
                    (vec![5u8], b"None".into()),
                ]),
            }),
        };
        let secondary = mapping::LightMapping {
            name: "Secondary",
            onoff: mapping::EnumMapping {
                address: 608,
                len: 1,
                bitmap: None,
                mapping: HashMap::from([
                    (vec![0u8], b"OFF".into()),
                    (vec![1u8], b"ON".into()),
                    (vec![2u8], b"ON".into()),
                    (vec![5u8], b"ON".into()),
                ]),
            },
            rgb: Some(mapping::PlainMapping {
                address: 0x263,
                len: 3,
            }),
            effects: Some(mapping::EnumMapping {
                address: 608,
                len: 1,
                bitmap: None,
                mapping: HashMap::from([
                    (vec![0u8], b"None".into()),
                    (vec![1u8], b"Slow Fade".into()),
                    (vec![2u8], b"Fast Fade".into()),
                    (vec![5u8], b"None".into()),
                ]),
            }),
        };
        mapping
            .add_light("primary", primary, &spa, &mut mqtt)
            .await?;
        mapping
            .add_light("secondary", secondary, &spa, &mut mqtt)
            .await?;
        let fountain = FanMapping {
            name: "Fountain",
            state_mapping: mapping::EnumMapping {
                address: 363,
                len: 1,
                bitmap: Some(vec![0x3]),
                mapping: HashMap::from([(vec![0u8], b"OFF".into()), (vec![1u8], b"ON".into())]),
            },
            percent_mapping: None,
        };
        let first = FanMapping {
            name: "Pump 1",
            state_mapping: mapping::EnumMapping {
                address: 259,
                len: 1,
                bitmap: Some(vec![0x3]),
                mapping: HashMap::from([
                    (vec![0u8], b"OFF".into()),
                    (vec![1u8], b"ON".into()),
                    (vec![2u8], b"ON".into()),
                ]),
            },
            percent_mapping: Some(mapping::EnumMapping {
                address: 259,
                bitmap: Some(vec![0x3]),
                len: 1,
                mapping: HashMap::from([
                    (vec![0u8], b"0".into()),
                    (vec![1u8], b"50".into()),
                    (vec![2u8], b"100".into()),
                ]),
            }),
        };
        let second = FanMapping {
            name: "Pump 2",
            state_mapping: mapping::EnumMapping {
                address: 261,
                len: 1,
                bitmap: Some(vec![0x4]),
                mapping: HashMap::from([(vec![0u8], b"OFF".into()), (vec![4u8], b"ON".into())]),
            },
            percent_mapping: None,
        };
        let third = FanMapping {
            name: "Pump 3",
            state_mapping: mapping::EnumMapping {
                address: 261,
                len: 1,
                bitmap: Some(vec![16]),
                mapping: HashMap::from([(vec![0u8], b"OFF".into()), (vec![16u8], b"ON".into())]),
            },
            percent_mapping: None,
        };
        mapping
            .add_pump("fountain", fountain, &spa, &mut mqtt)
            .await?;
        mapping.add_pump("first", first, &spa, &mut mqtt).await?;
        mapping.add_pump("second", second, &spa, &mut mqtt).await?;
        mapping.add_pump("third", third, &spa, &mut mqtt).await?;
        join_set.spawn(async move {
            loop {
                mapping.tick().await?;
            }
        });
        let mut mqtt_spy = mqtt.subscribe();
        join_set.spawn(async move {
            loop {
                let response = mqtt_spy.recv().await?;
                eprintln!("Got data: {:?}", response.packet);
            }
        });
        join_set.spawn(async move {
            loop {
                mqtt.tick().await?;
            }
        });
    }
    let spa_worker = spa.clone();
    join_set.spawn(async move {
        loop {
            spa_worker.recv().await?;
        }
    });
    while let Some(job) = join_set.join_next().await {
        job??;
    }
    Ok(())
}
