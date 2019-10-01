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
    b"APING\0" => Ok((input, NetworkPackageData::Pong)),
    b"AVERSJ" => Ok((input, NetworkPackageData::GetVersion)),
    x => if let (b"SVERS", data) = x.split_at(5) { Ok((input, NetworkPackageData::Version(data.to_vec()))) }
         else { Ok((input, NetworkPackageData::Unknown(x.to_vec()))) }
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
