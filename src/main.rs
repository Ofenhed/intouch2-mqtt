mod network_package;
extern crate palette;
extern crate rand;
extern crate futures;

use network_package::object::NetworkPackage;
use network_package::object::NetworkPackageData;
use network_package::parser::*;
use network_package::composer::*;

use std::net::UdpSocket;
use std::env::args;
use std::time::Duration;
use std::time::Instant;
use std::string::String;

use palette::{Yxy,IntoColor,Srgb};

use hyper::{Client, Request, Body};
use futures::{Future, Stream};

use rand::*;

fn generate_uuid() -> Vec<u8> {
  let mut rng = rand::thread_rng();
  let characters = b"0123456789abcdef".to_vec();
  let hexed: Vec<u8> = [0;32].iter().map(|_| characters[rng.gen_range(0, 16)]).collect();
  [b"IOS", &hexed[0..8], b"-", &hexed[8..12], b"-", &hexed[12..16], b"-", &hexed[16..24], b"-", &hexed[24..32]].concat()
}

fn main() {
  let mut buf = [0; 4096];
  let args: Vec<_> = args().collect();
  if args.len() != 4 {
    println!("Usage: {} spa-target deconz-target deconz-api-key", args[0]);
    return;
  }
  let api_key = &args[3];
  let deconz_host = &args[2];
  let mut socket = UdpSocket::bind("0.0.0.0:0").expect("Couln't bind");
  let client = Client::new();
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
                  if let Some((red, green, blue)) = get_status_rgb(&data) {
                    let xy = Yxy::from(Srgb::new(red as f32 / 256.0, green as f32 / 256.0, blue as f32 / 256.0).into_linear());
                    println!("Got red/green/blue {}/{}/{} or x/y {}/{} ({:?})", red, green, blue, xy.x, xy.y, &data);
                    let req = Request::builder()
                        .method("PUT")
                        .uri(["http://", deconz_host, "/api/", api_key, "/groups/1/action"].concat())
                        .body(Body::from(["{\"xy\":[", &xy.x.to_string(), ",", &xy.y.to_string(), "]}"].concat()))
                        .expect("request builder");
                    let rsp = client.request(req).and_then(|res| { println!("Got response {:?}", res); res.into_body().concat2()} );
                  }
                  println!("Got status {:?} ({:?})", &raw_whole, &data);
                },
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
