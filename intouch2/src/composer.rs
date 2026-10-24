use core::iter;
use std::borrow::Cow;

use crate::{object::NetworkPackage, object_traits::DatasContent};

#[must_use]
pub fn compose_network_data<'a, 'b: 'a>(input: &'a NetworkPackage<'b>) -> Vec<u8> {
    fn compose_option<'a>(
        before: &'a [u8],
        content: Option<&'a Cow<'a, [u8]>>,
        after: &'a [u8],
    ) -> Vec<Cow<'a, [u8]>> {
        if let Some(content) = content {
            vec![
                Cow::Borrowed(before),
                Cow::Borrowed(content),
                Cow::Borrowed(after),
            ]
        } else {
            vec![]
        }
    }
    match input {
        NetworkPackage::Hello(x) => [&b"<HELLO>"[..], x.as_ref(), b"</HELLO>"].concat(),
        NetworkPackage::Addressed { src, dst, data } => iter::once(Cow::Borrowed(&b"<PACKT>"[..]))
            .chain(compose_option(b"<SRCCN>", src.as_ref(), b"</SRCCN>"))
            .chain(compose_option(b"<DESCN>", dst.as_ref(), b"</DESCN>"))
            .chain(iter::once(Cow::Borrowed(&b"<DATAS>"[..])))
            .chain(data.compose())
            .chain([Cow::Borrowed(&b"</DATAS>"[..]), Cow::Borrowed(b"</PACKT>")])
            .fold(vec![], |mut state, new| {
                state.extend_from_slice(&new[..]);
                state
            }),
    }
}
