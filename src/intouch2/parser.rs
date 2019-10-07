extern crate nom;

use super::object::*;

use nom::*;

use std::collections::HashMap;
use num_traits::FromPrimitive;

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

fn parse_pushed_package(input: &[u8]) -> Option<HashMap<u8, (u8, (u8, u8))>> {
  let mut iter = input.iter();
  let count = iter.next()?;
  let mut ret = HashMap::new();
  for _ in 0..*count {
    let pkg_type = iter.next()?;
    let group = iter.next()?;
    let first = iter.next()?;
    let second = iter.next()?;
    let members = (*first, *second);
    
    ret.insert(*group, (*pkg_type, members));
  };
  Some(ret)
}

fn parse_datas(input: &[u8]) -> IResult<&[u8], NetworkPackageData> {
  let (input, datas) = surrounded(b"<DATAS>", b"</DATAS>")(input)?;
  match datas {
    b"APING" => Ok((input, NetworkPackageData::Ping)),
    b"APING\0" => Ok((input, NetworkPackageData::Pong)),
    b"AVERSJ" => Ok((input, NetworkPackageData::GetVersion)),
    b"STATQ\xe5" => Ok((input, NetworkPackageData::PushStatusAck)),
    b"PACKS" => Ok((input, NetworkPackageData::Packs)),
    x => if let (b"SVERS", data) = x.split_at(5) { Ok((input, NetworkPackageData::Version(data.to_vec()))) }
         else if let (b"STATP", data) = x.split_at(5) {
           if let Some(partitioned) = parse_pushed_package(data) {
             if partitioned.len() > 0 {
               let mut parsed = PushStatusList::new();
               for (field_type, (sub_msg_type, value)) in &partitioned {
                 if let Some(enumed) = FromPrimitive::from_isize(to_push_status_index(*sub_msg_type, *field_type)) {
                   parsed.insert(PushStatusKey::Keyed(enumed), *value);
                 } else {
                   parsed.insert(PushStatusKey::Indexed(*sub_msg_type, *field_type), *value);
                 }
               }
               Ok((input, NetworkPackageData::PushStatus(parsed)))
             } else {
               Ok((input, NetworkPackageData::UnparsablePushStatus(data.to_vec())))
             }
           } else {
             Ok((input, NetworkPackageData::UnparsablePushStatus(data.to_vec())))
           }
         }
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

fn calculate_rgba_from_rgb(r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
  let intencity = r + g + b;
  let max = ::std::cmp::max(r, ::std::cmp::max(g, b));

  let mul = intencity as f32 / max as f32;
  fn conv(x: f32) -> u8 {
      let y = x as u8;
      y
  }
  (conv(r as f32 * mul), conv(g as f32 * mul), conv(b as f32 * mul), intencity)
}

pub fn get_status_rgba(data: &PushStatusList) -> (Option<(u8, u8, u8, u8)>, Option<(u8, u8, u8, u8)>) {
  use PushStatusIndex::{Red,Green,Blue,SecondaryRed, SecondaryGreen, SecondaryBlue};

  let get = |x: &PushStatusIndex| data.get(&PushStatusKey::Keyed(*x));
  let fst = |&(x, _)| x;

  let (pr, pg, pb, got_primary) = match (get(&Red), get(&Green), get(&Blue)) {
    (None, None, None) => (0, 0, 0, false),
    (r, g, b) => (fst(r.unwrap_or(&(0,0))), fst(g.unwrap_or(&(0,0))), fst(b.unwrap_or(&(0,0))), true),
  };
  
  let (sr, sg, sb, got_secondary) = match (get(&SecondaryRed), get(&SecondaryGreen), get(&SecondaryBlue)) {
    (None, None, None) => (0, 0, 0, false),
    (r, g, b) => (fst(r.unwrap_or(&(0,0))), fst(g.unwrap_or(&(0,0))), fst(b.unwrap_or(&(0,0))), true),
  };
  
  let left = if got_primary { Some(calculate_rgba_from_rgb(pr, pg, pb)) } else { None };
  let right = if got_secondary { Some(calculate_rgba_from_rgb(sr, sg, sb)) } else { None };

  (left, right)
}

pub fn parse_network_data(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  alt!(input, parse_hello_package | parse_authorized_package)
}
