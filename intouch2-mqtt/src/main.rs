use clap::Parser;
use intouch2_mqtt::{port_forward::PortForward, spa::SpaConnection, WithBuffer};

use std::{
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

  pub fn forward_timeout() -> u16 {
    300
  }

  pub fn handshake_timeout() -> u16 {
    10
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
  #[serde(
    rename = "forward_timeout",
    default = "default_values::forward_timeout"
  )]
  #[arg(
    long = "spa-forward-timeout",
    id = "spa-forward-timeout",
    default_value = "300",
    alias = "forward-timeout"
  )]
  udp_timeout: u16,
  #[serde(
    rename = "forward_handshake_timeout",
    default = "default_values::handshake_timeout"
  )]
  #[arg(
    long = "spa-forward-handshake-timeout",
    id = "spa-forward-handshake-timeout",
    default_value = "10",
    alias = "forward-handshake-timeout"
  )]
  handshake_timeout: u16,
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
  #[serde(flatten)]
  #[command(flatten)]
  forward: Option<SpaForward>,
}

#[derive(clap::Args, Deserialize)]
struct MqttOptions {
  #[serde(rename = "mqtt_target")]
  #[arg(long = "mqtt-target", id = "mqtt-target", required = false)]
  target: Arc<str>,
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
    requires("mqtt-target")
  )]
  username: Arc<str>,
  #[serde(rename = "mqtt_password")]
  #[arg(
    short = 'p',
    long = "mqtt-password",
    id = "mqtt-password",
    required = false,
    requires("mqtt-username"),
    requires("mqtt-target")
  )]
  password: Arc<str>,
}

#[derive(Parser, Deserialize)]
struct Command {
  #[serde(flatten)]
  #[command(flatten)]
  spa: SpaOptions,
  #[serde(flatten)]
  #[command(flatten)]
  mqtt: Option<MqttOptions>,
  #[arg(short, long)]
  verbose: bool,
  #[arg(short, long)]
  dump_traffic: bool,
}

impl Command {
  fn get() -> &'static Command {
    static ARGS: OnceLock<Command> = OnceLock::new();
    ARGS.get_or_init(|| {
      if std::env::args_os().len() <= 1 {
        todo!("Read args from /data/options.json");
      } else {
        Command::parse()
      }
    })
  }
}

fn spa_state() -> &'static RwLock<intouch2::datas::GeckoDatas> {
  static DATA: OnceLock<RwLock<intouch2::datas::GeckoDatas>> = OnceLock::new();
  DATA.get_or_init(Default::default)
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
}

async fn with_mqtt(_mqtt: ()) -> anyhow::Result<()> {
  let args = Command::get();
  let mut spa_addrs = net::lookup_host(args.spa.target.as_ref()).await?;
  let spa_addr = if let Some(addr) = spa_addrs.next() {
    Ok(addr)
  } else {
    Err(Error::NoDnsMatch(args.spa.target.clone()))
  }?;
  println!("Spa addr: {spa_addr}");
  let mut join_set = JoinSet::<anyhow::Result<()>>::new();
  // let spa = Arc::new(timeout(Duration::from_secs(5),
  // SpaConnection::new(&spa_addr)).await.map_err(|_| Error::NoReplyFromSpa)??);
  // join_set.spawn(async move {
  //    let mut buf = Box::new(SpaConnection::make_buffer());
  //    loop {
  //      if let Some(received) = spa.recv(&mut buf).await? {
  //          println!("Got {}", received);
  //      }
  //    }
  //});
  if let Some(forward) = &args.spa.forward {
    let forward = PortForward {
      source_addr: std::net::SocketAddr::new(forward.listen_ip, forward.listen_port),
      target_addr: spa_addr,
      handshake_timeout: Duration::from_secs(forward.handshake_timeout.into()),
      udp_timeout: Duration::from_secs(forward.udp_timeout.into()),
      verbose: args.verbose,
      dump_traffic: args.dump_traffic,
    };
    join_set.spawn(async move {
      forward.run().await?;
      Ok(())
    });
  };
  // tickers.insert(spa, |spa: &mut SpaConnection| {
  //    let mut spa_buffer = spa_buffer.clone();
  //    Box::new(async move {
  //      let mut spa_buffer = Arc::make_mut(&mut spa_buffer);
  //      _ = spa.recv(&mut spa_buffer).await?;
  //      Ok(())
  //    })
  //});
  // let mut forward_buffer = Arc::new(PortForward::make_buffer());
  // tickers.insert(forwarder, move |forwarder: &mut PortForward| {
  //    let mut forward_buffer = forward_buffer.clone();
  //    Box::new(async move {
  //        let mut spa_buffer = Arc::make_mut(&mut forward_buffer);
  //        _ = forwarder.tick(&mut spa_buffer).await?;
  //        Ok(())
  //    })
  //});
  while let Some(job) = join_set.join_next().await {
    job??
  }
  Ok(())
  // loop {
  //  select! {
  //      spa_received = spa.recv(&mut spa_buffer) => match spa_received? {
  //          None => (),
  //          Some(_msg) => (),
  //      },
  //      forward_status = forwarder.tick(&mut forward_buffer) => {
  //          forward_status?;
  //      }
  //      // TODO: Forward connections to the Spa
  //  }
  //}
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
  // TODO: Connect MQTT
  let result = with_mqtt(()).await;
  // TODO: Disconnect MQTT
  result?;
  Ok(())
}
