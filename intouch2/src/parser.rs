extern crate nom;

use super::object::*;

use nom::{bytes::complete::*, combinator::opt, *};

use std::borrow::Cow;

pub type NomError = nom::error::Error<Vec<u8>>;
pub type InnerNomError<'a> = nom::error::Error<&'a [u8]>;

#[derive(thiserror::Error, Debug)]
pub enum ParseError {
    #[error("Error while parsing: {0}")]
    Parser(#[from] nom::Err<NomError>),
    #[error("Unexpected tailing data: {msg:?} {tail:?}")]
    TailingData {
        msg: NetworkPackage<'static>,
        tail: Box<[u8]>,
    },
}

fn surrounded<'a>(
    before: &'a [u8],
    after: &'a [u8],
) -> impl 'a + for<'r> Fn(&'r [u8]) -> IResult<&'r [u8], &'r [u8]> {
    move |input| {
        let (input, ((_, data), _)) = tag(before)
            .and(take_until(after))
            .and(tag(after))
            .parse(input)?;
        Ok((input, data))
    }
}

fn parse_hello_package<'a>(input: &'a [u8]) -> IResult<&'a [u8], NetworkPackage<'a>> {
    let (input, hello) = surrounded(b"<HELLO>", b"</HELLO>")(input)?;
    Ok((input, NetworkPackage::Hello(hello.into())))
}

fn parse_datas(input: &'_ [u8]) -> IResult<&'_ [u8], NetworkPackageData<'_>> {
    let (input, datas) = surrounded(b"<DATAS>", b"</DATAS>")(input)?;
    let ([], data) = NetworkPackageData::parse(datas)? else {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::TagClosure,
        )));
    };
    Ok((input, data))
}

fn parse_addressed_package<'a>(input: &'a [u8]) -> IResult<&'a [u8], NetworkPackage<'a>> {
    let (input, ((src, dst), datas)) = surrounded(b"<PACKT>", b"</PACKT>")
        .and_then(
            opt(surrounded(b"<SRCCN>", b"</SRCCN>"))
                .and(opt(surrounded(b"<DESCN>", b"</DESCN>")))
                .and(parse_datas),
        )
        .parse(input)?;
    Ok((
        input,
        NetworkPackage::Addressed {
            src: src.map(|x| x.into()),
            dst: dst.map(|x| x.into()),
            data: datas,
        },
    ))
}

impl<'a> DatasContent<'a> for u16 {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        nom::number::complete::be_u16(input)
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(self.to_be_bytes().into())
    }
}

impl<'a> DatasContent<'a> for i16 {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        nom::number::complete::le_i16(input)
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(self.to_le_bytes().into())
    }
}

impl<'a, T1: DatasContent<'a>, T2: DatasContent<'a>> DatasContent<'a> for (T1, T2) {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, t1) = T1::parse(input)?;
        let (input, t2) = T2::parse(input)?;
        Ok((input, (t1, t2)))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned([self.0.compose(), self.1.compose()].concat())
    }
}

impl<'a> DatasContent<'a> for &'a [u8] {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        Ok((&[], input))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Borrowed(self)
    }
}

impl<const LENGTH: usize> TransmutedArray for [u8; LENGTH] {
    const SIZE: usize = LENGTH;
}

impl<'a, T: DatasContent<'a> + Clone> DatasContent<'a> for Cow<'a, T> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, parsed) = T::parse(input)?;
        Ok((input, Cow::Owned(parsed)))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        T::compose(self)
    }
}

pub struct Take<T: ?Sized> {
    pub len: Option<usize>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: ?Sized> Default for Take<T> {
    fn default() -> Self {
        Self {
            len: None,
            _phantom: std::marker::PhantomData,
        }
    }
}

pub trait TakeMultiple<'a> {
    type Type;
    fn parse_multiple(self, input: &'a [u8]) -> nom::IResult<&'a [u8], Self::Type>;
}

impl<'a> TakeMultiple<'a> for Take<[u8]> {
    type Type = Cow<'a, [u8]>;

    fn parse_multiple(self, input: &'a [u8]) -> nom::IResult<&'a [u8], Self::Type> {
        if let Some(len) = self.len {
            let (input, result) =
                nom::bytes::complete::take::<usize, _, InnerNomError<'a>>(len)(input)?;
            Ok((input, Cow::Borrowed(result)))
        } else {
            Ok((&[], Cow::Borrowed(input)))
        }
    }
}

impl<'a> DatasContent<'a> for StatusChange<'a> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, (change, data)) = DatasContent::parse(input)?;
        Ok((input, Self { change, data }))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned([self.change.to_be_bytes().as_ref(), self.data.as_ref()][..].concat())
    }
}

impl<'a, T: DatasContent<'a> + Clone> DatasContent<'a> for Cow<'a, [T]>
where
    [T]: ToOwned,
{
    fn parse(mut input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let mut result = vec![];
        loop {
            if input.is_empty() {
                return Ok((input, result.into()));
            }
            let (new_input, new_parse) = <T as DatasContent>::parse(input)?;
            result.push(new_parse);
            input = new_input;
        }
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        let mut result = vec![];
        for element in self.as_ref() {
            result.push(element.compose().into_owned());
        }
        Cow::Owned(result.concat())
    }
}

pub fn parse_network_data<'a>(input: &'a [u8]) -> Result<NetworkPackage<'a>, ParseError> {
    match parse_hello_package
        .or(parse_addressed_package)
        .parse(input)
        .map_err(|x| ParseError::from(x.to_owned()))?
    {
        ([], msg) => Ok(msg),
        (tail, msg) => Err(ParseError::TailingData {
            tail: tail.into(),
            msg: NetworkPackage::<'static>::from(&msg),
        }),
    }
}
