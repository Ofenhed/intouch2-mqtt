#![allow(unused_variables)]

type ByteString = Vec<u8>;

pub enum NetworkPackageData {
    Ping,
    Pong,
    GetVersion,
    Unknown,
}

pub enum NetworkPackage {
    Authorized,
    Hello(ByteString),
}
