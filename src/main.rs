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

use std::thread::{spawn, JoinHandle};
use std::collections::HashMap;

use palette::{Yxy,Srgb};

use rand::*;

fn generate_uuid() -> Vec<u8> {
  let mut rng = rand::thread_rng();
  let characters = b"0123456789abcdef".to_vec();
  let hexed: Vec<u8> = [0;32].iter().map(|_| characters[rng.gen_range(0, 16)]).collect();
  [b"IOS", &hexed[0..8], b"-", &hexed[8..12], b"-", &hexed[12..16], b"-", &hexed[16..24], b"-", &hexed[24..32]].concat()
}

fn make_deconz(deconz_host: String, api_key: String) -> impl Fn(&[PushStatusValue]) -> () {
  let client = reqwest::Client::new();
  move |push_values| {
    if let Some((red, green, blue)) = get_status_rgb(&push_values) {
      let xy = Yxy::from(Srgb::new(red as f32 / std::u8::MAX as f32,
                                   green as f32 / std::u8::MAX as f32,
                                   blue as f32 / std::u8::MAX as f32).into_linear());
      println!("Got red/green/blue {}/{}/{} or x/y {}/{} ({:?})", red, green, blue, xy.x, xy.y, &push_values);

      let mut map = HashMap::new();
      map.insert("xy", [&xy.x, &xy.y]);
      let mut request = client.put(&["http://", &deconz_host, "/api/", &api_key, "/groups/1/action"].concat()).json(&map);
      spawn(move || { if let Ok(resp) = &mut request.send() {
        println!("{:?}", resp.text());
      }; });
    };
  }
}

fn main() {
  let mut buf = [0; 4096];
  let mut args: Vec<_> = args().collect();
  if args.len() != 4 {
    println!("Usage: {} spa-target deconz-target deconz-api-key", args[0]);
    return;
  }
  let mut socket = UdpSocket::bind("0.0.0.0:0").expect("Couln't bind");
  let api_key = args.remove(3);
  let deconz_host = args.remove(2);
  let deconz_client = make_deconz(deconz_host, api_key);
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
