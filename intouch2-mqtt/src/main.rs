use intouch2::{composer::*, object::*, parser::*, generate_uuid};

use clap::{Parser, CommandFactory, Args};
use intouch2_mqtt::{spa::SpaConnection, port_forward::start_port_forward, WithBuffer};

use std::{
  env::args,
  string::String,
  sync::{Arc, OnceLock},
  time::{Duration, Instant},
  net::IpAddr,
};

use serde::Deserialize;

use tokio::{net::{UdpSocket, self}, sync::RwLock, select, time::timeout, task::JoinSet};

// TODO: See https://www.home-assistant.io/integrations/mqtt/#mqtt-discovery
// Do NOT send a retained message, as it will be saved by mosquitto as a ghost device

use reqwest;

use palette::{convert::FromColorUnclamped, Srgb, Yxy};

use serde_json::{from_str, json, map::Map};

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
    #[arg(long = "spa-forward-listen-ip", id = "spa-forward-listen-ip", alias = "forward-ip", required = false)]
    listen_ip: IpAddr,
    #[serde(rename = "forward_listen_port", default = "default_values::spa_port")]
    #[arg(long = "spa-forward-listen-port", id = "spa-forward-listen-port", default_value = "10022", alias = "forward-port")]
    listen_port: u16,
    #[serde(rename = "forward_timeout", default = "default_values::forward_timeout")]
    #[arg(long = "spa-forward-timeout", id = "spa-forward-timeout", default_value = "300", alias = "forward-timeout")]
    udp_timeout: u16,
    #[serde(rename = "forward_handshake_timeout", default = "default_values::handshake_timeout")]
    #[arg(long = "spa-forward-handshake-timeout", id = "spa-forward-handshake-timeout", default_value = "10", alias = "forward-handshake-timeout")]
    handshake_timeout: u16,
}

#[derive(clap::Args, Deserialize)]
struct SpaOptions {
    #[serde(rename = "spa_target")]
    #[arg(long = "spa-target", id = "spa-target")]
    target: Arc<str>,
    #[serde(rename = "spa_name", default = "default_values::spa_name")]
    #[arg(default_value = "spa_pool", short = 'n', long = "spa-name", id = "spa-name", alias = "name")]
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
    #[arg(short = 'u', long = "mqtt-username", id = "mqtt-username", required = false, requires("mqtt-password"), requires("mqtt-target"))]
    username: Arc<str>,
    #[serde(rename = "mqtt_password")]
    #[arg(short = 'p', long = "mqtt-password", id = "mqtt-password", required = false, requires("mqtt-username"), requires("mqtt-target"))]
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

//async fn connect_spa() -> Result<UdpSocket> {
//    {
//      println!(
//        "Connected to {}, got version {:?}",
//        String::from_utf8_lossy(name),
//        x
//      );
//      let ping_timeout = Duration::new(3, 0);
//      let mut next_ping = Instant::now();
//      let mut unanswered_pings = 0;
//      let max_unanswered_pings = 10;
//      loop {
//        if next_ping <= Instant::now() {
//          next_ping = Instant::now() + ping_timeout;
//          socket.send(
//            compose_network_data(&NetworkPackage::Authorized {
//              src: Some(key.clone()),
//              dst: Some(receiver.clone()),
//              data: NetworkPackageData::Ping,
//            })
//            .as_slice(),
//          ).await?;
//          if unanswered_pings >= max_unanswered_pings {
//            lost_contact(LostContactReason::MissingPong);
//          }
//          if unanswered_pings > 2 {
//            println!("Missing {} ping responses", unanswered_pings);
//          }
//          unanswered_pings = unanswered_pings + 1;
//        }
//        //socket.set_read_timeout(Some(std::cmp::max(
//        //  Duration::new(0, 10),
//        //  next_ping - Instant::now(),
//        //)))?;
//        if let Ok(len) = socket.recv(&mut buf).await {
//          match parse_network_data(&buf[0..len]) {
//            Ok((
//              [],
//              NetworkPackage::Authorized {
//                src: _,
//                dst: _,
//                data: NetworkPackageData::Pong,
//              },
//            )) => unanswered_pings = 0,
//            Ok((
//              [],
//              NetworkPackage::Authorized {
//                src: x,
//                dst: y,
//                data: NetworkPackageData::PushStatus(data),
//              },
//            )) => {
//              socket.send(
//                compose_network_data(&NetworkPackage::Authorized {
//                  src: Some(key.clone()),
//                  dst: Some(receiver.clone()),
//                  data: NetworkPackageData::PushStatusAck,
//                })
//                .as_slice(),
//              ).await?;
//              // TODO: Push status to MQTT here
//              #[cfg(debug_assertions)]
//              {
//                println!("Got status {:?}", &data);
//                let recomposed = compose_network_data(&NetworkPackage::Authorized {
//                  src: x,
//                  dst: y,
//                  data: NetworkPackageData::PushStatus(data.clone()),
//                });
//                if recomposed == buf[0..len].to_vec() {
//                  println!("Same recomposed");
//                } else {
//                  println!("Recomposed differ!");
//                  println!("{:?}", &buf[0..len]);
//                  println!("{:?}", recomposed);
//                }
//              }
//            }
//            Ok((
//              [],
//              NetworkPackage::Authorized {
//                src: _,
//                dst: _,
//                data: NetworkPackageData::Packs,
//              },
//            )) => {
//              socket.send(
//                compose_network_data(&NetworkPackage::Authorized {
//                  src: Some(key.clone()),
//                  dst: Some(receiver.clone()),
//                  data: NetworkPackageData::PushStatusAck,
//                })
//                .as_slice(),
//              ).await?;
//            }
//            Ok((
//              [],
//              NetworkPackage::Authorized {
//                src: _,
//                dst: _,
//                data: NetworkPackageData::Error(ErrorType::Radio),
//              },
//            )) => lost_contact(LostContactReason::RadioError),
//            Ok((
//              [],
//              NetworkPackage::Authorized {
//                src: _,
//                dst: _,
//                data: NetworkPackageData::Unknown(x),
//              },
//            )) => {
//              #[cfg(debug_assertions)]
//              println!(
//                "Got payload \"{}\" {:?}",
//                String::from_utf8_lossy(x.as_slice()),
//                &x
//              );
//            }
//            _ => {
//              #[cfg(debug_assertions)]
//              println!("Unknown error, I guess")
//            }
//          }
//        }
//      }
//    }
//  }
//  Ok(())
//}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("IO Error: {0}")]
    Io(#[from] std::io::Error),
    #[error("No DNS match: {0}")]
    NoDnsMatch(Arc<str>),
    #[error("No reply from Spa")]
    NoReplyFromSpa,
}

async fn with_mqtt(mqtt: ()) -> anyhow::Result<()> {
  let args = Command::get();
  let mut spa_addrs = net::lookup_host(args.spa.target.as_ref()).await?;
  let spa_addr = if let Some(addr) = spa_addrs.next() {
      Ok(addr)
  } else {
      Err(Error::NoDnsMatch(args.spa.target.clone()))
  }?;
  println!("Spa addr: {spa_addr}");
  //let spa = timeout(Duration::from_secs(5), SpaConnection::new(&spa_addr)).await.map_err(|_| Error::NoReplyFromSpa)??;
  let mut join_set = JoinSet::<anyhow::Result<()>>::new();
  if let Some(forward) = &args.spa.forward {
    join_set.spawn(async move {
        start_port_forward(std::net::SocketAddr::new(forward.listen_ip, forward.listen_port), spa_addr, Duration::from_secs(forward.handshake_timeout.into()), Duration::from_secs(forward.udp_timeout.into())).await?;
        Ok(())
    });
  };
  let mut spa_buffer = Arc::new(SpaConnection::make_buffer());
  //tickers.insert(spa, |spa: &mut SpaConnection| {
  //    let mut spa_buffer = spa_buffer.clone();
  //    Box::new(async move {
  //      let mut spa_buffer = Arc::make_mut(&mut spa_buffer);
  //      _ = spa.recv(&mut spa_buffer).await?;
  //      Ok(())
  //    })
  //});
  //let mut forward_buffer = Arc::new(PortForward::make_buffer());
  //tickers.insert(forwarder, move |forwarder: &mut PortForward| {
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
  //loop {
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
