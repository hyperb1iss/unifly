//! Graphics protocol probing and optional true-pixel chart rendering.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::protocol::Protocol;
use ratatui_image::{Image, Resize, picker::Picker, picker::ProtocolType};

use crate::tui::render_caps::{self, GraphicsProtocol};

const CHART_CACHE_CAPACITY: usize = 48;
const CHART_QUEUE_CAPACITY: usize = 8;
const CHART_MIN_REFRESH_INTERVAL: Duration = Duration::from_secs(1);

static PICKER: OnceLock<RwLock<Option<Picker>>> = OnceLock::new();
static CHARTS: OnceLock<ChartManager> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChartImageKey(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChartSlotKey(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedChart {
    Rendered,
    Stale(CachedChartStatus),
    Missing,
    Pending,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachedChartStatus {
    Missing,
    Pending,
    Failed,
}

struct ChartRequest {
    slot: ChartSlotKey,
    key: ChartImageKey,
    picker: Picker,
    build: Box<dyn FnOnce() -> DynamicImage + Send>,
    target: Size,
}

struct ChartResponse {
    slot: ChartSlotKey,
    key: ChartImageKey,
    result: Result<Protocol, String>,
}

struct ChartManager {
    tx: mpsc::SyncSender<ChartRequest>,
    rx: Mutex<mpsc::Receiver<ChartResponse>>,
    cache: Mutex<ChartCache>,
}

#[derive(Default)]
struct ChartCache {
    ready: HashMap<ChartImageKey, Arc<Protocol>>,
    latest_by_slot: HashMap<ChartSlotKey, ChartImageKey>,
    pending: HashSet<ChartImageKey>,
    failed: HashSet<ChartImageKey>,
    order: VecDeque<ChartImageKey>,
    last_queued_by_slot: HashMap<ChartSlotKey, Instant>,
}

pub fn probe_stdio() -> GraphicsProtocol {
    if render_caps::graphics_disabled() {
        store_picker(None);
        return GraphicsProtocol::None;
    }

    match Picker::from_query_stdio() {
        Ok(mut picker) => {
            if let Some(protocol) = render_caps::forced_graphics_protocol() {
                if let Some(protocol_type) = picker_protocol_from_graphics(protocol) {
                    picker.set_protocol_type(protocol_type);
                    store_picker(Some(picker));
                } else {
                    store_picker(None);
                }
                return protocol;
            }

            let protocol = protocol_from_picker(picker.protocol_type());
            if protocol.is_pixels() {
                store_picker(Some(picker));
            } else {
                store_picker(None);
            }
            protocol
        }
        Err(error) => {
            tracing::debug!("graphics protocol query failed: {error}");
            store_picker(None);
            GraphicsProtocol::None
        }
    }
}

pub fn current_picker() -> Option<Picker> {
    PICKER
        .get()
        .and_then(|lock| lock.read().ok().and_then(|guard| guard.clone()))
}

/// Drain any completed chart encodes into the cache. Returns `true` when at
/// least one response landed, meaning the next frame should be drawn so the
/// fresh protocol (or cell-path fallback after a failure) becomes visible.
/// Called once per render tick by the app loop — keeping consumption here,
/// independent of which screen is active, means an encode that finishes
/// after a screen switch can never wedge the render gate open.
pub fn poll_ready_charts() -> bool {
    CHARTS.get().is_some_and(ChartManager::drain_responses)
}

pub fn render_cached_chart(
    slot: ChartSlotKey,
    key: ChartImageKey,
    area: Rect,
    buf: &mut Buffer,
) -> CachedChart {
    let Some(manager) = CHARTS.get() else {
        return CachedChart::Missing;
    };

    let Ok(cache) = manager.cache.lock() else {
        return CachedChart::Failed;
    };

    if let Some(protocol) = cache.ready.get(&key).cloned() {
        Image::new(protocol.as_ref())
            .allow_clipping(true)
            .render(area, buf);
        return CachedChart::Rendered;
    }

    let exact_status = if cache.pending.contains(&key) {
        CachedChartStatus::Pending
    } else if cache.failed.contains(&key) {
        CachedChartStatus::Failed
    } else {
        CachedChartStatus::Missing
    };

    if let Some(protocol) = cache
        .latest_by_slot
        .get(&slot)
        .and_then(|latest_key| cache.ready.get(latest_key))
        .cloned()
    {
        Image::new(protocol.as_ref())
            .allow_clipping(true)
            .render(area, buf);
        return CachedChart::Stale(exact_status);
    }

    match exact_status {
        CachedChartStatus::Missing => CachedChart::Missing,
        CachedChartStatus::Pending => CachedChart::Pending,
        CachedChartStatus::Failed => CachedChart::Failed,
    }
}

pub fn queue_chart(
    slot: ChartSlotKey,
    key: ChartImageKey,
    target: Size,
    build_image: impl FnOnce() -> DynamicImage + Send + 'static,
) -> bool {
    let Some(picker) = current_picker() else {
        return false;
    };
    let manager = CHARTS.get_or_init(ChartManager::spawn);
    if !manager.reserve(slot, key) {
        return false;
    }
    manager.queue(ChartRequest {
        slot,
        key,
        picker,
        build: Box::new(build_image),
        target,
    })
}

fn store_picker(picker: Option<Picker>) {
    let lock = PICKER.get_or_init(|| RwLock::new(picker.clone()));
    if let Ok(mut guard) = lock.write() {
        *guard = picker;
    }
}

fn protocol_from_picker(protocol: ProtocolType) -> GraphicsProtocol {
    match protocol {
        ProtocolType::Kitty => GraphicsProtocol::Kitty,
        ProtocolType::Sixel => GraphicsProtocol::Sixel,
        ProtocolType::Iterm2 => GraphicsProtocol::Iterm2,
        ProtocolType::Halfblocks => GraphicsProtocol::None,
    }
}

fn picker_protocol_from_graphics(protocol: GraphicsProtocol) -> Option<ProtocolType> {
    match protocol {
        GraphicsProtocol::None => None,
        GraphicsProtocol::Kitty => Some(ProtocolType::Kitty),
        GraphicsProtocol::Sixel => Some(ProtocolType::Sixel),
        GraphicsProtocol::Iterm2 => Some(ProtocolType::Iterm2),
    }
}

impl ChartManager {
    fn spawn() -> Self {
        let (tx, rx_worker) = mpsc::sync_channel::<ChartRequest>(CHART_QUEUE_CAPACITY);
        let (tx_main, rx) = mpsc::sync_channel::<ChartResponse>(CHART_QUEUE_CAPACITY);

        if let Err(error) = thread::Builder::new()
            .name("unifly-chart-graphics".to_string())
            .spawn(move || {
                while let Ok(request) = rx_worker.recv() {
                    let image = (request.build)();
                    let result = request
                        .picker
                        .new_protocol(image, request.target, Resize::Fit(None))
                        .map_err(|error| error.to_string());
                    if tx_main
                        .send(ChartResponse {
                            slot: request.slot,
                            key: request.key,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
        {
            tracing::debug!("graphics chart worker failed to start: {error}");
        }

        Self {
            tx,
            rx: Mutex::new(rx),
            cache: Mutex::new(ChartCache::default()),
        }
    }

    fn reserve(&self, slot: ChartSlotKey, key: ChartImageKey) -> bool {
        let Ok(mut cache) = self.cache.lock() else {
            return false;
        };
        cache.reserve(slot, key, Instant::now())
    }

    fn queue(&self, request: ChartRequest) -> bool {
        let key = request.key;
        match self.tx.try_send(request) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                if let Ok(mut cache) = self.cache.lock() {
                    cache.pending.remove(&key);
                }
                false
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                if let Ok(mut cache) = self.cache.lock() {
                    cache.pending.remove(&key);
                    cache.failed.insert(key);
                }
                false
            }
        }
    }

    fn drain_responses(&self) -> bool {
        let Ok(rx) = self.rx.lock() else {
            return false;
        };
        let mut saw_response = false;
        while let Ok(response) = rx.try_recv() {
            saw_response = true;
            if let Ok(mut cache) = self.cache.lock() {
                cache.pending.remove(&response.key);
                match response.result {
                    Ok(protocol) => {
                        cache.insert_ready(response.slot, response.key, Arc::new(protocol));
                    }
                    Err(error) => {
                        tracing::debug!(
                            key = response.key.0,
                            "graphics chart encode failed: {error}"
                        );
                        cache.ready.remove(&response.key);
                        cache.failed.insert(response.key);
                    }
                }
            }
        }
        saw_response
    }
}

impl ChartCache {
    fn reserve(&mut self, slot: ChartSlotKey, key: ChartImageKey, now: Instant) -> bool {
        if self.ready.contains_key(&key)
            || self.pending.contains(&key)
            || self.failed.contains(&key)
            || self.pending.len() >= CHART_QUEUE_CAPACITY
        {
            return false;
        }

        if self.latest_by_slot.contains_key(&slot)
            && self
                .last_queued_by_slot
                .get(&slot)
                .is_some_and(|last| now.duration_since(*last) < CHART_MIN_REFRESH_INTERVAL)
        {
            return false;
        }

        self.pending.insert(key);
        self.last_queued_by_slot.insert(slot, now);
        true
    }

    fn insert_ready(&mut self, slot: ChartSlotKey, key: ChartImageKey, protocol: Arc<Protocol>) {
        self.failed.remove(&key);
        if !self.ready.contains_key(&key) {
            self.order.push_back(key);
        }
        self.ready.insert(key, protocol);
        self.latest_by_slot.insert(slot, key);

        while self.ready.len() > CHART_CACHE_CAPACITY {
            let Some(expired) = self.order.pop_front() else {
                break;
            };
            if expired != key {
                self.ready.remove(&expired);
                self.latest_by_slot
                    .retain(|_, latest_key| *latest_key != expired);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHART_MIN_REFRESH_INTERVAL, ChartCache, ChartImageKey, ChartSlotKey,
        picker_protocol_from_graphics, protocol_from_picker,
    };
    use crate::tui::render_caps::GraphicsProtocol;
    use std::time::Instant;

    #[test]
    fn picker_protocol_maps_to_pixel_caps() {
        assert_eq!(
            protocol_from_picker(ratatui_image::picker::ProtocolType::Kitty),
            GraphicsProtocol::Kitty
        );
        assert_eq!(
            protocol_from_picker(ratatui_image::picker::ProtocolType::Sixel),
            GraphicsProtocol::Sixel
        );
        assert_eq!(
            protocol_from_picker(ratatui_image::picker::ProtocolType::Halfblocks),
            GraphicsProtocol::None
        );
    }

    #[test]
    fn graphics_protocol_maps_back_to_picker_protocol() {
        assert_eq!(
            picker_protocol_from_graphics(GraphicsProtocol::Kitty),
            Some(ratatui_image::picker::ProtocolType::Kitty)
        );
        assert_eq!(picker_protocol_from_graphics(GraphicsProtocol::None), None);
    }

    #[test]
    fn chart_cache_reserve_throttles_stale_slot_refreshes() {
        let slot = ChartSlotKey(1);
        let first_key = ChartImageKey(10);
        let second_key = ChartImageKey(11);
        let now = Instant::now();
        let mut cache = ChartCache::default();

        assert!(cache.reserve(slot, first_key, now));
        cache.pending.remove(&first_key);
        cache.latest_by_slot.insert(slot, first_key);

        assert!(!cache.reserve(slot, second_key, now + CHART_MIN_REFRESH_INTERVAL / 2));
        assert!(cache.reserve(slot, second_key, now + CHART_MIN_REFRESH_INTERVAL));
    }
}
