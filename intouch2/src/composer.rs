use std::borrow::Cow;

use crate::ToStatic;

use super::object::*;

pub fn compose_network_data<'a, 'b: 'a>(input: &'a NetworkPackage<'b>) -> Cow<'static, [u8]> {
    fn compose_option(before: &[u8], content: &Option<impl AsRef<[u8]>>, after: &[u8]) -> Vec<u8> {
        match content {
            Some(x) => [before, x.as_ref(), after].concat(),
            None => vec![],
        }
    }
    match input {
        NetworkPackage::Hello(x) => [b"<HELLO>", x.as_ref(), b"</HELLO>"].concat().into(),
        NetworkPackage::Addressed { src, dst, data } => [
            b"<PACKT>",
            compose_option(b"<SRCCN>", src, b"</SRCCN>").as_slice(),
            compose_option(b"<DESCN>", dst, b"</DESCN>").as_slice(),
            b"<DATAS>",
            &data.compose().to_static(),
            b"</DATAS>",
            b"</PACKT>",
        ]
        .concat()
        .into(),
    }
}
