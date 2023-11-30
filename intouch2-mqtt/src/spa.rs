use std::{
    backtrace::Backtrace,
    borrow::Cow,
    collections::HashMap,
    net::SocketAddr,
    ops::{Bound, Index, Range, RangeBounds},
    sync::{
        atomic::{AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use intouch2::{
    composer::compose_network_data,
    datas::GeckoDatas,
    generate_uuid,
    object::{
        package_data::{self, SetStatus},
        NetworkPackage, NetworkPackageData, PackAction, StatusChange,
    },
    parser::{parse_network_data, ParseError},
    ToStatic,
};
use tokio::{
    net::UdpSocket,
    select,
    sync::{self, Mutex},
    task::JoinSet,
    time::{self, timeout},
};

use crate::{port_forward::SpaPipe, unspecified_source_for_taget, WithBuffer};

pub struct SpaConnection {
    pipe: Arc<SpaPipe>,
    src: Arc<[u8]>,
    dst: Arc<[u8]>,
    ping_interval: Arc<Mutex<time::Interval>>,
    full_state_download_interval: Arc<Mutex<time::Interval>>,
    state: Arc<sync::Mutex<GeckoDatas>>,
    state_subscribers: Arc<sync::Mutex<HashMap<Range<usize>, sync::watch::Sender<Box<[u8]>>>>>,
    seq: Arc<AtomicU8>,
}

#[derive(thiserror::Error, Debug)]
pub enum SpaError {
    #[error("Unexpected answer: {0}")]
    UnexpectedAnswer(NetworkPackage<'static>),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Parse error: {0}")]
    Parse(#[from] ParseError),
    #[error("Spa timed out")]
    SpaConnectionLost,
    #[error("Spa pipe error: {0}")]
    PipeSendFailed(#[from] tokio::sync::mpsc::error::SendError<NetworkPackage<'static>>),
    #[error("Spa pipe error: {0}")]
    PipeReceiveFailed(#[from] tokio::sync::broadcast::error::RecvError),
    #[error("Runtime error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
    #[error("Invalid data received: {0}")]
    InvalidData(&'static str),
    #[error("Deadlock: {0}")]
    Deadlock(&'static str),
}

impl WithBuffer for SpaConnection {
    type Buffer = [u8; 4096];

    fn make_buffer() -> [u8; 4096] {
        [0; 4096]
    }
}

impl SpaConnection {
    pub async fn subscribe(&self, index: Range<usize>) -> sync::watch::Receiver<Box<[u8]>> {
        let mut subscribers = self.state_subscribers.lock().await;
        match subscribers.entry(index) {
            std::collections::hash_map::Entry::Occupied(subscribers) => {
                subscribers.get().subscribe()
            }
            std::collections::hash_map::Entry::Vacant(new) => {
                let state = self.state.lock().await;
                let current_value = state.index(new.key().clone());
                new.insert(sync::watch::Sender::new(current_value.into()))
                    .subscribe()
            }
        }
    }

    pub async fn len(&self) -> usize {
        self.state.lock().await.len()
    }

    pub async fn new(memory_size: usize, pipe: SpaPipe) -> Result<Self, SpaError> {
        pipe.tx
            .send(NetworkPackage::Hello(Cow::Borrowed(b"1")))
            .await?;

        let mut rx = pipe.subscribe();
        let msg = rx.recv().await?;

        let receiver = match msg {
            NetworkPackage::Hello(msg) => Ok(msg),
            msg => Err(SpaError::UnexpectedAnswer(msg.to_static())),
        }?;
        let (dst, name): (Arc<[u8]>, Box<[u8]>) = {
            let pos = receiver
                .iter()
                .position(|x| *x == '|' as u8)
                .unwrap_or(receiver.len());
            (receiver[0..pos].into(), receiver[pos + 1..].into())
        };
        let src: Arc<[u8]> = generate_uuid().into();
        pipe.tx
            .send(NetworkPackage::Hello(Cow::Owned((*src).into())))
            .await?;
        pipe.tx
            .send(
                NetworkPackage::Addressed {
                    src: Some((*src).into()),
                    dst: Some((*dst).into()),
                    data: package_data::GetVersion.into(),
                }
                .to_static(),
            )
            .await?;
        let state = GeckoDatas::new(memory_size);
        let mut full_state_download_interval =
            time::interval_at(time::Instant::now(), Duration::from_secs(1800));
        let mut ping_interval = time::interval_at(time::Instant::now(), Duration::from_secs(3));
        for interval in [&mut full_state_download_interval, &mut ping_interval] {
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        }
        let seq = Default::default();

        let spa_object = loop {
            let msg = rx.recv().await?;
            match msg {
                NetworkPackage::Addressed {
                    src: _,
                    dst: _,
                    data: NetworkPackageData::Version(x),
                } => {
                    println!(
                        "Connected to {}, got version {:?}",
                        String::from_utf8_lossy(&name),
                        x
                    );
                    break Ok(Self {
                        seq,
                        pipe: pipe.into(),
                        src,
                        dst,
                        ping_interval: Mutex::new(ping_interval).into(),
                        full_state_download_interval: Mutex::new(full_state_download_interval)
                            .into(),
                        state: Arc::new(state.into()),
                        state_subscribers: Default::default(),
                    });
                }
                NetworkPackage::Hello(_) => continue,
                msg => break Err(SpaError::UnexpectedAnswer(msg.into())),
            };
        }?;
        Ok(spa_object)
    }

    async fn tick(&self) {
        let mut interval = self.ping_interval.lock().await;
        interval.tick().await;
    }

    pub async fn recv<'a>(&self) -> Result<(), SpaError> {
        let receiver = self.pipe.subscribe();
        let notify_dirty = Arc::new(tokio::sync::Notify::new());
        let gecko_data_len = u16::try_from(self.state.lock().await.len()).expect(
            "If this isn't u16, then the data types are incorrect, and we should not keep going",
        );
        let mut jobs = JoinSet::new();
        {
            let notifier = notify_dirty.clone();
            let gecko_datas = self.state.clone();
            let subscribers = self.state_subscribers.clone();
            jobs.spawn(async move {
                loop {
                    notifier.notified().await;
                    let mut gecko_datas = gecko_datas.lock().await;
                    let subscribers = subscribers.lock().await;
                    while let Some(dirty_range) = gecko_datas.peek_dirty() {
                        for (range, subscriber) in subscribers.iter() {
                            if range.contains(&dirty_range.start)
                                || range.contains(&dirty_range.end)
                                || dirty_range.contains(&range.start)
                                || dirty_range.contains(&range.end)
                            {
                                let data = gecko_datas.index(range.clone());
                                subscriber.send_if_modified(|old_data| {
                                    if data != old_data.as_ref() {
                                        old_data.copy_from_slice(data);
                                        true
                                    } else {
                                        false
                                    }
                                });
                            }
                        }
                        gecko_datas.pop_dirty();
                    }
                }
            });
        }
        {
            let pinger = self.ping_interval.clone();
            let src = self.src.clone();
            let dst = self.dst.clone();
            let tx = self.pipe.tx.clone();
            let mut listener = self.pipe.subscribe();
            jobs.spawn(async move {
                let mut pinger = timeout(Duration::from_secs(1), pinger.lock()).await.map_err(|_| SpaError::Deadlock("pinger"))?;
                loop {
                    let mut unanswered_pings = 0;
                    select! {
                        _ = pinger.tick() => {
                            tx.send(NetworkPackage::Addressed { src: Some((*src).into()), dst: Some((*dst).into()), data: package_data::Ping.into() }.to_static()).await?;
                            unanswered_pings += 1;
                            if unanswered_pings > 10 {
                                return Err(SpaError::SpaConnectionLost)
                            }
                        }
                        new_data = listener.recv() => {
                            if let NetworkPackage::Addressed { data: NetworkPackageData::Pong, .. } = new_data? {

                            }
                        }
                    }
                }
            });
        }
        {
            let interval = self.full_state_download_interval.clone();
            let tx = self.pipe.tx.clone();
            let pipe = self.pipe.clone();
            let src = self.src.clone();
            let dst = self.dst.clone();
            let seq = self.seq.clone();
            let gecko_data = self.state.clone();
            let notify_dirty = notify_dirty.clone();
            jobs.spawn(async move {
                loop {
                    interval.lock().await.tick().await;
                    let seq = seq.fetch_add(1, Ordering::Relaxed);
                    let req = NetworkPackage::Addressed {
                        src: Some((*src).into()),
                        dst: Some((*dst).into()),
                        data: package_data::RequestStatus {
                            seq,
                            start: 0,
                            length: gecko_data_len,
                        }
                        .into(),
                    };
                    let mut rx = pipe.subscribe();
                    'retry: loop {
                        tx.send(req.to_static()).await?;
                        let mut expected = 0;
                        let mut data_read = 0;
                        let timeout = Duration::from_secs(5);
                        let timeout_at = time::Instant::now() + timeout;
                        loop {
                            match time::timeout_at(timeout_at.clone(), rx.recv()).await {
                                Ok(recv) => match recv? {
                                    NetworkPackage::Addressed {
                                        data:
                                            NetworkPackageData::Status(package_data::Status {
                                                seq,
                                                next,
                                                length,
                                                data,
                                            }),
                                        ..
                                    } if seq == expected => {
                                        if usize::from(length) != data.len() {
                                            return Err(SpaError::InvalidData(
                                                "Invalid Status length field",
                                            ))?;
                                        }
                                        let end = data_read + data.len();
                                        let mut gecko_data = gecko_data.lock().await;
                                        gecko_data[data_read..end].copy_from_slice(&*data);
                                        if end == usize::from(gecko_data_len) {
                                            break 'retry;
                                        }
                                        data_read = end;
                                        expected = next;
                                    }
                                    _ => continue,
                                },
                                Err(_timeout) => continue 'retry,
                            }
                        }
                    }
                    notify_dirty.notify_waiters();
                    interval.lock().await.reset();
                }
            });
        }
        {
            let mut rx = self.pipe.subscribe();
            let spa_id = self.dst.clone();
            let my_id = self.src.clone();
            let tx = self.pipe.tx.clone();
            let seq = self.seq.clone();
            let notify_dirty = notify_dirty.clone();
            let gecko_data = self.state.clone();
            jobs.spawn(async move {
                loop {
                    let package = rx.recv().await?;
                    match package {
                        NetworkPackage::Addressed {
                            data:
                                NetworkPackageData::SetStatus(package_data::SetStatus {
                                    action:
                                        PackAction::Set {
                                            pos,
                                            data: new_data,
                                            ..
                                        },
                                    ..
                                }),
                            dst,
                            ..
                        } if matches!(dst, Some(ref dst) if *dst == spa_id.as_ref()) => {
                            let mut data = gecko_data.lock().await;
                            let pos = usize::from(pos);
                            let old_data: &mut [u8] = &mut data[pos..pos + new_data.len()];
                            old_data.copy_from_slice(new_data.as_ref());
                            notify_dirty.notify_waiters();
                        }
                        NetworkPackage::Addressed {
                            data:
                                NetworkPackageData::PushStatus(package_data::PushStatus {
                                    length,
                                    changes,
                                }),
                            dst,
                            src,
                        } => {
                            if matches!(dst, Some(ref dst) if *dst == my_id.as_ref()) {
                                let rsp = NetworkPackage::Addressed {
                                    src: dst,
                                    dst: src,
                                    data: package_data::PushStatusAck {
                                        seq: seq.fetch_add(1, Ordering::Relaxed),
                                    }
                                    .into(),
                                };
                                tx.send(rsp.to_static()).await?;
                            }
                            if usize::from(length) != changes.len() {
                                return Err(SpaError::InvalidData(
                                    "Length field for pushed status invalid",
                                ))?;
                            }
                            let mut data = gecko_data.lock().await;
                            for change in changes.iter() {
                                let StatusChange {
                                    change: pos,
                                    data: new_data,
                                } = change;
                                let pos = usize::from(*pos);
                                let old_data: &mut [u8] = &mut data[pos..pos + 2];
                                old_data.copy_from_slice(new_data.as_ref());
                            }
                            notify_dirty.notify_waiters();
                        }
                        _ => (),
                    }
                }
            });
        }
        let gecko_data = self.state.clone();
        while let Some(job) = jobs.join_next().await {
            job??;
        }
        Ok(())
        // jobs.spawn(async { Ok(JobResult::StartRecv(receiver)) });
        // jobs.spawn(async { Ok(JobResult::RequestState) });
        // while let Some(job) = jobs.join_next().await {
        //    match job?? {
        //        JobResult::RequestState => {
        //            let interval = self.full_state_download_interval.clone();

        //        },
        //        JobResult::Ping => {
        //            jobs.spawn(async move {
        //            });
        //        }
        //        JobResult::Recv(mut recv, NetworkPackage::Addressed { data:
        // NetworkPackageData::Pong, .. }) | JobResult::StartRecv(mut recv) => {
        // self.pings_since_last_pong.store(0, Ordering::Relaxed);
        // jobs.spawn(async {                let new_data = recv.recv().await?;
        //                Ok(JobResult::Recv(recv, new_data))
        //            });
        //        }
        //        JobResult::Recv(_, data) => {
        //            return Ok(data);
        //        }
        //    }
        //}
        // Err(SpaError::SpaConnectionLost)
    }
}
