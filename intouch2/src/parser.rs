extern crate nom;

use super::object::*;

use nom::{bytes::complete::*, combinator::opt, *};

use std::collections::HashMap;

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
  #[error("Error while parsing: {0}")]
  Parser(#[from] nom::Err<nom::error::Error<Vec<u8>>>),
  #[error("Unexpected tailing data: {msg:?} {tail:?}")]
  TailingData {
    msg: NetworkPackage<'static>,
    tail: Box<[u8]>,
  },
}

fn surrounded<'a>(
  before: &'a [u8],
  after: &'a [u8],
) -> impl 'a + for<'r> Fn(&'r [u8]) -> IResult<&'r [u8], &'r [u8]> {
  move |input| {
    let (input, ((_, data), _)) = tag(before)
      .and(take_until(after))
      .and(tag(after))
      .parse(input)?;
    Ok((input, data))
  }
}

fn parse_hello_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, hello) = surrounded(b"<HELLO>", b"</HELLO>")(input)?;
  Ok((input, NetworkPackage::Hello(hello.into())))
}

fn parse_pushed_package(input: &[u8]) -> Option<HashMap<(u8, u8), (u8, u8)>> {
  let mut iter = input.iter();
  let count = iter.next()?;
  let mut ret = HashMap::new();
  for _ in 0..*count {
    let pkg_type = iter.next()?;
    let group = iter.next()?;
    let first = iter.next()?;
    let second = iter.next()?;
    let members = (*first, *second);

    ret.insert((*pkg_type, *group), members);
  }
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
    b"RFERR" => Ok((input, NetworkPackageData::Error(ErrorType::Radio))),
    b"WCERR" => Ok((input, NetworkPackageData::Error(ErrorType::WaterQuality))),
    x => {
      if let (b"SVERS", data) = x.split_at(5) {
        Ok((input, NetworkPackageData::Version(data.into())))
      } else if let (b"STATP", data) = x.split_at(5) {
        if let Some(partitioned) = parse_pushed_package(data) {
          if partitioned.len() > 0 {
            let mut parsed = PushStatusList::new();
            for ((field_group, field_name), value) in &partitioned {
              parsed.insert(to_push_status_key(*field_group, *field_name), *value);
            }
            Ok((input, NetworkPackageData::PushStatus(parsed)))
          } else {
            Ok((input, NetworkPackageData::UnparsablePushStatus(data.into())))
          }
        } else {
          Ok((input, NetworkPackageData::UnparsablePushStatus(data.into())))
        }
      } else {
        Ok((input, NetworkPackageData::Unknown(x.into())))
      }
    }
  }
}

fn parse_authorized_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, ((src, dst), datas)) = surrounded(b"<PACKT>", b"</PACKT>")
    .and_then(
      opt(surrounded(b"<SRCCN>", b"</SRCCN>"))
        .and(opt(surrounded(b"<DESCN>", b"</DESCN>")))
        .and(parse_datas),
    )
    .parse(input)?;
  Ok((
    input,
    NetworkPackage::Authorized {
      src: src.map(|x| x.into()),
      dst: dst.map(|x| x.into()),
      data: datas,
    },
  ))
}

fn calculate_rgba_from_rgb(r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
  let intencity = r + g + b;
  let max = ::std::cmp::max(r, ::std::cmp::max(g, b));

  let mul = intencity as f32 / max as f32;
  fn conv(x: f32) -> u8 {
    let y = x as u8;
    y
  }
  (
    conv(r as f32 * mul),
    conv(g as f32 * mul),
    conv(b as f32 * mul),
    intencity,
  )
}

pub fn get_status_rgba(
  data: &PushStatusList,
) -> (Option<(u8, u8, u8, u8)>, Option<(u8, u8, u8, u8)>) {
  use PushStatusIndex::{Blue, Green, Red, SecondaryBlue, SecondaryGreen, SecondaryRed};

  let get = |x: &PushStatusIndex| data.get(&PushStatusKey::Keyed(*x));
  let fst = |&(x, _)| x;

  let (pr, pg, pb, got_primary) = match (get(&Red), get(&Green), get(&Blue)) {
    (None, None, None) => (0, 0, 0, false),
    (r, g, b) => (
      fst(r.unwrap_or(&(0, 0))),
      fst(g.unwrap_or(&(0, 0))),
      fst(b.unwrap_or(&(0, 0))),
      true,
    ),
  };

  let (sr, sg, sb, got_secondary) = match (
    get(&SecondaryRed),
    get(&SecondaryGreen),
    get(&SecondaryBlue),
  ) {
    (None, None, None) => (0, 0, 0, false),
    (r, g, b) => (
      fst(r.unwrap_or(&(0, 0))),
      fst(g.unwrap_or(&(0, 0))),
      fst(b.unwrap_or(&(0, 0))),
      true,
    ),
  };

  let left = if got_primary {
    Some(calculate_rgba_from_rgb(pr, pg, pb))
  } else {
    None
  };
  let right = if got_secondary {
    Some(calculate_rgba_from_rgb(sr, sg, sb))
  } else {
    None
  };

  (left, right)
}

pub fn get_temperature(
  data: &'_ PushStatusList,
  previous_temperature: Option<u8>,
) -> Option<Temperature> {
  let mut best_result = None;
  for (index, (msb, lsb)) in data.iter() {
    if let PushStatusKey::Keyed(key) = index {
      let result = match key {
        PushStatusIndex::TargetTemperatureLsb | PushStatusIndex::TargetTemperatureLsbAgain => {
          Some(Temperature::uncertain(*msb, previous_temperature))
        }
        PushStatusIndex::TargetTemperatureMsb | PushStatusIndex::TargetTemperatureMsbAgain => {
          Some(Temperature::certain(*msb, *lsb))
        }
        _ => None,
      };
      match (&best_result, &result) {
        (_, Some(Temperature::Celcius(_))) => {
          best_result = result;
          break;
        }
        (None, _) => {
          best_result = result;
        }
        _ => (),
      }
    }
  }
  best_result
}

pub fn parse_network_data<'a>(input: &'a [u8]) -> Result<NetworkPackage<'a>, ParseError> {
  match parse_hello_package
    .or(parse_authorized_package)
    .parse(input)
    .map_err(|x| x.to_owned())?
  {
    ([], msg) => Ok(msg),
    (tail, msg) => Err(ParseError::TailingData {
      tail: tail.into(),
      msg: msg.to_static(),
    }),
  }
}
