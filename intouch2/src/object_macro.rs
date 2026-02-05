#[macro_export]
macro_rules! gen_packages {
  // Ignore tags when building struct
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $field:literal : Tag $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $($struct_lifetime)? $struct { $($current)* } => $($($rest)*)? }
  };

  // Add struct pointer member (and add lifetime if not already added)
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $(#[doc = $docs:literal])* $field:ident : &$field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum 'a $(#[$meta:meta])* $struct { $($current)* $(#[doc = $docs])* pub $field: <&'a $field_type as $crate::object::ActualType>::Type, } => $($($rest)*)? }
  };

  // Add non-pointer struct member
  (BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* } => $(#[doc = $docs:literal])* $field:ident : $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $($struct_lifetime)? $(#[$meta:meta])* $struct { $($current)* $(#[doc = $docs])* pub $field: <$field_type as $crate::object::ActualType>::Type, } => $($($rest)*)? }
  };

  // Completed struct with lifetime
  (BUILD_STRUCT_ARGS $enum:ident $struct_lifetime:lifetime $(#[$meta:meta])* $struct:ident { $($current:tt)* } => ) => {
      $crate::gen_packages!{ FINISH_BUILD_STRUCT_ARGS $enum $struct_lifetime $(#[$meta])* $struct { $($current)* } }
  };

  // Complete struct which does not have a lifetime
  (BUILD_STRUCT_ARGS $enum:ident $(#[$meta:meta])* $struct:ident { $($current:tt)* } => ) => {
      $crate::gen_packages!{ FINISH_BUILD_STRUCT_ARGS $enum $struct $(#[$meta])* { $($current)* } }
  };

  // Finalize the result of BUILD_STRUCT_ARGS
  (FINISH_BUILD_STRUCT_ARGS $enum:ident $($struct_lifetime:lifetime)? $(#[$meta:meta])* $struct:ident { $($current:tt)* }) => {
      #[derive(Debug, PartialEq, Eq, Clone)]
      $(#[$meta])*
      #[cfg_attr(feature = "serialize", derive(serde::Serialize))]
      pub struct $struct $(<$struct_lifetime>)? {
          $($current)*
      }
  };

  // Add tag to parser and composer
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $(#[doc = $docs:literal])* $field:literal : Tag $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          $($li)? $struct $tag
          [ $($member)* ]
          [ $($parser)* (_tag, nom::bytes::complete::tag($field)) ]
          [ $($composer)* ($field) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };

  // Add a pointer field to the parser and composer
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $(#[doc = $docs:literal])* $vis:vis $field:ident : & $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          'a $struct $tag
          [ $($member)* $field ]
          [ $($parser)* ($field: &'a $field_type) ]
          [ $($composer)* (.$field.compose()) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };

  // Add a non-pointer member to the parser and composer
  (BUILD_STRUCT_IMPLS $($li:lifetime)? $struct:ident $tag:literal [$($member:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => $(#[doc = $docs:literal])* $vis:vis $field:ident : $field_type:ty $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS
          $($li)? $struct $tag
          [ $($member)* $field ]
          [ $($parser)* ($field: $field_type) ]
          [ $($composer)* (.$field.compose()) ]
          { $($saved)* }
          => $($($rest)*)?
      }
  };

  // Struct with lifetime created, generate implementations for type with lifetime
  (BUILD_STRUCT_IMPLS $struct_life:lifetime $struct:ident $tag:literal [$($field:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => ) => {
    $crate::gen_packages!{ GENERATE_STRUCT_IMPLS { SAME $struct_life } $struct $tag [$($field)*] [ $($parser)* ] [$($composer)*] { $($saved)* } }
  };
  // Struct without lifetime created, generate implementations for static type
  (BUILD_STRUCT_IMPLS $struct:ident $tag:literal [$($field:ident)*] [ $($parser:tt)* ] [ $($composer:tt)* ] { $($saved:tt)* } => ) => {
    $crate::gen_packages!{ GENERATE_STRUCT_IMPLS { STATIC 'a } $struct $tag [$($field)*] [ $($parser)* ] [ $($composer)* ] { $($saved)* } }
  };

  // Enumerate to_static for all struct members for structs with a lifetime
  (ENUMERATE_STRUCT_TO_STATIC $struct:ident $struct_life:lifetime [ $($($field:ident)+)? ]) => {
      impl<$struct_life> $crate::ToStatic for $struct<$struct_life> {
          type Static = $struct<'static>;

          fn to_static(&self) -> Self::Static {
              Self::Static $({
                  $($field: $crate::ToStatic::to_static(&self.$field),)*
              })?
          }
      }
  };

  // Structs without lifetime can simply be cloned to be made static
  (ENUMERATE_STRUCT_TO_STATIC $struct:ident [ $($($field:ident)+)? ] ) => {
      impl $crate::ToStatic for $struct {
          type Static = Self;

          fn to_static(&self) -> Self::Static {
              Self {$(
                  $($field: $crate::ToStatic::to_static(&self.$field),)*
              )?}
          }
      }
      impl From<&$struct> for $struct {
          fn from(other: &$struct) -> $struct {
              other.to_static()
          }
      }
  };

  // Throw compile error if both $struct_life and $trait_life has a lifetime
  (ASSERT_HAS_SINGLE_LIFETIME $lt:lifetime) => {};
  (ASSERT_HAS_SINGLE_LIFETIME $($lt:lifetime $($lt2:lifetime)+)? ) => { compile_error!{ "Exactly one lifetime must be set for GENERATE_STRUCT_IMPLS" } };

  // Generate implementations with either anonymous lifetime ($trait_life) or struct lifetime
  // ($struct_life). This is required, since both the TaggedDatasContent and DatasContent requires
  // a lifetime.
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
    { $($enum:ident)? [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] $($rest:tt)* } ) => {
      $crate::gen_packages!{ ASSERT_HAS_SINGLE_LIFETIME $($struct_life)? $($trait_life)? }
      $crate::gen_packages!{ WITH_TYPES_LIST $($enum)? [$($const)*] [$($($life)? $arg)* $($struct_life)? $struct] => $($rest)* }

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

      // Add trait implementations for structs with lifetime
      $crate::gen_packages!{ ENUMERATE_STRUCT_TO_STATIC $struct $($struct_life)? [ $($field)* ] }
      $crate::gen_packages!{ ADD_FROM_ENUM_IMPL $($enum)? $struct { $(SAME $struct_life)? $(STATIC $trait_life)? } }
  };

  // Ignore when no enum is defined
  (ADD_FROM_ENUM_IMPL $struct:ident
    $( { SAME $struct_life:lifetime } )?
    $( { STATIC $trait_life:lifetime } )?) => {};

  // Add from struct for enum
  (ADD_FROM_ENUM_IMPL $enum:ident $struct:ident
    $( { SAME $struct_life:lifetime } )?
    $( { STATIC $trait_life:lifetime } )?) => {
      $(
        impl<$struct_life> From<&$struct<$struct_life>> for $struct<'static> {
            fn from(other: &$struct<$struct_life>) -> $struct<'static> {
                $crate::ToStatic::to_static(other)
            }
        }
        impl<$struct_life> From<$struct<$struct_life>> for $enum<$struct_life> {
            fn from(other: $struct<$struct_life>) -> $enum<$struct_life> {
                $enum::$struct(other)
            }
        }
      )?

      // Add trait implementations for structs without lifetime
      $(
        impl<$trait_life> From<$struct> for $enum<$trait_life> {
            fn from(other: $struct) -> $enum<$trait_life> {
                $enum::$struct(other)
            }
        }
      )?
  };

  // Entrypoint for generating a struct member
  (WITH_TYPES_LIST $($struct_lifetime:lifetime)? $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $(#[$meta:meta])* $struct:ident { $tag:literal : Tag, $($args:tt)* } $(,$($rest:tt)*)?) => {
      $crate::gen_packages!{ BUILD_STRUCT_ARGS $enum $struct {} => $($args)* }
      $crate::gen_packages!{ BUILD_STRUCT_IMPLS $struct $tag [] [] [] { $enum [$($const)*] [$($($life)? $arg)*] $($($rest)*)? } => $tag: Tag, $($args)* }
  };

  // Implement a simple package, only holding a tag and an array with the rest of the data
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

  // Ignore non-nested type lists
  (WITH_TYPES_LIST $enum:ident [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $($(#[$meta:meta])* $vis:vis $arg2:ident : $ty:ty ),+ $(,)*) => {};

  // Implement simple package, only holding a single tag
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

  // All members added, generate enum
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
        $crate::gen_packages!( PARSER_CONTENT $($const)* $($arg)* )(input)
      }
      #[allow(dead_code)]
      pub fn compose(&self) -> std::borrow::Cow<'a, [u8]> {
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

  // No enum defined, ignore
  (WITH_TYPES_LIST [$($const:ident)*] [$($($life:lifetime)? $arg:ident)*] => $($rest:tt)*) => {};

  // Generate nom parser. Nom has a limit of 21 parsers, so if there are more, then split the
  // parser into multiple levels.
  (PARSER_CONTENT $type1:ident $type2:ident $type3:ident $type4:ident $type5:ident $type6:ident $type7:ident $type8:ident $type9:ident $type10:ident $type11:ident $type12:ident $type13:ident $type14:ident $type15:ident $type16:ident $type17:ident $type18:ident $type19:ident $type20:ident $($type:ident)+) => {
    nom::branch::alt((
        Self :: parse_inner :: <$type1>,
        Self :: parse_inner :: <$type2>,
        Self :: parse_inner :: <$type3>,
        Self :: parse_inner :: <$type4>,
        Self :: parse_inner :: <$type5>,
        Self :: parse_inner :: <$type6>,
        Self :: parse_inner :: <$type7>,
        Self :: parse_inner :: <$type8>,
        Self :: parse_inner :: <$type9>,
        Self :: parse_inner :: <$type10>,
        Self :: parse_inner :: <$type11>,
        Self :: parse_inner :: <$type12>,
        Self :: parse_inner :: <$type13>,
        Self :: parse_inner :: <$type14>,
        Self :: parse_inner :: <$type15>,
        Self :: parse_inner :: <$type16>,
        Self :: parse_inner :: <$type17>,
        Self :: parse_inner :: <$type18>,
        Self :: parse_inner :: <$type19>,
        Self :: parse_inner :: <$type20>,
        $crate::gen_packages!{PARSER_CONTENT $($type)+}
    ))
  };
  (PARSER_CONTENT $($type:ident)*) => {
    nom::branch::alt(($(Self :: parse_inner :: <$type>),*))
  };

  // Entrypoint
  (pub enum $parse:ident { $($rest:tt)* }) => {
    $crate::gen_packages!{ WITH_TYPES_LIST $parse [] [] => $($rest)* }
  };
  ($(#[$meta:meta])* pub struct $parse:ident { $($rest:tt)* }) => {
    $(#[$meta])* pub struct $parse { $($rest)* }
    mod fuck_you {
        use super::*;
        $crate::gen_packages!{ BUILD_STRUCT_IMPLS $parse b"" [] [] [] {[] [] $($rest)*} => $($rest)*}
    }
  };
}
