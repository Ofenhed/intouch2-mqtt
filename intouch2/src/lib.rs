//#![feature(generic_const_exprs)]
use rand::*;

pub mod composer;
pub mod object;
pub mod parser;
pub mod datas;
pub mod known_datas;

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
  .concat().into()
}


#[cfg(test)]
mod tests;
