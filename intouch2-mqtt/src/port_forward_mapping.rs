use std::{
    borrow::Borrow,
    cell::SyncUnsafeCell,
    cmp::max,
    collections::{
        hash_map::{self, Entry},
        HashMap,
    },
    net::SocketAddr,
    sync::{Arc, Weak},
    time::Duration,
};

use tokio::time::Instant;

#[derive(Debug)]
pub struct ForwardMappingInfo {
    id: Weak<[u8]>,
    addr: Weak<SocketAddr>,
    last_forward: Instant,
    last_reply: Option<Instant>,
}

impl ForwardMappingInfo {
    pub fn id(&self) -> Arc<[u8]> {
        let Some(id) = self.id.upgrade() else {
            unreachable!("ForwardMappingInfo is only addressable when mapped with an id")
        };
        id
    }
    pub fn addr(&self) -> Arc<SocketAddr> {
        let Some(addr) = self.addr.upgrade() else {
            unreachable!("ForwardMappingInfo is only addressable when mapped with an addr")
        };
        addr
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

#[derive(Default)]
pub struct ForwardMapping {
    ids: HashMap<Arc<[u8]>, Arc<SyncUnsafeCell<ForwardMappingInfo>>>,
    addrs: HashMap<Arc<SocketAddr>, Arc<SyncUnsafeCell<ForwardMappingInfo>>>,
}

fn unpack_owned_cell<'a>(from: Arc<SyncUnsafeCell<ForwardMappingInfo>>) -> &'a ForwardMappingInfo {
    unsafe { &*from.get() }
}

fn unpack_cell<'a>(from: &'a Arc<SyncUnsafeCell<ForwardMappingInfo>>) -> &'a ForwardMappingInfo {
    unsafe { &*from.get() }
}

fn unpack_owned_cell_mut<'a>(
    from: Arc<SyncUnsafeCell<ForwardMappingInfo>>,
) -> &'a ForwardMappingInfo {
    unsafe { &mut *from.get() }
}

fn unpack_cell_mut(from: &Arc<SyncUnsafeCell<ForwardMappingInfo>>) -> &mut ForwardMappingInfo {
    unsafe { &mut *from.get() }
}

impl ForwardMapping {
    pub fn get_id_mut(&mut self, id: &[u8]) -> Option<&mut ForwardMappingInfo> {
        self.ids.get(id).map(unpack_cell_mut)
    }
    pub fn get_id(&self, id: &[u8]) -> Option<&ForwardMappingInfo> {
        self.ids.get(id).map(unpack_cell)
    }
    pub fn get_addr_mut(&mut self, addr: &SocketAddr) -> Option<&mut ForwardMappingInfo> {
        self.addrs.get(addr).map(unpack_cell_mut)
    }
    pub fn get_addr(&self, addr: &SocketAddr) -> Option<&ForwardMappingInfo> {
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
    ) -> Option<Instant> {
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
        for id in to_remove {
            self.remove_id(&id);
        }
        lowest
    }
    pub fn remove_and_reuse_arcs(
        &mut self,
        addr: impl Borrow<SocketAddr> + Into<Arc<SocketAddr>>,
        id: impl Borrow<[u8]> + Into<Arc<[u8]>>,
    ) -> (Arc<SocketAddr>, Arc<[u8]>) {
        if let Some((new_addr, id)) = self.remove_id(id.borrow()) {
            if new_addr.as_ref() == addr.borrow() {
                (new_addr, id)
            } else {
                if let Some((addr, _)) = self.remove_addr(addr.borrow()) {
                    (addr, id)
                } else {
                    (addr.into(), id)
                }
            }
        } else {
            if let Some((addr, _)) = self.remove_addr(addr.borrow()) {
                (addr, id.into())
            } else {
                (addr.into(), id.into())
            }
        }
    }

    pub fn insert<'a, 's: 'a>(
        &'s mut self,
        addr: impl Borrow<SocketAddr> + Into<Arc<SocketAddr>>,
        id: impl Borrow<[u8]> + Into<Arc<[u8]>>,
    ) -> &'a ForwardMappingInfo {
        if let Some(from_addr) = self
            .addrs
            .get(addr.borrow())
            .map(|x| unpack_owned_cell(x.clone()))
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
        }));
        self.ids.insert(id, info.clone());
        match self.addrs.entry(addr) {
            hash_map::Entry::Occupied(mut entry) => {
                entry.insert(info);
                unpack_owned_cell(entry.get().clone())
            }
            hash_map::Entry::Vacant(entry) => unpack_cell(entry.insert(info)),
        }
    }
    pub fn remove_id(&mut self, id: &[u8]) -> Option<(Arc<SocketAddr>, Arc<[u8]>)> {
        if let Some((id, info)) = self.ids.remove_entry(id) {
            let ForwardMappingInfo { addr, .. } = unsafe { &*info.get() };
            let Some(addr) = addr.upgrade() else {
                unreachable!("id and addr are treated as inseperable")
            };
            self.addrs.remove(&addr);
            Some((addr, id))
        } else {
            None
        }
    }
    pub fn remove_addr(&mut self, addr: &SocketAddr) -> Option<(Arc<SocketAddr>, Arc<[u8]>)> {
        if let Some((addr, info)) = self.addrs.remove_entry(addr) {
            let ForwardMappingInfo { id, .. } = unsafe { &*info.get() };
            let Some(id) = id.upgrade() else {
                unreachable!("id and addr are treated as inseperable")
            };
            self.addrs.remove(&addr);
            Some((addr, id))
        } else {
            None
        }
    }
}
