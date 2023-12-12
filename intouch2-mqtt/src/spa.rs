use std::{
    borrow::Cow,
    collections::HashMap,
    ops::{Index, Range},
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::Duration,
};

use intouch2::{
    datas::GeckoDatas,
    generate_uuid,
    object::{package_data, NetworkPackage, NetworkPackageData, StatusChange},
    parser::ParseError,
};
use tokio::{
    select,
    sync::{self, Mutex},
    task::JoinSet,
    time::{self, timeout},
};

use crate::{port_forward::SpaPipe, WithBuffer};

pub struct SpaConnection {
    pipe: Arc<SpaPipe>,
    src: Arc<[u8]>,
    dst: Arc<[u8]>,
    name: Arc<[u8]>,
    watercare_mode: Arc<Mutex<sync::watch::Sender<Option<u8>>>>,
    ping_interval: Arc<Mutex<time::Interval>>,
    get_watercare_mode_interval: Arc<Mutex<time::Interval>>,
    full_state_download_interval: Arc<Mutex<time::Interval>>,
    state: Arc<sync::Mutex<GeckoDatas>>,
    state_valid: Arc<AtomicBool>,
    state_subscribers:
        Arc<sync::Mutex<HashMap<Range<usize>, sync::watch::Sender<Option<Box<[u8]>>>>>>,
    commanders: Arc<sync::Mutex<sync::mpsc::Receiver<SpaCommand>>>,
    new_commander: Arc<sync::mpsc::Sender<SpaCommand>>,
    seq: Arc<AtomicU8>,
    version: package_data::Version,
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
    #[error("Spa keypress pipe error: {0}")]
    KeypressSendFailed(#[from] tokio::sync::broadcast::error::SendError<u8>),
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

#[derive(Debug)]
pub enum SpaCommand {
    SetStatus {
        config_version: u8,
        log_version: u8,
        pack_type: u8,
        pos: u16,
        data: Box<[u8]>,
    },
    SetWatercare(u8),
}

impl SpaConnection {
    pub async fn subscribe(&self, index: Range<usize>) -> sync::watch::Receiver<Option<Box<[u8]>>> {
        let mut subscribers = self.state_subscribers.lock().await;
        match subscribers.entry(index) {
            std::collections::hash_map::Entry::Occupied(subscribers) => {
                subscribers.get().subscribe()
            }
            std::collections::hash_map::Entry::Vacant(new) => {
                let state = self.state.lock().await;
                let current_value = if self.state_valid.load(Ordering::Acquire) {
                    Some(state.index(new.key().clone()).into())
                } else {
                    None
                };
                new.insert(sync::watch::Sender::new(current_value))
                    .subscribe()
            }
        }
    }

    pub fn version(&self) -> &package_data::Version {
        &self.version
    }

    pub async fn subscribe_watercare_mode(&self) -> sync::watch::Receiver<Option<u8>> {
        self.watercare_mode.lock().await.subscribe()
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
        let seq = AtomicU8::default();
        pipe.tx
            .send(
                NetworkPackage::Addressed {
                    src: Some((*src).into()),
                    dst: Some((*dst).into()),
                    data: package_data::GetVersion {
                        seq: seq.fetch_add(1, Ordering::Relaxed),
                    }
                    .into(),
                }
                .to_static(),
            )
            .await?;
        let state = GeckoDatas::new(memory_size);
        let mut full_state_download_interval =
            time::interval_at(time::Instant::now(), Duration::from_secs(1800));
        let mut ping_interval = time::interval_at(time::Instant::now(), Duration::from_secs(3));
        let mut get_watercare_mode_interval =
            time::interval_at(time::Instant::now(), Duration::from_secs(1800));
        for interval in [
            &mut full_state_download_interval,
            &mut ping_interval,
            &mut get_watercare_mode_interval,
        ] {
            interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        }

        let spa_object = loop {
            let msg = rx.recv().await?;
            match msg {
                NetworkPackage::Addressed {
                    src: _,
                    dst: _,
                    data: NetworkPackageData::Version(version),
                } => {
                    println!(
                        "Connected to {}, got version {:?}",
                        String::from_utf8_lossy(&name),
                        version
                    );
                    let (new_commander, commanders) = sync::mpsc::channel(10);
                    break Ok(Self {
                        seq: seq.into(),
                        name: name.into(),
                        pipe: pipe.into(),
                        src,
                        dst,
                        version,
                        new_commander: new_commander.into(),
                        state_valid: Arc::new(false.into()),
                        commanders: Mutex::new(commanders).into(),
                        watercare_mode: Mutex::new(sync::watch::Sender::new(None)).into(),
                        ping_interval: Mutex::new(ping_interval).into(),
                        get_watercare_mode_interval: Mutex::new(get_watercare_mode_interval).into(),
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

    pub fn name(&self) -> &[u8] {
        self.name.as_ref()
    }

    pub fn sender(&self) -> sync::mpsc::Sender<SpaCommand> {
        (*self.new_commander).clone()
    }

    pub async fn run<'a>(&self) -> Result<(), SpaError> {
        let notify_dirty = Arc::new(tokio::sync::Notify::new());
        let gecko_data_len = u16::try_from(self.state.lock().await.len()).expect(
            "If this isn't u16, then the data types are incorrect, and we should not keep going",
        );
        let mut jobs = JoinSet::new();
        {
            let notifier = notify_dirty.clone();
            let gecko_datas = self.state.clone();
            let subscribers = self.state_subscribers.clone();
            let state_valid = self.state_valid.clone();
            jobs.spawn(async move {
                loop {
                    notifier.notified().await;
                    if !state_valid.load(Ordering::Acquire) {
                        continue;
                    }
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
                                subscriber.send_if_modified(|old_data| match old_data {
                                    Some(old_data) if data != old_data.as_ref() => {
                                        old_data.copy_from_slice(data);
                                        true
                                    }
                                    empty @ None => {
                                        *empty = Some(Box::from(data));
                                        true
                                    }
                                    _ => false,
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
                let mut unanswered_pings = 0;
                loop {
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
                                unanswered_pings = 0;
                            }
                        }
                    }
                }
            });
        }
        {
            let commanders = self.commanders.clone();
            let src = self.src.clone();
            let dst = self.src.clone();
            let tx = self.pipe.tx.clone();
            let seq = self.seq.clone();
            jobs.spawn(async move {
                let mut commanders = commanders.lock().await;
                loop {
                    match commanders.recv().await {
                        None => break Ok(()),
                        Some(SpaCommand::SetWatercare(mode)) => {
                            tx.send(
                                NetworkPackage::Addressed {
                                    src: Some((*src).into()),
                                    dst: Some((*dst).into()),
                                    data: package_data::SetWatercare {
                                        seq: seq.fetch_add(1, Ordering::Relaxed),
                                        mode,
                                    }
                                    .into(),
                                }
                                .to_static(),
                            )
                            .await?;
                        }
                        Some(SpaCommand::SetStatus {
                            config_version,
                            log_version,
                            pack_type,
                            pos,
                            data,
                        }) => match data.len().try_into() {
                            Ok(len) => {
                                tx.send(
                                    NetworkPackage::Addressed {
                                        src: Some((*src).into()),
                                        dst: Some((*dst).into()),
                                        data: package_data::SetStatus {
                                            seq: seq.fetch_add(1, Ordering::Relaxed),
                                            pack_type,
                                            len,
                                            config_version,
                                            log_version,
                                            pos,
                                            data: Cow::Owned(data.into()),
                                        }
                                        .into(),
                                    }
                                    .to_static(),
                                )
                                .await?;
                            }
                            Err(e) => {
                                eprintln!("Length is not 8 bits: {e}");
                            }
                        },
                    }
                }
            });
        }
        {
            let watercare_interval = self.get_watercare_mode_interval.clone();
            let src = self.src.clone();
            let dst = self.dst.clone();
            let tx = self.pipe.tx.clone();
            let watercare_mode = self.watercare_mode.clone();
            let seq = self.seq.clone();
            let mut listener = self.pipe.subscribe();
            jobs.spawn(async move {
                let mut watercare_interval = watercare_interval.lock().await;
                loop {
                    select! {
                        _ = watercare_interval.tick() => {
                            tx.send(NetworkPackage::Addressed {
                                src: Some(src.as_ref().into()),
                                dst: Some(dst.as_ref().into()),
                                data: NetworkPackageData::GetWatercare(
                                    package_data::GetWatercare {
                                        seq: seq.fetch_add(1, Ordering::Relaxed)
                                    }
                                )
                            }.to_static()).await?;
                        }
                        new_data = listener.recv() => {
                            match new_data? {
                                NetworkPackage::Addressed { data: NetworkPackageData::WatercareGet(package_data::WatercareGet { mode }), .. }
                                | NetworkPackage::Addressed { data: NetworkPackageData::WatercareSet(package_data::WatercareSet { mode }), .. } => {
                                    watercare_mode.lock().await.send_if_modified(|old_value| {
                                        if *old_value != Some(mode) {
                                            *old_value = Some(mode);
                                            true
                                        } else {
                                            false
                                        }
                                    });
                                },
                                _ => (),
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
            let mut state_valid = Some(self.state_valid.clone());
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
                    if let Some(state_valid) = std::mem::take(&mut state_valid) {
                        state_valid.store(true, Ordering::Release);
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
                                    pos,
                                    data: new_data,
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
        while let Some(job) = jobs.join_next().await {
            job??;
        }
        Ok(())
    }
}
