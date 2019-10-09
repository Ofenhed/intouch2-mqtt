#[macro_use]
extern crate num_derive;

mod intouch2;
extern crate palette;
extern crate rand;

use intouch2::object::*;
use intouch2::parser::*;
use intouch2::composer::*;

use std::net::UdpSocket;
use std::env::args;
use std::time::Duration;
use std::time::Instant;
use std::string::String;

use std::thread::spawn;

use palette::{Yxy,Srgb};

use rand::*;

use json::{object,array};

use num_traits::FromPrimitive;

fn generate_uuid() -> Vec<u8> {
  let mut rng = rand::thread_rng();
  let characters = b"0123456789abcdef".to_vec();
  let hexed: Vec<u8> = [0;32].iter().map(|_| characters[rng.gen_range(0, 16)]).collect();
  [b"IOS", &hexed[0..8], b"-", &hexed[8..12], b"-", &hexed[12..16], b"-", &hexed[16..24], b"-", &hexed[24..32]].concat()
}

fn merge_json_if_not_defined(target: &mut Option<json::JsonValue>, extra: json::JsonValue) {
  match target {
    Some(target) => {
      for (key, group) in extra.entries() {
        target[key] = group.clone();
      }
    },
    _ => { *target = Some(extra); }
  }
}

fn make_deconz(deconz_host: String, api_key: String, group_name: String, dark_group_name: String) -> Result<impl FnMut(&PushStatusList) -> (), &'static str> {
  use PushStatusKey::Keyed;
  let api_url = ["http://", &deconz_host, "/api/", &api_key, "/"].concat();
  let group_number_valid = Duration::new(3600, 0);
  let get_group_apis = move || {
    let client = reqwest::Client::new();
    let mut response = client.get(&[&api_url, "groups"].concat()).send().unwrap();
    let groups = json::parse(&response.text().unwrap_or("{}".to_string())).unwrap();
    let (light, dark) = groups.entries().fold((None, None), |(light, dark), (key, group)| { if group["name"].to_string() == group_name { (Some(key), dark) }
                                                                                       else if group["name"].to_string() == dark_group_name { (light, Some(key)) }
                                                                                       else { (light, dark) } });
    println!("Found light groups {:?} and {:?}", light, dark);
    match (light, dark) {
      (Some(light), Some(dark)) => Ok(([&api_url, "groups/", light].concat(), [&api_url, "groups/", dark].concat())),
      _ => Err("No such group found"),
    }
  };
  get_group_apis().map(|(group_api, dark_group_api)| {
    let mut group_api_url = group_api;
    let mut dark_group_api_url = dark_group_api;
    let mut group_fetched_at = Instant::now();
    move |push_values: &_| {
      if group_fetched_at + group_number_valid < Instant::now() {
        if let Ok((new_group_api, new_dark_group_api)) = get_group_apis() {
          group_api_url = new_group_api;
          dark_group_api_url = new_dark_group_api;
          group_fetched_at = Instant::now();
        }
      }
      let mut request_object = None;
      let mut dark_request_object = None;
      if let (Some((red, green, blue, intencity)), _) = get_status_rgba(push_values) {
        let xy = Yxy::from(Srgb::new(red as f32 / std::u8::MAX as f32,
                                     green as f32 / std::u8::MAX as f32,
                                     blue as f32 / std::u8::MAX as f32).into_linear());
        println!("Got red/green/blue {}/{}/{} or x/y {}/{} ({:?})", red, green, blue, xy.x, xy.y, push_values);
        let map = object!{
          "xy" => array![xy.x, xy.y],
          "bri" => intencity,
        };
        merge_json_if_not_defined(&mut request_object, map);
      }
      if let Some((x, _)) = push_values.get(&Keyed(PushStatusIndex::ColorType)) {
        use intouch2::object::StatusColorsType::*;
        match FromPrimitive::from_u8(*x) as Option<StatusColorsType> {
          Some(Solid) => merge_json_if_not_defined(&mut request_object, object!{"on" => true}),
          Some(SlowFade) => merge_json_if_not_defined(&mut request_object, object!{"on" => true, "effect" => "colorloop", "colorloopspeed" => 150}),
          Some(FastFade) => merge_json_if_not_defined(&mut request_object, object!{"on" => true, "effect" => "colorloop", "colorloopspeed" => 5}),
          Some(Off) => merge_json_if_not_defined(&mut request_object, object!{"on" => false}),
          None => {},
        }
      } else if let Some((0, _)) = push_values.get(&Keyed(PushStatusIndex::LightOnTimer)) {
        merge_json_if_not_defined(&mut request_object, object!{"on" => false});
      }
      if let Some((fountain_on, _)) = push_values.get(&Keyed(PushStatusIndex::Fountain)) {
        dark_request_object = Some(object!{ "on" => *fountain_on == 0 });
      }
      let current_group_api_url = group_api_url.clone();
      let current_dark_group_api_url = dark_group_api_url.clone();
      spawn(move || {
        let client = reqwest::Client::new();
        println!("Objects are {:?} and {:?}", request_object, dark_request_object);
        if let Some(request_object) = request_object {
          println!("Sending command {}", &request_object.dump());
          let response = client.put(&[&current_group_api_url, "/action"].concat()).body(request_object.dump()).send();
          println!("{:?}", response);
          let _ = response.map(|mut x| println!("Response for matching light {}: {}", x.status(), x.text().unwrap_or("NO RESPONSE RECEIVER".to_string())));
        };
        if let Some(dark_request_object) = dark_request_object {
          println!("Sending command {}", dark_request_object.dump());
          let response = client.put(&[&current_dark_group_api_url, "/action"].concat()).body(dark_request_object.dump()).send();
          println!("{:?}", response);
          let _ = response.map(|mut x| println!("Response for dark light {}: {}", x.status(), x.text().unwrap_or("NO RESPONSE RECEIVER".to_string())));
        };
      });
    }
  })
}

fn main() -> Result<(), std::io::Error> {
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
  let mut deconz_client = make_deconz(deconz_host, api_key, deconz_group_name, deconz_dark_group_name).unwrap();
  socket.set_read_timeout(Some(Duration::new(3, 0)))?;
  socket.set_broadcast(true)?;
  socket.send_to(compose_network_data(&NetworkPackage::Hello(b"1".to_vec())).as_slice(), &args[1])?;

  let (len, remote) = socket.recv_from(& mut buf)?;
  socket.set_broadcast(false)?;
  socket.connect(remote)?;

  if let Ok(([], NetworkPackage::Hello(receiver))) = parse_network_data(&buf[0..len]) {
    let (receiver, name) = {
      let pos = receiver.iter().position(|x| *x == '|' as u8).unwrap_or(receiver.len());
      let (before, after) = (&receiver[0..pos], &receiver[pos+1..]);
      (before.to_vec(), after)
    };
    let key = generate_uuid();
    socket.send(compose_network_data(&NetworkPackage::Hello(key.clone())).as_slice())?;
    socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::GetVersion}).as_slice())?;
    let len = socket.recv(&mut buf)?;
    if let Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Version(x)})) = parse_network_data(&buf[0..len]) {
      println!("Connected to {}, got version {:?}", String::from_utf8_lossy(name), x);
      let ping_timeout = Duration::new(3, 0);
      let mut next_ping = Instant::now();
      let mut unanswered_pings = 0;
      let max_unanswered_pings = 10;
      loop {
        if next_ping <= Instant::now() {
          next_ping = Instant::now() + ping_timeout;
          socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::Ping}).as_slice())?;
          if unanswered_pings >= max_unanswered_pings {
            println!("Spa disconnected");
            ::std::process::exit(66);
          }
          unanswered_pings = unanswered_pings + 1;
        }
        socket.set_read_timeout(Some(std::cmp::max(Duration::new(0, 10), next_ping - Instant::now())))?;
        if let Ok(len) = socket.recv(&mut buf) {
          match parse_network_data(&buf[0..len]) {
            Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Pong})) => unanswered_pings = 0,
            Ok(([], NetworkPackage::Authorized{src: x, dst: y, data: NetworkPackageData::PushStatus(data)})) => { 
              socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::PushStatusAck}).as_slice())?;
              deconz_client(&data);
              #[cfg(debug_assertions)] {
                println!("Got status {:?}", &data);
                let recomposed = compose_network_data(&NetworkPackage::Authorized{src: x, dst: y, data: NetworkPackageData::PushStatus(data.clone())});
                if recomposed == buf[0..len].to_vec() {
                  println!("Same recomposed");
                } else {
                  println!("Recomposed differ!");
                  println!("{:?}", &buf[0..len]);
                  println!("{:?}", recomposed);
                }
              }
            },
            Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Packs})) => { socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::PushStatusAck}).as_slice())?; }
            Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Unknown(x)})) => { #[cfg(debug_assertions)] println!("Got payload \"{}\" {:?}", String::from_utf8_lossy(x.as_slice()), &x); },
            _ => { #[cfg(debug_assertions)] println!("Unknown error, I guess") },
          }
          
        }
      }
    }
  }
  Ok(())
}
