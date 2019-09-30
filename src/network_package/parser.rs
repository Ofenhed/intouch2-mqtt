extern crate nom;

use super::object::NetworkPackage;
use super::object::NetworkPackageData;

use nom::*;
use nom::bytes::complete::*;

fn surrounded<'a>(before: &'a [u8], after: &'a [u8]) -> impl 'a + for<'r> Fn(&'r [u8]) -> IResult<&'r [u8], &'r [u8]> {
  move |input| 
    do_parse!(input, 
              tag!(before) >> 
              data: take_until!(after) >> 
              tag!(after) >> 
              (data))
}

fn parse_hello_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, hello) = surrounded(b"<HELLO>", b"</HELLO>")(input)?;
  Ok((input, NetworkPackage::Hello(hello.to_vec())))
}

fn parse_datas(input: &[u8]) -> IResult<&[u8], NetworkPackageData> {
  let (input, datas) = surrounded(b"<DATAS>", b"</DATAS>")(input)?;
  match datas {
    b"APING" => Ok((input, NetworkPackageData::Ping)),
    b"APING." => Ok((input, NetworkPackageData::Pong)),
    _ => Err(Err::Incomplete(Needed::Unknown)),
  }
}

fn parse_authorized_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  do_parse!(input,
            tag!(b"<PACKT>") >>
            src: opt!(surrounded(b"<SRCCN>", b"</SRCCN>")) >>
            dst: opt!(surrounded(b"<DESCN>", b"</DESCN>")) >>
            datas: parse_datas >>
            tag!(b"</PACKT>") >>
            (NetworkPackage::Authorized{src: src.map(|x| x.to_vec()), dst: dst.map(|x| x.to_vec()), data: datas}))

}

pub fn parse_network_data(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  alt!(input, parse_hello_package | parse_authorized_package)
}


#[cfg(test)]
mod tests {
  use super::*;

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

}
