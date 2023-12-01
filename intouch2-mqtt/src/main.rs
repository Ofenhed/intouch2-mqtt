use clap::Parser;
use intouch2_mqtt::{
    home_assistant,
    mapping::{self, FanMapping, HardcodedMapping},
    mqtt_session::{MqttAuth, SessionBuilder as MqttSession},
    port_forward::{FullPackagePipe, PortForward, PortForwardError},
    spa::{SpaConnection, SpaError},
};
use std::{
    ffi::OsStr,
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

    pub fn r#false() -> bool {
        false
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
struct SpaOptions<'a> {
    #[serde(rename = "spa_target")]
    #[arg(long = "spa-target", id = "spa-target")]
    target: Arc<str>,
    #[serde(rename = "spa_unique_id", default = "default_values::spa_name")]
    #[arg(
        default_value = "spa_pool",
        short = 'n',
        long = "spa-unique-id",
        id = "spa-unique-id",
        alias = "name"
    )]
    unique_id: Arc<str>,
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
    #[arg(skip)]
    #[serde(borrow = "'a", flatten)]
    devices: Option<SpaDevices<'a>>,
}

#[derive(Deserialize, Default)]
struct SpaDevices<'a> {
    #[serde(borrow = "'a")]
    lights: Vec<mapping::LightMapping<'a>>,
    #[serde(borrow = "'a")]
    pumps: Vec<FanMapping<'a>>,
}

#[derive(clap::Args, Deserialize)]
struct MqttOptions {
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
    #[serde(borrow = "'a")]
    #[command(flatten)]
    spa: SpaOptions<'a>,
    #[serde(flatten)]
    #[command(flatten)]
    mqtt: MqttOptions,
    #[serde(default = "default_values::r#false")]
    #[arg(short, long)]
    verbose: bool,
    #[serde(default = "default_values::r#false")]
    #[arg(short, long)]
    dump_traffic: bool,
    #[arg(long)]
    memory_changes_topic: Option<Arc<str>>,
}

impl Command<'_> {
    fn get() -> &'static Command<'static> {
        static ARGS: OnceLock<Command> = OnceLock::new();
        ARGS.get_or_init(|| {
            let config_file = "/data/options.json";
            if std::env::args_os().len() <= 1 {
                if let Ok(config_file) = std::fs::read(config_file) {
                    let loaded_config = Box::new(config_file);
                    let json = loaded_config.leak();
                    match serde_json::from_slice::<Command>(json) {
                        Ok(config) => return config,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Command::get();
    let mqtt = if let Some(target) = &args.mqtt.target {
        let mut mqtt_addrs = net::lookup_host(target.as_ref()).await?;
        let mqtt_addr = if let Some(addr) = mqtt_addrs.next() {
            Ok(addr)
        } else {
            Err(Error::NoDnsMatch(target.clone()))
        }?;
        let auth = if let Some(auth) = &args.mqtt.auth {
            eprintln!("Trying to login with username {} and password {}", auth.username, auth.password.to_str().unwrap_or("NON-ASCII"));
            MqttAuth::Simple {
                username: &auth.username,
                password: &auth.password,
            }
        } else {
            MqttAuth::None
        };
        let session = MqttSession {
            discovery_topic: args.mqtt.prefix.clone(),
            target: mqtt_addr,
            auth,
            keep_alive: 30,
        };
        Some(session.connect().await?)
    } else {
        None
    };
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
        let spa_name = String::from_utf8_lossy(spa.name());
        let mut mapping = HardcodedMapping::new(home_assistant::ConfigureDevice {
            identifiers: Box::from([&*args.spa.unique_id]),
            name: spa_name.to_string(),
        })?;
        if let Some(ref devices) = args.spa.devices {
            let mut counter = 0;
            for light in &devices.lights {
                counter += 1;
                mapping
                    .add_light(&format!("light{counter}"), light.clone(), &spa, &mut mqtt)
                    .await?;
            }
            counter = 0;
            for pump in &devices.pumps {
                counter += 1;
                mapping
                    .add_pump(&format!("pump{counter}"), pump.clone(), &spa, &mut mqtt)
                    .await?;
            }
        }
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
