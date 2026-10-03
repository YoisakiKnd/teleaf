//! Decode and encode only requested images, off the UI thread.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread;
use std::time::Duration;

use image::GenericImageView;
use ratatui::layout::Size;
use ratatui_image::Resize;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::protocol::Protocol;
use ratatui_image::sliced::SlicedProtocol;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct View {
    pub zoom: u16,
    pub x: u16,
    pub y: u16,
    pub inline: bool,
}
impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 100,
            x: 500,
            y: 500,
            inline: false,
        }
    }
}
impl View {
    pub fn step(&mut self, direction: isize) -> bool {
        const LEVELS: [u16; 5] = [100, 150, 200, 300, 400];
        let before = *self;
        let current = LEVELS.iter().position(|z| *z == self.zoom).unwrap_or(0);
        self.zoom = LEVELS[current
            .saturating_add_signed(direction)
            .min(LEVELS.len() - 1)];
        if self.zoom == 100 {
            self.x = 500;
            self.y = 500;
        }
        before != *self
    }
}
type Key = (i32, u16, u16, View);
struct DecodedImage {
    file_id: i32,
    path: String,
    image: image::DynamicImage,
    cropped_view: Option<View>,
}
type Decoded = VecDeque<DecodedImage>;
const DECODED_BUDGET: usize = 8 * 1024 * 1024;
const NATIVE_PIXELS: u64 = 1_500_000;
const SOURCE_EDGE: u32 = 32_768;

struct Job {
    key: Key,
    path: String,
    kitty_id: u32,
}

enum Encoded {
    Regular(Protocol, usize),
    Inline(SlicedProtocol, usize),
}
impl Encoded {
    fn regular(&self) -> Option<&Protocol> {
        if let Self::Regular(image, _) = self {
            Some(image)
        } else {
            None
        }
    }
    fn inline(&self) -> Option<&SlicedProtocol> {
        if let Self::Inline(image, _) = self {
            Some(image)
        } else {
            None
        }
    }
    fn weight(&self) -> usize {
        match self {
            Self::Regular(_, bytes) | Self::Inline(_, bytes) => *bytes,
        }
    }
}

const ENCODED_BUDGET: usize = 12 * 1024 * 1024;

struct Finished {
    key: Key,
    result: Option<Result<Encoded, String>>,
}

pub struct MediaManager {
    jobs: SyncSender<Job>,
    finished: Receiver<Finished>,
    pending: HashSet<Key>,
    failed: HashSet<Key>,
    cache: HashMap<Key, Encoded>,
    recent: VecDeque<Key>,
    cache_bytes: usize,
    visible: HashSet<Key>,
    frame_pinning: bool,
    kitty_slots: HashMap<Key, u32>,
    is_kitty: bool,
    pub last_error: Option<String>,
    pub protocol_name: String,
}

impl MediaManager {
    pub fn new() -> Self {
        let picker = crate::terminal::picker();
        Self::from_picker(picker)
    }

    pub(crate) fn from_picker(picker: Picker) -> Self {
        let protocol_name = format!("{:?}", picker.protocol_type());
        let is_kitty = picker.protocol_type() == ProtocolType::Kitty;
        let (jobs, job_receiver) = mpsc::sync_channel::<Job>(2);
        let (result_sender, finished) = mpsc::sync_channel::<Finished>(2);
        thread::spawn(move || {
            let mut decoded = Decoded::new();
            loop {
                let mut job = match job_receiver.recv_timeout(Duration::from_secs(30)) {
                    Ok(job) => job,
                    Err(RecvTimeoutError::Timeout) => {
                        decoded.clear();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                };
                // Keep the newest queued viewport for a file; cancelled jobs
                // still acknowledge their keys so pending work stays bounded.
                let mut jobs = VecDeque::new();
                jobs.push_back(job);
                while let Ok(next) = job_receiver.try_recv() {
                    if let Some(index) = jobs.iter().position(|old: &Job| {
                        old.key.0 == next.key.0
                            && old.key.1 == next.key.1
                            && old.key.2 == next.key.2
                            && old.key.3.inline == next.key.3.inline
                    }) {
                        let old = jobs.remove(index).expect("job");
                        if result_sender
                            .send(Finished {
                                key: old.key,
                                result: None,
                            })
                            .is_err()
                        {
                            return;
                        }
                    }
                    jobs.push_back(next);
                }
                while let Some(next) = jobs.pop_front() {
                    job = next;
                    let prepared = prepare(&picker, &job, &mut decoded, job.kitty_id);
                    let result = Some(prepared);
                    if result_sender
                        .send(Finished {
                            key: job.key,
                            result,
                        })
                        .is_err()
                    {
                        return;
                    }
                }
            }
        });
        Self {
            jobs,
            finished,
            pending: HashSet::new(),
            failed: HashSet::new(),
            cache: HashMap::new(),
            recent: VecDeque::new(),
            cache_bytes: 0,
            visible: HashSet::new(),
            frame_pinning: false,
            kitty_slots: HashMap::new(),
            is_kitty,
            last_error: None,
            protocol_name,
        }
    }

    pub fn begin_frame(&mut self) {
        self.frame_pinning = true;
        self.visible.clear();
    }

    pub fn end_frame(&mut self) {
        self.evict_hidden();
    }

    fn evict_hidden(&mut self) {
        while self.cache.len() > 12 || self.cache_bytes > ENCODED_BUDGET {
            let Some(index) = self
                .recent
                .iter()
                .position(|key| !self.visible.contains(key))
            else {
                break;
            };
            let key = self.recent.remove(index).expect("cached key");
            if let Some(old) = self.cache.remove(&key) {
                self.cache_bytes -= old.weight();
            }
            self.kitty_slots.remove(&key);
        }
    }

    pub fn request(&mut self, file_id: i32, path: &str, area: Size) {
        self.request_view(file_id, path, area, View::default());
    }
    pub fn request_view(&mut self, file_id: i32, path: &str, area: Size, view: View) {
        if file_id == 0 || area.width == 0 || area.height == 0 {
            return;
        }
        let key = (file_id, area.width, area.height, view);
        if self.frame_pinning {
            self.visible.insert(key);
        }
        if self.cache.contains_key(&key) {
            self.recent.retain(|candidate| *candidate != key);
            self.recent.push_back(key);
            return;
        }
        if self.pending.contains(&key) || self.failed.contains(&key) {
            return;
        }
        // A cached or pending frame owns its ID until eviction/cancellation.
        // Counting successful encodes cannot establish whether an old ID is free.
        let kitty_id = if self.is_kitty {
            let prefix = std::process::id().wrapping_shl(8);
            let Some(id) = (1..=32)
                .map(|slot| prefix | slot)
                .find(|id| !self.kitty_slots.values().any(|used| used == id))
            else {
                return;
            };
            id
        } else {
            0
        };
        match self.jobs.try_send(Job {
            key,
            path: path.to_owned(),
            kitty_id,
        }) {
            Ok(()) => {
                self.pending.insert(key);
                if self.is_kitty {
                    self.kitty_slots.insert(key, kitty_id);
                }
            }
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(_)) => {
                self.last_error = Some("图片处理线程已停止".into());
            }
        }
    }

    pub fn request_inline(&mut self, file_id: i32, path: &str, full_size: Size) {
        self.request_view(
            file_id,
            path,
            full_size,
            View {
                inline: true,
                ..View::default()
            },
        );
    }
    pub fn get_inline(&self, file_id: i32, full_size: Size) -> Option<&SlicedProtocol> {
        self.cache
            .get(&(
                file_id,
                full_size.width,
                full_size.height,
                View {
                    inline: true,
                    ..View::default()
                },
            ))
            .and_then(Encoded::inline)
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(finished) = self.finished.try_recv() {
            self.pending.remove(&finished.key);
            match finished.result {
                Some(Ok(protocol)) => {
                    self.cache_bytes += protocol.weight();
                    if let Some(old) = self.cache.insert(finished.key, protocol) {
                        self.cache_bytes -= old.weight();
                    }
                    self.recent.retain(|key| *key != finished.key);
                    self.recent.push_back(finished.key);
                    self.evict_hidden();
                    self.last_error = None;
                }
                Some(Err(error)) => {
                    self.kitty_slots.remove(&finished.key);
                    if self.failed.len() >= 64
                        && let Some(old) = self.failed.iter().next().copied()
                    {
                        self.failed.remove(&old);
                    }
                    self.failed.insert(finished.key);
                    self.last_error = Some(error);
                }
                None => {
                    self.kitty_slots.remove(&finished.key);
                    continue;
                }
            }
            changed = true;
        }
        changed
    }

    pub fn get(&self, file_id: i32, area: Size) -> Option<&Protocol> {
        self.cache
            .get(&(file_id, area.width, area.height, View::default()))
            .and_then(Encoded::regular)
    }
    pub fn get_view(&mut self, file_id: i32, area: Size, view: View) -> Option<&Protocol> {
        let requested = (file_id, area.width, area.height, view);
        let key = if self.cache.contains_key(&requested) {
            Some(requested)
        } else {
            self.recent
                .iter()
                .rev()
                .find(|key| {
                    key.0 == file_id && key.1 == area.width && key.2 == area.height && !key.3.inline
                })
                .copied()
        }?;
        if self.frame_pinning {
            self.visible.insert(key);
        }
        self.cache.get(&key).and_then(Encoded::regular)
    }
}

fn prepare(
    picker: &Picker,
    job: &Job,
    decoded: &mut Decoded,
    kitty_id: u32,
) -> Result<Encoded, String> {
    if let Some(index) = decoded.iter().position(|cached| {
        cached.file_id == job.key.0
            && cached.path == job.path
            && cached.cropped_view.is_none_or(|view| view == job.key.3)
    }) {
        let cached = decoded.remove(index).expect("image");
        decoded.push_back(cached);
    } else {
        let metadata = fs::metadata(&job.path).map_err(|error| error.to_string())?;
        if metadata.len() > 32 * 1024 * 1024 {
            return Err("图片文件超过预览限制（32 MiB），请用外部程序打开".into());
        }
        let mut reader = image::ImageReader::open(&job.path)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())?;
        let (width, height) = image::ImageReader::open(&job.path)
            .map_err(|error| error.to_string())?
            .with_guessed_format()
            .map_err(|error| error.to_string())?
            .into_dimensions()
            .map_err(|error| error.to_string())?;
        if width > SOURCE_EDGE
            || height > SOURCE_EDGE
            || u64::from(width) * u64::from(height) > 16_000_000
        {
            return Err("图片像素过大，请用外部程序打开".into());
        }
        // Reclaim old decoded sources before allocating a new large original.
        // Evicting only after decode lets both source working sets overlap.
        reserve_decoded(
            decoded,
            (u64::from(width) * u64::from(height) * 4).min(DECODED_BUDGET as u64) as usize,
        );
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode().map_err(|error| error.to_string())?;
        let (image, cropped_view) = if picker.protocol_type() == ProtocolType::Halfblocks {
            (image.thumbnail(768, 768), None)
        } else if u64::from(image.width()) * u64::from(image.height()) * 4 <= DECODED_BUDGET as u64
        {
            // Retain enough source detail for zoom instead of permanently shrinking
            // every image to 1024 px before its crop is known.
            (image, None)
        } else {
            let view = job.key.3;
            let (x, y, width, height) = crop_region(image.width(), image.height(), view);
            let (w, h) = bounded_pixels(width, height, 2048, NATIVE_PIXELS);
            (
                image::DynamicImage::ImageRgba8(crate::resample::crop_resize(
                    &image,
                    (x, y, width, height),
                    w,
                    h,
                )),
                Some(view),
            )
        };
        // 16-bit / floating point originals must not multiply retained cache memory.
        let image = image::DynamicImage::ImageRgba8(image.into_rgba8());
        decoded.push_back(DecodedImage {
            file_id: job.key.0,
            path: job.path.clone(),
            image,
            cropped_view,
        });
        while decoded.len() > 2
            || decoded
                .iter()
                .map(|cached| cached.image.as_bytes().len())
                .sum::<usize>()
                > DECODED_BUDGET
        {
            decoded.pop_front();
        }
    }
    let cached = decoded.back().expect("decoded");
    let image = &cached.image;
    let view = job.key.3;
    let (x, y, width, height) = crop_region(
        image.width(),
        image.height(),
        if cached.cropped_view.is_some() {
            View::default()
        } else {
            view
        },
    );
    let cropped = image
        .as_rgba8()
        .expect("RGBA8 cache")
        .view(x, y, width, height);
    let mut size = Size::new(job.key.1.min(160), job.key.2.min(60));
    if picker.protocol_type() == ProtocolType::Halfblocks {
        let size = fitted_cells(width, height, picker.font_size(), size);
        // Encode at the two pixels per cell actually visible in the fallback.
        // Avoid allocating a full terminal-font-sized intermediate bitmap.
        let pixels = image::DynamicImage::ImageRgba8(crate::resample::canvas(
            &*cropped,
            u32::from(size.width),
            u32::from(size.height) * 2,
        ));
        let halfblocks = ratatui_image::protocol::halfblocks::Halfblocks::new(pixels, size)
            .map_err(|error| error.to_string())?;
        let bytes = usize::from(size.width)
            * usize::from(size.height)
            * std::mem::size_of::<ratatui_image::protocol::halfblocks::HalfBlock>();
        return Ok(if view.inline {
            Encoded::Inline(SlicedProtocol::Halfblocks(halfblocks), bytes)
        } else {
            Encoded::Regular(Protocol::Halfblocks(halfblocks), bytes)
        });
    }
    // Inline images stay small; the detailed viewer gets a larger pixel canvas.
    let font = picker.font_size();
    if font.width > 1024 || font.height > 1024 {
        return Err("终端报告的字体尺寸过大，请使用字符预览或外部程序打开".into());
    }
    let edge = if view.inline { 960 } else { 2048 };
    size.width = size.width.min((edge / font.width.max(1)).max(1));
    size.height = size.height.min((edge / font.height.max(1)).max(1));
    let pixels = u64::from(size.width)
        * u64::from(size.height)
        * u64::from(font.width)
        * u64::from(font.height);
    if pixels > NATIVE_PIXELS {
        let ratio = (NATIVE_PIXELS as f64 / pixels as f64).sqrt();
        size.width = ((f64::from(size.width) * ratio) as u16).max(1);
        size.height = ((f64::from(size.height) * ratio) as u16).max(1);
    }
    size = fitted_cells(width, height, font, size);
    let canvas_width = u32::from(size.width) * u32::from(font.width);
    let canvas_height = u32::from(size.height) * u32::from(font.height);
    // Count the fitted canvas, rather than unused space in the available area.
    let bytes = canvas_width as usize
        * canvas_height as usize
        * if picker.protocol_type() == ProtocolType::Kitty {
            6
        } else {
            12
        };
    let pixels = image::DynamicImage::ImageRgba8(crate::resample::canvas(
        &*cropped,
        canvas_width,
        canvas_height,
    ));
    if picker.protocol_type() == ProtocolType::Kitty {
        let is_tmux = std::env::var_os("TMUX").is_some()
            || std::env::var("TERM").is_ok_and(|s| s.starts_with("tmux"))
            || std::env::var("TERM_PROGRAM").as_deref() == Ok("tmux");
        ratatui_image::protocol::kitty::Kitty::new(pixels, size, kitty_id, is_tmux)
            .map(|image| {
                if view.inline {
                    Encoded::Inline(SlicedProtocol::Kitty(image), bytes)
                } else {
                    Encoded::Regular(Protocol::Kitty(image), bytes)
                }
            })
            .map_err(|e| e.to_string())
    } else if view.inline {
        // Already fitted and padded: protocol constructors must not resample.
        SlicedProtocol::new_with_resize(picker, pixels, size, Resize::Crop(None))
            .map(|protocol| Encoded::Inline(protocol, bytes))
            .map_err(|error| error.to_string())
    } else {
        picker
            .new_protocol(pixels, size, Resize::Fit(None))
            .map(|protocol| Encoded::Regular(protocol, bytes))
            .map_err(|error| error.to_string())
    }
}

fn reserve_decoded(decoded: &mut Decoded, incoming: usize) {
    while decoded.len() >= 2
        || decoded
            .iter()
            .map(|cached| cached.image.as_bytes().len())
            .sum::<usize>()
            + incoming
            > DECODED_BUDGET
    {
        if decoded.pop_front().is_none() {
            break;
        }
    }
}

fn fitted_cells(width: u32, height: u32, font: ratatui_image::FontSize, available: Size) -> Size {
    let (width, height) = crate::resample::fit(
        width,
        height,
        u32::from(available.width) * u32::from(font.width),
        u32::from(available.height) * u32::from(font.height),
    );
    Size::new(
        width.div_ceil(u32::from(font.width)) as u16,
        height.div_ceil(u32::from(font.height)) as u16,
    )
}

fn crop_region(width: u32, height: u32, view: View) -> (u32, u32, u32, u32) {
    let crop_width = (width * 100 / u32::from(view.zoom.clamp(100, 400))).max(1);
    let crop_height = (height * 100 / u32::from(view.zoom.clamp(100, 400))).max(1);
    let x = (width - crop_width) * u32::from(view.x.min(1000)) / 1000;
    let y = (height - crop_height) * u32::from(view.y.min(1000)) / 1000;
    (x, y, crop_width, crop_height)
}

fn bounded_pixels(width: u32, height: u32, edge: u32, pixels: u64) -> (u32, u32) {
    let ratio = (f64::from(edge) / f64::from(width.max(height)))
        .min((pixels as f64 / (u64::from(width) * u64::from(height)).max(1) as f64).sqrt())
        .min(1.0);
    (
        ((f64::from(width) * ratio) as u32).max(1),
        ((f64::from(height) * ratio) as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extreme_aspect_ratio_is_rejected_before_allocating_resampler_scratch() {
        let path = std::env::temp_dir().join(format!("tg-wide-source-{}.png", std::process::id()));
        image::RgbImage::from_pixel(SOURCE_EDGE + 1, 1, image::Rgb([20, 30, 40]))
            .save(&path)
            .unwrap();
        let job = Job {
            key: (1, 8, 8, View::default()),
            path: path.to_string_lossy().into_owned(),
            kitty_id: 1,
        };
        let mut decoded = Decoded::new();
        assert!(prepare(&Picker::halfblocks(), &job, &mut decoded, 1).is_err());
        assert!(decoded.is_empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn visible_images_survive_budget_pressure_and_settle_without_reload_loops() {
        let path = std::env::temp_dir().join(format!("tg-pinned-{}.png", std::process::id()));
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 100, 30]))
            .save(&path)
            .unwrap();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut manager = MediaManager::from_picker(picker);
        let size = Size::new(160, 60);
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        loop {
            manager.poll();
            manager.begin_frame();
            for id in 1..=3 {
                manager.request(id, path.to_str().unwrap(), size);
            }
            manager.end_frame();
            if (1..=3).all(|id| manager.get(id, size).is_some()) && manager.pending.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "visible images keep reloading under budget pressure"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            manager.cache_bytes > ENCODED_BUDGET,
            "fixture must exercise working-set pressure"
        );
        for _ in 0..30 {
            assert!(!manager.poll());
            manager.begin_frame();
            for id in 1..=3 {
                manager.request(id, path.to_str().unwrap(), size);
                assert!(manager.get(id, size).is_some());
            }
            manager.end_frame();
            assert!(manager.pending.is_empty());
        }
        manager.begin_frame();
        manager.request(3, path.to_str().unwrap(), size);
        manager.end_frame();
        assert!(
            manager.cache_bytes <= ENCODED_BUDGET,
            "hidden images should be reclaimed"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_hot_kitty_frame_keeps_its_id_while_many_other_frames_are_encoded() {
        let path = std::env::temp_dir().join(format!("tg-held-id-{}.png", std::process::id()));
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 100, 30]))
            .save(&path)
            .unwrap();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut manager = MediaManager::from_picker(picker);
        let size = Size::new(8, 8);
        let held = (1, 8, 8, View::default());
        for id in 1..=70 {
            manager.request(1, path.to_str().unwrap(), size);
            manager.request(id, path.to_str().unwrap(), size);
            let key = (id, 8, 8, View::default());
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !manager.cache.contains_key(&key) {
                manager.poll();
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(
                manager.kitty_slots.get(&held),
                Some(&(std::process::id().wrapping_shl(8) | 1))
            );
            let ids: HashSet<_> = manager.kitty_slots.values().collect();
            assert_eq!(
                ids.len(),
                manager.kitty_slots.len(),
                "a live image ID was overwritten"
            );
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn large_native_zoom_crops_original_before_downsampling_and_bounds_decoded_bytes() {
        let path = std::env::temp_dir().join(format!("tg-zoom-detail-{}.png", std::process::id()));
        image::RgbImage::from_pixel(2048, 2048, image::Rgb([200, 100, 30]))
            .save(&path)
            .unwrap();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let view = View {
            zoom: 400,
            ..View::default()
        };
        let job = Job {
            key: (1, 16, 8, view),
            path: path.to_string_lossy().into_owned(),
            kitty_id: 1,
        };
        let mut decoded = Decoded::new();
        prepare(&picker, &job, &mut decoded, 1).unwrap();
        assert_eq!(decoded[0].cropped_view, Some(view));
        assert_eq!(
            (decoded[0].image.width(), decoded[0].image.height()),
            (512, 512),
            "4x zoom must use the original 512px crop, not a shrunken-source 256px crop"
        );
        assert!(
            decoded
                .iter()
                .map(|image| image.image.as_bytes().len())
                .sum::<usize>()
                <= DECODED_BUDGET
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn native_cache_evicts_by_memory_budget_and_caps_large_font_canvas() {
        let path =
            std::env::temp_dir().join(format!("tg-native-budget-{}.png", std::process::id()));
        image::RgbImage::from_fn(96, 96, |x, y| image::Rgb([x as u8, y as u8, 50]))
            .save(&path)
            .unwrap();
        // Fixture-only constructor: emulate high DPI without querying the user's terminal.
        #[allow(deprecated)]
        let mut picker = Picker::from_fontsize(ratatui_image::FontSize::new(40, 80));
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut manager = MediaManager::from_picker(picker);
        for index in 0..4 {
            let view = View {
                zoom: 200,
                x: index * 200,
                ..View::default()
            };
            let size = Size::new(160, 60);
            let key = (55, size.width, size.height, view);
            manager.request_view(55, path.to_str().unwrap(), size, view);
            let deadline = std::time::Instant::now() + Duration::from_secs(4);
            while !manager.cache.contains_key(&key) {
                manager.poll();
                assert!(
                    std::time::Instant::now() < deadline,
                    "bounded canvas failed to settle"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            let encoded = manager.cache.get(&key).unwrap();
            let size = encoded.regular().unwrap().size();
            assert!(u32::from(size.width) * 40 <= 2048);
            assert!(u32::from(size.height) * 80 <= 2048);
            assert!(u64::from(size.width) * 40 * u64::from(size.height) * 80 <= NATIVE_PIXELS);
            assert!(manager.cache_bytes <= ENCODED_BUDGET);
        }
        assert!(
            manager.cache.len() < 4,
            "byte budget must evict before the item limit"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decoded_cache_reduces_large_16_bit_images_to_bounded_rgba8() {
        let path =
            std::env::temp_dir().join(format!("tg-decode-budget-{}.png", std::process::id()));
        image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
            1024,
            1024,
            image::Rgba([65535, 20000, 10000, 65535]),
        )
        .save(&path)
        .unwrap();
        let mut decoded = Decoded::new();
        let job = Job {
            kitty_id: 1,
            key: (51, 160, 60, View::default()),
            path: path.to_string_lossy().into_owned(),
        };
        let encoded = prepare(&Picker::halfblocks(), &job, &mut decoded, 1).unwrap();
        let image = &decoded[0].image;
        assert_eq!(image.color(), image::ColorType::Rgba8);
        assert!(image.width() <= 768 && image.height() <= 768);
        assert!(image.as_bytes().len() <= 768 * 768 * 4);
        assert!(
            encoded.weight() < 256 * 1024,
            "fallback should cache cells, not font-sized pixels"
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn halfblock_fallback_can_decode_png() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("tg-tui-media-{}-{nonce}.png", std::process::id()));
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([240, 80, 40, 255]));
        image.save(&path).expect("write image fixture");
        let job = Job {
            kitty_id: 1,
            key: (1, 4, 4, View::default()),
            path: path.to_string_lossy().into_owned(),
        };
        let result = prepare(&Picker::halfblocks(), &job, &mut Decoded::new(), 1);
        let _ = fs::remove_file(&path);
        if let Err(error) = result {
            panic!("halfblock preview failed: {error}");
        }
    }
    #[test]
    fn zoom_crops_reuse_decoded_image_after_source_is_removed() {
        let path = std::env::temp_dir().join(format!("tg-zoom-cache-{}.png", std::process::id()));
        let image = image::RgbImage::from_fn(32, 32, |x, y| {
            image::Rgb([(x * 7) as u8, (y * 7) as u8, 50])
        });
        image.save(&path).unwrap();
        let mut decoded = Decoded::new();
        let mut job = Job {
            kitty_id: 1,
            key: (999, 8, 8, View::default()),
            path: path.to_string_lossy().into_owned(),
        };
        let first = prepare(&Picker::halfblocks(), &job, &mut decoded, 1).unwrap();
        let first = first.regular().unwrap();
        std::fs::remove_file(&path).unwrap();
        job.key.3 = View {
            zoom: 300,
            x: 0,
            y: 1000,
            ..View::default()
        };
        let second = prepare(&Picker::halfblocks(), &job, &mut decoded, 1).unwrap();
        let second = second.regular().unwrap();
        let render = |protocol: &Protocol| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 8)).unwrap();
            terminal
                .draw(|frame| {
                    frame.render_widget(ratatui_image::Image::new(protocol), frame.area())
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        assert_eq!(
            first.size(),
            second.size(),
            "zoom must magnify content without shrinking its display area"
        );
        assert_ne!(render(first), render(second));
        assert_eq!(decoded.len(), 1);
    }
    #[test]
    fn inline_scroll_clips_pixels_without_rescaling_or_reencoding() {
        use ratatui_image::sliced::{SignedPosition, SlicedImage};
        let path = std::env::temp_dir().join(format!("tg-inline-{}.png", std::process::id()));
        image::RgbImage::from_fn(32, 64, |x, y| image::Rgb([x as u8 * 7, y as u8 * 3, 60]))
            .save(&path)
            .unwrap();
        let job = Job {
            kitty_id: 1,
            key: (
                44,
                8,
                8,
                View {
                    inline: true,
                    ..View::default()
                },
            ),
            path: path.to_string_lossy().into_owned(),
        };
        let prepared = prepare(&Picker::halfblocks(), &job, &mut Decoded::new(), 1).unwrap();
        let full = prepared.inline().unwrap();
        let render = |top: i16, height: u16| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, height)).unwrap();
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        SlicedImage::new(full, SignedPosition::from((0, -top))),
                        frame.area(),
                    )
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let full = render(0, 8);
        let clipped = render(2, 3);
        for y in 0..3 {
            for x in 0..8 {
                assert_eq!(full[(x, y + 2)], clipped[(x, y)]);
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rapid_zoom_requests_settle_on_final_view_with_bounded_cache_and_no_pending_leaks() {
        let path = std::env::temp_dir().join(format!("tg-zoom-queue-{}.png", std::process::id()));
        image::RgbImage::from_fn(64, 64, |x, y| image::Rgb([x as u8, y as u8, 0]))
            .save(&path)
            .unwrap();
        let mut manager = MediaManager::from_picker(Picker::halfblocks());
        let size = Size::new(12, 8);
        let final_view = View {
            zoom: 400,
            x: 999,
            y: 0,
            ..View::default()
        };
        for x in 0..100 {
            manager.request_view(
                91,
                path.to_str().unwrap(),
                size,
                View {
                    zoom: 200,
                    x: x * 10,
                    y: 500,
                    ..View::default()
                },
            );
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        loop {
            manager.poll();
            manager.request_view(91, path.to_str().unwrap(), size, final_view);
            if manager.pending.is_empty() && manager.cache.contains_key(&(91, 12, 8, final_view)) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "zoom pipeline did not settle"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(manager.cache.len() <= 12);
        assert!(manager.recent.len() <= 12);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn kitty_zoom_reuses_bounded_terminal_ids_without_colliding_with_cached_frames() {
        let path = std::env::temp_dir().join(format!("tg-kitty-zoom-{}.png", std::process::id()));
        image::RgbImage::from_pixel(16, 16, image::Rgb([200, 100, 30]))
            .save(&path)
            .unwrap();
        let mut picker = Picker::halfblocks();
        picker.set_protocol_type(ProtocolType::Kitty);
        let mut manager = MediaManager::from_picker(picker);
        let mut ids = HashSet::new();
        for index in 0..40 {
            let view = View {
                zoom: 200,
                x: index * 20,
                y: 500,
                ..View::default()
            };
            let key = (77, 8, 8, view);
            manager.request_view(77, path.to_str().unwrap(), Size::new(8, 8), view);
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !manager.cache.contains_key(&key) {
                manager.poll();
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(1));
            }
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 8)).unwrap();
            terminal
                .draw(|frame| {
                    frame.render_widget(
                        ratatui_image::Image::new(
                            manager.cache.get(&key).unwrap().regular().unwrap(),
                        ),
                        frame.area(),
                    )
                })
                .unwrap();
            let symbols = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            let start = symbols.find("i=").expect("Kitty upload ID") + 2;
            let id = symbols[start..]
                .split(',')
                .next()
                .unwrap()
                .parse::<u32>()
                .unwrap();
            ids.insert(id);
            assert!(manager.cache.len() <= 12);
        }
        assert!(ids.len() <= 32);
        std::fs::remove_file(path).unwrap();
    }
}
