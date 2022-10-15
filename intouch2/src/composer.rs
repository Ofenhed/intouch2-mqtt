use super::object::*;

fn compose_push_status(x: &PushStatusList) -> Vec<u8> {
    let mut res = vec![x.len() as u8];
    let mut keys: Vec<_> = x.keys().collect();
    keys.sort();
    for key in keys {
        let (p1, p2) = from_push_status_key(key);
        res.push(p1);
        res.push(p2);
        let (d1, d2) = x.get(key).unwrap();
        res.push(*d1);
        res.push(*d2);
    }
    res
}

fn compose_datas(input: &NetworkPackageData) -> Vec<u8> {
    match input {
        NetworkPackageData::Ping => b"APING".to_vec(),
        NetworkPackageData::Pong => b"APING\0".to_vec(),
        NetworkPackageData::GetVersion => b"AVERSJ".to_vec(),
        NetworkPackageData::Version(x) => [b"SVERS", x.as_slice()].concat(),
        NetworkPackageData::PushStatus(datas) => {
            [b"STATP", compose_push_status(datas).as_slice()].concat()
        }
        NetworkPackageData::UnparsablePushStatus(raw_whole) => {
            [b"STATP", raw_whole.as_slice()].concat()
        }
        NetworkPackageData::PushStatusAck => b"STATQ\xe5".to_vec(),
        NetworkPackageData::Error(ErrorType::Radio) => b"RFERR".to_vec(),
        NetworkPackageData::Error(ErrorType::WaterQuality) => b"WCERR".to_vec(),
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
        NetworkPackage::Authorized {
            src,
            dst,
            data: datas,
        } => [
            b"<PACKT>",
            compose_option(b"<SRCCN>", src, b"</SRCCN>").as_slice(),
            compose_option(b"<DESCN>", dst, b"</DESCN>").as_slice(),
            b"<DATAS>",
            compose_datas(datas).as_slice(),
            b"</DATAS>",
            b"</PACKT>",
        ]
        .concat(),
    }
}
