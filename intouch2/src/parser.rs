extern crate nom;



use super::object::*;

use nom::{bytes::complete::*, combinator::opt, *};

use std::{borrow::Cow};

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

// impl<T: DatasContent<'static> + 'static> TakeMultiple<'static> for [T] where [T]: ToOwned {
//    type Type = Cow<'static, [T]>;
//
//    fn take(self, input: &'a [u8]) -> nom::IResult<&'a [u8], Self::Type> {
//        if let Some(len) = self.len {
//            //let collection =
//            let (input, result) = nom::bytes::complete::take::<usize, _, InnerNomError<'a>>(len as
// usize)(input)?;            Ok((input, Cow::Borrowed(result)))
//        } else {
//            Ok((&[], Cow::Borrowed(input)))
//        }
//    }
//}

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

impl<'a> DatasContent<'a> for PackAction<'a> {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        match u8::parse.and(u8::parse).parse(input)? {
            (input, (57, key)) => Ok((input, PackAction::KeyPress { key })),
            (input, (70, config_version)) => {
                let (input, ((log_version, pos), data)) = DatasContent::parse
                    .and(DatasContent::parse)
                    .and(DatasContent::parse)
                    .parse(input)?;
                Ok((
                    input,
                    PackAction::Set {
                        config_version,
                        log_version,
                        pos,
                        data,
                    },
                ))
            }
            _ => Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Tag,
            ))),
        }
    }

    fn compose(&self) -> Cow<'a, [u8]> {
        Cow::Owned(match self {
            PackAction::KeyPress { key } => [57u8, *key].into(),
            PackAction::Set {
                config_version,
                log_version,
                pos,
                data,
            } => {
                let mut result = Vec::with_capacity(5 + data.len());
                result.push(70u8);
                result.extend_from_slice(&config_version.to_be_bytes());
                result.extend_from_slice(&log_version.to_be_bytes());
                result.extend_from_slice(&pos.to_be_bytes());
                result.extend_from_slice(data);
                result
            }
        })
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

// fn calculate_rgba_from_rgb(r: u8, g: u8, b: u8) -> (u8, u8, u8, u8) {
//  let intencity = r + g + b;
//  let max = ::std::cmp::max(r, ::std::cmp::max(g, b));
//
//  let mul = intencity as f32 / max as f32;
//  fn conv(x: f32) -> u8 {
//    let y = x as u8;
//    y
//  }
//  (
//    conv(r as f32 * mul),
//    conv(g as f32 * mul),
//    conv(b as f32 * mul),
//    intencity,
//  )
//}
// pub fn get_status_rgba(
//  data: &PushStatusList,
//) -> (Option<(u8, u8, u8, u8)>, Option<(u8, u8, u8, u8)>) {
//  use PushStatusIndex::{Blue, Green, Red, SecondaryBlue, SecondaryGreen, SecondaryRed};
//
//  let get = |x: &PushStatusIndex| data.get(&PushStatusKey::Keyed(*x));
//  let fst = |&(x, _)| x;
//
//  let (pr, pg, pb, got_primary) = match (get(&Red), get(&Green), get(&Blue)) {
//    (None, None, None) => (0, 0, 0, false),
//    (r, g, b) => (
//      fst(r.unwrap_or(&(0, 0))),
//      fst(g.unwrap_or(&(0, 0))),
//      fst(b.unwrap_or(&(0, 0))),
//      true,
//    ),
//  };
//
//  let (sr, sg, sb, got_secondary) = match (
//    get(&SecondaryRed),
//    get(&SecondaryGreen),
//    get(&SecondaryBlue),
//  ) {
//    (None, None, None) => (0, 0, 0, false),
//    (r, g, b) => (
//      fst(r.unwrap_or(&(0, 0))),
//      fst(g.unwrap_or(&(0, 0))),
//      fst(b.unwrap_or(&(0, 0))),
//      true,
//    ),
//  };
//
//  let left = if got_primary {
//    Some(calculate_rgba_from_rgb(pr, pg, pb))
//  } else {
//    None
//  };
//  let right = if got_secondary {
//    Some(calculate_rgba_from_rgb(sr, sg, sb))
//  } else {
//    None
//  };
//
//  (left, right)
//}
// pub fn get_temperature(
//  data: &'_ PushStatusList,
//  previous_temperature: Option<u8>,
//) -> Option<Temperature> {
//  let mut best_result = None;
//  for (index, (msb, lsb)) in data.iter() {
//    if let PushStatusKey::Keyed(key) = index {
//      let result = match key {
//        PushStatusIndex::TargetTemperatureLsb | PushStatusIndex::TargetTemperatureLsbAgain => {
//          Some(Temperature::uncertain(*msb, previous_temperature))
//        }
//        PushStatusIndex::TargetTemperatureMsb | PushStatusIndex::TargetTemperatureMsbAgain => {
//          Some(Temperature::certain(*msb, *lsb))
//        }
//        _ => None,
//      };
//      match (&best_result, &result) {
//        (_, Some(Temperature::Celcius(_))) => {
//          best_result = result;
//          break;
//        }
//        (None, _) => {
//          best_result = result;
//        }
//        _ => (),
//      }
//    }
//  }
//  best_result
//}

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
