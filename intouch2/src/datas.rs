use std::{
    borrow::Borrow,
    collections::VecDeque,
    marker::PhantomData,
    ops::{Bound, Deref, DerefMut, Index, IndexMut, Range, RangeBounds},
    ptr::addr_of,
    slice::SliceIndex,
};

use crate::known_datas::{data_size_for, SpaModel};

pub struct GeckoDatas {
    data: Box<[u8]>,
    model: SpaModel,
    dirty: VecDeque<Range<usize>>,
}

impl GeckoDatas {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn peek_dirty(&self) -> Option<&Range<usize>> {
        self.dirty.front()
    }

    pub fn pop_dirty(&mut self) -> Option<Range<usize>> {
        self.dirty.pop_front()
    }
}

pub struct GeckoDataReference<'a> {
    data: &'a [u8],
    model: SpaModel,
}

impl GeckoDatas {
    pub fn new(model: SpaModel) -> Self {
        let length = data_size_for(model);
        let mut vec = Vec::with_capacity(length.into());
        vec.resize(length.into(), 0);
        Self {
            data: vec.into(),
            model,
            dirty: Default::default(),
        }
    }
}

pub trait KnownData<'a> {
    const POSITION: u16;
    const LENGTH: u16;

    type ReturnType;

    fn read_from(from: &'a GeckoDatas) -> Self::ReturnType;
}

impl<Idx> Index<Idx> for GeckoDatas
where
    Idx: SliceIndex<[u8]>,
{
    type Output = Idx::Output;

    fn index(&self, index: Idx) -> &Self::Output {
        self.data.index(index)
    }
}

// impl Index<usize> for GeckoDatas {
//    type Output = u8;
//
//    fn index(&self, index: usize) -> &Self::Output {
//        self.data.index(index)
//    }
//}

// struct DirtyIndex<T: ?Sized> {
//    phantom: PhantomData<T>,
//}
// impl<T: ?Sized> DirtyIndex<T> {
//    fn new() -> Self {
//        Self {
//            phantom: PhantomData,
//        }
//    }
//}

trait SliceLen {
    fn slice_len(&self) -> usize;
}

impl SliceLen for u8 {
    fn slice_len(&self) -> usize {
        1
    }
}

impl SliceLen for [u8] {
    fn slice_len(&self) -> usize {
        self.len()
    }
}

// impl DirtyIndex<u8> {
//    fn get(self, needle: &u8, haystack: &[u8]) -> DirtyData {
//        let start = haystack.as_ptr();
//        let index = addr_of!(needle) as usize - haystack.as_ptr() as usize;
//        debug_assert!(index < haystack.len());
//        DirtyData {
//            index,
//            len: 1
//        }
//    }
//}
// impl DirtyIndex<[u8]> {
//    fn get(self, needle: &[u8], haystack: &[u8]) -> DirtyData {
//        let start = haystack.as_ptr();
//        let index = addr_of!(needle) as usize - haystack.as_ptr() as usize;
//        debug_assert!(index < haystack.len());
//        DirtyData {
//            index,
//            len: needle.len(),
//        }
//    }
//}

impl<Idx> IndexMut<Idx> for GeckoDatas
where
    Idx: SliceIndex<[u8]>,
    Idx::Output: SliceLen,
{
    fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
        let start = self.data.as_ptr() as *const u8;
        let slice = self.data.index_mut(index);
        let slice_addr = addr_of!(*slice) as *const u8;
        let index = slice_addr as usize - start as usize;
        let len = slice.slice_len();
        let range = Range {
            start: index,
            end: index + len,
        };
        self.dirty.push_back(range);
        slice
    }
}

// impl IndexMut<usize> for GeckoDatas {
//    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
//        self.data.index_mut(index)
//    }
//}

#[derive(thiserror::Error, Debug)]
enum GeckoDatasError {
    #[error("Gecko data out of bounds")]
    OutOfBounds,
}

// TODO
// pub async fn subscribe_to<'a, T: KnownData>(receiver: &'a mut
// tokio::sync::watch::Receiver<GeckoDatas>) -> T::Result {    receiver.
//}

impl GeckoDatas {
    // pub fn read<'a, T: KnownData>(&'a self, from: T) -> T::ReturnType {
    //    let start = T::POSITION as usize;
    //    let end = start + T::LENGTH as usize;
    //    debug_assert!(start < self.data.len());
    //    debug_assert!(end < self.data.len());
    //    debug_assert!(end > start);
    //    self.data[start..end].into()
    //}
}
