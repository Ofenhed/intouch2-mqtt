use std::{
    borrow::Borrow,
    cell::SyncUnsafeCell,
    cmp::max,
    collections::{
        hash_map::{self},
        HashMap,
    },
    net::SocketAddr,
    sync::{Arc, Weak},
    time::Duration,
};

use tokio::time::Instant;

#[derive(Eq, PartialEq, Debug, Hash)]
pub enum ForwardAddr {
    Pipe,
    Socket(SocketAddr),
}

impl std::fmt::Display for ForwardAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ForwardAddr::Pipe => f.write_str("Pipe"),
            ForwardAddr::Socket(s) => s.fmt(f),
        }
    }
}

impl From<SocketAddr> for ForwardAddr {
    fn from(value: SocketAddr) -> Self {
        ForwardAddr::Socket(value)
    }
}

pub type PeerIdType = [u8];
pub type PeerAddrType = ForwardAddr;

#[derive(Debug)]
pub struct ForwardMappingInfo<T> {
    id: Weak<PeerIdType>,
    addr: Weak<PeerAddrType>,
    last_forward: Instant,
    last_reply: Option<Instant>,
    context: Option<T>,
}

impl<T: Send + Sync> ForwardMappingInfo<T> {
    pub fn id(&self) -> Arc<PeerIdType> {
        let Some(id) = self.id.upgrade() else {
            unreachable!("ForwardMappingInfo is only addressable when mapped with an id")
        };
        id
    }
    pub fn addr(&self) -> Arc<PeerAddrType> {
        let Some(addr) = self.addr.upgrade() else {
            unreachable!("ForwardMappingInfo is only addressable when mapped with an addr")
        };
        addr
    }
    pub fn context(&self) -> &T {
        match self.context {
            Some(ref context) => context,
            None => unreachable!("Context is never called on a dead ForwardMappingInfo, and context is always Some for a valid ForwardMappingInfo")
        }
    }
    pub fn timeout(&self, handshake_timeout: Duration, timeout: Duration) -> Instant {
        let reply_timeout = if let Some(last_reply) = self.last_reply {
            last_reply + timeout
        } else {
            self.last_forward + handshake_timeout
        };
        let forward_timeout = self.last_forward + timeout;
        max(reply_timeout, forward_timeout)
    }
    pub fn did_forward(&mut self) {
        self.last_forward = Instant::now()
    }
    pub fn got_reply(&mut self) {
        self.last_reply = Instant::now().into()
    }
}

#[derive(Default, Debug)]
pub struct ForwardMapping<T> {
    ids: HashMap<Arc<PeerIdType>, Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>>,
    addrs: HashMap<Arc<PeerAddrType>, Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>>,
}

#[allow(dead_code)]
fn unpack_owned_cell<'a, T>(
    from: Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>,
) -> &'a ForwardMappingInfo<T> {
    unsafe { &*from.get() }
}

fn unpack_cell<'a, T>(
    from: &'a Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>,
) -> &'a ForwardMappingInfo<T> {
    unsafe { &*from.get() }
}

fn unpack_owned_cell_mut<'a, T>(
    from: Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>,
) -> &'a mut ForwardMappingInfo<T> {
    unsafe { &mut *from.get() }
}

fn unpack_cell_mut<'a, T>(
    from: &'a Arc<SyncUnsafeCell<ForwardMappingInfo<T>>>,
) -> &'a mut ForwardMappingInfo<T> {
    unsafe { &mut *from.get() }
}

impl<T: Send + Sync> ForwardMapping<T> {
    pub fn get_id_mut(&mut self, id: &PeerIdType) -> Option<&mut ForwardMappingInfo<T>> {
        self.ids.get(id).map(unpack_cell_mut)
    }
    pub fn get_id(&self, id: &PeerIdType) -> Option<&ForwardMappingInfo<T>> {
        self.ids.get(id).map(unpack_cell)
    }
    pub fn get_addr_mut(&mut self, addr: &PeerAddrType) -> Option<&mut ForwardMappingInfo<T>> {
        self.addrs.get(addr).map(unpack_cell_mut)
    }
    pub fn get_addr(&self, addr: &PeerAddrType) -> Option<&ForwardMappingInfo<T>> {
        self.addrs.get(addr).map(unpack_cell)
    }
    pub fn len(&self) -> usize {
        let len = self.ids.len();
        debug_assert_eq!(self.addrs.len(), len);
        len
    }
    pub fn clear_timeouts(
        &mut self,
        handshake_timeout: Duration,
        timeout: Duration,
    ) -> (Box<[T]>, Option<Instant>) {
        let mut to_remove = Vec::with_capacity(self.addrs.len());
        let cutoff = Instant::now();
        let mut lowest = None;
        for info in self.addrs.values() {
            let cell = unsafe { &*info.get() };
            let timeout = cell.timeout(handshake_timeout, timeout);
            if timeout < cutoff {
                to_remove.push(cell.id());
            } else if lowest.map(|old| timeout > old) != Some(true) {
                lowest = Some(timeout)
            }
        }
        let mut removed = Vec::with_capacity(to_remove.len());
        for id in to_remove {
            if let Some(x) = self.remove_id(&id) {
                removed.push(x);
            }
        }
        (removed.into(), lowest)
    }
    pub fn remove_and_reuse_arcs(
        &mut self,
        addr: impl Borrow<PeerAddrType> + Into<Arc<PeerAddrType>>,
        id: impl Borrow<PeerIdType> + Into<Arc<PeerIdType>>,
    ) -> (Arc<PeerAddrType>, Arc<PeerIdType>) {
        if let Some((_, new_addr, id)) = self._remove_id(id.borrow()) {
            if new_addr.as_ref() == addr.borrow() {
                (new_addr, id)
            } else {
                if let Some((_, addr, _)) = self._remove_addr(addr.borrow()) {
                    (addr, id)
                } else {
                    (addr.into(), id)
                }
            }
        } else {
            if let Some((_, addr, _)) = self._remove_addr(addr.borrow()) {
                (addr, id.into())
            } else {
                (addr.into(), id.into())
            }
        }
    }

    pub fn insert<'a, 's: 'a>(
        &'s mut self,
        addr: impl Borrow<PeerAddrType> + Into<Arc<PeerAddrType>>,
        id: impl Borrow<PeerIdType> + Into<Arc<PeerIdType>>,
        context: T,
    ) -> &'a mut ForwardMappingInfo<T> {
        if let Some(from_addr) = self
            .addrs
            .get_mut(addr.borrow())
            .map(|x| unpack_owned_cell_mut(x.clone()))
        {
            if matches!(self.ids.get(id.borrow()).map(unpack_cell), Some(from_id) if std::ptr::eq(from_addr, from_id))
            {
                return from_addr;
            }
        }
        let (addr, id) = self.remove_and_reuse_arcs(addr, id);
        let info = Arc::new(SyncUnsafeCell::new(ForwardMappingInfo {
            id: Arc::downgrade(&id),
            addr: Arc::downgrade(&addr),
            last_forward: Instant::now(),
            last_reply: None,
            context: Some(context),
        }));
        self.ids.insert(id, info.clone());
        let reply = match self.addrs.entry(addr) {
            hash_map::Entry::Occupied(mut entry) => {
                entry.insert(info);
                unpack_owned_cell_mut(entry.get().clone())
            }
            hash_map::Entry::Vacant(entry) => unpack_cell_mut(entry.insert(info)),
        };
        reply
    }
    pub fn remove_id(&mut self, id: &PeerIdType) -> Option<T> {
        self._remove_id(id).map(|x| x.0)
    }
    fn _remove_id(&mut self, id: &PeerIdType) -> Option<(T, Arc<PeerAddrType>, Arc<PeerIdType>)> {
        if let Some((id, info)) = self.ids.remove_entry(id) {
            let mapping = unsafe { &mut *info.get() };
            let Some(addr) = mapping.addr.upgrade() else {
                unreachable!("id and addr are treated as inseperable")
            };
            self.addrs.remove(&addr);
            let context = std::mem::take(&mut mapping.context)
                .expect("This invalidates the mapping. It was valid before, so context is Some.");
            Some((context, addr, id))
        } else {
            None
        }
    }
    pub fn remove_addr(&mut self, addr: &PeerAddrType) -> Option<T> {
        self._remove_addr(addr).map(|x| x.0)
    }
    fn _remove_addr(
        &mut self,
        addr: &PeerAddrType,
    ) -> Option<(T, Arc<PeerAddrType>, Arc<PeerIdType>)> {
        if let Some((addr, info)) = self.addrs.remove_entry(addr) {
            let mapping = unsafe { &mut *info.get() };
            let Some(id) = mapping.id.upgrade() else {
                unreachable!("id and addr are treated as inseperable")
            };
            self.ids.remove(&id);
            let context = std::mem::take(&mut mapping.context)
                .expect("This invalidates the mapping. It was valid before, so context is Some.");
            Some((context, addr, id))
        } else {
            None
        }
    }
}
