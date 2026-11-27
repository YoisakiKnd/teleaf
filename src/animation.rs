//! One local animation preview at a time. Decoding and process waits stay off the UI thread.
use image::{AnimationDecoder, DynamicImage, ImageDecoder, RgbaImage, codecs::gif::GifDecoder};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread,
    time::{Duration, Instant},
};

const FRAME_BUDGET: usize = 8 * 1024 * 1024;
const MAX_FRAMES: usize = 60;
pub const MAX_INLINE_SLOTS: usize = 4;
const INLINE_SLOT_BUDGET: usize = 2 * 1024 * 1024;
const INLINE_MAX_FRAMES: usize = 36;
const MAX_INLINE_DECODES: usize = 2;
const FRAMERATE: u64 = 10;
const DECODE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, PartialEq, Eq)]
struct Target {
    file: i32,
    path: PathBuf,
    bounds: (u32, u32),
    budget: usize,
    max_frames: usize,
}

struct Frame {
    rgba: Box<[u8]>,
    duration: Duration,
}

struct Frames {
    size: (u32, u32),
    frames: Vec<Frame>,
}

struct Job {
    target: Target,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Frames, &'static str>>,
}

pub struct Player {
    budget: usize,
    max_frames: usize,
    target: Option<Target>,
    job: Option<Job>,
    frames: Option<Frames>,
    error: Option<&'static str>,
    frame: usize,
    next: Option<Instant>,
}

impl Default for Player {
    fn default() -> Self {
        Self::with_limits(FRAME_BUDGET, MAX_FRAMES)
    }
}

impl Player {
    pub fn with_limits(budget: usize, max_frames: usize) -> Self {
        Self {
            budget,
            max_frames,
            target: None,
            job: None,
            frames: None,
            error: None,
            frame: 0,
            next: None,
        }
    }

    pub fn request(&mut self, file: i32, path: &str, bounds: (u32, u32)) {
        self.request_with_spawn(file, path, bounds, true);
    }

    fn request_with_spawn(&mut self, file: i32, path: &str, bounds: (u32, u32), allow_spawn: bool) {
        let target = Target {
            file,
            path: path.into(),
            bounds,
            budget: self.budget,
            max_frames: self.max_frames,
        };
        if self.target.as_ref() == Some(&target) {
            if allow_spawn && self.frames.is_none() && self.error.is_none() {
                self.start();
            }
            return;
        }
        self.stop();
        self.target = Some(target);
        if allow_spawn {
            self.start();
        }
    }

    fn needs_start(&self) -> bool {
        self.target.is_some() && self.job.is_none() && self.frames.is_none() && self.error.is_none()
    }

    fn is_decoding(&self) -> bool {
        self.job.is_some()
    }

    fn start(&mut self) {
        if self.job.is_some() {
            return;
        }
        let Some(target) = self.target.clone() else {
            return;
        };
        let worker_target = target.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (send, result) = mpsc::sync_channel(1);
        self.job = Some(Job {
            target,
            cancel,
            result,
        });
        thread::spawn(move || {
            let decoded = decode(&worker_target, &worker_cancel);
            let _ = send.send(decoded);
        });
    }

    pub fn stop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
            // Discard a result that already arrived, or cancel the worker's next frame.
            let _ = job.result.try_recv();
        }
        self.target = None;
        self.frames = None;
        self.error = None;
        self.frame = 0;
        self.next = None;
    }

    pub fn is_requested(&self) -> bool {
        self.target.is_some()
    }

    pub fn poll(&mut self, now: Instant) -> bool {
        self.poll_with_spawn(now, true)
    }

    fn poll_with_spawn(&mut self, now: Instant, allow_spawn: bool) -> bool {
        if let Some(job) = &self.job {
            let result = match job.result.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err("动图解码已中断")),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let job = self.job.take().unwrap();
                if job.cancel.load(Ordering::Relaxed) || self.target.as_ref() != Some(&job.target) {
                    if allow_spawn {
                        self.start();
                    }
                    return false;
                }
                match result {
                    Ok(frames) => {
                        self.next =
                            (frames.frames.len() > 1).then(|| now + frames.frames[0].duration);
                        self.frames = Some(frames);
                    }
                    Err(error) => self.error = Some(error),
                }
                return true;
            }
        }
        if self.next.is_some_and(|next| now >= next)
            && let Some(frames) = &self.frames
        {
            self.frame = (self.frame + 1) % frames.frames.len();
            self.next = Some(now + frames.frames[self.frame].duration);
            return true;
        }
        false
    }

    pub fn toggle(&mut self, now: Instant) -> bool {
        if let Some(frames) = self
            .frames
            .as_ref()
            .filter(|frames| frames.frames.len() > 1)
        {
            self.next = if self.next.is_some() {
                None
            } else {
                Some(now + frames.frames[self.frame].duration)
            };
            return true;
        }
        false
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.next
    }

    pub fn hint(&self) -> &'static str {
        if let Some(error) = self.error {
            error
        } else if self
            .frames
            .as_ref()
            .is_some_and(|frames| frames.frames.len() == 1)
        {
            "静态帧 · o 用系统程序打开"
        } else if self.frames.is_some() {
            if self.next.is_some() {
                "Space 暂停"
            } else {
                "Space 继续"
            }
        } else {
            "正在准备动图…"
        }
    }

    pub fn frame_index(&self) -> Option<usize> {
        self.frames.as_ref().map(|_| self.frame)
    }

    pub fn image(&self) -> Option<DynamicImage> {
        let frames = self.frames.as_ref()?;
        RgbaImage::from_raw(
            frames.size.0,
            frames.size.1,
            frames.frames[self.frame].rgba.to_vec(),
        )
        .map(DynamicImage::ImageRgba8)
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct InlinePool {
    slots: [Player; MAX_INLINE_SLOTS],
    seen: [bool; MAX_INLINE_SLOTS],
}

impl Default for InlinePool {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| {
                Player::with_limits(INLINE_SLOT_BUDGET, INLINE_MAX_FRAMES)
            }),
            seen: [false; MAX_INLINE_SLOTS],
        }
    }
}

impl InlinePool {
    pub fn begin_frame(&mut self) {
        self.seen = [false; MAX_INLINE_SLOTS];
    }

    fn active_decodes(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_decoding()).count()
    }

    pub fn request(&mut self, file: i32, path: &str, bounds: (u32, u32)) -> Option<(usize, usize)> {
        let slot_index = if let Some(index) = self
            .slots
            .iter()
            .position(|slot| slot.target.as_ref().is_some_and(|t| t.file == file))
        {
            index
        } else if let Some(index) = self.slots.iter().position(|slot| !slot.is_requested()) {
            index
        } else {
            let index = self.seen.iter().position(|seen| !*seen)?;
            self.slots[index].stop();
            index
        };
        self.seen[slot_index] = true;
        let allow_spawn = self.active_decodes() < MAX_INLINE_DECODES;
        self.slots[slot_index].request_with_spawn(file, path, bounds, allow_spawn);
        self.slots[slot_index]
            .frame_index()
            .map(|frame| (slot_index, frame))
    }

    pub fn image(&self, slot: usize) -> Option<DynamicImage> {
        self.slots.get(slot)?.image()
    }

    pub fn finish_frame(&mut self) -> Vec<usize> {
        let mut evicted = Vec::new();
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !self.seen[index] && slot.is_requested() {
                slot.stop();
                evicted.push(index);
            }
        }
        self.kick_pending();
        evicted
    }

    pub fn stop_all(&mut self) -> bool {
        let mut stopped = false;
        self.seen = [false; MAX_INLINE_SLOTS];
        for slot in &mut self.slots {
            if slot.is_requested() {
                stopped = true;
            }
            slot.stop();
        }
        stopped
    }

    fn kick_pending(&mut self) {
        let mut active = self.active_decodes();
        if active >= MAX_INLINE_DECODES {
            return;
        }
        for slot in &mut self.slots {
            if active >= MAX_INLINE_DECODES {
                break;
            }
            if slot.needs_start() {
                slot.start();
                if slot.is_decoding() {
                    active += 1;
                }
            }
        }
    }

    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for slot in &mut self.slots {
            changed |= slot.poll_with_spawn(now, false);
        }
        self.kick_pending();
        changed
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.slots.iter().filter_map(Player::deadline).min()
    }

    #[cfg(test)]
    pub fn active_count(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_requested()).count()
    }
}

pub fn has_ffmpeg() -> bool {
    FFMPEG.get_or_init(find_ffmpeg).is_some()
}

fn decode(target: &Target, cancel: &AtomicBool) -> Result<Frames, &'static str> {
    let mut file =
        BufReader::new(File::open(&target.path).map_err(|_| "无法读取动图 · o 用系统程序打开")?);
    let mut magic = [0; 6];
    file.read_exact(&mut magic)
        .map_err(|_| "无法读取动图 · o 用系统程序打开")?;
    if matches!(&magic, b"GIF87a" | b"GIF89a") {
        return decode_gif(target, cancel);
    }
    let program = FFMPEG.get_or_init(find_ffmpeg).as_deref();
    decode_video(program, target, cancel)
}

fn frame_limit(size: (u32, u32), budget: usize, max_frames: usize) -> usize {
    // Reserve one more raw frame for preparing the visible image.
    (budget / frame_bytes(size))
        .saturating_sub(1)
        .min(max_frames)
}

fn frame_bytes((w, h): (u32, u32)) -> usize {
    w as usize * h as usize * 4
}

fn decode_gif(target: &Target, cancel: &AtomicBool) -> Result<Frames, &'static str> {
    const ERROR: &str = "无法解码 GIF · o 用系统程序打开";
    let mut decoder = GifDecoder::new(BufReader::new(File::open(&target.path).map_err(|_| ERROR)?))
        .map_err(|_| ERROR)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 4_000_000 {
        return Err("GIF 像素超出预览限制 · o 用系统程序打开");
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(32 * 1024 * 1024);
    decoder.set_limits(limits).map_err(|_| ERROR)?;
    let size = crate::resample::fit(
        width,
        height,
        target.bounds.0.min(width),
        target.bounds.1.min(height),
    );
    let mut frames = Vec::new();
    let deadline = Instant::now() + DECODE_TIMEOUT;
    for frame in decoder
        .into_frames()
        .take(frame_limit(size, target.budget, target.max_frames))
    {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break;
        }
        let frame = frame.map_err(|_| ERROR)?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let duration = if numerator == 0 {
            100
        } else {
            u64::from(numerator / denominator).clamp(84, 10_000)
        };
        let source = frame.into_buffer();
        let rgba = if (width, height) == size {
            source
        } else {
            crate::resample::crop_resize(
                &DynamicImage::ImageRgba8(source),
                (0, 0, width, height),
                size.0,
                size.1,
            )
        };
        frames.push(Frame {
            rgba: rgba.into_raw().into_boxed_slice(),
            duration: Duration::from_millis(duration),
        });
    }
    if frames.is_empty() {
        Err(ERROR)
    } else {
        Ok(Frames { size, frames })
    }
}

static FFMPEG: OnceLock<Option<PathBuf>> = OnceLock::new();

fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

fn find_ffmpeg() -> Option<PathBuf> {
    let binary = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    if let Some(path) = std::env::var_os("PATH")
        && let Some(found) = std::env::split_paths(&path)
            .filter(|dir| dir.is_absolute())
            .map(|dir| dir.join(binary))
            .find(|candidate| is_executable_file(candidate))
    {
        return Some(found);
    }
    #[cfg(target_os = "macos")]
    {
        for fallback in ["/opt/homebrew/bin/ffmpeg", "/usr/local/bin/ffmpeg"] {
            let candidate = PathBuf::from(fallback);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn decode_video(
    program: Option<&Path>,
    target: &Target,
    cancel: &AtomicBool,
) -> Result<Frames, &'static str> {
    const ERROR: &str = "无法播放动图 · o 用系统程序打开";
    let program = program.ok_or("安装 ffmpeg 可播放动图 · o 用系统程序打开")?;
    let path = target.path.canonicalize().map_err(|_| ERROR)?;
    let size @ (width, height) = target.bounds;
    let limit = frame_limit(size, target.budget, target.max_frames);
    let filter = format!(
        "fps={FRAMERATE},scale={width}:{height}:force_original_aspect_ratio=decrease,format=rgba,pad={width}:{height}:(ow-iw)/2:(oh-ih)/2:color=black@0"
    );
    let mut child = Command::new(program)
        .args([
            "-nostdin",
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            "mov,matroska,webm",
            "-threads",
            "1",
            "-filter_threads",
            "1",
            "-i",
        ])
        .arg(path)
        .args([
            "-an",
            "-sn",
            "-dn",
            "-map",
            "0:v:0",
            "-vf",
            &filter,
            "-frames:v",
            &limit.to_string(),
            "-threads",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ERROR)?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let frame_size = frame_bytes(size);
    let reader = thread::spawn(move || {
        let mut frames = Vec::with_capacity(limit);
        for _ in 0..limit {
            let mut rgba = vec![0; frame_size].into_boxed_slice();
            if stdout.read_exact(&mut rgba).is_err() {
                break;
            }
            frames.push(Frame {
                rgba,
                duration: Duration::from_millis(1000 / FRAMERATE),
            });
        }
        frames
    });
    let deadline = Instant::now() + DECODE_TIMEOUT;
    let success = loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            break false;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => break false,
        }
    };
    if !success {
        let _ = child.kill();
        let _ = child.wait();
    }
    let frames = reader.join().map_err(|_| ERROR)?;
    if success && !frames.is_empty() {
        Ok(Frames { size, frames })
    } else {
        Err(ERROR)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{Delay, Frame as GifFrame, Rgba, codecs::gif::GifEncoder};

    pub(crate) fn gif_fixture() -> crate::clipboard::TemporaryImage {
        let (file, stream) = crate::clipboard::temporary_file("gif").unwrap();
        let mut encoder = GifEncoder::new(stream);
        for color in [Rgba([200, 10, 20, 255]), Rgba([30, 40, 200, 255])] {
            encoder
                .encode_frame(GifFrame::from_parts(
                    RgbaImage::from_pixel(24, 12, color),
                    0,
                    0,
                    Delay::from_numer_denom_ms(100, 1),
                ))
                .unwrap();
        }
        drop(encoder);
        file
    }

    #[test]
    fn gif_decodes_multiple_frames_and_frame_capacity_is_bounded() {
        let file = gif_fixture();
        let target = Target {
            file: 1,
            path: file.path.clone(),
            bounds: (12, 12),
            budget: FRAME_BUDGET,
            max_frames: MAX_FRAMES,
        };
        let frames = decode_gif(&target, &AtomicBool::new(false)).unwrap();
        assert_eq!(frames.size, (12, 6));
        assert_eq!(frames.frames.len(), 2);
        assert_ne!(frames.frames[0].rgba, frames.frames[1].rgba);
        for size in [(1, 1), (384, 288), (512, 512)] {
            assert!(frame_limit(size, FRAME_BUDGET, MAX_FRAMES) <= MAX_FRAMES);
            assert!(
                (frame_limit(size, FRAME_BUDGET, MAX_FRAMES) + 1) * frame_bytes(size)
                    <= FRAME_BUDGET
            );
        }
    }

    #[test]
    fn inline_pool_evicts_offscreen_animations_immediately() {
        let first = gif_fixture();
        let second = gif_fixture();
        let mut pool = InlinePool::default();
        pool.begin_frame();
        pool.request(10, first.path.to_str().unwrap(), (24, 12));
        pool.request(11, second.path.to_str().unwrap(), (24, 12));
        assert!(pool.finish_frame().is_empty());
        assert_eq!(pool.active_count(), 2);
        let timeout = Instant::now() + Duration::from_secs(3);
        while pool.deadline().is_none() {
            assert!(Instant::now() < timeout);
            pool.poll(Instant::now());
            thread::sleep(Duration::from_millis(5));
        }
        // Only file 10 remains visible in the next frame; file 11 must stop immediately.
        pool.begin_frame();
        let ready = pool.request(10, first.path.to_str().unwrap(), (24, 12));
        assert!(ready.is_some());
        let evicted = pool.finish_frame();
        assert_eq!(evicted.len(), 1);
        assert_eq!(pool.active_count(), 1);
        // No visible animations -> deadline clears to None.
        pool.begin_frame();
        assert_eq!(pool.finish_frame().len(), 1);
        assert_eq!(pool.active_count(), 0);
        assert!(pool.deadline().is_none());
    }

    #[test]
    fn missing_ffmpeg_provides_static_fallback_hint() {
        let target = Target {
            file: 1,
            path: "/video.mp4".into(),
            bounds: (120, 80),
            budget: FRAME_BUDGET,
            max_frames: MAX_FRAMES,
        };
        let error = decode_video(None, &target, &AtomicBool::new(false))
            .err()
            .unwrap();
        assert!(error.contains("安装 ffmpeg") && error.contains("用系统程序打开"));
    }
}
