use super::{composer::*, object::*, parser::*};

#[test]
fn parse_hello() {
  assert!(matches!(
      parse_network_data(b"<HELLO>1</HELLO>"),
      Ok(package) if package == NetworkPackage::Hello(b"1".as_slice().into())
  ));
}

#[test]
fn parse_ping_and_pong() {
  assert!(matches!(
      parse_network_data(b"<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>"),
  Ok(package) if package == NetworkPackage::Authorized {
      src: Some(b"sender-id".as_slice().into()),
      dst: None,
      data: NetworkPackageData::Ping
    }
  ));
  assert!(matches!(
      parse_network_data(
        b"<PACKT><SRCCN>sender-id</SRCCN><DESCN>receiver-id</DESCN><DATAS>APING\0</DATAS></PACKT>"
      ),
  Ok(package) if package == NetworkPackage::Authorized {
      src: Some(b"sender-id".as_slice().into()),
      dst: Some(b"receiver-id".as_slice().into()),
      data: NetworkPackageData::Pong
    }
  ));
}

#[test]
fn parse_invalid_datas() {
  assert!(matches!(
      parse_network_data(b"<PACKT><DATAS>APUNG</DATAS></PACKT>"),
      Ok(package) if package == NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::Unknown(b"APUNG".as_slice().into())
    }
  ))
}

#[test]
fn id_packets() {
  let packets = vec![
    NetworkPackage::Hello(b"My hello".as_slice().into()),
    NetworkPackage::Authorized {
      src: Some(b"some-src".as_slice().into()),
      dst: None,
      data: NetworkPackageData::Ping,
    },
    NetworkPackage::Authorized {
      src: Some(b"some-src".as_slice().into()),
      dst: Some(b"some-dest".as_slice().into()),
      data: NetworkPackageData::Pong,
    },
    NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::GetVersion,
    },
    NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::Version(b"some_version".as_slice().into()),
    },
    // NetworkPackage::Authorized{src: None, dst: None, data: NetworkPackageData::PushStatus(b"Some
    // status".as_slice().into())},
    NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::PushStatusAck,
    },
    NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::Error(ErrorType::WaterQuality),
    },
    NetworkPackage::Authorized {
      src: None,
      dst: None,
      data: NetworkPackageData::Error(ErrorType::Radio),
    },
  ];
  for pkg in packets.into_iter() {
    let composed = compose_network_data(&pkg);
    assert!(matches!(parse_network_data(&composed), Ok(package) if package == pkg));
  }
}
