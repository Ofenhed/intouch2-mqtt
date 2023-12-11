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

fn parse_datas(input: &[u8]) -> IResult<&[u8], NetworkPackageData> {
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

impl<'a> DatasContent<'a> for u8 {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        Ok(nom::number::complete::u8(input)?)
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(self.to_be_bytes().into())
    }
}

impl<'a> DatasContent<'a> for u16 {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        Ok(nom::number::complete::be_u16(input)?)
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(self.to_be_bytes().into())
    }
}

impl<'a, T1: DatasContent<'a>, T2: DatasContent<'a>> DatasContent<'a> for (T1, T2) {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, t1) = T1::parse(input)?;
        let (input, t2) = T2::parse(input)?;
        Ok((input, (t1, t2)))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned([self.0.compose(), self.1.compose()].concat().into())
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

impl<'a, const LENGTH: usize> DatasContent<'a> for Cow<'a, [u8; LENGTH]> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (sized, rest) = input.split_at(LENGTH);
        if let Ok(sized) = sized.try_into() {
            Ok((rest, Cow::Borrowed(sized)))
        } else {
            debug_assert!(
                LENGTH > sized.len(),
                "Could not create sized array, even though data was available"
            );
            Err(nom::Err::Incomplete(nom::Needed::Size(unsafe {
                std::num::NonZeroUsize::new_unchecked(LENGTH - sized.len())
            })))
        }
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        match self {
            Cow::Borrowed(from) => Cow::Borrowed(&from[..]),
            Cow::Owned(x) => Cow::Owned(x[..].to_owned()),
        }
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
            _phantom: std::marker::PhantomData::default(),
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
                nom::bytes::complete::take::<usize, _, InnerNomError<'a>>(len as usize)(input)?;
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
        Cow::Owned(
            [self.change.to_be_bytes().as_ref(), self.data.as_ref()][..]
                .concat()
                .into(),
        )
    }
}

impl<'a> DatasContent<'a> for ReminderInfo {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (new_input, index) = u8::parse(input)?;
        let (input, index) = (
            new_input,
            ReminderIndex::from_repr(index).ok_or_else(|| {
                nom::Err::Failure(nom::error::make_error(
                    new_input,
                    nom::error::ErrorKind::OneOf,
                ))
            })?,
        );
        let (input, data) = DatasContent::parse(input)?;
        let (before_valid, _) = nom::bytes::complete::tag(b"\x01")(input)?;
        let (input, valid) = <u8 as DatasContent>::parse(before_valid)?;
        let valid = match valid {
            0 => false,
            1 => true,
            _ => {
                return Err(nom::Err::Failure(nom::error::make_error(
                    before_valid,
                    nom::error::ErrorKind::OneOf,
                )))?
            }
        };
        Ok((input, Self { index, data, valid }))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(
            [
                &[self.index as u8],
                self.data.to_be_bytes().as_ref(),
                if self.valid { b"\x01" } else { b"\x00" },
            ][..]
                .concat()
                .into(),
        )
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
