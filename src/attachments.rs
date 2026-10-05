//! Local attachment browsing and path parsing; never executes pasted shell text.
use crate::{actions, text};
use crossterm::event::KeyCode;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

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
}
pub struct Picker {
    pub directory: PathBuf,
    pub entries: Vec<Entry>,
    pub cursor: usize,
    pub offset: usize,
    pub visible: usize,
    pub selected: Vec<PathBuf>,
    pub clipboard_images: Vec<crate::clipboard::TemporaryImage>,
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
    pub fn key(&mut self, key: KeyCode) {
        match key {
            KeyCode::F(5) => {
                self.photos = !self.photos;
                self.error = None;
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
    pub fn request(&self, chat_id: i64) -> Result<Value, String> {
        if self.selected.is_empty() {
            return Err("先选择文件，再点击发送或按 F8".into());
        }
        if self.caption.chars().count() > 1024 {
            return Err("说明文字最多 1024 个字符".into());
        }
        let mut contents = vec![];
        for (index, path) in self.selected.iter().enumerate() {
            if !path.is_file() || fs::File::open(path).is_err() {
                return Err(format!("文件已移动或无法读取：{}", path.display()));
            }
            let path = path.to_str().ok_or("文件名不是有效的 UTF-8")?;
            if self.photos {
                let reader = image::ImageReader::open(path)
                    .map_err(|e| e.to_string())?
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                if !matches!(
                    reader.format(),
                    Some(
                        image::ImageFormat::Jpeg
                            | image::ImageFormat::Png
                            | image::ImageFormat::WebP
                    )
                ) {
                    return Err("图片模式支持 JPEG / PNG / WebP；其他格式请切换为原文件".into());
                }
                reader
                    .into_dimensions()
                    .map_err(|_| "图片无法读取，请检查文件或切换为原文件")?;
            }
            let request = if self.photos {
                actions::send_photo(chat_id, path)
            } else {
                actions::send_file(chat_id, path)
            };
            let mut content = request["input_message_content"].clone();
            if index == 0 && !self.caption.is_empty() {
                content["caption"] =
                    json!({"@type":"formattedText","text":self.caption,"entities":[]});
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
        picker.clipboard_images.push(image);
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
        picker.caption = "说明".into();
        let request = picker.request(17).unwrap();
        assert_eq!(request["@type"], "sendMessageAlbum");
        assert_eq!(request["reply_to"]["message_id"], 91);
        assert_eq!(
            request["input_message_contents"][0]["caption"]["text"],
            "说明"
        );
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
