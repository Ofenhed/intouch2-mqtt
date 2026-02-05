#![allow(incomplete_features)]
#![feature(generic_const_exprs)]
#![recursion_limit = "512"]
use std::borrow::Cow;

use rand::*;

pub mod composer;
pub mod datas;
pub mod object;
mod object_macro;
mod object_traits;
pub mod parser;
mod to_static;
pub use to_static::*;

pub fn static_cow<T>(from: impl AsRef<[T]>) -> Cow<'static, [T]>
where
    [T]: ToOwned,
{
    Cow::Owned(from.as_ref().to_owned())
}

pub fn generate_uuid() -> Box<[u8]> {
    let mut rng = rand::thread_rng();
    let characters = b"0123456789abcdef".to_vec();
    let hexed: Vec<u8> = [0; 32]
        .iter()
        .map(|_| characters[rng.gen_range(0..16)])
        .collect();
    [
        b"IOS",
        &hexed[0..8],
        b"-",
        &hexed[8..12],
        b"-",
        &hexed[12..16],
        b"-",
        &hexed[16..24],
        b"-",
        &hexed[24..32],
    ]
    .concat()
    .into()
}

#[cfg(test)]
mod tests;
