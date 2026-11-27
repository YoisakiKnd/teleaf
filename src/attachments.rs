//! Local attachment browsing and path parsing; never executes pasted shell text.
use crate::{actions, text};
use crossterm::event::KeyCode;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, BufReader, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

pub const MAX_FILES: usize = 10;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Browser,
    Path,
    Caption,
}
#[derive(Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub directory: bool,
    pub bytes: Option<u64>,
}
pub struct Picker {
    pub directory: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub offset: usize,
    pub visible: usize,
    pub selected: Vec<PathBuf>,
    pub clipboard_images: Vec<Arc<crate::clipboard::TemporaryImage>>,
    pub quick_confirm: bool,
    pub preparing: Option<Preparation>,
    pub photo_summary: Option<(PathBuf, String)>,
    pub queue_offset: usize,
    pub queue_visible: usize,
    pub consume_path_draft: bool,
    pub photos: bool,
    pub path: String,
    pub path_cursor: usize,
    pub caption: String,
    pub caption_cursor: usize,
    pub focus: Focus,
    pub error: Option<String>,
    pub reply_to: Option<i64>,
}
pub struct Preparation {
    pub result: Receiver<Result<Vec<PreparedPhoto>, String>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Preparation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Picker {
    pub fn new(photos: bool, reply_to: Option<i64>) -> Self {
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut picker = Self {
            directory: directory.clone(),
            entries: vec![],
            cursor: 0,
            offset: 0,
            visible: 1,
            selected: vec![],
            clipboard_images: vec![],
            quick_confirm: false,
            preparing: None,
            photo_summary: None,
            queue_offset: 0,
            queue_visible: 1,
            consume_path_draft: false,
            photos,
            path: String::new(),
            path_cursor: usize::MAX,
            caption: String::new(),
            caption_cursor: usize::MAX,
            focus: Focus::Browser,
            error: None,
            reply_to,
        };
        picker.navigate(directory);
        picker
    }
    pub fn navigate(&mut self, path: PathBuf) {
        if self.preparing.is_some() {
            return;
        }
        self.quick_confirm = false;
        let result = fs::read_dir(&path).map(|entries| {
            let mut list: Vec<_> = entries
                .filter_map(Result::ok)
                .take(2000)
                .filter_map(|entry| {
                    let kind = entry.file_type().ok()?;
                    let directory = kind.is_dir() || (kind.is_symlink() && entry.path().is_dir());
                    (directory || kind.is_file() || kind.is_symlink()).then(|| Entry {
                        path: entry.path(),
                        directory,
                        bytes: (!directory)
                            .then(|| entry.metadata().ok().map(|m| m.len()))
                            .flatten(),
                    })
                })
                .collect();
            list.sort_by(|a, b| {
                b.directory
                    .cmp(&a.directory)
                    .then_with(|| a.path.file_name().cmp(&b.path.file_name()))
            });
            if let Some(parent) = path.parent() {
                list.insert(
                    0,
                    Entry {
                        path: parent.to_path_buf(),
                        directory: true,
                        bytes: None,
                    },
                );
            }
            list
        });
        match result {
            Ok(entries) => {
                self.directory = path;
                self.entries = entries;
                self.cursor = 0;
                self.offset = 0;
                self.error = None;
            }
            Err(error) => self.error = Some(format!("无法打开目录：{error}")),
        }
    }
    pub fn activate(&mut self, index: usize) {
        if self.preparing.is_some() {
            return;
        }
        self.quick_confirm = false;
        if let Some(entry) = self.entries.get(index).cloned() {
            self.cursor = index;
            self.focus = Focus::Browser;
            if entry.directory {
                self.navigate(entry.path);
            } else if let Some(index) = self.selected.iter().position(|p| *p == entry.path) {
                self.remove(index);
            } else if let Err(error) = self.add(vec![entry.path]) {
                self.error = Some(error);
            }
        }
    }
    pub fn add(&mut self, paths: Vec<PathBuf>) -> Result<(), String> {
        let mut selected = self.selected.clone();
        for path in paths {
            let path = fs::canonicalize(path).map_err(|e| format!("无法读取文件：{e}"))?;
            if !path.is_file() {
                return Err("请选择文件，目录不能直接发送".into());
            }
            if path.to_str().is_none() {
                return Err("文件名不是有效的 UTF-8，无法交给 TDLib".into());
            }
            fs::File::open(&path).map_err(|e| format!("文件无法读取：{e}"))?;
            if !selected.contains(&path) {
                selected.push(path);
            }
        }
        if selected.len() > MAX_FILES {
            return Err(format!("一次最多选择 {MAX_FILES} 个文件"));
        }
        self.selected = selected;
        self.error = None;
        Ok(())
    }
    pub fn remove(&mut self, index: usize) {
        if self.preparing.is_some() {
            return;
        }
        self.quick_confirm = false;
        if index < self.selected.len() {
            let removed = self.selected.remove(index);
            self.clipboard_images
                .retain(|image| fs::canonicalize(&image.path).is_ok_and(|path| path != removed));
        }
    }
    pub fn paste_paths(&mut self, value: &str) {
        match parse_paths(value, &self.directory) {
            Ok(paths) => {
                if paths.len() == 1 && paths[0].is_dir() {
                    self.navigate(paths[0].clone());
                } else if let Err(error) = self.add(paths) {
                    self.error = Some(error);
                }
            }
            Err(error) => {
                self.path = value.trim().to_owned();
                self.path_cursor = usize::MAX;
                self.focus = Focus::Path;
                self.error = Some(error);
            }
        }
    }
    pub fn set_photos(&mut self, photos: bool) {
        if self.preparing.is_none() {
            self.photos = photos;
            self.error = None;
        }
    }

    pub fn key(&mut self, key: KeyCode) {
        if self.preparing.is_some() {
            return;
        }
        if key != KeyCode::F(5) {
            self.quick_confirm = false;
        }
        match key {
            KeyCode::F(5) => {
                self.set_photos(!self.photos);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match (self.focus, key) {
                    (Focus::Browser, KeyCode::BackTab) | (Focus::Path, KeyCode::Tab) => {
                        Focus::Caption
                    }
                    (Focus::Caption, KeyCode::Tab) | (Focus::Path, KeyCode::BackTab) => {
                        Focus::Browser
                    }
                    _ => Focus::Path,
                }
            }
            _ if self.focus == Focus::Path => {
                if key == KeyCode::Enter {
                    let path = self.path.clone();
                    self.paste_paths(&path);
                    if self.error.is_none() {
                        self.path.clear();
                        self.focus = Focus::Browser;
                    } else {
                        self.focus = Focus::Path;
                    }
                } else {
                    text::edit(&mut self.path, &mut self.path_cursor, key);
                }
            }
            _ if self.focus == Focus::Caption => {
                if self.caption.len() < 4096 || !matches!(key, KeyCode::Char(_)) {
                    text::edit(&mut self.caption, &mut self.caption_cursor, key);
                }
            }
            KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Down => {
                self.cursor = (self.cursor + 1).min(self.entries.len().saturating_sub(1))
            }
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(self.visible),
            KeyCode::PageDown => {
                self.cursor = (self.cursor + self.visible).min(self.entries.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Char(' ') => self.activate(self.cursor),
            KeyCode::Delete => {
                self.remove(self.selected.len().saturating_sub(1));
            }
            KeyCode::Char(c) => {
                self.focus = Focus::Path;
                text::insert(&mut self.path, &mut self.path_cursor, &c.to_string(), 65536);
            }
            KeyCode::Backspace => {
                if let Some(parent) = self.directory.parent() {
                    self.navigate(parent.to_path_buf());
                }
            }
            _ => {}
        }
        if self.cursor < self.offset {
            self.offset = self.cursor;
        }
        if self.cursor >= self.offset + self.visible {
            self.offset = self.cursor + 1 - self.visible;
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.selected.is_empty() {
            return Err("先选择文件，再点击发送或按 F8".into());
        }
        if self.caption.chars().count() > 1024 {
            return Err("说明文字最多 1024 个字符".into());
        }
        Ok(())
    }

    #[cfg(test)]
    fn request(&self, chat_id: i64) -> Result<Value, String> {
        let photos = if self.photos {
            prepare_photos(
                &self.selected,
                (256 * 1024 * 1024, MAX_FILES),
                &AtomicBool::default(),
            )?
        } else {
            Vec::new()
        };
        self.send_request(chat_id, &photos)
    }

    pub fn start_preparation(
        &mut self,
        busy: &Arc<AtomicBool>,
        budget: (u64, usize),
    ) -> Result<std::thread::JoinHandle<()>, String> {
        self.validate()?;
        if busy.swap(true, Ordering::AcqRel) {
            return Err("上一批图片仍在处理，请稍后重试".into());
        }
        let paths = self.selected.clone();
        let sources = self.clipboard_images.clone();
        let busy = Arc::clone(busy);
        let (sender, result) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        self.error = None;
        self.preparing = Some(Preparation {
            result,
            cancelled: Arc::clone(&cancelled),
        });
        Ok(std::thread::spawn(move || {
            let result = prepare_photos(&paths, budget, &cancelled);
            drop(sources);
            let _ = sender.send(result);
            busy.store(false, Ordering::Release);
        }))
    }

    pub fn send_request(&self, chat_id: i64, photos: &[PreparedPhoto]) -> Result<Value, String> {
        self.validate()?;
        let mut contents = vec![];
        for (index, path) in self.selected.iter().enumerate() {
            let request = if self.photos {
                let image = photos.get(index).ok_or("图片尚未准备好")?;
                let path = image
                    .staged
                    .as_ref()
                    .map_or(path.as_path(), |file| &file.path);
                actions::send_photo(
                    chat_id,
                    path.to_str().ok_or("文件名不是有效的 UTF-8")?,
                    image.width,
                    image.height,
                )
            } else {
                if !path.is_file() || fs::File::open(path).is_err() {
                    return Err(format!("文件已移动或无法读取：{}", path.display()));
                }
                actions::send_file(chat_id, path.to_str().ok_or("文件名不是有效的 UTF-8")?)
            };
            let mut content = request["input_message_content"].clone();
            if index == 0 && !self.caption.is_empty() {
                content["caption"] = crate::markdown::formatted_text(&self.caption);
            }
            contents.push(content);
        }
        let reply = self.reply_to.map(|id| json!({"@type":"inputMessageReplyToMessage","message_id":id,"quote":null,"checklist_task_id":0}));
        Ok(if contents.len() == 1 {
            json!({"@type":"sendMessage","chat_id":chat_id,"topic_id":null,"reply_to":reply,"options":null,"reply_markup":null,"input_message_content":contents.remove(0)})
        } else {
            json!({"@type":"sendMessageAlbum","chat_id":chat_id,"topic_id":null,"reply_to":reply,"options":null,"input_message_contents":contents})
        })
    }
}

const PHOTO_EDGE: u32 = 2560;
const DIRECT_JPEG_BYTES: u64 = 1536 * 1024;
const PHOTO_PIXELS: u64 = 24_000_000;
const PHOTO_SOURCE_BYTES: u64 = 32 * 1024 * 1024;
const PHOTO_HEADER_BYTES: u64 = 512 * 1024;

pub struct PreparedPhoto {
    pub staged: Option<Arc<crate::clipboard::TemporaryImage>>,
    width: u32,
    height: u32,
}

fn photo_reader<R: BufRead + Seek>(source: R) -> Result<image::ImageReader<R>, String> {
    let mut reader = image::ImageReader::new(source)
        .with_guessed_format()
        .map_err(|e| format!("无法读取图片：{e}"))?;
    if !matches!(
        reader.format(),
        Some(image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP)
    ) {
        return Err("图片模式支持 JPEG / PNG / WebP，请将其他格式作为原文件发送".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32768);
    limits.max_image_height = Some(32768);
    limits.max_alloc = Some(96 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

fn direct_jpeg(format: Option<image::ImageFormat>, size: (u32, u32), bytes: u64) -> bool {
    format == Some(image::ImageFormat::Jpeg)
        && size.0.max(size.1) <= PHOTO_EDGE
        && bytes <= DIRECT_JPEG_BYTES
}

pub fn photo_description(path: &Path) -> Option<String> {
    let file = fs::File::open(path).ok()?;
    let bytes = file.metadata().ok()?.len();
    let mut header = Vec::new();
    file.take(PHOTO_HEADER_BYTES)
        .read_to_end(&mut header)
        .ok()?;
    let reader = photo_reader(Cursor::new(header)).ok()?;
    let format = reader.format();
    let (w, h) = reader.into_dimensions().ok()?;
    let mode = if direct_jpeg(format, (w, h), bytes) {
        "直接发送"
    } else {
        "压缩 JPEG"
    };
    Some(format!("{w}×{h} · {} → {mode}", display_size(bytes)))
}

pub fn display_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

struct StagingWriter {
    file: fs::File,
    remaining: u64,
}
impl Write for StagingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() as u64 > self.remaining {
            return Err(io::Error::other("图片暂存已达 256 MiB"));
        }
        let written = self.file.write(bytes)?;
        self.remaining -= written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn prepare_photos(
    paths: &[PathBuf],
    (mut bytes, mut slots): (u64, usize),
    cancelled: &AtomicBool,
) -> Result<Vec<PreparedPhoto>, String> {
    use image::{ColorType, ImageDecoder};

    let mut photos = Vec::with_capacity(paths.len());
    for path in paths {
        if cancelled.load(Ordering::Relaxed) {
            return Err("图片压缩已取消".into());
        }
        let file = fs::File::open(path).map_err(|e| format!("无法读取图片：{e}"))?;
        let metadata = file.metadata().map_err(|e| format!("无法读取图片：{e}"))?;
        if !metadata.is_file() || metadata.len() > PHOTO_SOURCE_BYTES {
            return Err("图片超过 32 MiB，或已无法读取，请作为原文件发送".into());
        }
        let reader = photo_reader(BufReader::new(file))?;
        let format = reader.format();
        let mut decoder = reader
            .into_decoder()
            .map_err(|e| format!("无法读取图片：{e}"))?;
        let (mut width, mut height) = decoder.dimensions();
        if direct_jpeg(format, (width, height), metadata.len()) {
            photos.push(PreparedPhoto {
                staged: None,
                width,
                height,
            });
            continue;
        }
        if u64::from(width) * u64::from(height) > PHOTO_PIXELS
            || decoder.total_bytes() > 96 * 1024 * 1024
        {
            return Err("图片像素超出压缩限制，请作为原文件发送".into());
        }
        if slots == 0 || bytes == 0 {
            return Err("图片暂存已满，请等待上传完成".into());
        }
        let orientation = decoder
            .orientation()
            .unwrap_or(image::metadata::Orientation::NoTransforms);
        let source =
            image::DynamicImage::from_decoder(decoder).map_err(|e| format!("无法解码图片：{e}"))?;
        let (target_w, target_h) = if width.max(height) > PHOTO_EDGE {
            crate::resample::fit(width, height, PHOTO_EDGE, PHOTO_EDGE)
        } else {
            (width, height)
        };
        let rgb = crate::resample::jpeg_pixels(&source, target_w, target_h);
        drop(source);
        let mut output = image::DynamicImage::ImageRgb8(rgb);
        output.apply_orientation(orientation);
        let rgb = output.into_rgb8();
        (width, height) = rgb.dimensions();
        if cancelled.load(Ordering::Relaxed) {
            return Err("图片压缩已取消".into());
        }
        let (mut staged, file) = crate::clipboard::temporary_file("jpg")?;
        let writer = StagingWriter {
            file,
            remaining: bytes,
        };
        image::codecs::jpeg::JpegEncoder::new_with_quality(writer, 90)
            .encode(&rgb, width, height, ColorType::Rgb8.into())
            .map_err(|e| format!("无法编码 JPEG：{e}"))?;
        staged.bytes = fs::metadata(&staged.path).map_err(|e| e.to_string())?.len();
        bytes -= staged.bytes;
        slots -= 1;
        photos.push(PreparedPhoto {
            staged: Some(Arc::new(staged)),
            width,
            height,
        });
    }
    Ok(photos)
}

fn decode_uri(value: &str) -> Result<String, String> {
    let Some(value) = value.strip_prefix("file://") else {
        return Ok(value.into());
    };
    let value = if let Some(value) = value.strip_prefix("localhost/") {
        format!("/{value}")
    } else if value.starts_with('/') {
        value.to_owned()
    } else {
        return Err("只支持本地 file:// 路径".into());
    };
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = value.get(i + 1..i + 3).ok_or("file:// 路径编码不完整")?;
            output.push(u8::from_str_radix(hex, 16).map_err(|_| "file:// 路径编码不正确")?);
            i += 3;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    let mut result = String::from_utf8(output).map_err(|_| "file:// 路径不是 UTF-8")?;
    if (cfg!(windows) || std::env::var_os("WSL_DISTRO_NAME").is_some())
        && result.as_bytes().get(2) == Some(&b':')
    {
        result.remove(0);
    }
    Ok(result)
}
fn local_path(value: &str, base: &Path) -> Result<PathBuf, String> {
    let value = decode_uri(value)?;
    let path = if value == "~" || value.starts_with("~/") || value.starts_with("~\\") {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .ok_or("无法获取主目录")?;
        PathBuf::from(home).join(value.get(2..).unwrap_or(""))
    } else if cfg!(unix)
        && std::env::var_os("WSL_DISTRO_NAME").is_some()
        && value.as_bytes().get(1) == Some(&b':')
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
    {
        PathBuf::from(format!(
            "/mnt/{}/{}",
            value[..1].to_ascii_lowercase(),
            value[2..]
                .trim_start_matches(['/', '\\'])
                .replace('\\', "/")
        ))
    } else {
        PathBuf::from(value)
    };
    Ok(if path.is_absolute() {
        path
    } else {
        base.join(path)
    })
}

pub fn parse_paths(value: &str, base: &Path) -> Result<Vec<PathBuf>, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("粘贴或输入一个本地文件路径".into());
    }
    if value.len() > 65536 {
        return Err("路径文本过长".into());
    }
    // A raw path containing spaces is common in Windows / file managers.
    if let Ok(path) = local_path(value, base)
        && path.exists()
    {
        return Ok(vec![path]);
    }
    // URI lists and file managers often use one raw path per line.
    let lines: Vec<_> = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if lines.len() > 1 {
        if lines.len() > MAX_FILES {
            return Err(format!("一次最多选择 {MAX_FILES} 个文件"));
        }
        if let Ok(paths) = lines
            .iter()
            .map(|line| local_path(line, base))
            .collect::<Result<Vec<_>, _>>()
            && paths.iter().all(|path| path.exists())
        {
            return Ok(paths);
        }
    }
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if quote == Some(c) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(c),
            '\\' if quote != Some('\'')
                && chars
                    .peek()
                    .is_some_and(|c| c.is_whitespace() || *c == '\'' || *c == '"') =>
            {
                token.push(chars.next().unwrap())
            }
            c if c.is_whitespace() && quote.is_none() => {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            }
            c => token.push(c),
        }
    }
    if quote.is_some() {
        return Err("文件路径的引号没有闭合".into());
    }
    if !token.is_empty() {
        tokens.push(token);
    }
    if tokens.is_empty() || tokens.len() > MAX_FILES {
        return Err(format!("一次支持 1–{MAX_FILES} 个本地路径"));
    }
    let paths: Vec<_> = tokens
        .iter()
        .map(|v| local_path(v, base))
        .collect::<Result<_, _>>()?;
    if paths.iter().any(|p| !p.exists()) {
        return Err("路径不存在；可粘贴带引号、转义空格或 file:// 的路径".into());
    }
    Ok(paths)
}
pub fn pasted_files(value: &str) -> Option<Vec<PathBuf>> {
    let base = std::env::current_dir().ok()?;
    let paths = parse_paths(value, &base).ok()?;
    paths.iter().all(|p| p.is_file()).then_some(paths)
}
pub fn photo_paths(paths: &[PathBuf]) -> bool {
    paths.iter().all(|p| {
        p.extension().and_then(|s| s.to_str()).is_some_and(|s| {
            matches!(
                s.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        })
    })
}

pub fn preview_id(path: &Path) -> i32 {
    // Local previews use negative IDs, separate from TDLib's positive file IDs.
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    -((hash.finish() & 0x3fff_ffff) as i32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keyboard_mode_toggle_works_in_every_attachment_field() {
        let mut picker = Picker::new(true, Some(42));
        picker.caption = "说明".into();
        picker.path = "路径".into();
        for focus in [Focus::Browser, Focus::Path, Focus::Caption] {
            picker.focus = focus;
            picker.error = Some("validation error".into());
            picker.key(KeyCode::F(5));
            assert!(!picker.photos);
            assert!(picker.error.is_none());
            picker.key(KeyCode::F(5));
            assert!(picker.photos);
            assert!(picker.focus == focus);
        }
        assert_eq!(picker.caption, "说明");
        assert_eq!(picker.path, "路径");
        assert_eq!(picker.reply_to, Some(42));
    }
    #[test]
    fn removing_clipboard_attachment_deletes_its_temporary_source() {
        let image = crate::clipboard::test_image();
        let path = image.path.clone();
        let mut picker = Picker::new(true, None);
        picker.add(vec![path.clone()]).unwrap();
        picker.clipboard_images.push(Arc::new(image));
        picker.remove(0);
        assert!(picker.selected.is_empty());
        assert!(picker.clipboard_images.is_empty());
        assert!(!path.exists());
    }
    fn fixture(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("tg-attach-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        folder
    }
    #[test]
    fn parses_dragged_quoted_escaped_unicode_and_uri_paths_without_shell_execution() {
        let folder = fixture("paths");
        let a = folder.join("中文 photo.png");
        let b = folder.join("a'b.txt");
        fs::write(&a, b"a").unwrap();
        fs::write(&b, b"b").unwrap();
        assert_eq!(
            parse_paths(&a.to_string_lossy(), &folder).unwrap(),
            vec![a.clone()]
        );
        assert_eq!(
            parse_paths("'中文 photo.png' \"a'b.txt\"", &folder).unwrap(),
            vec![a.clone(), b.clone()]
        );
        assert_eq!(
            parse_paths("中文\\ photo.png", &folder).unwrap(),
            vec![a.clone()]
        );
        assert_eq!(
            parse_paths(&format!("{}\n{}", a.display(), b.display()), &folder).unwrap(),
            vec![a.clone(), b.clone()]
        );
        let uri_path = a.to_string_lossy().replace(' ', "%20").replace('\\', "/");
        let uri = if cfg!(windows) {
            format!("file:///{uri_path}")
        } else {
            format!("file://{uri_path}")
        };
        assert_eq!(parse_paths(&uri, &folder).unwrap(), vec![a]);
        assert!(parse_paths("file://remote-host/nope", &folder).is_err());
        assert!(parse_paths("'unclosed", &folder).is_err());
        assert!(parse_paths("$(touch SHOULD_NOT_EXIST)", &folder).is_err());
        assert!(!folder.join("SHOULD_NOT_EXIST").exists());
        assert_eq!(
            local_path(r"C:\Users\name\file.txt", Path::new("/"))
                .unwrap()
                .to_string_lossy(),
            if cfg!(windows) {
                r"C:\Users\name\file.txt"
            } else if std::env::var_os("WSL_DISTRO_NAME").is_some() {
                "/mnt/c/Users/name/file.txt"
            } else {
                r"/C:\Users\name\file.txt"
            }
        );
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn grouped_photos_keep_order_caption_reply_and_validate_before_upload() {
        let folder = fixture("album");
        let a = folder.join("a.png");
        let b = folder.join("b.png");
        image::RgbImage::from_pixel(2, 2, image::Rgb([255, 0, 0]))
            .save(&a)
            .unwrap();
        fs::copy(&a, &b).unwrap();
        let mut picker = Picker::new(true, Some(91));
        picker.add(vec![a.clone(), b.clone(), a.clone()]).unwrap();
        assert_eq!(picker.selected.len(), 2);
        picker.caption = "中😀 **说明**".into();
        let request = picker.request(17).unwrap();
        assert_eq!(request["@type"], "sendMessageAlbum");
        assert_eq!(request["reply_to"]["message_id"], 91);
        let caption = &request["input_message_contents"][0]["caption"];
        assert_eq!(caption["text"], "中😀 说明");
        assert_eq!(caption["entities"][0]["offset"], 4);
        assert_eq!(caption["entities"][0]["length"], 2);
        assert!(request["input_message_contents"][1]["caption"].is_null());
        picker.photos = false;
        assert_eq!(
            picker.request(17).unwrap()["input_message_contents"][1]["@type"],
            "inputMessageDocument"
        );
        fs::remove_file(&b).unwrap();
        assert!(picker.request(17).is_err());
        assert_eq!(
            picker.selected.len(),
            2,
            "validation failure must keep the selection"
        );
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn png_scales_to_rgb_jpeg_and_small_jpeg_and_documents_keep_their_bytes() {
        let folder = fixture("compression");
        let png = folder.join("transparent.png");
        let jpeg = folder.join("small.jpg");
        image::RgbaImage::from_pixel(3000, 1500, image::Rgba([0, 40, 80, 0]))
            .save(&png)
            .unwrap();
        image::RgbImage::from_pixel(40, 20, image::Rgb([0, 40, 80]))
            .save(&jpeg)
            .unwrap();
        let jpeg_bytes = fs::read(&jpeg).unwrap();
        let png_bytes = fs::read(&png).unwrap();
        let mut picker = Picker::new(true, None);
        picker.add(vec![png.clone(), jpeg.clone()]).unwrap();
        let prepared = prepare_photos(
            &picker.selected,
            (256 * 1024 * 1024, 10),
            &AtomicBool::default(),
        )
        .unwrap();
        let staged = &prepared[0].staged.as_ref().unwrap().path;
        assert!(prepared[1].staged.is_none());
        let image = image::open(staged).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (2560, 1280));
        assert!(
            image
                .get_pixel(1280, 640)
                .0
                .iter()
                .all(|value| *value >= 250)
        );
        let request = picker.send_request(7, &prepared).unwrap();
        let contents = request["input_message_contents"].as_array().unwrap();
        assert_eq!(
            (
                contents[0]["width"].as_u64(),
                contents[0]["height"].as_u64()
            ),
            (Some(2560), Some(1280))
        );
        assert_eq!(
            contents[1]["photo"]["path"],
            picker.selected[1].to_str().unwrap()
        );
        assert_eq!(
            (
                contents[1]["width"].as_u64(),
                contents[1]["height"].as_u64()
            ),
            (Some(40), Some(20))
        );
        assert!(prepare_photos(&picker.selected[..1], (100, 1), &AtomicBool::default()).is_err());
        picker.set_photos(false);
        let documents = picker.send_request(7, &[]).unwrap();
        let contents = documents["input_message_contents"].as_array().unwrap();
        assert_eq!(
            contents[0]["document"]["path"],
            picker.selected[0].to_str().unwrap()
        );
        assert_eq!(fs::read(&png).unwrap(), png_bytes);
        assert_eq!(fs::read(&jpeg).unwrap(), jpeg_bytes);
        let staged = staged.clone();
        drop(prepared);
        assert!(!staged.exists());
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn selection_is_atomic_and_unsupported_photo_can_be_sent_as_file() {
        let folder = fixture("bounds");
        let paths: Vec<_> = (0..11)
            .map(|i| {
                let p = folder.join(format!("{i}.txt"));
                fs::write(&p, b"text").unwrap();
                p
            })
            .collect();
        let mut picker = Picker::new(true, None);
        picker.add(vec![paths[0].clone()]).unwrap();
        assert!(picker.add(paths[1..].to_vec()).is_err());
        assert_eq!(picker.selected.len(), 1);
        assert!(picker.request(1).is_err());
        picker.photos = false;
        assert_eq!(
            picker.request(1).unwrap()["input_message_content"]["@type"],
            "inputMessageDocument"
        );
        picker.caption = "中".repeat(1025);
        assert!(picker.request(1).is_err());
        fs::remove_dir_all(folder).unwrap();
    }
}
