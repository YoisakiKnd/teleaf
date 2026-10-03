//! Terminal preferences are explicit overrides; capabilities are otherwise queried.
use base64::Engine;
use std::io::{self, Write};

pub fn sync_output() -> bool {
    std::env::var("TG_SYNC_OUTPUT").as_deref() != Ok("0")
}
pub fn scroll_lines() -> isize {
    std::env::var("TG_SCROLL_LINES")
        .ok()
        .and_then(|s| s.parse::<isize>().ok())
        .unwrap_or(3)
        .clamp(1, 12)
}
pub fn osc52(value: &str, writer: &mut impl Write) -> Result<(), String> {
    if value.len() > 100_000 {
        return Err("文字超过终端剪贴板传输上限（100 KB）".into());
    }
    let encoded = base64::engine::general_purpose::STANDARD.encode(value.as_bytes());
    let sequence = format!("\x1b]52;c;{encoded}\x07");
    // tmux must forward the control string to the outer terminal.
    if std::env::var_os("TMUX").is_some() {
        write!(
            writer,
            "\x1bPtmux;{}\x1b\\",
            sequence.replace('\x1b', "\x1b\x1b")
        )
    } else {
        writer.write_all(sequence.as_bytes())
    }
    .and_then(|_| writer.flush())
    .map_err(|e| e.to_string())
}
pub fn terminal_clipboard(value: &str) -> Result<(), String> {
    osc52(value, &mut io::stdout().lock())
}
pub fn prefer_terminal_clipboard() -> bool {
    std::env::var("TG_CLIPBOARD").as_deref() == Ok("osc52")
        || (std::env::var("TG_CLIPBOARD").as_deref() != Ok("system")
            && std::env::var_os("SSH_CONNECTION").is_some())
}
pub fn summary() -> String {
    let name = std::env::var("TERM_PROGRAM")
        .or_else(|_| std::env::var("TERM"))
        .unwrap_or("未知终端".into());
    format!(
        "{name} · 同步输出 {} · 滚轮 {} 行",
        if sync_output() { "开" } else { "关" },
        scroll_lines()
    )
}

// Query on the calling thread with a bounded read. A detached blocking stdin
// reader can steal later input and restore cooked mode after the TUI has started.
pub fn picker() -> ratatui_image::picker::Picker {
    // Reconnects happen after the event reader is active. Only query once.
    static STARTUP: std::sync::OnceLock<ratatui_image::picker::Picker> = std::sync::OnceLock::new();
    STARTUP.get_or_init(detect_picker).clone()
}
fn detect_picker() -> ratatui_image::picker::Picker {
    use ratatui_image::picker::cap_parser::{Parser, QueryStdioOptions, Response};
    use ratatui_image::picker::{Picker, ProtocolType};
    let requested = std::env::var("TG_IMAGE_PROTOCOL")
        .unwrap_or("auto".into())
        .to_ascii_lowercase();
    if requested == "halfblocks" {
        return Picker::halfblocks();
    }
    let forced = match requested.as_str() {
        "kitty" => Some(ProtocolType::Kitty),
        "sixel" => Some(ProtocolType::Sixel),
        "iterm2" => Some(ProtocolType::Iterm2),
        _ => None,
    };
    if forced.is_none() && std::env::var("TERM").as_deref() == Ok("dumb") {
        return Picker::halfblocks();
    }
    let font = std::env::var("TG_CELL_SIZE")
        .ok()
        .and_then(|s| cell_size(&s))
        .or_else(|| {
            let size = crossterm::terminal::window_size().ok()?;
            cell_size(&format!(
                "{}x{}",
                size.width.checked_div(size.columns)?,
                size.height.checked_div(size.rows)?
            ))
        })
        .unwrap_or(ratatui_image::FontSize::new(10, 20));
    #[allow(deprecated)]
    let mut picker = Picker::from_fontsize(font);
    if let Some(protocol) = forced {
        picker.set_protocol_type(protocol);
        return picker;
    }
    let incompatible_placeholders = std::env::var_os("WEZTERM_EXECUTABLE").is_some()
        || std::env::var_os("KONSOLE_VERSION").is_some();
    let options = QueryStdioOptions {
        blacklist_protocols: if incompatible_placeholders {
            vec![ProtocolType::Kitty, ProtocolType::Sixel]
        } else {
            vec![]
        },
        ..QueryStdioOptions::default()
    };
    let query = Parser::query(std::env::var_os("TMUX").is_some(), options);
    let mut output = io::stdout().lock();
    if output
        .write_all(query.as_bytes())
        .and_then(|_| output.flush())
        .is_err()
    {
        return Picker::halfblocks();
    }
    drop(output);
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    let mut parser = Parser::new();
    let mut protocol = None;
    let mut measured_font = None;
    let mut done = false;
    while !done && std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        let Ok(bytes) = query_input(remaining) else {
            break;
        };
        if bytes.is_empty() {
            break;
        }
        for byte in bytes {
            for response in parser.push(char::from(byte)) {
                match response {
                    Response::Kitty if !incompatible_placeholders => {
                        protocol = Some(ProtocolType::Kitty)
                    }
                    Response::Sixel if !incompatible_placeholders && protocol.is_none() => {
                        protocol = Some(ProtocolType::Sixel)
                    }
                    Response::CellSize(Some((w, h))) => {
                        measured_font = cell_size(&format!("{w}x{h}"))
                    }
                    Response::Status => done = true,
                    _ => {}
                }
            }
        }
    }
    if std::env::var_os("TG_CELL_SIZE").is_none()
        && let Some(font) = measured_font
    {
        let previous = picker.protocol_type();
        #[allow(deprecated)]
        {
            picker = Picker::from_fontsize(font);
        }
        picker.set_protocol_type(previous);
    }
    if let Some(protocol) = protocol {
        picker.set_protocol_type(protocol);
    }
    picker
}
fn cell_size(value: &str) -> Option<ratatui_image::FontSize> {
    let (w, h) = value.split_once('x')?;
    let w: u16 = w.parse().ok()?;
    let h: u16 = h.parse().ok()?;
    (w > 0 && h > 0 && w <= 256 && h <= 256).then(|| ratatui_image::FontSize::new(w, h))
}
#[cfg(unix)]
fn query_input(timeout: std::time::Duration) -> io::Result<Vec<u8>> {
    let mut descriptor = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll receives one valid descriptor for a bounded duration; this
    // startup query owns stdin before crossterm's event reader is initialized.
    let ready = unsafe {
        libc::poll(
            &mut descriptor,
            1,
            timeout.as_millis().min(i32::MAX as u128) as i32,
        )
    };
    if ready < 0 {
        return Err(io::Error::last_os_error());
    }
    if ready == 0 {
        return Ok(vec![]);
    }
    let mut bytes = vec![0u8; 256];
    // SAFETY: stdin is ready; the destination is valid for the supplied length.
    let count = unsafe { libc::read(libc::STDIN_FILENO, bytes.as_mut_ptr().cast(), bytes.len()) };
    if count < 0 {
        return Err(io::Error::last_os_error());
    }
    bytes.truncate(count as usize);
    Ok(bytes)
}
#[cfg(windows)]
fn query_input(timeout: std::time::Duration) -> io::Result<Vec<u8>> {
    use windows_sys::Win32::System::Console::{
        GetStdHandle, INPUT_RECORD, KEY_EVENT, PeekConsoleInputW, ReadConsoleInputW,
        STD_INPUT_HANDLE,
    };
    // SAFETY: GetStdHandle borrows the process console handle; it is not closed.
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let mut record = INPUT_RECORD::default();
        let mut count = 0;
        // SAFETY: peek writes at most one initialized record and a count.
        if unsafe { PeekConsoleInputW(input, &mut record, 1, &mut count) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if count > 0 {
            // SAFETY: the query is the only reader at startup; a record is ready.
            if unsafe { ReadConsoleInputW(input, &mut record, 1, &mut count) } == 0 {
                return Err(io::Error::last_os_error());
            }
            if record.EventType == KEY_EVENT as u16 {
                // SAFETY: EventType identifies the active KeyEvent union field.
                let event = unsafe { record.Event.KeyEvent };
                // SAFETY: ReadConsoleInputW fills the UnicodeChar union field.
                let character = unsafe { event.uChar.UnicodeChar };
                if event.bKeyDown != 0 && character > 0 && character < 128 {
                    return Ok(vec![character as u8]);
                }
            }
        }
        if std::time::Instant::now() >= deadline {
            return Ok(vec![]);
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
#[cfg(not(any(unix, windows)))]
fn query_input(_: std::time::Duration) -> io::Result<Vec<u8>> {
    Ok(vec![])
}

#[cfg(test)]
mod tests {
    #[test]
    fn clipboard_payload_encodes_controls_instead_of_executing_them() {
        let mut output = vec![];
        super::osc52("中文\x1b[31m", &mut output).unwrap();
        assert!(!output.windows(5).any(|s| s == b"[31m\x07"));
        assert!(String::from_utf8(output).unwrap().contains("5Lit5paH"));
        assert!(super::osc52(&"a".repeat(100001), &mut vec![]).is_err());
    }
}
