use super::object::*;

fn compose_datas(input: &NetworkPackageData) -> Vec<u8> {
  match input {
    NetworkPackageData::Ping => b"APING".to_vec(),
    NetworkPackageData::Pong => b"APING\0".to_vec(),
    NetworkPackageData::GetVersion => b"AVERSJ".to_vec(),
    NetworkPackageData::Version(x) => [b"SVERS", x.as_slice()].concat(),
    NetworkPackageData::PushStatus{raw_whole, status_type, data} => [b"STATP", raw_whole.as_slice()].concat(),
    NetworkPackageData::PushStatusAck => b"STATQ\xe5".to_vec(),
    NetworkPackageData::Packs => b"PACKS".to_vec(),
    _ => vec![],
  }
}

pub fn compose_network_data(input: &NetworkPackage) -> Vec<u8> {
  fn compose_option(before: &[u8], content: &Option<Vec<u8>>, after: &[u8]) -> Vec<u8> {
    match content {
      Some(x) => [before, x.as_slice(), after].concat(),
      None => vec![],
    }
  }
  match input {
    NetworkPackage::Hello(x) => [b"<HELLO>", x.as_slice(), b"</HELLO>"].concat(),
    NetworkPackage::Authorized{src, dst, data: datas} => [b"<PACKT>", 
                                                          compose_option(b"<SRCCN>", src, b"</SRCCN>").as_slice(),
                                                          compose_option(b"<DESCN>", dst, b"</DESCN>").as_slice(),
                                                          b"<DATAS>",
                                                          compose_datas(datas).as_slice(),
                                                          b"</DATAS>",
                                                          b"</PACKT>"].concat(),
    _ => vec![],
  }
}
