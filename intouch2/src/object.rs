pub use num_derive::{FromPrimitive, ToPrimitive};
pub use num_traits::{FromPrimitive, ToPrimitive};
use std::borrow::Cow;

pub use crate::object_traits::*;

pub use package_data::NetworkPackageData;

use crate::{static_cow, ToStatic};

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

impl<const N: usize> ToStatic for Cow<'_, [u8; N]> {
    type Static = Cow<'static, [u8; N]>;

    fn to_static(&self) -> Self::Static {
        match *self {
            Cow::Owned(x) => Cow::Owned(x),
            Cow::Borrowed(x) => Cow::Owned(x.clone()),
        }
    }
}

impl ToStatic for ReminderInfo {
    type Static = ReminderInfo;

    fn to_static(&self) -> Self::Static {
        self.clone()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::FromRepr)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
#[repr(u8)]
pub enum ReminderIndex {
    Invalid = 0,
    RinseFilter = 1,
    CleanFilter = 2,
    ChangeWater = 3,
    CheckSpa = 4,
    ChangeOzonator = 5,
    ChangeVisionCartridge = 6,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
pub struct ReminderInfo {
    pub index: ReminderIndex,
    pub data: u16,
    pub valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::FromRepr)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
#[repr(u8)]
pub enum WatercareType {
    Economy = 1,
    FilterCycle = 2,
}

pub struct StatusChangePlaceholder;

impl<'a, const LENGTH: usize> ActualType for &'a [u8; LENGTH] {
    type Type = Cow<'a, [u8; LENGTH]>;
}

impl<'a> ActualType for &'a [StatusChangePlaceholder] {
    type Type = Cow<'a, [StatusChange<'a>]>;
}

impl<'a> ActualType for &'a [ReminderInfo] {
    type Type = Cow<'a, [ReminderInfo]>;
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
actually_self!(u8, u16, WatercareType);

pub mod package_data {
    use super::*;
    crate::gen_packages! {
      pub enum NetworkPackageData {
        Ping(b"APING": Simple),
        Pong(b"APING\0": Simple),
        GetVersion {
            b"AVERS": Tag,
            seq: u8,
        },
        Packs( b"PACKS": Simple),
        RadioError(b"RFERR": Simple),
        WaterQualityError(b"WCERR": Simple),
        Version {
            b"SVERS": Tag,
            en_build: u16,
            en_major: u8,
            en_minor: u8,
            co_build: u16,
            co_major: u8,
            co_minor: u8,
        },
        PushStatus {
            b"STATP": Tag,
            length: u8,
            changes: &[StatusChangePlaceholder],
        },
        SetStatus {
            b"SPACK": Tag,
            seq: u8,
            pack_type: u8,
            len: u8,
            b"\x46": Tag,
            config_version: u8,
            log_version: u8,
            pos: u16,
            data: &[u8],
        },
        KeyPress {
            b"SPACK": Tag,
            seq: u8,
            pack_type: u8,
            b"\x02\x39": Tag,
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
        GetWatercare {
            b"GETWC": Tag,
            seq: u8,
        },
        WatercareGet {
            b"WCGET": Tag,
            mode: u8,
        },
        SetWatercare {
            b"SETWC": Tag,
            seq: u8,
            mode: u8,
        },
        WatercareSet {
            b"WCSET": Tag,
            mode: u8,
        },
        RequestWatercare {
            b"REQWC": Tag,
            remainder: u8,
        },
        ModifyWatercare {
            b"MDFWC": Tag,
            seq: u8,
            mode: u8,
            r#type: WatercareType,
            rule_index: u8,
            unknown: &[u8; 2],
            start_hour: u8,
            start_minute: u8,
            end_hour: u8,
            end_minutes: u8,
        },
        DeleteWatercare {
            b"DELWC": Tag,
            seq: u8,
            mode: u8,
            r#type: WatercareType,
            index: u8,
        },
        WatercareDeleted {
            b"WCDEL": Tag,
            mode: u8,
            r#type: WatercareType,
            index: u8,
        },
        AddWatercare {
            b"ADDWC": Tag,
            seq: u8,
            mode: u8,
            r#type: WatercareType,
            data: &[u8],
        },
        WatercareAdded {
            b"WCADD": Tag,
            mode: u8,
            r#type: WatercareType,
            unknown: u8,
        },
        ModifyWatercareResponse {
            b"WCMDF": Tag,
            data: &[u8],
        },
        RequestReminders {
            b"REQRM": Tag,
            seq: u8,
        },
        RemindersRequest {
            b"RMREQ": Tag,
            reminders: &[ReminderInfo],
        },
        MalformedRemindersRequest {
            b"RMREQ": Tag,
            reminders: &[u8],
        },
        WatercareRequest(b"WCREQ": Tailing),
        ChannelCurrent {
            b"CHCUR": Tag,
            channel: u8,
            signal_strength: u8,
        },
        GetChannel {
            b"CURCH": Tag,
            seq: u8,
        },
        FilesRequest(b"SFILE?": Simple),
        Files(b"FILES": Tailing),
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

#[derive(Eq, Debug, PartialEq, Clone)]
#[cfg_attr(feature = "serialize", derive(serde::Serialize))]
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
