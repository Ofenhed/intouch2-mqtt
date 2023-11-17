use std::{marker::PhantomData, ops::Deref};

pub struct GeckoDatas {
    data: [u8; 1024],
}

impl Default for GeckoDatas {
    fn default() -> Self {
        Self { data: [0; 1024] }
    }
}

pub trait KnownData {
    const POSITION: u16;
    const LENGTH: u16;
}

pub struct DataRef<'a, T: KnownData> {
    data: &'a [u8],
    _phantom: PhantomData<T>,
}

impl<'a, T: KnownData> From<&'a [u8]> for DataRef<'a, T> {
    fn from(data: &'a [u8]) -> Self {
        debug_assert!(data.len() == T::LENGTH as usize);
        Self {
            data,
            _phantom: Default::default(),
        }
    }
}

impl<T: KnownData> Deref for DataRef<'_, T> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T: KnownData> KnownData for DataRef<'a, T> {
    const POSITION: u16 = T::POSITION;
    const LENGTH: u16 = T::POSITION;
}

impl GeckoDatas {
    pub fn read<'a, T: KnownData>(&'a self) -> DataRef<T> {
        let start = T::POSITION as usize;
        let end = start + T::LENGTH as usize;
        debug_assert!(start < self.data.len());
        debug_assert!(end < self.data.len());
        debug_assert!(end > start);
        self.data[start..end].into()
    }

    pub fn write<T: KnownData>(&mut self, value: &[u8]) {
        debug_assert!(value.len() == T::LENGTH as usize);
        self.write_raw(T::POSITION, value);
    }

    pub fn write_raw(&mut self, position: u16, value: &[u8]) {
        let start: usize = position.into();
        let end = start + value.len();
        debug_assert!(start < self.data.len());
        debug_assert!(end < self.data.len());
        debug_assert!(end > start);
        self.data[start..end].copy_from_slice(value);
    }
}
