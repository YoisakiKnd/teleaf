//! Read the local clipboard only on request; release decoded pixels after staging.
use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::InputMode;

#[cfg(any(windows, test))]
mod dib;
#[cfg(windows)]
mod windows;

pub enum Content {
    Files(Vec<PathBuf>),
    Image(TemporaryImage),
    Text(String),
}

pub struct TemporaryImage {
    pub path: PathBuf,
    bytes: u64,
}
impl Drop for TemporaryImage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

type ResultMessage = (i64, InputMode, Result<Content, String>);
#[derive(Default)]
pub struct State {
    pending: Option<Receiver<ResultMessage>>,
    busy: Arc<AtomicBool>,
    // TDLib reads inputFileLocal asynchronously. Keep submitted files until its
    // worker has shut down, rather than deleting them when the picker closes.
    submitted: Vec<TemporaryImage>,
}
impl State {
    #[cfg(test)]
    pub fn queued(chat: i64, mode: InputMode, content: Content) -> Self {
        let (sender, receiver) = mpsc::sync_channel(1);
        sender
            .send((chat, mode, Ok(content)))
            .unwrap_or_else(|_| panic!("test channel"));
        Self {
            pending: Some(receiver),
            ..Self::default()
        }
    }

    pub fn start(&mut self, chat: i64, mode: InputMode) -> bool {
        if self.pending.is_some() || self.busy.swap(true, Ordering::Relaxed) {
            return false;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending = Some(receiver);
        let busy = Arc::clone(&self.busy);
        std::thread::spawn(move || {
            let _ = sender.send((chat, mode, read()));
            busy.store(false, Ordering::Relaxed);
        });
        true
    }
    pub fn cancel(&mut self) {
        self.pending = None;
    }
    pub fn poll(&mut self) -> Option<ResultMessage> {
        match self.pending.as_ref()?.try_recv() {
            Ok(result) => {
                self.pending = None;
                Some(result)
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
    }
    pub fn can_stage(&self, queue: &[TemporaryImage], image: &TemporaryImage) -> bool {
        // Disk budget only: no pixels or encoded PNGs remain in App memory.
        self.submitted
            .iter()
            .chain(queue)
            .map(|i| i.bytes)
            .sum::<u64>()
            + image.bytes
            <= 256 * 1024 * 1024
            && self.submitted.len() + queue.len() < 1024
    }
    pub fn submitted(&mut self, images: Vec<TemporaryImage>) {
        self.submitted.extend(images);
    }
}

fn read() -> Result<Content, String> {
    if ["SSH_CONNECTION", "SSH_TTY"]
        .iter()
        .any(|key| std::env::var_os(key).is_some())
    {
        return Err("SSH 无法读取本机图片剪贴板；请先传文件，再拖入/粘贴远端路径".into());
    }
    read_local()
}

#[cfg(not(target_os = "macos"))]
fn read_local() -> Result<Content, String> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| format!("无法读取系统剪贴板：{e}；可拖入/粘贴文件路径"))?;
    if let Ok(paths) = clipboard.get().file_list()
        && !paths.is_empty()
    {
        return Ok(Content::Files(paths));
    }
    if let Ok(image) = clipboard.get_image() {
        return stage_image(image.width, image.height, &image.bytes).map(Content::Image);
    }
    // arboard reads PNG / CF_DIBV5 on Windows. Some screenshot tools only
    // provide CF_DIB, or an invalid PNG alongside a valid bitmap.
    #[cfg(windows)]
    if let Some(image) = windows::read_dib()? {
        return stage_pixels(
            image.width() as usize,
            image.height() as usize,
            image.as_bytes(),
            image.color(),
        )
        .map(Content::Image);
    }
    let text = clipboard
        .get_text()
        .map_err(|_| "剪贴板没有可用图片、文件或文字；可拖入文件路径".to_owned())?;
    if text.len() > 262144 {
        return Err("剪贴板文字过长，请减少到 256 KiB 以内".into());
    }
    Ok(Content::Text(text))
}

// AppKit is loaded by a short-lived system helper, keeping it out of Teleaf's
// startup and idle RSS. Clipboard contents are data, never executable source.
#[cfg(target_os = "macos")]
const MAC_SCRIPT: &str = r#"
ObjC.import('AppKit');
function present(value) { return value && ObjC.unwrap(value) != null; }
function readClipboard(pb, destination) {
    var items = pb.pasteboardItems, paths = [];
    if (present(items)) {
        for (var i = 0; i < items.count; ++i) {
            var value = items.objectAtIndex(i).stringForType('public.file-url');
            if (present(value)) {
                var url = $.NSURL.URLWithString(value);
                if (present(url) && url.isFileURL) paths.push(ObjC.unwrap(url.path));
                if (paths.length > 10) return {error: '一次最多选择 10 个文件'};
            }
        }
    }
    if (paths.length) return {kind: 'files', paths: paths};
    var data = pb.dataForType('public.png');
    if (!present(data)) {
        var tiff = pb.dataForType('public.tiff');
        if (present(tiff)) {
            var bitmap = $.NSBitmapImageRep.imageRepWithData(tiff);
            if (!present(bitmap)) return {error: '剪贴板图片无法读取'};
            if (bitmap.pixelsWide * bitmap.pixelsHigh > 16000000)
                return {error: '剪贴板图片超过 1600 万像素；请以原文件发送'};
            data = bitmap.representationUsingTypeProperties(4, $.NSDictionary.dictionary);
        }
    }
    if (present(data)) {
        if (data.length > 67108864) return {error: '剪贴板图片过大；请以原文件发送'};
        if (!data.writeToFileAtomically(destination, false)) return {error: '无法暂存剪贴板图片'};
        return {kind: 'image'};
    }
    var text = pb.stringForType('public.utf8-plain-text');
    if (present(text)) {
        if (text.lengthOfBytesUsingEncoding($.NSUTF8StringEncoding) > 262144)
            return {error: '剪贴板文字过长，请减少到 256 KiB 以内'};
        return {kind: 'text', text: ObjC.unwrap(text)};
    }
    return {error: '剪贴板没有可用图片、文件或文字；可拖入文件路径'};
}
function run(argv) { return JSON.stringify(readClipboard($.NSPasteboard.generalPasteboard, argv[0])); }
"#;

#[cfg(target_os = "macos")]
fn read_local() -> Result<Content, String> {
    read_macos(MAC_SCRIPT, &[])
}

#[cfg(target_os = "macos")]
fn read_macos(script: &str, arguments: &[&std::path::Path]) -> Result<Content, String> {
    let (mut staged, file) = temporary_file()?;
    drop(file);
    let output = std::process::Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", script, "--"])
        .arg(&staged.path)
        .args(arguments)
        .output()
        .map_err(|_| "无法启动系统剪贴板读取；可拖入/粘贴文件路径")?;
    if !output.status.success() {
        return Err("系统剪贴板读取失败；可拖入/粘贴文件路径".into());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "系统剪贴板返回了无效数据")?;
    if let Some(error) = value["error"].as_str() {
        return Err(error.into());
    }
    match value["kind"].as_str() {
        Some("files") => Ok(Content::Files(
            value["paths"]
                .as_array()
                .ok_or("无效的剪贴板文件列表")?
                .iter()
                .filter_map(|v| v.as_str().map(PathBuf::from))
                .collect(),
        )),
        Some("text") => {
            let text = value["text"].as_str().ok_or("无效的剪贴板文字")?;
            if text.len() > 262144 {
                return Err("剪贴板文字过长，请减少到 256 KiB 以内".into());
            }
            Ok(Content::Text(text.into()))
        }
        Some("image") => {
            let (width, height) = image::ImageReader::open(&staged.path)
                .map_err(|e| e.to_string())?
                .into_dimensions()
                .map_err(|_| "剪贴板图片无法读取")?;
            if width == 0
                || height == 0
                || u64::from(width) * u64::from(height) > 16_000_000
                || width > 32768
                || height > 32768
            {
                return Err("剪贴板图片超过 1600 万像素；请以原文件发送".into());
            }
            staged.bytes = fs::metadata(&staged.path).map_err(|e| e.to_string())?.len();
            Ok(Content::Image(staged))
        }
        _ => Err("系统剪贴板返回了未知格式".into()),
    }
}

#[cfg(any(test, not(target_os = "macos")))]
fn stage_image(width: usize, height: usize, pixels_rgba: &[u8]) -> Result<TemporaryImage, String> {
    stage_pixels(width, height, pixels_rgba, image::ColorType::Rgba8)
}

#[cfg(any(test, not(target_os = "macos")))]
fn stage_pixels(
    width: usize,
    height: usize,
    pixels: &[u8],
    color: image::ColorType,
) -> Result<TemporaryImage, String> {
    if !matches!(color, image::ColorType::Rgb8 | image::ColorType::Rgba8) {
        return Err("不支持的剪贴板像素格式".into());
    }
    let count = width.checked_mul(height).ok_or("剪贴板图片尺寸过大")?;
    if count == 0 || count > 16_000_000 || width > 32768 || height > 32768 {
        return Err("剪贴板图片超过 1600 万像素；请保存为文件后以原文件发送".into());
    }
    if pixels.len() != count * usize::from(color.channel_count()) {
        return Err("剪贴板图片像素数据不完整".into());
    }
    let (mut staged, file) = temporary_file()?;
    use image::ImageEncoder;
    image::codecs::png::PngEncoder::new(file)
        .write_image(pixels, width as u32, height as u32, color.into())
        .map_err(|e| format!("无法编码剪贴板图片：{e}"))?;
    staged.bytes = fs::metadata(&staged.path).map_err(|e| e.to_string())?.len();
    Ok(staged)
}

fn temporary_file() -> Result<(TemporaryImage, fs::File), String> {
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|e| e.to_string())?;
    let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let path = std::env::temp_dir().join(format!("teleaf-clipboard-{name}.png"));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&path)
        .map_err(|e| format!("无法暂存剪贴板图片：{e}"))?;
    Ok((TemporaryImage { path, bytes: 0 }, file))
}

#[cfg(test)]
pub fn test_image() -> TemporaryImage {
    stage_image(1, 1, &[255, 0, 0, 255]).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_private_pasteboard_reads_images_files_and_text_without_touching_user_clipboard() {
        let source = test_image();
        let script = format!(
            "{MAC_SCRIPT}\nfunction run(argv) {{ var pb = $.NSPasteboard.pasteboardWithUniqueName; pb.setDataForType($.NSData.dataWithContentsOfFile(argv[1]), 'public.png'); return JSON.stringify(readClipboard(pb, argv[0])); }}"
        );
        let Content::Image(image) = read_macos(&script, &[&source.path]).unwrap() else {
            panic!("image")
        };
        assert_eq!(
            image::open(&image.path).unwrap().into_rgba8().as_raw(),
            &[255, 0, 0, 255]
        );
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&image.path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let script = format!(
            "{MAC_SCRIPT}\nfunction run(argv) {{ var pb = $.NSPasteboard.pasteboardWithUniqueName; var bitmap = $.NSBitmapImageRep.imageRepWithData($.NSData.dataWithContentsOfFile(argv[1])); pb.setDataForType(bitmap.TIFFRepresentation, 'public.tiff'); return JSON.stringify(readClipboard(pb, argv[0])); }}"
        );
        let Content::Image(image) = read_macos(&script, &[&source.path]).unwrap() else {
            panic!("TIFF image")
        };
        assert_eq!(
            image::open(&image.path).unwrap().into_rgba8().as_raw(),
            &[255, 0, 0, 255]
        );
        let script = format!(
            "{MAC_SCRIPT}\nfunction run(argv) {{ var pb = $.NSPasteboard.pasteboardWithUniqueName; pb.writeObjects($.NSArray.arrayWithObject($.NSURL.fileURLWithPath(argv[1]))); return JSON.stringify(readClipboard(pb, argv[0])); }}"
        );
        let Content::Files(files) = read_macos(&script, &[&source.path]).unwrap() else {
            panic!("files")
        };
        assert_eq!(files, vec![source.path.clone()]);
        let script = format!(
            "{MAC_SCRIPT}\nfunction run(argv) {{ var pb = $.NSPasteboard.pasteboardWithUniqueName; pb.setStringForType('文字 $(not-code)', 'public.utf8-plain-text'); return JSON.stringify(readClipboard(pb, argv[0])); }}"
        );
        let Content::Text(text) = read_macos(&script, &[]).unwrap() else {
            panic!("text")
        };
        assert_eq!(text, "文字 $(not-code)");
        let script = format!(
            "{MAC_SCRIPT}\nfunction run(argv) {{ var pb = $.NSPasteboard.pasteboardWithUniqueName; pb.setStringForType('界'.repeat(87382), 'public.utf8-plain-text'); return JSON.stringify(readClipboard(pb, argv[0])); }}"
        );
        assert!(read_macos(&script, &[]).err().unwrap().contains("256 KiB"));
    }

    #[test]
    fn cancelled_read_drops_late_temporary_files_and_staging_is_bounded() {
        let mut state = State::default();
        let (sender, receiver) = mpsc::sync_channel(1);
        state.pending = Some(receiver);
        state.cancel();
        let image = test_image();
        let path = image.path.clone();
        drop(sender.send((1, InputMode::Off, Ok(Content::Image(image)))));
        assert!(!path.exists());
        let mut image = test_image();
        image.bytes = 256 * 1024 * 1024 + 1;
        assert!(!state.can_stage(&[], &image));
    }

    #[test]
    fn clipboard_pixels_stage_losslessly_and_files_live_through_submission() {
        let pixels = [255, 0, 0, 255, 0, 0, 255, 128];
        let image = stage_image(2, 1, &pixels).unwrap();
        let path = image.path.clone();
        assert_eq!(image::open(&path).unwrap().into_rgba8().as_raw(), &pixels);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let mut state = State::default();
        assert!(state.can_stage(&[], &image));
        state.submitted(vec![image]);
        assert!(path.exists());
        drop(state);
        assert!(!path.exists());
        assert!(stage_image(20_000, 20_000, &[]).is_err());
        assert!(stage_image(2, 1, &[0; 4]).is_err());
    }
}
