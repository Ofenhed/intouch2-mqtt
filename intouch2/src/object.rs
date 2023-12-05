#![allow(unused_variables)]

use disjoint_impls::disjoint_impls;
use nom::{bytes::complete::tag, error::ErrorKind};
pub use num_derive::{FromPrimitive, ToPrimitive};
pub use num_traits::{FromPrimitive, ToPrimitive};
use std::{borrow::Cow, cmp::Ordering, ops::Deref};

#[derive(Eq, Debug, PartialEq, FromPrimitive)]
pub enum StatusColorsType {
    Off = 0,
    SlowFade = 1,
    FastFade = 2,
    Solid = 5,
}

pub mod dispatch {
    pub trait DatasType {
        type Group;
    }

    pub struct Simple;

    pub struct Tailing;
}

pub trait SimpleDatasContent:
    Default + dispatch::DatasType<Group = dispatch::Simple> + 'static
{
    const VERB: &'static [u8];
}

pub trait TailingDatasContent<'a>:
    dispatch::DatasType<Group = dispatch::Tailing> + Deref<Target = [u8]> + 'a
{
    const VERB: &'static [u8];

    fn from(tail: &'a [u8]) -> Self;

    fn into(&'_ self) -> &'a [u8];
}

use dispatch::{DatasType, Simple, Tailing};

use crate::{static_cow, ToStatic};
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
          Err(nom::Err::Error(nom::error::Error::new(input, ErrorKind::OneOf)))
      }
    }

    fn compose(&self) -> Cow<'a, [u8]> {
      Cow::Borrowed(Self::VERB)
    }
  }

  impl<'a, A: TailingDatasContent<'a> + DatasType<Group=Tailing>> DatasContent<'a> for A {
    fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
      let (input, _) = tag(Self::VERB)(input)?;
      Ok((&input[input.len()..], A::from(input)))
    }

    fn compose(&self) -> Cow<'a, [u8]> {
      let parts: &[&'a [u8]] = &[Self::VERB, A::into(&self)];
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

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct StatusChange<'a> {
    pub change: u16,
    pub data: Cow<'a, [u8; 2]>,
}

impl ToStatic for StatusChange<'_> {
    type Static = StatusChange<'static>;

    fn to_static(&self) -> Self::Static {
        Self::Static {
            change: self.change,
            data: self.data.to_static(),
        }
    }
}

pub trait ActualType {
    type Type;
}

impl<'a> ActualType for &'a [u8] {
    type Type = Cow<'a, [u8]>;
}

pub struct StatusChangePlaceholder;

impl<'a, const LENGTH: usize> ActualType for &'a [u8; LENGTH] {
    type Type = Cow<'a, [u8; LENGTH]>;
}

impl<'a> ActualType for &'a [StatusChangePlaceholder] {
    type Type = Cow<'a, [StatusChange<'a>]>;
}

macro_rules! actually_self {
    ($ty:ty $(,$($rest:tt)*)?) => {
        impl ActualType for $ty {
            type Type = $ty;
        }
        impl ToStatic for $ty {
            type Static = $ty;
            fn to_static(&self) -> Self::Static {
                *self
            }
        }
        actually_self!{ $($($rest)*)? }
    };
    () => {};
}
actually_self!(u8, u16);

#[macro_export]
macro_rules! gen_packages {
  (FIND_STRUCT_LIFETIMES $field:literal : Tag $(,$($rest:tt)*)?) => {
    $crate::gen_packages!{ FIND_STRUCT_LIFETIMES $($($rest)*)? }
  };
  (FIND_STRUCT_LIFETIMES $field:literal : &$type:ty $(,$($rest:tt)*)?) => {
    'a
  };
  (FIND_STRUCT_LIFETIMES $field:literal : $type:ty $(,$($rest:tt)*)?) => {
    $crate::gen_packages!{ FIND_STRUCT_LIFETIMES $($($rest)*)? }
  };
  (FIND_STRUCT_LIFETIMES ) => {};
  (FIND_STRUCT_ENUM_LIFETIME $($rest:tt)+) => {
    $crate::gen_packages!{ FIND_STRUCT_ENUM_LIFETIME => $crate::gen_packages!{ FIND_STRUCT_LIFETIMES $rest }}
  };
  (STRUCT_LIFETIME_TAG => 'static) => {};
  (STRUCT_LIFETIME_TAG => $l:lifetime) => {
    <$l>
  };

  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $field:literal : Tag $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $($struct_lifetime)? $struct { $($current)* } => $($($rest)*)? }
  };
  // Fixed size field pointer
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $field:ident : &$field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum 'a $(#[$meta:meta])* $struct { $($current)* pub $field: <&'a $field_type as $crate::object::ActualType>::Type, } => $($($rest)*)? }
  };
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $field:ident : $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $($struct_lifetime)? $(#[$meta:meta])* $struct { $($current)* pub $field: <$field_type as $crate::object::ActualType>::Type, } => $($($rest)*)? }
  };
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $field:ident : [$arr_type:ty $(; $len:expr)?] $(,$($rest:tt)*)?) => {
      compile_error!{"Type {} is not supported", $arr_type}
  };
  (DERIVE_CLONE_FOR_STATIC $struct_lifetime:lifetime) => {
    $enum_name <$struct_lifetime>
  };
  (DERIVE_CLONE_FOR_STATIC) => {
    #[derive(Clone)]
  };
  (BUILD_STRUCT_ARGS $enum:ident $struct_lifetime:lifetime $(#[$meta:meta])* $struct:ident { $($current:tt)* } => ) => {
      $crate::gen_packages!{ FINISH_BUILD_STRUCT_ARGS $enum $struct_lifetime $(#[$meta])* $struct { $($current)* } }
  };
  (BUILD_STRUCT_ARGS $enum:ident $(#[$meta:meta])* $struct:ident { $($current:tt)* } => ) => {
      $crate::gen_packages!{ FINISH_BUILD_STRUCT_ARGS $enum $struct $(#[$meta])* { $($current)* } }
  };
  (FINISH_BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* }) => {
      #[derive(Debug, PartialEq, Eq, Clone)]
      $(#[$meta])*
      #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
      pub struct $struct $(<$struct_lifetime>)? {
          $($current)*
      }
  };

  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $field:literal : Tag $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          $($li)? $struct $tag
          [ $($member)* ]
          [ $($parser)* (_tag, nom::bytes::complete::tag($field)) ]
          [ $($composer)* ($field) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $field:ident : & $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          'a $struct $tag
          [ $($member)* $field ]
          [ $($parser)* ($field: &'a $field_type) ]
          [ $($composer)* (.$field.compose()) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $field:ident : $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          $($li)? $struct $tag
          [ $($member)* $field ]
          [ $($parser)* ($field: $field_type) ]
          [ $($composer)* (.$field.compose()) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $field:ident : [u8], $($rest:tt)+) => {
    compile_error!{ "If you want arbitraty length elements that's not last, take that up with nom, or build it yourself!" }
  };
  (DATAS_CONTENT_LIFETIME) => {
    $crate :: object :: DatasContent< '_ >
  };
  (DATAS_CONTENT_LIFETIME $li:lifetime) => {
    $crate :: object :: DatasContent<$li>
  };
  (BUILD_STRUCT_IMPLS $struct_life:lifetime $struct:ident $tag:literal [$($field:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => ) => {
    $crate::gen_packages!{ GENERATE_STRUCT_IMPLS { SAME $struct_life } $struct $tag [$($field)*] [ $($parser)* ] [$($composer)*] { $($saved)* } }
  };
  // Above, but without lifetime
  (BUILD_STRUCT_IMPLS $struct:ident $tag:literal [$($field:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => ) => {
    $crate::gen_packages!{ GENERATE_STRUCT_IMPLS { STATIC 'a } $struct $tag [$($field)*] [ $($parser)* ] [ $($composer)* ] { $($saved)* } }
  };
  (GENERATE_STRUCT_TO_STATIC $enum:ident $struct:ident $struct_life:lifetime [ $($($field:ident)+)? ]) => {
      impl<$struct_life> $crate::ToStatic for $struct<$struct_life> {
          type Static = $struct<'static>;

          fn to_static(&self) -> Self::Static {
              Self::Static $({
                  $($field: $crate::ToStatic::to_static(&self.$field),)*
              })?
          }
      }
      impl<$struct_life> From<$struct<$struct_life>> for $enum<$struct_life> {
          fn from(other: $struct<$struct_life>) -> $enum<$struct_life> {
              $enum::$struct(other)
          }
      }
  };
  (GENERATE_STRUCT_TO_STATIC $enum:ident $struct:ident [ $($field:ident)* ] ) => {
      impl $crate::ToStatic for $struct {
          type Static = $struct;

          fn to_static(&self) -> $struct {
              self.to_owned()
          }
      }
      impl From<&$struct> for $struct {
          fn from(other: &$struct) -> $struct {
              other.to_owned()
          }
      }
      impl<'any> From<$struct> for $enum<'any> {
          fn from(other: $struct) -> $enum<'any> {
              $enum::$struct(other)
          }
      }
  };
  (ASSERT_HAS_SINGLE_LIFETIME $lt:lifetime) => {};
  (ASSERT_HAS_SINGLE_LIFETIME $($lt:lifetime $($lt2:lifetime)+)? ) => { compile_error!{ "Exactly one lifetime must be set for GENERATE_STRUCT_IMPLS" } };
  (GENERATE_STRUCT_IMPLS
    $( { SAME $struct_life:lifetime } )?
    $( { STATIC $trait_life:lifetime } )?
    $struct:ident $tag:literal
    [$($field:ident)*]
    [ $( ( $var:ident $(, $($parser:tt)* )? $(: $var_type:ty )? ) )* ]
    [ $( (
        $($static:literal)?
        $($(.$member:ident)+ $( ( $($args:tt)* ) )? )?
    ) )* ]
    // Saved
    { $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] $($rest:tt)* } ) => {
      $crate::gen_packages!{ ASSERT_HAS_SINGLE_LIFETIME $($struct_life)? $($trait_life)? }
      $crate::gen_packages!{ WITH_TYPES_LIST $enum [$($const)*] [$($($life)? $arg)* $($struct_life)? $struct] => $($rest)* }

      impl<$($struct_life)? $($trait_life)?> $crate::object::TaggedDatasContent<$($struct_life)? $($trait_life)?> for $struct<$($struct_life)?> {
        const VERB: &'static [u8] = $tag;
      }
      impl<$($struct_life)? $($trait_life)?> $crate::object::DatasContent<$($struct_life)? $($trait_life)?> for $struct<$($struct_life)?> {
        fn parse(input: & $($struct_life)? [u8]) -> nom::IResult<& $($struct_life)? [u8], Self> {
            #[allow(unused_imports)]
            use nom::Parser;

            $( let (input, $var) = $(
                    $($parser)*(input)?;)?
                    $(<<$var_type as ActualType>::Type as DatasContent>::parse(input)?;)?
            )*
            Ok((input, Self { $($field: $field.into(),)* }))
        }

        fn compose(&self) -> std::borrow::Cow<$($struct_life)? $($trait_life)?, [u8]> {
            let mut output = vec![];
            $(
                $(output.extend_from_slice($static);)?
                $({
                    let from_self = &self$(.$member)+ $(( $($args)* ))?;
                    output.extend_from_slice(AsRef::<[u8]>::as_ref(from_self));
                })?
            )*
            output.into()
        }
      }

      $crate::gen_packages!{ GENERATE_STRUCT_TO_STATIC $enum $struct $($struct_life)? [ $($field)* ] }
      $(
        impl<'a> From<&$struct<$struct_life>> for $struct<'static> {
            fn from(other: &$struct<$struct_life>) -> $struct<'static> {
                $crate::ToStatic::to_static(other)
            }
        }
      )?
  };
  (WITH_TYPES_LIST $($struct_lifetime:lifetime)? $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $(#[$meta:meta])* $struct:ident { $tag:literal : Tag, $($args:tt)* } $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $struct {} => $($args)* }
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS $struct $tag [] [] [] { $enum [$($const)*] [$($($life)? $arg)*] $($($rest)*)? } => $tag: Tag, $($args)* }
  };

  (WITH_TYPES_LIST $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $(#[$meta:meta])* $tailing:ident ( $verb:literal : Tailing ) $(,$($rest:tt)*)?) => {
    #[derive(Debug, PartialEq, Eq, Clone)]
    $(#[$meta])*
    #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
    pub struct $tailing<'a>(pub std::borrow::Cow<'a, [u8]>);
    impl $crate :: object :: dispatch :: DatasType for $tailing<'_> {
      type Group = $crate :: object :: dispatch :: Tailing;
    }
    impl<'a> $crate :: object :: TailingDatasContent<'a> for $tailing<'a> {
      const VERB: &'static [u8] = $verb;
      fn from(tail: &'a [u8]) -> Self {
        Self(tail.into())
      }

      fn into(&'_ self) -> &'a [u8] {
        // TODO: Figure out why this lifetime screws up
        unsafe { std::mem::transmute(&*self.0) }
      }
    }
    impl std::ops::Deref for $tailing<'_> {
        type Target = [u8];

        fn deref(&self) -> &[u8] {
            self.0.as_ref()
        }
    }
    impl $crate::ToStatic for $tailing<'_> {
        type Static = $tailing<'static>;

        fn to_static(&self) -> Self::Static {
            $tailing($crate::ToStatic::to_static(&self.0))
        }
    }
    impl<'a> From<&$tailing<'a>> for $tailing<'static> {
        fn from(other: &$tailing<'a>) -> $tailing<'static> {
            $crate::ToStatic::to_static(other)
        }
    }
    impl<'a> From<$tailing<'a>> for $enum<'a> {
      fn from(inner: $tailing<'a>) -> Self {
        Self :: $tailing ( inner )
      }
    }
    $crate::gen_packages!{ WITH_TYPES_LIST $enum [$($const)*] [$($($life)? $arg)* 'a $tailing] => $($($rest)*)? }
  };
  (WITH_TYPES_LIST $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $(#[$meta:meta])* $simple:ident ( $verb:literal : Simple ) $(,$($rest:tt)*)?) => {
    #[derive(Default)]
    #[derive(Debug, PartialEq, Eq, Clone, Copy)]
    $(#[$meta])*
    #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
    pub struct $simple;
    impl $crate::ToStatic for $simple {
        type Static = $simple;

        fn to_static(&self) -> Self {
            self.to_owned()
        }
    }
    impl $crate :: object :: dispatch :: DatasType for $simple {
      type Group = $crate :: object :: dispatch :: Simple;
    }
    impl $crate :: object :: SimpleDatasContent for $simple {
      const VERB: &'static [u8] = $verb;
    }
    impl From<$simple> for $enum<'_> {
      fn from(_: $simple) -> Self {
        Self :: $simple
      }
    }
    $crate::gen_packages!{ WITH_TYPES_LIST $enum [$($const)* $simple] [$($($life)? $arg)*] => $($($rest)*)? }
  };
  (WITH_TYPES_LIST $enum_name:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $(,)?) => {
    #[derive(Debug, PartialEq, Eq, Clone)]
    #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
    pub enum $enum_name<'a> {
      $($const,)*
      $($arg($arg$(<$life>)?),)*
    }
    impl<'a> $enum_name<'a> {
      pub fn parse_inner<I: DatasContent<'a> + Into<Self>>(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        let (input, parsed) = I :: parse(input)?;
        Ok((input, parsed.into()))
      }
      pub fn parse(input: &'a [u8]) -> nom::IResult<&'a [u8], Self> {
        $crate::gen_packages!{ PARSER_CONTENT input $($const)* $($arg)* }
      }
      pub fn compose(&self) -> std::borrow::Cow<[u8]> {
          match self {
              $($enum_name::$const => $const.compose(),)*
              $($enum_name::$arg(x) => x.compose(),)*
          }
      }
    }

    impl<'a> From<&$enum_name<'a>> for $enum_name<'static> {
        fn from(from: &$enum_name<'a>) -> Self {
            match from {
                $($enum_name::$const => $enum_name::$const,)*
                $($enum_name::$arg(x) => $enum_name::$arg(std::convert::Into::into(x)),)*
            }

        }
    }
  };
  (PARSER_CONTENT $input:ident $($type:ident)*) => {
    nom::branch::alt(($(Self :: parse_inner :: <$type>),*))($input)
  };
  (pub enum $parse:ident { $($rest:tt)* }) => {
    $crate::gen_packages!{ WITH_TYPES_LIST $parse [] [] => $($rest)* }
  };
}

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub enum PackAction<'a> {
    Set {
        config_version: u8,
        log_version: u8,
        pos: u16,
        data: Cow<'a, [u8]>,
    },
    KeyPress {
        key: u8,
    },
}

impl ToStatic for PackAction<'_> {
    type Static = PackAction<'static>;

    fn to_static(&self) -> Self::Static {
        match self.to_owned() {
            PackAction::Set {
                config_version,
                log_version,
                pos,
                data,
            } => PackAction::Set {
                config_version,
                log_version,
                pos,
                data: data.to_static(),
            },
            PackAction::KeyPress { key } => PackAction::KeyPress { key },
        }
    }
}

pub struct PackActionPlaceholder;
impl<'a> ActualType for &'a PackActionPlaceholder {
    type Type = PackAction<'a>;
}

pub mod package_data {
    use super::*;
    gen_packages! {
      pub enum NetworkPackageData {
        Ping(b"APING": Simple),
        Pong(b"APING\0": Simple),
        GetVersion( b"AVERS": Simple),
        Packs( b"PACKS": Simple),
        RadioError(b"RFERR": Simple),
        WaterQualityError(b"WCERR": Simple),
        Version(b"SVERS": Tailing),
        PushStatus {
            b"STATP": Tag,
            length: u8,
            changes: &[StatusChangePlaceholder],
        },
        SetStatus {
            b"SPACK": Tag,
            seq: u8,
            b"\x46": Tag,
            len: u8,
            config_version: u8,
            log_version: u8,
            pos: u16,
            data: &[u8],
        },
        KeyPress {
            b"SPACK": Tag,
            seq: u8,
            b"\x39": Tag,
            len: u8,
            key: u8,
        },
        PushStatusAck {
            b"STATQ": Tag,
            seq: u8,
        },
        RequestStatus {
            b"STATU": Tag,
            seq: u8,
            start: u16,
            length: u16,
        },
        Status {
            b"STATV": Tag,
            seq: u8,
            next: u8,
            length: u8,
            data: &[u8],
        },
        GetWaterQuality(b"GETWC": Simple),
        Unknown(b"": Tailing),
      }
    }
}

impl NetworkPackageData<'_> {
    pub fn display(&self) -> String {
        match self {
            NetworkPackageData::Unknown(data) => {
                format!("Unknown: {}", String::from_utf8_lossy(data))
            }
            x => format!("{:?}", x),
        }
    }
}

impl ToStatic for NetworkPackageData<'_> {
    type Static = NetworkPackageData<'static>;
    fn to_static(&self) -> Self::Static {
        self.into()
    }
}
impl ToStatic for NetworkPackage<'_> {
    type Static = NetworkPackage<'static>;
    fn to_static(&self) -> Self::Static {
        self.into()
    }
}

pub use package_data::NetworkPackageData;
// trace_macros!(false);

pub type PushStatusValue = (u8, u8);

const fn key(v1: u8, v2: u8) -> isize {
    (((v1 as u16) << 8) + v2 as u16) as isize
}

fn to_push_status_index(v1: u8, v2: u8) -> isize {
    key(v1, v2)
}

fn from_push_status_index(v: isize) -> (u8, u8) {
    ((v >> 8) as u8, v as u8)
}

#[derive(Eq, Debug, PartialEq, FromPrimitive, ToPrimitive, Hash, Copy, Clone)]
pub enum PushStatusIndex {
    ColorType = key(2, 89),
    Red = key(2, 92),
    Green = key(2, 93),
    Blue = key(2, 94),
    SecondaryColorType = key(2, 96),
    TargetTemperatureLsb = key(0, 2),
    TargetTemperatureMsb = key(0, 1),
    TargetTemperatureLsbAgain = key(1, 20),
    TargetTemperatureMsbAgain = key(1, 19),
    SecondaryRed = key(2, 99),
    SecondaryGreen = key(2, 100),
    SecondaryBlue = key(2, 101),
    LightOnTimer = key(1, 49),
    Fountain = key(1, 107),
}

#[derive(Debug, Hash, Eq, PartialEq, Copy, Clone)]
pub enum PushStatusKey {
    Keyed(PushStatusIndex),
    Indexed(u8, u8),
}

pub type PushStatusList = ::std::collections::HashMap<PushStatusKey, PushStatusValue>;

#[derive(Eq, Debug, PartialEq, Clone)]
pub enum ErrorType {
    Radio,
    WaterQuality,
}

//#[derive(Default, Debug)]
// struct Ping;
//
// impl SimpleDatasContent for Ping {
//  const VERB: &'static [u8] = b"PING";
//}
//
//#[derive(Default, Debug)]
// struct Pong;
//
// impl SimpleDatasContent for Pong {
//  const VERB: &'static [u8] = b"PING\0";
//}
//
//#[derive(Default, Debug)]
// struct GetVersion;
//
// impl SimpleDatasContent for GetVersion {
//  const VERB: &'static [u8] = b"AVERS";
//}
//
//#[derive(Default, Debug)]
// struct Version<'a>(ByteString<'a>);
//
// impl TailingDatasContent for Version<'_> {
//  const VERB: &'static [u8] = b"AVERS";
//
//  fn from(tail: &[u8]) -> Self {
//    Self(tail.into())
//  }
//
//  fn into(&self) -> impl From<&[u8]> {
//    self.0
//  }
//}

//#[derive(Eq, Debug, PartialEq)]
// pub enum NetworkPackageData<'a> {
//  Ping,
//  Pong,
//  GetVersion,
//  Version(ByteString<'a>),
//  PushStatus(ByteString<'a>),
//  UnparsablePushStatus(ByteString<'a>),
//  PushStatusAck,
//  Packs,
//  Error(ErrorType),
//  Unknown(ByteString<'a>),
//}
// impl NetworkPackageData<'_> {
//  pub fn to_static<'a>(&'a self) -> NetworkPackageData<'static> {
//    use NetworkPackageData as X;
//    match self {
//      X::Ping => X::Ping,
//      X::Pong => X::Pong,
//      X::GetVersion => X::GetVersion,
//      X::Version(x) => X::Version(x.clone().into_owned().into()),
//      X::PushStatus(x) => X::PushStatus(x.clone().into_owned().into()),
//      X::UnparsablePushStatus(x) => X::UnparsablePushStatus(x.clone().into_owned().into()),
//      X::PushStatusAck => X::PushStatusAck,
//      X::Packs => X::Packs,
//      X::Error(x) => X::Error(x.clone()),
//      X::Unknown(x) => X::Unknown(x.clone().into_owned().into()),
//    }
//  }
//}

// impl std::fmt::Display for NetworkPackageData<'_> {
//    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//        fn bs<'a>(x: &'a ByteString) -> Cow<'a, str> {
//            String::from_utf8_lossy(x)
//        }
//
//        match self {
//            NetworkPackageData::Ping => f.write_str("Ping"),
//            NetworkPackageData::Pong => f.write_str("Pong"),
//            NetworkPackageData::GetVersion => f.write_str("GetVersion"),
//            NetworkPackageData::Version(version) => f.write_fmt(format_args!("Version({})",
// bs(version))),            NetworkPackageData::PushStatus(status) =>
// f.write_fmt(format_args!("PushStatus({status:?})")),
// NetworkPackageData::UnparsablePushStatus(status) =>
// f.write_fmt(format_args!("UnparsablePushStatus({})", bs(status))),
// NetworkPackageData::PushStatusAck => f.write_str("PushStatusAck"),
// NetworkPackageData::Packs => f.write_str("Packs"),            NetworkPackageData::Error(e) =>
// f.write_fmt(format_args!("Error({e:?})")),            NetworkPackageData::Unknown(data) =>
// f.write_fmt(format_args!("Unknown({})", bs(data))),        }
//    }
//}

#[derive(Eq, Debug, PartialEq, Clone)]
pub enum NetworkPackage<'a> {
    Addressed {
        src: Option<Cow<'a, [u8]>>,
        dst: Option<Cow<'a, [u8]>>,
        data: NetworkPackageData<'a>,
    },
    Hello(Cow<'a, [u8]>),
}

impl<'a> From<&NetworkPackage<'a>> for NetworkPackage<'static> {
    fn from(package: &NetworkPackage<'a>) -> NetworkPackage<'static> {
        match package {
            NetworkPackage::Addressed { src, dst, data } => NetworkPackage::Addressed {
                src: src.as_ref().map(static_cow),
                dst: dst.as_ref().map(static_cow),
                data: data.to_static(),
            },
            NetworkPackage::Hello(x) => NetworkPackage::Hello(static_cow(x)),
        }
    }
}

impl std::fmt::Display for NetworkPackage<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkPackage::Addressed { src, dst, data } => f.write_fmt(format_args!(
                "Addressed({}, {}, {})",
                src.as_ref()
                    .map(|x| String::from_utf8_lossy(&x))
                    .unwrap_or("NULL".into()),
                dst.as_ref()
                    .map(|x| String::from_utf8_lossy(&x))
                    .unwrap_or("NULL".into()),
                data.display()
            )),
            NetworkPackage::Hello(msg) => {
                f.write_fmt(format_args!("Hello({})", String::from_utf8_lossy(msg)))
            }
        }
    }
}

impl NetworkPackage<'_> {
    pub fn to_static(&self) -> NetworkPackage<'static> {
        use NetworkPackage as X;
        match self {
            X::Addressed { src, dst, data } => X::Addressed {
                src: src.clone().map(|x| x.into_owned().into()),
                dst: dst.clone().map(|x| x.into_owned().into()),
                data: data.to_static(),
            },
            X::Hello(x) => X::Hello(x.clone().into_owned().into()),
        }
    }
}

#[derive(Eq, Debug, PartialEq)]
pub enum Temperature {
    Celcius(u8),
    UncertainCelcius(u8, u8),
}

impl Temperature {
    pub fn uncertain(lsb: u8, previous: Option<u8>) -> Self {
        let lsb32 = lsb as u32;
        let low_result = ((1 << 8) + lsb32) / 18;
        let high_result = ((2 << 8) + lsb32) / 18;
        if let Some(previous) = previous {
            let translated = (previous as u32) * 18;
            let msb = translated >> 8;
            Temperature::certain(msb as u8, lsb)
        } else {
            Temperature::UncertainCelcius(low_result as u8, high_result as u8)
        }
    }
    pub fn certain(msb: u8, lsb: u8) -> Self {
        let msb = msb as u32;
        let lsb = lsb as u32;
        let result = ((msb << 8) + lsb) / 18;
        Temperature::Celcius(result as u8)
    }
}

impl Ord for PushStatusKey {
    fn cmp(&self, other: &Self) -> Ordering {
        let first = match self {
            PushStatusKey::Indexed(x, y) => (*x, *y),
            PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
        };
        let second = match other {
            PushStatusKey::Indexed(x, y) => (*x, *y),
            PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
        };
        first.cmp(&second)
    }
}

impl PartialOrd for PushStatusKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(&other))
    }
}

pub fn to_push_status_key(field_group: u8, field_name: u8) -> PushStatusKey {
    if let Some(enumed) = FromPrimitive::from_isize(to_push_status_index(field_group, field_name)) {
        PushStatusKey::Keyed(enumed)
    } else {
        PushStatusKey::Indexed(field_group, field_name)
    }
}

pub fn from_push_status_key(key: &PushStatusKey) -> (u8, u8) {
    match key {
        PushStatusKey::Keyed(x) => from_push_status_index(ToPrimitive::to_isize(x).unwrap()),
        PushStatusKey::Indexed(x, y) => (*x, *y),
    }
}
