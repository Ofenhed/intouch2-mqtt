#![allow(unused_variables)]

pub use num_derive::{FromPrimitive, ToPrimitive};
pub use num_traits::{FromPrimitive, ToPrimitive};
use std::{borrow::Cow, cmp::Ordering};

type ByteString<'a> = Cow<'a, [u8]>;

#[derive(Eq, Debug, PartialEq, FromPrimitive)]
pub enum StatusColorsType {
  Off = 0,
  SlowFade = 1,
  FastFade = 2,
  Solid = 5,
}

pub type PushStatusValue = (u8, u8);

const fn key(v1: u8, v2: u8) -> isize {
  (((v1 as u16) << 8) + v2 as u16) as isize
}

fn to_push_status_index(v1: u8, v2: u8) -> isize {
  key(v1, v2)
}

fn from_push_status_index(v: isize) -> (u8, u8) {
  ((v >> 8) as u8, v as u8)
}

#[derive(Eq, Debug, PartialEq, FromPrimitive, ToPrimitive, Hash, Copy, Clone)]
pub enum PushStatusIndex {
  ColorType = key(2, 89),
  Red = key(2, 92),
  Green = key(2, 93),
  Blue = key(2, 94),
  SecondaryColorType = key(2, 96),
  TargetTemperatureLsb = key(0, 2),
  TargetTemperatureMsb = key(0, 1),
  TargetTemperatureLsbAgain = key(1, 20),
  TargetTemperatureMsbAgain = key(1, 19),
  SecondaryRed = key(2, 99),
  SecondaryGreen = key(2, 100),
  SecondaryBlue = key(2, 101),
  LightOnTimer = key(1, 49),
  Fountain = key(1, 107),
}

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
pub enum PushStatusKey {
  Keyed(PushStatusIndex),
  Indexed(u8, u8),
}

pub type PushStatusList = ::std::collections::HashMap<PushStatusKey, PushStatusValue>;

#[derive(Eq, Debug, PartialEq, Clone)]
pub enum ErrorType {
  Radio,
  WaterQuality,
}

#[derive(Eq, Debug, PartialEq)]
pub enum NetworkPackageData<'a> {
  Ping,
  Pong,
  GetVersion,
  Version(ByteString<'a>),
  PushStatus(PushStatusList),
  UnparsablePushStatus(ByteString<'a>),
  PushStatusAck,
  Packs,
  Error(ErrorType),
  Unknown(ByteString<'a>),
}

impl NetworkPackageData<'_> {
  pub fn to_static<'a>(&'a self) -> NetworkPackageData<'static> {
    use NetworkPackageData as X;
    match self {
      X::Ping => X::Ping,
      X::Pong => X::Pong,
      X::GetVersion => X::GetVersion,
      X::Version(x) => X::Version(x.clone().into_owned().into()),
      X::PushStatus(x) => X::PushStatus(x.clone()),
      X::UnparsablePushStatus(x) => X::UnparsablePushStatus(x.clone().into_owned().into()),
      X::PushStatusAck => X::PushStatusAck,
      X::Packs => X::Packs,
      X::Error(x) => X::Error(x.clone()),
      X::Unknown(x) => X::Unknown(x.clone().into_owned().into()),
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum NetworkPackage<'a> {
  Authorized {
    src: Option<ByteString<'a>>,
    dst: Option<ByteString<'a>>,
    data: NetworkPackageData<'a>,
  },
  Hello(ByteString<'a>),
}

impl NetworkPackage<'_> {
  pub fn to_static(&self) -> NetworkPackage<'static> {
    use NetworkPackage as X;
    match self {
      X::Authorized { src, dst, data } => X::Authorized {
        src: src.clone().map(|x| x.into_owned().into()),
        dst: dst.clone().map(|x| x.into_owned().into()),
        data: data.to_static(),
      },
      X::Hello(x) => X::Hello(x.clone().into_owned().into()),
    }
  }
}

#[derive(Eq, Debug, PartialEq)]
pub enum Temperature {
  Celcius(u8),
  UncertainCelcius(u8, u8),
}

impl Temperature {
  pub fn uncertain(lsb: u8, previous: Option<u8>) -> Self {
    let lsb32 = lsb as u32;
    let low_result = ((1 << 8) + lsb32) / 18;
    let high_result = ((2 << 8) + lsb32) / 18;
    if let Some(previous) = previous {
      let translated = (previous as u32) * 18;
      let msb = translated >> 8;
      Temperature::certain(msb as u8, lsb)
    } else {
      Temperature::UncertainCelcius(low_result as u8, high_result as u8)
    }
  }
  pub fn certain(msb: u8, lsb: u8) -> Self {
    let msb = msb as u32;
    let lsb = lsb as u32;
    let result = ((msb << 8) + lsb) / 18;
    Temperature::Celcius(result as u8)
  }
}

impl Ord for PushStatusKey {
  fn cmp(&self, other: &Self) -> Ordering {
    let first = match self {
      PushStatusKey::Indexed(x, y) => (*x, *y),
      PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
    };
    let second = match other {
      PushStatusKey::Indexed(x, y) => (*x, *y),
      PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
    };
    first.cmp(&second)
  }
}

impl PartialOrd for PushStatusKey {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(&other))
  }
}

pub fn to_push_status_key(field_group: u8, field_name: u8) -> PushStatusKey {
  if let Some(enumed) = FromPrimitive::from_isize(to_push_status_index(field_group, field_name)) {
    PushStatusKey::Keyed(enumed)
  } else {
    PushStatusKey::Indexed(field_group, field_name)
  }
}

pub fn from_push_status_key(key: &PushStatusKey) -> (u8, u8) {
  match key {
    PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
    PushStatusKey::Indexed(x, y) => (*x, *y),
  }
}
