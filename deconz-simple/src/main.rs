use intouch2::{composer::*, object::*, parser::*};

use std::{
  env::args,
  net::UdpSocket,
  string::String,
  time::{Duration, Instant},
};

use reqwest;

use palette::{convert::FromColorUnclamped, Srgb, Yxy};

use rand::*;

use serde_json::{from_str, json, map::Map};

fn generate_uuid() -> Vec<u8> {
  let mut rng = rand::thread_rng();
  let characters = b"0123456789abcdef".to_vec();
  let hexed: Vec<u8> = [0; 32]
    .iter()
    .map(|_| characters[rng.gen_range(0..16)])
    .collect();
  [
    b"IOS",
    &hexed[0..8],
    b"-",
    &hexed[8..12],
    b"-",
    &hexed[12..16],
    b"-",
    &hexed[16..24],
    b"-",
    &hexed[24..32],
  ]
  .concat()
}

fn merge_json_if_not_defined(target: &mut Option<serde_json::Value>, extra: serde_json::Value) {
  match target {
    Some(target) => {
      for (key, group) in extra.as_object().unwrap().iter() {
        target[key] = group.clone();
      }
    }
    _ => {
      *target = Some(extra);
    }
  }
}

const GROUP_VALID_TIMEOUT: Duration = Duration::new(3600, 0);

struct DeconzClient {
  api_url: String,
  group_name: String,
  dark_group_name: String,
  last_temperature: Option<u8>,
  cache: Option<DeconzCache>,
}

struct DeconzCache {
  group_api_url: String,
  dark_group_api_url: String,
  group_fetched_at: Instant,
}

impl DeconzClient {
  pub fn make_deconz(
    deconz_host: String,
    api_key: String,
    group_name: String,
    dark_group_name: String,
  ) -> Self {
    DeconzClient {
      api_url: ["http://", &deconz_host, "/api/", &api_key, "/"].concat(),
      group_name,
      dark_group_name,
      cache: None,
      last_temperature: None,
    }
  }
  pub async fn update_cache(&mut self) -> Result<&DeconzCache, &'static str> {
    let (group_api_url, dark_group_api_url) = async {
      let client = reqwest::Client::new();
      let response = client
        .get(&[&self.api_url, "groups"].concat())
        .send()
        .await
        .unwrap();
      let groups =
        from_str(&response.text().await.unwrap_or("{}".to_string())).unwrap_or(json!(null));
      let empty_map = Map::new();
      let groups_object = groups.as_object().unwrap_or(&empty_map).iter();
      let (light, dark) = groups_object.fold((None, None), |(light, dark), (key, group)| {
        if group["name"].as_str() == Some(&self.group_name) {
          (Some(key), dark)
        } else if group["name"].as_str() == Some(&self.dark_group_name) {
          (light, Some(key))
        } else {
          (light, dark)
        }
      });
      println!("Found light groups {:?} and {:?}", light, dark);
      match (light, dark) {
        (Some(light), Some(dark)) => Ok((
          [&self.api_url, "groups/", light].concat(),
          [&self.api_url, "groups/", dark].concat(),
        )),
        _ => Err("No such group found"),
      }
    }
    .await?;
    self.cache = Some(DeconzCache {
      group_api_url,
      dark_group_api_url,
      group_fetched_at: Instant::now(),
    });
    Ok(self.cache.as_ref().unwrap())
  }
  pub async fn assure_cache_current<'a>(&'a mut self) -> Result<(), &'static str> {
    if let Some(ref cache) = self.cache {
      if cache.group_fetched_at + GROUP_VALID_TIMEOUT < Instant::now() {
        return Ok(());
      }
    }
    self.update_cache().await?;
    Ok(())
  }
  async fn push_status(&mut self, push_values: &PushStatusList) -> Result<(), &'static str> {
    use PushStatusKey::Keyed;
    self.assure_cache_current().await?;
    let (group_api_url, dark_group_api_url) = {
      let cache = self.cache.as_ref().expect("Could not read cache");
      (&cache.group_api_url, &cache.dark_group_api_url)
    };

    let mut request_object = None;
    let mut dark_request_object = None;
    if let (Some((red, green, blue, intencity)), _) = get_status_rgba(push_values) {
      let xy = Yxy::from_color_unclamped(
        Srgb::new(
          red as f32 / std::u8::MAX as f32,
          green as f32 / std::u8::MAX as f32,
          blue as f32 / std::u8::MAX as f32,
        )
        .into_linear(),
      );
      println!(
        "Got red/green/blue {}/{}/{} or x/y {}/{} ({:?})",
        red, green, blue, xy.x, xy.y, push_values
      );
      let map = json!({
        "xy": [xy.x, xy.y],
        "bri": intencity,
      });
      merge_json_if_not_defined(&mut request_object, map);
    }
    if let Some(temp) = get_temperature(push_values, self.last_temperature) {
      println!("Got new temperature {:?}", temp);
      if let Temperature::Celcius(temp) = temp {
        self.last_temperature = Some(temp);
      }
    }
    if let Some((x, _)) = push_values.get(&Keyed(PushStatusIndex::ColorType)) {
      use intouch2::object::StatusColorsType::*;
      match FromPrimitive::from_u8(*x) as Option<StatusColorsType> {
        Some(Solid) => merge_json_if_not_defined(&mut request_object, json!({"on": true})),
        Some(SlowFade) => merge_json_if_not_defined(
          &mut request_object,
          json!({"effect": "colorloop", "colorloopspeed": 150}),
        ),
        Some(FastFade) => merge_json_if_not_defined(
          &mut request_object,
          json!({"effect": "colorloop", "colorloopspeed": 5}),
        ),
        Some(Off) => merge_json_if_not_defined(&mut request_object, json!({"on": false})),
        None => {}
      }
    } else if let Some((0, _)) = push_values.get(&Keyed(PushStatusIndex::LightOnTimer)) {
      merge_json_if_not_defined(&mut request_object, json!({"on": false}));
    }
    if let Some((fountain_on, _)) = push_values.get(&Keyed(PushStatusIndex::Fountain)) {
      dark_request_object = Some(json!({ "on": *fountain_on == 0 }));
    }
    let current_group_api_url = group_api_url.to_owned();
    let current_dark_group_api_url = dark_group_api_url.to_owned();
    let client = reqwest::Client::new();
    println!(
      "Objects are {:?} and {:?}",
      request_object, dark_request_object
    );
    if let Some(request_object) = request_object {
      println!("Sending command {}", &request_object.to_string());
      let response = client
        .put(&[&current_group_api_url, "/action"].concat())
        .body(request_object.to_string())
        .send()
        .await;
      println!("{:?}", response);
      if let Ok(response) = response {
        let _ = println!(
          "Response for matching light {}: {}",
          response.status(),
          response
            .text()
            .await
            .unwrap_or("NO RESPONSE RECEIVER".to_string())
        );
      }
    };
    if let Some(dark_request_object) = dark_request_object {
      println!("Sending command {}", dark_request_object.to_string());
      let response = client
        .put(&[&current_dark_group_api_url, "/action"].concat())
        .body(dark_request_object.to_string())
        .send()
        .await;
      println!("{:?}", response);
      if let Ok(response) = response {
        println!(
          "Response for dark light {}: {}",
          response.status(),
          response
            .text()
            .await
            .unwrap_or("NO RESPONSE RECEIVER".to_string())
        );
      }
    };
    Ok(())
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), std::io::Error> {
  let mut buf = [0; 4096];
  let mut args: Vec<_> = args().collect();
  if args.len() != 6 {
    println!("Usage: {} spa-target deconz-target deconz-api-key deconz-matched-group-name deconz-dark-group-name", args[0]);
    return Ok(());
  }
  let socket = UdpSocket::bind("0.0.0.0:0")?;
  let deconz_dark_group_name = args.remove(5);
  let deconz_group_name = args.remove(4);
  let api_key = args.remove(3);
  let deconz_host = args.remove(2);
  let mut deconz_client = DeconzClient::make_deconz(
    deconz_host,
    api_key,
    deconz_group_name,
    deconz_dark_group_name,
  );
  socket.set_read_timeout(Some(Duration::new(3, 0)))?;
  socket.set_broadcast(true)?;
  socket.send_to(
    compose_network_data(&NetworkPackage::Hello(b"1".to_vec())).as_slice(),
    &args[1],
  )?;

  let (len, remote) = socket.recv_from(&mut buf)?;
  socket.set_broadcast(false)?;
  socket.connect(remote)?;

  if let Ok(([], NetworkPackage::Hello(receiver))) = parse_network_data(&buf[0..len]) {
    let (receiver, name) = {
      let pos = receiver
        .iter()
        .position(|x| *x == '|' as u8)
        .unwrap_or(receiver.len());
      let (before, after) = (&receiver[0..pos], &receiver[pos + 1..]);
      (before.to_vec(), after)
    };
    let key = generate_uuid();
    socket.send(compose_network_data(&NetworkPackage::Hello(key.clone())).as_slice())?;
    socket.send(
      compose_network_data(&NetworkPackage::Authorized {
        src: Some(key.clone()),
        dst: Some(receiver.clone()),
        data: NetworkPackageData::GetVersion,
      })
      .as_slice(),
    )?;
    let len = socket.recv(&mut buf)?;
    if let Ok((
      [],
      NetworkPackage::Authorized {
        src: _,
        dst: _,
        data: NetworkPackageData::Version(x),
      },
    )) = parse_network_data(&buf[0..len])
    {
      println!(
        "Connected to {}, got version {:?}",
        String::from_utf8_lossy(name),
        x
      );
      let ping_timeout = Duration::new(3, 0);
      let mut next_ping = Instant::now();
      let mut unanswered_pings = 0;
      let max_unanswered_pings = 10;
      loop {
        if next_ping <= Instant::now() {
          next_ping = Instant::now() + ping_timeout;
          socket.send(
            compose_network_data(&NetworkPackage::Authorized {
              src: Some(key.clone()),
              dst: Some(receiver.clone()),
              data: NetworkPackageData::Ping,
            })
            .as_slice(),
          )?;
          if unanswered_pings >= max_unanswered_pings {
            lost_contact(LostContactReason::MissingPong);
          }
          if unanswered_pings > 2 {
            println!("Missing {} ping responses", unanswered_pings);
          }
          unanswered_pings = unanswered_pings + 1;
        }
        socket.set_read_timeout(Some(std::cmp::max(
          Duration::new(0, 10),
          next_ping - Instant::now(),
        )))?;
        if let Ok(len) = socket.recv(&mut buf) {
          match parse_network_data(&buf[0..len]) {
            Ok((
              [],
              NetworkPackage::Authorized {
                src: _,
                dst: _,
                data: NetworkPackageData::Pong,
              },
            )) => unanswered_pings = 0,
            Ok((
              [],
              NetworkPackage::Authorized {
                src: x,
                dst: y,
                data: NetworkPackageData::PushStatus(data),
              },
            )) => {
              socket.send(
                compose_network_data(&NetworkPackage::Authorized {
                  src: Some(key.clone()),
                  dst: Some(receiver.clone()),
                  data: NetworkPackageData::PushStatusAck,
                })
                .as_slice(),
              )?;
              deconz_client.push_status(&data).await.map_err(|e| {
                std::io::Error::new(
                  std::io::ErrorKind::Other,
                  format!("Push status error: {}", e),
                )
              })?;
              #[cfg(debug_assertions)]
              {
                println!("Got status {:?}", &data);
                let recomposed = compose_network_data(&NetworkPackage::Authorized {
                  src: x,
                  dst: y,
                  data: NetworkPackageData::PushStatus(data.clone()),
                });
                if recomposed == buf[0..len].to_vec() {
                  println!("Same recomposed");
                } else {
                  println!("Recomposed differ!");
                  println!("{:?}", &buf[0..len]);
                  println!("{:?}", recomposed);
                }
              }
            }
            Ok((
              [],
              NetworkPackage::Authorized {
                src: _,
                dst: _,
                data: NetworkPackageData::Packs,
              },
            )) => {
              socket.send(
                compose_network_data(&NetworkPackage::Authorized {
                  src: Some(key.clone()),
                  dst: Some(receiver.clone()),
                  data: NetworkPackageData::PushStatusAck,
                })
                .as_slice(),
              )?;
            }
            Ok((
              [],
              NetworkPackage::Authorized {
                src: _,
                dst: _,
                data: NetworkPackageData::Error(ErrorType::Radio),
              },
            )) => lost_contact(LostContactReason::RadioError),
            Ok((
              [],
              NetworkPackage::Authorized {
                src: _,
                dst: _,
                data: NetworkPackageData::Unknown(x),
              },
            )) => {
              #[cfg(debug_assertions)]
              println!(
                "Got payload \"{}\" {:?}",
                String::from_utf8_lossy(x.as_slice()),
                &x
              );
            }
            _ => {
              #[cfg(debug_assertions)]
              println!("Unknown error, I guess")
            }
          }
        }
      }
    }
  }
  Ok(())
}
