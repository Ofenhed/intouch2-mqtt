use core::fmt::Debug;
use std::borrow::Cow;

use crate::{ToStatic as _, parser};

use super::{composer::*, object::*, parser::*};

#[test]
fn parse_hello() {
    assert!(matches!(
        parse_network_data(b"<HELLO>1</HELLO>"),
        Ok(package) if package == NetworkPackage::Hello(b"1".as_slice().into())
    ));
}

#[test]
fn parse_new_ping() -> Result<(), parser::ParseError> {
    fn to_err(e: nom::Err<nom::error::Error<&[u8]>>) -> parser::ParseError {
        ParseError::from(e.to_owned())
    }
    assert_eq!(
        NetworkPackageData::parse(b"APING").map_err(to_err)?,
        (&[][..], NetworkPackageData::Ping),
    );
    assert_eq!(
        NetworkPackageData::parse(b"APING\0").map_err(to_err)?,
        (&[][..], NetworkPackageData::Pong),
    );
    assert_eq!(
        NetworkPackageData::parse(b"WCERR").map_err(to_err)?,
        (&[][..], NetworkPackageData::WatercareError),
    );
    assert_eq!(
        NetworkPackageData::parse(b"PUNG").map_err(to_err)?,
        (
            &[][..],
            package_data::Unknown(Cow::Borrowed(b"PUNG")).into()
        )
    );
    Ok(())
}

#[test]
fn parse_ping_and_pong() -> Result<(), parser::ParseError> {
    let data = b"<PACKT><SRCCN>sender-id</SRCCN><DATAS>APING</DATAS></PACKT>";
    let expected = NetworkPackage::Addressed {
        src: Some(b"sender-id".as_slice().into()),
        dst: None,
        data: package_data::Ping.into(),
    };
    assert_eq!(data, compose_network_data(&expected).as_ref());
    assert_eq!(parse_network_data(data)?, expected);
    assert_eq!(parse_network_data(
          b"<PACKT><SRCCN>sender-id</SRCCN><DESCN>receiver-id</DESCN><DATAS>APING\0</DATAS></PACKT>"
        )?,
    NetworkPackage::Addressed {
        src: Some(b"sender-id".as_slice().into()),
        dst: Some(b"receiver-id".as_slice().into()),
        data: package_data::Pong.into()
      }
    );
    Ok(())
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
fn parse_reminders() {
    let valid_payload = [
        2, 60, 0, 1, 3, 120, 0, 1, 4, 218, 2, 1, 5, 28, 2, 0, 6, 180, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut raw_data = b"RMREQ".to_vec();
    raw_data.append(&mut valid_payload.to_vec());
    let parse_fail = match NetworkPackageData::parse(&raw_data) {
        Ok(([], NetworkPackageData::Reminders(reminders))) => {
            assert_eq!(&raw_data[..], &reminders.compose()[..]);
            None
        }
        x => Some(x),
    };
    assert_eq!(parse_fail, None);
}

#[test]
fn parse_watercare() {
    let valid_payload = [
        0, 1, 0, 1, 0, 0, 6, 0, 0, 0, 0, 0, 2, 1, 0, 6, 6, 30, 6, 40, 0, 2, 2, 0, 6, 22, 0, 22, 10,
        1, 2, 0, 0, 6, 18, 0, 18, 10,
    ];
    let mut raw_data = b"WCREQ".to_vec();
    raw_data.append(&mut valid_payload.to_vec());
    let parse_fail = match NetworkPackageData::parse(&raw_data) {
        Ok(([], NetworkPackageData::WatercareRequest(req))) => {
            assert_eq!(&raw_data[..], &req.compose()[..]);
            None
        }
        x => Some(x),
    };
    assert_eq!(parse_fail, None);
}

#[test]
fn status_update() {
    let inner_data = (0..39).collect::<Vec<u8>>();
    fn parse_data(inner: &[u8]) -> NetworkPackageData<'static> {
        let data = [
            &b"<PACKT><SRCCN>SPA1</SRCCN><DESCN>DEV1</DESCN><DATAS>STATV\x00\x01\x27"[..],
            &inner[..],
            &b"</DATAS></PACKT>"[..],
        ]
        .concat();
        let data = parse_network_data(&data);
        println!("{data:?}");
        let Ok(NetworkPackage::Addressed {
            data: status_package,
            ..
        }) = data
        else {
            panic!("Package not parsed to addressed: {data:?}");
        };
        let NetworkPackageData::Status(package_data::Status {
            length, ref data, ..
        }) = status_package
        else {
            panic!("Package not parsed as status");
        };
        assert_eq!(length as usize, data.len());
        status_package.to_static()
    }
    let pkg = {
        let pkg = parse_data(&inner_data[..]);
        let NetworkPackageData::Status(package_data::Status { ref data, .. }) = pkg else {
            unreachable!()
        };
        assert_eq!(data[..], inner_data[..]);
        pkg.to_static()
    };
    let NetworkPackageData::Status(status) = pkg else {
        unreachable!();
    };
    assert_eq!(status.data, inner_data);
}

#[test]
fn id_packets() -> Result<(), parser::ParseError> {
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
            data: package_data::Version {
                en_build: 1,
                en_major: 2,
                en_minor: 3,
                co_build: 4,
                co_major: 5,
                co_minor: 6,
            }
            .into(),
        },
        NetworkPackage::Addressed {
            src: None,
            dst: None,
            data: package_data::PushStatusAck { seq: 9 }.into(),
        },
    ];
    for pkg in packets.iter() {
        let composed = compose_network_data(&pkg);
        let parsed = parse_network_data(&composed)?;
        let composed_again = compose_network_data(&parsed);
        assert_eq!(composed, composed_again);
        let cloned = parsed.clone();
        assert_eq!(parsed, cloned);
        // match parsed {
        //    Ok(package) if package == *pkg => (),
        //    others => assert!(false, "Invalid data from parser"),
        //}
    }
    Ok(())
}
