#![feature(sync_unsafe_cell)]

pub mod port_forward;
pub mod port_forward_mapping;
pub mod spa;

use std::{
    mem::MaybeUninit,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    ops::{Deref},
    sync::Arc,
};

pub trait WithBuffer {
    type Buffer: AsRef<[u8]>;

    fn make_buffer() -> Self::Buffer;
}

pub fn unspecified_source_for_taget(addr: impl ToOwned<Owned = SocketAddr>) -> SocketAddr {
    let mut target_ip = addr.to_owned();
    target_ip.set_port(0);
    match &mut target_ip {
        SocketAddr::V4(addr) => addr.set_ip(Ipv4Addr::UNSPECIFIED),
        SocketAddr::V6(addr) => addr.set_ip(Ipv6Addr::UNSPECIFIED),
    };
    target_ip
}

pub struct StaticBox<T> {
    inner: Arc<T>,
}

pub struct NoClone<T> {
    inner: Arc<T>,
}

impl<T> StaticBox<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn to_no_clone(&self) -> NoClone<T> {
        NoClone {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for NoClone<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.inner
    }
}

pub struct Buffers<const COUNT: usize, T> {
    bufs: [MaybeUninit<T>; COUNT],
    size: usize,
}

impl<const COUNT: usize, T> Buffers<COUNT, T> {
    pub fn new() -> Self {
        Self {
            bufs: unsafe { std::mem::uninitialized() },
            size: 0,
        }
    }

    pub fn take_or(&mut self, create: impl FnOnce() -> T) -> T {
        if self.size == 0 {
            (create)()
        } else {
            self.size -= 1;
            let buf = std::mem::replace(&mut self.bufs[self.size], MaybeUninit::uninit());
            unsafe { buf.assume_init() }
        }
    }

    pub fn release(&mut self, buf: T) {
        if self.size != COUNT {
            self.bufs[self.size] = MaybeUninit::new(buf);
            self.size += 1;
        }
    }
}

impl<const COUNT: usize, T: Default> Buffers<COUNT, T> {
    pub fn get(&mut self) -> T {
        self.take_or(Default::default)
    }
}
