#![allow(unused_variables)]

use num_traits::FromPrimitive;
use num_derive::ToPrimitive;
use num_traits::ToPrimitive;
use std::cmp::Ordering;

type ByteString = Vec<u8>;

#[derive(Eq,Debug,PartialEq,FromPrimitive)]
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

pub fn to_push_status_index(v1: u8, v2: u8) -> isize {
  key(v1, v2)
}

pub fn from_push_status_index(v: isize) -> (u8, u8) {
  ((v >> 8) as u8, v as u8)
}

#[derive(Eq,Debug,PartialEq,FromPrimitive,ToPrimitive,Hash,Copy,Clone)]
pub enum PushStatusIndex {
  ColorType = key(2, 89),
  Red = key(2, 92),
  Green = key(2, 93),
  Blue = key(2, 94),
  SecondaryColorType = key(2, 96),
  SecondaryRed = key(2, 99),
  SecondaryGreen = key(2, 100),
  SecondaryBlue = key(2, 101),
  LightOnTimer = key(1, 49),
  Fountain = key(1, 107),
}

#[derive(Debug,Hash,Eq,PartialEq,Copy,Clone)]
pub enum PushStatusKey {
  Keyed(PushStatusIndex),
  Indexed(u8, u8),
}

pub type PushStatusList = ::std::collections::HashMap<PushStatusKey, PushStatusValue>;


#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackageData {
    Ping,
    Pong,
    GetVersion,
    Version(ByteString),
    PushStatus(PushStatusList),
    UnparsablePushStatus(ByteString),
    PushStatusAck,
    Packs,
    Unknown(ByteString),
}

#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackage {
    Authorized{src: Option<ByteString>, dst: Option<ByteString>, data: NetworkPackageData},
    Hello(ByteString),
}

impl Ord for PushStatusKey {
  fn cmp(&self, other: &Self) -> Ordering {
    let first = match self {
      PushStatusKey::Indexed(x, y) => (*x, *y),
      PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap())
    };
    let second = match other {
      PushStatusKey::Indexed(x, y) => (*x, *y),
      PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap())
    };
    first.cmp(&second)
  }
}

impl PartialOrd for PushStatusKey {
  fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
    Some(self.cmp(&other))
  }
}
