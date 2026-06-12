use bytes::Bytes;
use arc_swap::ArcSwap;
use crossbeam_utils::CachePadded;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;
use tokio::sync::Notify;

#[derive(Clone)]
pub(crate) struct SourceMetadata {
    pub content_type: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub genre: Option<String>,
    pub url: Option<String>,
    pub bitrate: Option<String>,
    pub is_public: Option<String>,
    pub audio_info: Option<String>,
}

impl Default for SourceMetadata {
    fn default() -> Self {
        Self {
            content_type: "audio/mpeg".to_string(),
            name: None,
            description: None,
            genre: None,
            url: None,
            bitrate: None,
            is_public: None,
            audio_info: None,
        }
    }
}

pub const RING_SIZE: usize = 1024; // Power of 2 for fast modulo

pub struct RingBuffer {
    pub buffer: Vec<CachePadded<ArcSwap<Bytes>>>,
    pub head: CachePadded<AtomicUsize>,
    pub notify: Notify,
}

impl RingBuffer {
    pub fn new() -> Self {
        let mut buffer = Vec::with_capacity(RING_SIZE);
        for _ in 0..RING_SIZE {
            buffer.push(CachePadded::new(ArcSwap::from_pointee(Bytes::new())));
        }
        Self {
            buffer,
            head: CachePadded::new(AtomicUsize::new(0)),
            notify: Notify::new(),
        }
    }

    pub fn push(&self, bytes: Bytes) {
        let idx = self.head.load(Ordering::Relaxed);
        let next_idx = idx + 1;
        
        self.buffer[idx % RING_SIZE].store(Arc::new(bytes));
        self.head.store(next_idx, Ordering::Release);
        self.notify.notify_waiters();
    }
}

pub(crate) struct Mountpoint {
    pub ring: Arc<RingBuffer>,
    pub created_at: Instant,
    pub listeners: Arc<AtomicUsize>,
    pub mobile_listeners: Arc<AtomicUsize>,
    pub desktop_listeners: Arc<AtomicUsize>,
    pub metadata: Arc<SourceMetadata>,
}

impl Mountpoint {
    pub fn new(metadata: SourceMetadata) -> Self {
        Self {
            ring: Arc::new(RingBuffer::new()),
            created_at: Instant::now(),
            listeners: Arc::new(AtomicUsize::new(0)),
            mobile_listeners: Arc::new(AtomicUsize::new(0)),
            desktop_listeners: Arc::new(AtomicUsize::new(0)),
            metadata: Arc::new(metadata),
        }
    }
}

impl Default for Mountpoint {
    fn default() -> Self {
        Self::new(SourceMetadata::default())
    }
}

pub(crate) type MountMap = Arc<DashMap<String, Mountpoint>>;

#[derive(Clone)]
pub struct State {
    pub(crate) mounts: MountMap,
}

impl State {
    pub fn new() -> Self {
        Self {
            mounts: Arc::new(DashMap::new()),
        }
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct ListenerGuard {
    listeners: Arc<AtomicUsize>,
    mobile_listeners: Arc<AtomicUsize>,
    desktop_listeners: Arc<AtomicUsize>,
    is_mobile: bool,
}

impl ListenerGuard {
    pub fn new(
        listeners: Arc<AtomicUsize>,
        mobile_listeners: Arc<AtomicUsize>,
        desktop_listeners: Arc<AtomicUsize>,
        is_mobile: bool,
    ) -> Self {
        listeners.fetch_add(1, Ordering::SeqCst);
        if is_mobile {
            mobile_listeners.fetch_add(1, Ordering::SeqCst);
        } else {
            desktop_listeners.fetch_add(1, Ordering::SeqCst);
        }
        Self {
            listeners,
            mobile_listeners,
            desktop_listeners,
            is_mobile,
        }
    }
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        let prev = self.listeners.fetch_sub(1, Ordering::SeqCst);
        if self.is_mobile {
            self.mobile_listeners.fetch_sub(1, Ordering::SeqCst);
        } else {
            self.desktop_listeners.fetch_sub(1, Ordering::SeqCst);
        }
        tracing::info!(
            "ListenerGuard dropped. Listeners count decremented from {} to {}",
            prev,
            prev - 1
        );
    }
}
