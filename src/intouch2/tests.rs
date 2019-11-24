use super::*;
use super::object::*;
use super::parser::*;
use super::composer::*;

const EMPTY:&[u8] = b"";

#[test]
fn parse_hello() {
  assert_eq!(parse_network_data(b"<HELLO>1</HELLO>"), Ok((EMPTY, NetworkPackage::Hello(b"1".to_vec()))));
}

#[test]
fn parse_ping_and_pong() {
  assert_eq!(parse_network_data(b"<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: Some(b"sender-id".to_vec()), dst: None, data: NetworkPackageData::Ping})));
  assert_eq!(parse_network_data(b"<PACKT><SRCCN>sender-id</SRCCN><DESCN>receiver-id</DESCN><DATAS>APING\0</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: Some(b"sender-id".to_vec()), dst: Some(b"receiver-id".to_vec()), data: NetworkPackageData::Pong})));
}

#[test]
fn parse_invalid_datas() {
  assert_eq!(parse_network_data(b"<PACKT><DATAS>APUNG</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::Unknown(b"APUNG".to_vec())})))
}

#[test]
fn id_packets() {
  let packets = vec![NetworkPackage::Hello(b"My hello".to_vec()),
                     NetworkPackage::Authorized{src: Some(b"some-src".to_vec()), dst: None, data: NetworkPackageData::Ping},
                     NetworkPackage::Authorized{src: Some(b"some-src".to_vec()), dst: Some(b"some-dest".to_vec()), data: NetworkPackageData::Pong},
                     NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::GetVersion},
                     NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::Version(b"some_version".to_vec())},
                     //NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::PushStatus(b"Some status".to_vec())},
                     NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::PushStatusAck},
                     NetworkPackage::Error(ErrorType::WaterQuality),
                     NetworkPackage::Error(ErrorType::Radio),
                    ];
  for pkg in packets.into_iter() {
    let composed = compose_network_data(&pkg);
    assert_eq!(parse_network_data(composed.as_slice()), Ok((EMPTY, pkg)));
  }
}


