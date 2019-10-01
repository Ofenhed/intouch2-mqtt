#![allow(unused_variables)]

type ByteString = Vec<u8>;

#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackageData {
    Ping,
    Pong,
    GetVersion,
    Version(ByteString),
    Unknown(ByteString),
}

#[derive(Eq,Debug,PartialEq)]
pub enum NetworkPackage {
    Authorized{src: Option<ByteString>, dst: Option<ByteString>, data: NetworkPackageData},
    Hello(ByteString),
}
