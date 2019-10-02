#![allow(unused_variables)]

type ByteString = Vec<u8>;

#[derive(Eq,Debug,PartialEq)]
pub enum StatusFadeColors {
  Slow,
  Quick,
  Off,
}

#[derive(Eq,Debug,PartialEq)]
pub enum PushStatusValue {
  FadeColors(StatusFadeColors),
  Red(u8),
  Green(u8),
  Blue(u8),
  SecondaryRed(u8),
  SecondaryGreen(u8),
  SecondaryBlue(u8),
  LightIntencity(u8),
  LightOnTimer(u8),
}

#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackageData {
    Ping,
    Pong,
    GetVersion,
    Version(ByteString),
    PushStatus{ status_type: u8, data: Vec<PushStatusValue>, raw_whole: ByteString },
    PushStatusAck,
    Packs,
    Unknown(ByteString),
}

#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackage {
    Authorized{src: Option<ByteString>, dst: Option<ByteString>, data: NetworkPackageData},
    Hello(ByteString),
}
