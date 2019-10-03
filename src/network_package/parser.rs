extern crate nom;

use super::object::*;

use nom::*;

use std::collections::HashMap;

fn surrounded<'a>(before: &'a [u8], after: &'a [u8]) -> impl 'a + for<'r> Fn(&'r [u8]) -> IResult<&'r [u8], &'r [u8]> {
  move |input| 
    do_parse!(input, 
              tag!(before) >> 
              data: take_until!(after) >> 
              tag!(after) >> 
              (data))
}

fn parse_hello_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  let (input, hello) = surrounded(b"<HELLO>", b"</HELLO>")(input)?;
  Ok((input, NetworkPackage::Hello(hello.to_vec())))
}

fn parse_pushed_package(input: &[u8]) -> Option<HashMap<u8, (u8, Vec<u8>)>> {
  let mut iter = input.iter();
  let count = iter.next()?;
  let mut ret = HashMap::new();
  for i in 0..*count {
    let pkg_type = iter.next()?;
    let group = iter.next()?;
    let mut members = vec![];
    for i in 0..2 {
      let curr = iter.next()?;
      members.push(*curr);
    }
    ret.insert(*group, (*pkg_type, members));
  };
  Some(ret)
}

fn parse_datas(input: &[u8]) -> IResult<&[u8], NetworkPackageData> {
  let (input, datas) = surrounded(b"<DATAS>", b"</DATAS>")(input)?;
  match datas {
    b"APING" => Ok((input, NetworkPackageData::Ping)),
    b"APING\0" => Ok((input, NetworkPackageData::Pong)),
    b"AVERSJ" => Ok((input, NetworkPackageData::GetVersion)),
    b"STATQ\xe5" => Ok((input, NetworkPackageData::PushStatusAck)),
    b"PACKS" => Ok((input, NetworkPackageData::Packs)),
    x => if let (b"SVERS", data) = x.split_at(5) { Ok((input, NetworkPackageData::Version(data.to_vec()))) }
         else if let (b"STATP", data) = x.split_at(5) {
           if let Some(partitioned) = parse_pushed_package(data) {
             if partitioned.len() > 1 {
               use PushStatusValue::*;
               let mut parsed = vec![];
               for (field_type, (sub_msg_type, value)) in &partitioned {
                 match sub_msg_type {
                   2 => { 
                     match field_type {
                       89 => match value[0] { 1 => parsed.push(FadeColors(StatusFadeColors::Slow)),
                                              2 => parsed.push(FadeColors(StatusFadeColors::Quick)),
                                              5 => parsed.push(FadeColors(StatusFadeColors::Off)),
                                              _ => {}},
                       92 => parsed.push(Red(value[0])),
                       93 => parsed.push(Green(value[0])),
                       94 => parsed.push(Blue(value[0])),
                       99 => parsed.push(SecondaryRed(value[0])),
                       100=> parsed.push(SecondaryGreen(value[0])),
                       101=> parsed.push(SecondaryBlue(value[0])),
                       _  => {},
                     }
                   },
                   1 => {
                     match field_type {
                       49 => parsed.push(LightOnTimer(value[0])),
                       107=> parsed.push(Fountain(value[0] != 0)),
                       _ => {},
                     }
                   },
                   _ => {},
                 }
               }
               let (intencity, max, has_colors): (u16, u8, bool) = parsed.iter().fold((0, 0, false), |(sum, max, has_colors), i| match i { Red(i) | Green(i) | Blue(i) => (sum + *i as u16, if i > &max { *i } else { max }, true), x => (sum, max, has_colors)});
               let mul = intencity as f32 / max as f32;
               fn conv(x: f32) -> u8 {
                   let y = x as u8;
                   y
               }
               if has_colors {
                 parsed.push(LightIntencity(std::cmp::min(std::u8::MAX, intencity as u8)));
               }
               let parsed = parsed.into_iter().map(|x| match x { Red(i)   => Red(  conv(i as f32 * mul)),
                                                                 Green(i) => Green(conv(i as f32 * mul)),
                                                                 Blue(i)  => Blue( conv(i as f32 * mul)),
                                                                 x => x, });
               Ok((input, NetworkPackageData::PushStatus{status_type: data[0], data: parsed.collect(), raw_whole: data.to_vec()}))
             } else {
               Ok((input, NetworkPackageData::PushStatus{status_type: data[0], data: vec![], raw_whole: data.to_vec()}))
             }
           } else {
             Ok((input, NetworkPackageData::PushStatus{status_type: data[0], data: vec![], raw_whole: data.to_vec()}))
           }
         }
         else { Ok((input, NetworkPackageData::Unknown(x.to_vec()))) }
  }
}

fn parse_authorized_package(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  do_parse!(input,
            tag!(b"<PACKT>") >>
            src: opt!(surrounded(b"<SRCCN>", b"</SRCCN>")) >>
            dst: opt!(surrounded(b"<DESCN>", b"</DESCN>")) >>
            datas: parse_datas >>
            tag!(b"</PACKT>") >>
            (NetworkPackage::Authorized{src: src.map(|x| x.to_vec()), dst: dst.map(|x| x.to_vec()), data: datas}))

}

pub fn get_status_rgb(data: &[PushStatusValue]) -> Option<(u8, u8, u8)> {
    use PushStatusValue::*;
  let colors = data.iter().filter(|x| match x { Red(_) | Green(_) | Blue(_) => true, _ => false } );
  let (rgb, c) = colors.fold(((0, 0, 0), 0), |((r, g, b), c), i| match i { Red(x) => ((*x, g, b), c+1), Green(x) => ((r, *x, b), c+1), Blue(x) => ((r, g, *x), c+1), _ => ((r, g, b), c) });
  if c == 0 {
    None
  } else {
    Some(rgb)
  }
}

pub fn parse_network_data(input: &[u8]) -> IResult<&[u8], NetworkPackage> {
  alt!(input, parse_hello_package | parse_authorized_package)
}
