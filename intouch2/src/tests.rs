use std::borrow::Cow;

use super::{composer::*, object::*, parser::*};

#[test]
fn parse_hello() {
    assert!(matches!(
        parse_network_data(b"<HELLO>1</HELLO>"),
        Ok(package) if package == NetworkPackage::Hello(b"1".as_slice().into())
    ));
}

#[test]
fn parse_new_ping() {
    assert!(matches!(
    NetworkPackageData::parse(b"APING"),
    Ok(package) if package == (&[], NetworkPackageData::Ping),
    ));
    assert!(matches!(
    NetworkPackageData::parse(b"APING\0"),
    Ok(package) if package == (&[], NetworkPackageData::Pong),
    ));
    assert!(matches!(
    NetworkPackageData::parse(b"WCERR"),
    Ok(package) if package == (&[], NetworkPackageData::WaterQualityError),
    ));
    assert!(matches!(
    NetworkPackageData::parse(b"PUNG"),
    Ok(package) if package == (&[], package_data::Unknown(Cow::Borrowed(b"PUNG")).into())
    ));
}

#[test]
fn parse_ping_and_pong() {
    let data = b"<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>";
    let expected = NetworkPackage::Addressed {
        src: Some(b"sender-id".as_slice().into()),
        dst: None,
        data: package_data::Ping.into(),
    };
    assert_eq!(data, compose_network_data(&expected).as_ref());
    assert!(matches!(
        parse_network_data(data),
    Ok(package) if package == expected
    ));
    assert!(matches!(
        parse_network_data(
          b"<PACKT><SRCCN>sender-id</SRCCN><DESCN>receiver-id</DESCN><DATAS>APING\0</DATAS></PACKT>"
        ),
    Ok(package) if package == NetworkPackage::Addressed {
        src: Some(b"sender-id".as_slice().into()),
        dst: Some(b"receiver-id".as_slice().into()),
        data: package_data::Pong.into()
      }
    ));
}

#[test]
fn test_dumb_data() {
    crate::gen_packages! {
        pub enum TestPackageData {
            Ping(b"APING": Simple),
            Version(b"SVERS": Tailing),
            PushStatus {
                b"STATP": Tag,
                data1: &[u8; 4],
                data2: &[u8; 4],
                data3: u8,
                data4: &[u8],
                //data5: u8,
            },
            PushStatusAck(b"STATQ": Simple),
            UpdateStatus(b"STATU": Tailing),
            Unknown(b"": Tailing),
        }
    }
    let data = b"STATP\x00\x01\x02\x03\x10\x11\x12\x13\x30\x40\x41\x42\x43\x44\x45\x46\x47\x50";
    match TestPackageData::parse(data) {
        Ok(([], TestPackageData::PushStatus { .. })) => (),
        Ok(([], x)) => assert!(false, "Invalid parse result {x:?}"),
        Ok((x, _)) => assert!(false, "Trailing data after parse: {x:?}"),
        Err(e) => assert!(false, "Error: {e}"),
    }
}

#[test]
fn parse_invalid_datas() {
    assert!(matches!(
        parse_network_data(b"<PACKT><DATAS>APUNG</DATAS></PACKT>"),
        Ok(package) if package == NetworkPackage::Addressed {
        src: None,
        dst: None,
        data: package_data::Unknown(b"APUNG".as_slice().into()).into()
      }
    ))
}

#[test]
fn id_packets() {
    let packets = vec![
        NetworkPackage::Hello(b"My hello".as_slice().into()),
        NetworkPackage::Addressed {
            src: Some(b"some-src".as_slice().into()),
            dst: None,
            data: package_data::Ping.into(),
        },
        NetworkPackage::Addressed {
            src: Some(b"some-src".as_slice().into()),
            dst: Some(b"some-dest".as_slice().into()),
            data: package_data::Pong.into(),
        },
        NetworkPackage::Addressed {
            src: None,
            dst: None,
            data: package_data::GetVersion { seq: 0 }.into(),
        },
        NetworkPackage::Addressed {
            src: None,
            dst: None,
            data: package_data::Version { en_build: 1, en_major: 2, en_minor: 3, co_build: 4, co_major: 5, co_minor: 6 }.into(),
        },
        // NetworkPackage::Authorized{src: None, dst: None, data: package_data::PushStatus(b"Some
        // status".as_slice().into())},
        NetworkPackage::Addressed {
            src: None,
            dst: None,
            data: package_data::PushStatusAck { seq: 9 }.into(),
        },
        // NetworkPackage::Addressed {
        //  src: None,
        //  dst: None,
        //  data: package_data::Error(ErrorType::WaterQuality).into(),
        //},
        // NetworkPackage::Addressed {
        //  src: None,
        //  dst: None,
        //  data: NetworkPackageData::Error(ErrorType::Radio),
        //},
    ];
    for pkg in packets.iter() {
        let composed = compose_network_data(&pkg);
        let parsed = parse_network_data(&composed)
            .expect("This comes from a valid package, and must thus be valid");
        let composed_again = compose_network_data(&parsed);
        assert_eq!(composed, composed_again);
        // match parsed {
        //    Ok(package) if package == *pkg => (),
        //    others => assert!(false, "Invalid data from parser"),
        //}
    }
}
