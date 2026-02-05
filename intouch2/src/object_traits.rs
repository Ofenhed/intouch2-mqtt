use disjoint_impls::disjoint_impls;
use std::{borrow::Cow, ops::Deref};

pub mod dispatch {
    pub trait DatasType {
        type Group;
    }

    pub struct Simple;

    pub struct Tailing;

    pub struct Transmuted;
}

pub trait ActualType {
    type Type;
}

impl<'a> ActualType for &'a [u8] {
    type Type = Cow<'a, [u8]>;
}

pub trait SimpleDatasContent:
    Default + dispatch::DatasType<Group = dispatch::Simple> + 'static
{
    const VERB: &'static [u8];
}

pub trait TransmutedArray
where
    Self: Sized,
{
    const SIZE: usize = std::mem::size_of::<Self>();

    fn parse_bytes(input: &[u8]) -> nom::IResult<&[u8], &Self>
    where
        [(); Self::SIZE]:,
    {
        let array = input.split_first_chunk();
        if let Some((chunk, rest)) = array {
            let transmuted = unsafe { std::mem::transmute::<&[u8; Self::SIZE], &Self>(chunk) };
            Ok((rest, transmuted))
        } else {
            Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::IsNot,
            )))
        }
    }

    fn to_bytes(&self) -> &[u8; Self::SIZE] {
        unsafe { std::mem::transmute(self) }
    }

    fn to_bytes_soft(&self) -> &[u8]
    where
        [(); Self::SIZE]:,
    {
        &self.to_bytes()[..]
    }
}

impl<A: TransmutedArray> DatasType for A {
    type Group = Transmuted;
}

pub trait TailingDatasContent<'a>:
    dispatch::DatasType<Group = dispatch::Tailing> + Deref<Target = [u8]> + 'a
{
    const VERB: &'static [u8];

    fn from(tail: &'a [u8]) -> Self;

    fn into(&'_ self) -> &'a [u8];
}

use dispatch::*;
disjoint_impls! {
  pub trait DatasContent<'a>: Sized {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self>;

    fn compose(&self) -> Cow<'a, [u8]>;
  }

  impl<'a, A: SimpleDatasContent + DatasType<Group=Simple>> DatasContent<'a> for A {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
      if input == Self::VERB {
          Ok((&input[input.len()..], Default::default()))
      } else {
          Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::OneOf)))
      }
    }

    fn compose(&self) -> Cow<'a, [u8]> {
      Cow::Borrowed(Self::VERB)
    }
  }

  impl<'a, A: TransmutedArray + Clone + DatasType<Group=Transmuted> + 'a> DatasContent<'a> for A where [(); <A as TransmutedArray>::SIZE]: {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        A::parse_bytes(input).map(|(input, this)| (input, this.to_owned()))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
      Cow::Owned(self.to_bytes_soft().into())
    }
  }

  impl<'a, A: TailingDatasContent<'a> + DatasType<Group=Tailing>> DatasContent<'a> for A {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
      let (input, _) = nom::bytes::complete::tag(Self::VERB)(input)?;
      Ok((&input[input.len()..], A::from(input)))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
      let parts: &[&'a [u8]] = &[Self::VERB, A::into(self)];
      Cow::Owned(parts.concat())
    }
  }
}
disjoint_impls! {
  pub trait TaggedDatasContent<'a>: DatasContent<'a> {
    const VERB: &'static [u8];
  }

  impl<'a, A: SimpleDatasContent + DatasType<Group=Simple>> TaggedDatasContent<'a> for A {
    const VERB: &'static [u8] = A::VERB;
  }

  impl<'a, A: TailingDatasContent<'a> + DatasType<Group=Tailing>> TaggedDatasContent<'a> for A {
    const VERB: &'static [u8] = A::VERB;
  }
}
