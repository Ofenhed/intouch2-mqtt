use super::*;
use super::object::*;
use super::parser::*;

const EMPTY:&[u8] = b"";

#[test]
fn parse_hello() {
  assert_eq!(parse_network_data(b"<HELLO>1</HELLO>"), Ok((EMPTY, NetworkPackage::Hello(b"1".to_vec()))));
}

#[test]
fn parse_ping_and_pong() {
  assert_eq!(parse_network_data(b"<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: Some(b"sender-id".to_vec()), dst: None, data: NetworkPackageData::Ping})));
  assert_eq!(parse_network_data(b"<PACKT><SRCCN>sender-id</SRCCN><DESCN>receiver-id</DESCN><DATAS>APING.</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: Some(b"sender-id".to_vec()), dst: Some(b"receiver-id".to_vec()), data: NetworkPackageData::Pong})));
}

#[test]
fn parse_invalid_datas() {
  assert_eq!(parse_network_data(b"<PACKT><DATAS>APUNG</DATAS></PACKT>"), Ok((EMPTY, NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::Unknown(b"APUNG".to_vec())})))
}

