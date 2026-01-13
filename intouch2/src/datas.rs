use std::{
    collections::VecDeque,
    ops::{Index, IndexMut, Range},
    ptr::addr_of,
    slice::SliceIndex,
};

pub struct GeckoDatas {
    data: Box<[u8]>,
    dirty: VecDeque<Range<usize>>,
}

impl GeckoDatas {
    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn peek_dirty(&self) -> Option<&Range<usize>> {
        self.dirty.front()
    }

    pub fn pop_dirty(&mut self) -> Option<Range<usize>> {
        self.dirty.pop_front()
    }
}

impl GeckoDatas {
    pub fn new(area_size: usize) -> Self {
        Self {
            data: vec![0; area_size].into(),
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

impl<Idx> IndexMut<Idx> for GeckoDatas
where
    Idx: SliceIndex<[u8]>,
    Idx::Output: SliceLen,
{
    fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
        let start = self.data.as_ptr();
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
