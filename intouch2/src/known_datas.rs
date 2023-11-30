// use crate::datas::{ReadableData, WritableData};
use crate::datas::GeckoDatas;
use strum::{AsRefStr, FromRepr};

#[derive(Eq, Debug, PartialEq, Clone, Copy)]
pub enum SpaModel {
    Mine,
}

pub const fn data_size_for(model: SpaModel) -> u16 {
    match model {
        SpaModel::Mine => 637,
    }
}

macro_rules! color_type {
    ($name:ident, $position:expr) => {
        pub struct $name;

        impl $crate::datas::KnownData<'_> for $name {
            const POSITION: u16 = $position;
            const LENGTH: u16 = 1;

            type ReturnType = Option<StatusColorsType>;

            fn read_from(datas: &GeckoDatas) -> Self::ReturnType {
                StatusColorsType::from_repr(datas[usize::from(Self::POSITION)])
            }
        }
    };
}

macro_rules! color {
    ($name:ident, $position:expr) => {
        pub struct $name;

        impl<'a> $crate::datas::KnownData<'a> for $name {
            const POSITION: u16 = $position;
            const LENGTH: u16 = 3;

            type ReturnType = Color<'a>;

            fn read_from(datas: &'a GeckoDatas) -> Self::ReturnType {
                let start = usize::from(Self::POSITION);
                let end = start + usize::from(Self::LENGTH);
                Color(&datas[start..end])
            }
        }
    };
}

#[derive(Eq, Debug, PartialEq, FromRepr, AsRefStr)]
#[repr(u8)]
pub enum StatusColorsType {
    Off = 0,
    #[strum(serialize = "Slow Fade")]
    SlowFade = 1,
    #[strum(serialize = "Fast Fade")]
    FastFade = 2,
    Solid = 5,
}

// trait HasColorType: std::ops::Deref<Target = [u8]> {}
// impl HasColorType for DataRef<'_, PrimaryColorType> {}
// impl HasColorType for DataRef<'_, SecondaryColorType> {}

// pub trait ColorType {
//    fn color_type(&self) -> Option<StatusColorsType>;
//}
// impl<T: HasColorType> ColorType for T {
//    fn color_type(&self) -> Option<StatusColorsType> {
//        StatusColorsType::from_repr(self[0].into())
//    }
//}

color_type!(PrimaryColorType, 0x259);
color!(PrimaryColor, 0x25c);
color_type!(SecondaryColorType, 0x260);
color!(SecondaryColor, 0x263);

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Color<'a>(&'a [u8]);
impl Color<'_> {
    fn red(&self) -> u8 {
        self.0[0]
    }
    fn green(&self) -> u8 {
        self.0[1]
    }
    fn blue(&self) -> u8 {
        self.0[2]
    }
}

// data_type!(TargetTemperature, 0x1, 2);
// data_type!(TargetTemperatureCopy, 0x113, 2);
// data_type!(LightOnTimer, 0x131, 1);
// data_type!(Fountain, 0x16b, 1);

// struct ColorType;
// impl ReadableData for ColorType {
//    const POSITION: u16 = 0x259;
//    const LENGTH: u16 = 1;
//}
// struct Color;
// impl ReadableData for Color {
//    const POSITION: u16 = 0x25c;
//    const LENGTH: u16 = 3;
//}

//  ColorType = key(2, 89),
//  Red = key(2, 92),
//  Green = key(2, 93),
//  Blue = key(2, 94),
//  SecondaryColorType = key(2, 96),
//  TargetTemperatureLsb = key(0, 2),
//  TargetTemperatureMsb = key(0, 1),
//  TargetTemperatureLsbAgain = key(1, 20),
//  TargetTemperatureMsbAgain = key(1, 19),
//  SecondaryRed = key(2, 99),
//  SecondaryGreen = key(2, 100),
//  SecondaryBlue = key(2, 101),
//  LightOnTimer = key(1, 49),
//  Fountain = key(1, 107),
