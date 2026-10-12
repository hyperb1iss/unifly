//! Graphics protocol probing and optional true-pixel chart rendering.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, mpsc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use image::DynamicImage;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::widgets::Widget;
use ratatui_image::picker::{Capability, Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::protocol::kitty::Kitty;
use ratatui_image::{Image, Resize};

use crate::tui::render_caps::{self, GraphicsProtocol};

const CHART_CACHE_CAPACITY: usize = 48;
/// Encoded images kept per chart slot: the one on screen plus a few recent
/// ones, so toggling a view back renders without a re-encode.
const CHART_IMAGES_PER_SLOT: usize = 4;
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
    result: Result<EncodedChart, String>,
}

/// An encoded chart plus, for kitty, the terminal-side image it occupies.
struct EncodedChart {
    protocol: Protocol,
    kitty: Option<KittyImage>,
}

/// A kitty image this process transmitted, which it must delete once no
/// frame can draw it again: the terminal keeps every image until told
/// otherwise, and a chart refreshing once a second fills its image store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KittyImage {
    id: u32,
    tmux: bool,
}

struct ReadyChart {
    slot: ChartSlotKey,
    protocol: Arc<Protocol>,
    kitty: Option<KittyImage>,
}

struct ChartManager {
    tx: mpsc::SyncSender<ChartRequest>,
    rx: Mutex<mpsc::Receiver<ChartResponse>>,
    cache: Mutex<ChartCache>,
}

#[derive(Default)]
struct ChartCache {
    ready: HashMap<ChartImageKey, ReadyChart>,
    latest_by_slot: HashMap<ChartSlotKey, ChartImageKey>,
    pending: HashSet<ChartImageKey>,
    failed: HashSet<ChartImageKey>,
    order: VecDeque<ChartImageKey>,
    last_queued_by_slot: HashMap<ChartSlotKey, Instant>,
    /// Images dropped from the cache whose terminal copies await deletion.
    retired: Vec<KittyImage>,
    /// Transmit commands for new images, sent ahead of the next frame.
    transmits: Vec<String>,
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

    if let Some(protocol) = cache
        .ready
        .get(&key)
        .map(|entry| Arc::clone(&entry.protocol))
    {
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
        .map(|entry| Arc::clone(&entry.protocol))
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

/// Transmit commands for chart images new since the last frame.
///
/// The caller writes them before the frame, so every placeholder in it
/// refers to an image the terminal already holds.
pub fn take_pending_image_transmits() -> Option<String> {
    let manager = CHARTS.get()?;
    let mut cache = manager.cache.lock().ok()?;
    let transmits = std::mem::take(&mut cache.transmits);
    (!transmits.is_empty()).then(|| transmits.concat())
}

/// Kitty's Unicode image placeholder character.
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// Pull a kitty image's one-shot transmit command out of its protocol.
///
/// ratatui-image puts the transmit in the first placeholder cell the first
/// time a protocol renders and never sends it again, so a widget drawn over
/// that cell later in the frame (the help overlay, a dialog) would drop the
/// image for good. Rendering once into a scratch cell takes the command
/// out, and every later render carries only placeholders.
fn take_transmit(protocol: &Protocol) -> Option<String> {
    let area = Rect::new(0, 0, 1, 1);
    let mut scratch = Buffer::empty(area);
    Image::new(protocol)
        .allow_clipping(true)
        .render(area, &mut scratch);
    let symbol = scratch.cell((0, 0))?.symbol();
    let (transmit, _) = symbol.split_once(KITTY_PLACEHOLDER)?;
    (!transmit.is_empty()).then(|| transmit.to_owned())
}

/// Kitty delete commands for chart images the cache has dropped.
///
/// The caller writes them after a frame: a dropped image can no longer be
/// drawn, so that frame has already replaced its placeholder cells.
pub fn take_retired_image_deletes() -> Option<String> {
    let manager = CHARTS.get()?;
    let mut cache = manager.cache.lock().ok()?;
    delete_commands(&std::mem::take(&mut cache.retired))
}

/// Kitty delete commands for every chart image still held, for teardown.
pub fn take_all_image_deletes() -> Option<String> {
    let manager = CHARTS.get()?;
    let mut cache = manager.cache.lock().ok()?;
    let mut images = std::mem::take(&mut cache.retired);
    images.extend(cache.ready.drain().filter_map(|(_, entry)| entry.kitty));
    cache.transmits.clear();
    cache.order.clear();
    cache.latest_by_slot.clear();
    delete_commands(&images)
}

fn delete_commands(images: &[KittyImage]) -> Option<String> {
    if images.is_empty() {
        return None;
    }
    let mut commands = String::new();
    for image in images {
        let (start, escape, end) = if image.tmux {
            ("\x1bPtmux;", "\x1b\x1b", "\x1b\\")
        } else {
            ("", "\x1b", "")
        };
        // Uppercase `d=I` frees the image data along with its placements.
        let _ = write!(
            commands,
            "{start}{escape}_Ga=d,d=I,i={},q=2{escape}\\{end}",
            image.id
        );
    }
    Some(commands)
}

/// Encode a rasterized chart for the terminal.
///
/// Kitty charts are built directly rather than through the picker so the
/// cache knows each image's id and can delete it later. The raster is
/// already `target` cells at the picker's font size, which is exactly what
/// the picker would hand the kitty protocol after fitting.
fn encode_chart(
    picker: &Picker,
    image: DynamicImage,
    target: Size,
) -> Result<EncodedChart, String> {
    if picker.protocol_type() != ProtocolType::Kitty {
        return picker
            .new_protocol(image, target, Resize::Fit(None))
            .map(|protocol| EncodedChart {
                protocol,
                kitty: None,
            })
            .map_err(|error| error.to_string());
    }

    let kitty = KittyImage {
        id: next_kitty_image_id(),
        tmux: picker.tmux_detected(),
    };
    let compress = picker
        .capabilities()
        .contains(&Capability::KittyCompression);
    Kitty::new(image, target, kitty.id, kitty.tmux, compress)
        .map(|protocol| EncodedChart {
            protocol: Protocol::Kitty(protocol),
            kitty: Some(kitty),
        })
        .map_err(|error| error.to_string())
}

/// Next kitty image id, seeded per process so ids rarely meet images that
/// another program left in the terminal. Zero is not a valid id.
fn next_kitty_image_id() -> u32 {
    static NEXT: OnceLock<AtomicU32> = OnceLock::new();
    let next = NEXT.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.subsec_nanos());
        AtomicU32::new(nanos ^ std::process::id().rotate_left(16))
    });
    loop {
        let id = next.fetch_add(1, Ordering::Relaxed);
        if id != 0 {
            return id;
        }
    }
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
                    let result = encode_chart(&request.picker, image, request.target);
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
                    Ok(encoded) => {
                        if encoded.kitty.is_some()
                            && let Some(transmit) = take_transmit(&encoded.protocol)
                        {
                            cache.transmits.push(transmit);
                        }
                        cache.insert_ready(response.slot, response.key, encoded);
                    }
                    Err(error) => {
                        tracing::debug!(
                            key = response.key.0,
                            "graphics chart encode failed: {error}"
                        );
                        cache.remove_ready(response.key);
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

    fn insert_ready(&mut self, slot: ChartSlotKey, key: ChartImageKey, encoded: EncodedChart) {
        self.failed.remove(&key);
        let entry = ReadyChart {
            slot,
            protocol: Arc::new(encoded.protocol),
            kitty: encoded.kitty,
        };
        match self.ready.insert(key, entry) {
            Some(replaced) => self.retired.extend(replaced.kitty),
            None => self.order.push_back(key),
        }
        self.latest_by_slot.insert(slot, key);
        self.evict_over_capacity(slot, key);
    }

    /// Drop the oldest images past the per-slot and overall limits, never
    /// the one just inserted.
    fn evict_over_capacity(&mut self, slot: ChartSlotKey, keep: ChartImageKey) {
        let older_in_slot: Vec<ChartImageKey> = self
            .order
            .iter()
            .copied()
            .filter(|key| {
                *key != keep && self.ready.get(key).is_some_and(|entry| entry.slot == slot)
            })
            .collect();
        let slot_excess = (older_in_slot.len() + 1).saturating_sub(CHART_IMAGES_PER_SLOT);
        for expired in older_in_slot.into_iter().take(slot_excess) {
            self.remove_ready(expired);
        }

        while self.ready.len() > CHART_CACHE_CAPACITY {
            let Some(expired) = self.order.iter().copied().find(|key| *key != keep) else {
                break;
            };
            self.remove_ready(expired);
        }
    }

    /// Forget an image and queue its terminal copy for deletion.
    fn remove_ready(&mut self, key: ChartImageKey) {
        self.order.retain(|ordered| *ordered != key);
        if let Some(entry) = self.ready.remove(&key) {
            self.retired.extend(entry.kitty);
        }
        self.latest_by_slot
            .retain(|_, latest_key| *latest_key != key);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CHART_CACHE_CAPACITY, CHART_IMAGES_PER_SLOT, CHART_MIN_REFRESH_INTERVAL, ChartCache,
        ChartImageKey, ChartSlotKey, EncodedChart, KittyImage, delete_commands,
        picker_protocol_from_graphics, protocol_from_picker,
    };
    use crate::tui::render_caps::GraphicsProtocol;
    use image::DynamicImage;
    use ratatui::layout::Size;
    use ratatui_image::protocol::Protocol;
    use ratatui_image::protocol::kitty::Kitty;
    use std::time::Instant;

    fn kitty_chart(id: u32) -> EncodedChart {
        let kitty = Kitty::new(
            DynamicImage::new_rgba8(1, 1),
            Size::new(1, 1),
            id,
            false,
            false,
        )
        .expect("a 1x1 kitty image should encode");
        EncodedChart {
            protocol: Protocol::Kitty(kitty),
            kitty: Some(KittyImage { id, tmux: false }),
        }
    }

    fn retired_ids(cache: &ChartCache) -> Vec<u32> {
        cache.retired.iter().map(|image| image.id).collect()
    }

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

    #[test]
    fn a_slot_keeps_its_newest_images_and_retires_the_rest() {
        let slot = ChartSlotKey(1);
        let mut cache = ChartCache::default();
        let total = u32::try_from(CHART_IMAGES_PER_SLOT).expect("small constant") + 2;

        for id in 1..=total {
            cache.insert_ready(slot, ChartImageKey(u64::from(id)), kitty_chart(id));
        }

        assert_eq!(cache.ready.len(), CHART_IMAGES_PER_SLOT);
        assert_eq!(retired_ids(&cache), vec![1, 2]);
        assert_eq!(
            cache.latest_by_slot.get(&slot),
            Some(&ChartImageKey(u64::from(total)))
        );
        assert!(!cache.order.contains(&ChartImageKey(1)));
    }

    #[test]
    fn other_slots_keep_their_images_until_the_overall_cap() {
        let mut cache = ChartCache::default();
        for id in 1..=3 {
            cache.insert_ready(
                ChartSlotKey(u64::from(id)),
                ChartImageKey(u64::from(id)),
                kitty_chart(id),
            );
        }
        assert_eq!(retired_ids(&cache), Vec::<u32>::new());

        let slots = u32::try_from(CHART_CACHE_CAPACITY).expect("small constant") + 1;
        for id in 4..=slots {
            cache.insert_ready(
                ChartSlotKey(u64::from(id)),
                ChartImageKey(u64::from(id)),
                kitty_chart(id),
            );
        }
        assert_eq!(cache.ready.len(), CHART_CACHE_CAPACITY);
        assert_eq!(retired_ids(&cache), vec![1]);
        assert!(!cache.latest_by_slot.contains_key(&ChartSlotKey(1)));
    }

    #[test]
    fn a_failed_reencode_retires_the_image_it_replaces() {
        let slot = ChartSlotKey(1);
        let key = ChartImageKey(7);
        let mut cache = ChartCache::default();
        cache.insert_ready(slot, key, kitty_chart(70));

        cache.remove_ready(key);

        assert!(cache.ready.is_empty());
        assert!(cache.order.is_empty());
        assert_eq!(retired_ids(&cache), vec![70]);
    }

    #[test]
    fn a_new_image_hands_over_its_transmit_once() {
        let encoded = kitty_chart(42);

        let transmit = super::take_transmit(&encoded.protocol).expect("first render transmits");
        assert!(transmit.starts_with("\x1b_G"));
        assert!(transmit.contains("i=42"));
        assert_eq!(super::take_transmit(&encoded.protocol), None);
    }

    #[test]
    fn delete_commands_free_each_image_by_id() {
        assert_eq!(delete_commands(&[]), None);
        assert_eq!(
            delete_commands(&[
                KittyImage { id: 5, tmux: false },
                KittyImage { id: 6, tmux: false },
            ])
            .as_deref(),
            Some("\x1b_Ga=d,d=I,i=5,q=2\x1b\\\x1b_Ga=d,d=I,i=6,q=2\x1b\\")
        );
        assert_eq!(
            delete_commands(&[KittyImage { id: 9, tmux: true }]).as_deref(),
            Some("\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=9,q=2\x1b\x1b\\\x1b\\")
        );
    }
}
