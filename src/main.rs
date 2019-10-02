mod network_package;
extern crate palette;
extern crate rand;

use network_package::object::NetworkPackage;
use network_package::object::NetworkPackageData;
use network_package::object::PushStatusValue;
use network_package::parser::*;
use network_package::composer::*;

use std::net::UdpSocket;
use std::env::args;
use std::time::Duration;
use std::time::Instant;
use std::string::String;

use std::thread::spawn;

use palette::{Yxy,Srgb};

use rand::*;

use json::{object,array};

fn generate_uuid() -> Vec<u8> {
  let mut rng = rand::thread_rng();
  let characters = b"0123456789abcdef".to_vec();
  let hexed: Vec<u8> = [0;32].iter().map(|_| characters[rng.gen_range(0, 16)]).collect();
  [b"IOS", &hexed[0..8], b"-", &hexed[8..12], b"-", &hexed[12..16], b"-", &hexed[16..24], b"-", &hexed[24..32]].concat()
}

fn make_deconz(deconz_host: String, api_key: String, group_name: String) -> Result<impl FnMut(&[PushStatusValue]) -> (), &'static str> {
  let client = reqwest::Client::new();
  let api_url = ["http://", &deconz_host, "/api/", &api_key, "/"].concat();
  let group_number_valid = Duration::new(3600, 0);
  let get_group_api = move || {
    let client = reqwest::Client::new();
    let mut response = client.get(&[&api_url, "groups"].concat()).send().unwrap();
    let groups = json::parse(&response.text().unwrap_or("{}".to_string())).unwrap();
    for (key, group) in groups.entries() {
      println!("{}, {}, {:?}", key, group["name"], group);
      if group["name"].to_string() == group_name {
        return Ok([&api_url, "groups/", key].concat())
      }
    }
    Err("No such group found")
  };
  get_group_api().map(|group_api| {
    let mut group_api_url = group_api;
    let mut group_fetched_at = Instant::now();
    move |push_values: &_| {
      if group_fetched_at + group_number_valid < Instant::now() {
        if let Ok(new_group_api) = get_group_api() {
          group_api_url = new_group_api;
          group_fetched_at = Instant::now();
          println!("Fetched new group number");
        }
      }
      if let Some((red, green, blue)) = get_status_rgb(push_values) {
        let xy = Yxy::from(Srgb::new(red as f32 / std::u8::MAX as f32,
                                     green as f32 / std::u8::MAX as f32,
                                     blue as f32 / std::u8::MAX as f32).into_linear());
        println!("Got red/green/blue {}/{}/{} or x/y {}/{} ({:?})", red, green, blue, xy.x, xy.y, push_values);
        let map = if let Some(PushStatusValue::LightIntencity(li)) = push_values.iter().find(|x| match x { PushStatusValue::LightIntencity(_) => true, _ => false }) {
          object!{
            "xy" => array![xy.x, xy.y],
            "bri" => *li,
          }
        } else { object!{"xy" => array![xy.x, xy.y]} };

        let mut request = client.put(&[&group_api_url, "/action"].concat()).body(map.dump());
        spawn(move || { if let Ok(resp) = &mut request.send() {
          println!("{:?}", resp.text());
        }; });
      }
      if let Some(PushStatusValue::LightOnTimer(x)) = push_values.iter().find(|x| match x { PushStatusValue::LightOnTimer(_) => true, _ => false }) {
        let map = object!{"on" => *x != 0};
        let mut request_query = client.get(&group_api_url);
        let mut request_action = client.put(&[&group_api_url, "/action"].concat()).body(map.dump());
        let should_be_on = *x != 0;
        
        spawn(move || if let Ok(resp) = &mut request_query.send() {
          let status = json::parse(&resp.text().unwrap_or("{}".to_string())).unwrap();
          if status["action"]["on"] != should_be_on {
            let client = reqwest::Client::new();
            if let Ok(resp) = &mut request_action.send() {
              println!("{:?}", resp.text());
            };
          };
        });
      }
      if let Some(PushStatusValue::FadeColors(x)) = push_values.iter().find(|x| match x { PushStatusValue::FadeColors(_) => true, _ => false }) {
        use network_package::object::StatusFadeColors::*;
        let map = object!{"effect" => if *x == Off { "none" } else { "colorloop" }, "colorloopspeed" => if *x == Slow {150} else {20}};
        let mut request = client.put(&[&group_api_url, "/action"].concat()).body(map.dump());
        spawn(move || { if let Ok(resp) = &mut request.send() {
          println!("{:?}", resp.text());
        }; });
      };
    }
  })
}

fn main() {
  let mut buf = [0; 4096];
  let mut args: Vec<_> = args().collect();
  if args.len() != 5 {
    println!("Usage: {} spa-target deconz-target deconz-api-key deconz-group-name", args[0]);
    return;
  }
  let mut socket = UdpSocket::bind("0.0.0.0:0").expect("Couln't bind");
  let deconz_group_name = args.remove(4);
  let api_key = args.remove(3);
  let deconz_host = args.remove(2);
  let mut deconz_client = make_deconz(deconz_host, api_key, deconz_group_name).unwrap();
  socket.connect(&args[1]);
  socket.send(compose_network_data(&NetworkPackage::Hello(b"1".to_vec())).as_slice());
  if let Ok(len) = socket.recv(& mut buf) {
    if let Ok(([], NetworkPackage::Hello(receiver))) = parse_network_data(&buf[0..len]) {
      let key = generate_uuid();
      socket.send(compose_network_data(&NetworkPackage::Hello(key.clone())).as_slice());
      socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::GetVersion}).as_slice());
      if let Ok(len) = socket.recv(& mut buf) {
        if let Ok(([], NetworkPackage::Authorized{src, dst, data: NetworkPackageData::Version(x)})) = parse_network_data(&buf[0..len]) {
          println!("Got version {:?}", x);
          socket.set_read_timeout(Some(Duration::new(0, 100000)));
          let ping_timeout = Duration::new(3, 0);
          let mut last_ping = Instant::now();
          loop {
            if last_ping + ping_timeout <= Instant::now() {
              last_ping = Instant::now();
              socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::Ping}).as_slice());
            }
            
            if let Ok(len) = socket.recv(&mut buf) {
              match parse_network_data(&buf[0..len]) {
                Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Pong})) => {},
                Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::PushStatus{status_type, data, raw_whole}})) => { 
                  socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::PushStatusAck}).as_slice());
                  deconz_client(&data);
                  println!("Got status {:?} ({:?})", &raw_whole, &data);
                },
                Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Packs})) => { socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::PushStatusAck}).as_slice()); }
                Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Unknown(x)})) => { println!("Got payload \"{}\" {:?}", String::from_utf8_lossy(x.as_slice()), &x); },
                _ => println!("Unknown error, I guess"),
              }
              
            }
          }
        }
      }
    };
  };
}
