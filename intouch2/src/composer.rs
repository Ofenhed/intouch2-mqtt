use std::borrow::Cow;

use crate::{object::NetworkPackage, object_traits::DatasContent};

#[must_use]
pub fn compose_network_data<'a, 'b: 'a>(input: &'a NetworkPackage<'b>) -> Vec<u8> {
    fn compose_option<'a>(
        before: &'a [u8],
        content: Option<&'a Cow<'a, [u8]>>,
        after: &'a [u8],
    ) -> Option<Vec<Cow<'a, [u8]>>> {
        content.map(|x| {
            vec![
                Cow::Borrowed(before),
                Cow::Borrowed(x),
                Cow::Borrowed(after),
            ]
        })
    }
    match input {
        NetworkPackage::Hello(x) => [&b"<HELLO>"[..], x.as_ref(), b"</HELLO>"].concat(),
        NetworkPackage::Addressed { src, dst, data } => {
            let mut composed: Vec<Cow<[u8]>> = vec![Cow::Borrowed(b"<PACKT>")];
            if let Some(mut src) = compose_option(b"<SRCCN>", src.as_ref(), b"</SRCCN>") {
                composed.append(&mut src);
            }
            if let Some(mut dst) = compose_option(b"<DESCN>", dst.as_ref(), b"</DESCN>") {
                composed.append(&mut dst);
            }
            composed.push(Cow::Borrowed(b"<DATAS>"));
            composed.append(&mut data.compose().collect::<Vec<_>>());
            composed.append(&mut vec![
                Cow::Borrowed(b"</DATAS>"),
                Cow::Borrowed(b"</PACKT>"),
            ]);
            composed.into_iter().fold(vec![], |mut state, new| {
                state.extend_from_slice(&new[..]);
                state
            })
        }
    }
}
