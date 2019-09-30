extern crate nom;

use super::object::NetworkPackage;
use super::object::NetworkPackageData;

use nom::*;
use nom::bytes::complete::*;

fn surrounded<'a>(before: &'a [u8], after: &'a [u8]) -> impl 'a + for<'r> Fn(&'r [u8]) -> IResult<&'r [u8], &'r [u8]> {
  move |input| {
    let (input, _) = tag(before)(input)?;
    let (input, data) = take_until!(input, after)?;
    let (input, _) = tag(after)(input)?;
    Ok((input, data))
  }
}

fn parse_hello_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, hello) = surrounded(b"<HELLO>", b"</HELLO>")(input)?;
  Ok((input, NetworkPackage::Hello(hello.to_vec())))
}

fn parse_datas(input: &[u8]) -> IResult<&[u8], NetworkPackageData> {
  match input {
    b"APING" => Ok((b"", NetworkPackageData::Ping)),
    b"APING." => Ok((b"", NetworkPackageData::Pong)),
    _ => Err(Err::Incomplete(Needed::Unknown)),
  }
}

fn parse_authorized_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, _) = tag(b"<PACKT>")(input)?;
  let (input, src) = opt!(input, surrounded(b"<SRCCN>", b"</SRCCN>"))?;
  let (input, dst) = opt!(input, surrounded(b"<DESCN>", b"</DESCN>"))?;
  let (input, datas) = surrounded(b"<DATAS>", b"</DATAS>")(input)?;
  let (unparsed, datas) = parse_datas(datas)?;
  // eof!(unparsed)?;
  let (input, _) = tag(b"</PACKT>")(input)?;
  Ok((input, NetworkPackage::Authorized{src: src.map(|x| x.to_vec()), dst: dst.map(|x| x.to_vec()), data: datas}))
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
