mod network_package;
use network_package::object::NetworkPackage;
use network_package::object::NetworkPackageData;
use network_package::parser::*;
use network_package::composer::*;

use std::net::UdpSocket;
use std::env::args;

fn main() {
    let mut buf = [0; 4096];
    let args: Vec<_> = args().collect();
    if args.len() != 3 {
      println!("Usage: {} target key", args[0]);
      return;
    }
    let mut socket = UdpSocket::bind("0.0.0.0:0").expect("Couln't bind");
    socket.connect(&args[1]);
    socket.send(compose_network_data(&NetworkPackage::Hello(b"1".to_vec())).as_slice());
    if let Ok(len) = socket.recv(& mut buf) {
      if let Ok(([], NetworkPackage::Hello(receiver))) = parse_network_data(&buf[0..len]) {
        let key = args[2].clone().into_bytes();
        socket.send(compose_network_data(&NetworkPackage::Hello(key.clone())).as_slice());
        socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::GetVersion}).as_slice());
        if let Ok(len) = socket.recv(& mut buf) {
          if let Ok(([], NetworkPackage::Authorized{src, dst, data: NetworkPackageData::Version(x)})) = parse_network_data(&buf[0..len]) {
            println!("Got version {:?}", x);
            socket.send(compose_network_data(&NetworkPackage::Authorized{src: Some(key.clone()), dst: Some(receiver.clone()), data: NetworkPackageData::Ping}).as_slice());
            if let Ok(len) = socket.recv(& mut buf) {
              if let Ok(([], NetworkPackage::Authorized{src: _, dst: _, data: NetworkPackageData::Pong})) = parse_network_data(&buf[0..len]) {
                println!("Got pong");
              }
            }
          }
        }
      };
    };
}
