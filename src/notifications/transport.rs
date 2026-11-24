//! Terminal transport and the optional argv hook.
use std::io::{self, IsTerminal, Write};
use std::process::{Command as Process, Stdio};
use std::time::{Duration, Instant};

use super::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Protocol {
    Osc99,
    Osc777,
    Osc9,
    Bell,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mux {
    None,
    Tmux,
    Screen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Terminal {
    protocol: Protocol,
    mux: Mux,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Backend {
    Terminal(Terminal),
    Native,
    Hook(Vec<String>),
    Off,
}

impl Backend {
    pub(super) fn detect() -> Result<Self, String> {
        let preference = std::env::var("TG_NOTIFY_BACKEND").unwrap_or_default();
        let terminal = io::stdout().is_terminal();
        let remote =
            std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some();
        let desktop = !cfg!(target_os = "linux")
            || ["DBUS_SESSION_BUS_ADDRESS", "DISPLAY", "WAYLAND_DISPLAY"]
                .iter()
                .any(|key| std::env::var_os(key).is_some());
        let env = |key| std::env::var(key).unwrap_or_default().to_ascii_lowercase();
        let program = env("TERM_PROGRAM");
        let term = env("TERM");
        let present = |key: &str| std::env::var_os(key).is_some();
        let mux = if present("TMUX") {
            Mux::Tmux
        } else if present("STY") || term.starts_with("screen") {
            Mux::Screen
        } else {
            Mux::None
        };
        let terminal_with = |protocol| Self::Terminal(Terminal { protocol, mux });
        let detected = detect_protocol(&program, &term, present);
        let native = cfg!(any(windows, target_os = "linux"));
        match preference.to_ascii_lowercase().as_str() {
            "off" => Ok(Self::Off),
            "bell" => Ok(terminal_with(Protocol::Bell)),
            "native" if native => Ok(Self::Native),
            "native" => Ok(terminal_with(Protocol::Bell)),
            "osc" => Ok(terminal_with(detected.unwrap_or(Protocol::Osc99))),
            "osc99" => Ok(terminal_with(Protocol::Osc99)),
            "osc777" => Ok(terminal_with(Protocol::Osc777)),
            "osc9" => Ok(terminal_with(Protocol::Osc9)),
            "auto" | "" | "command" => {
                if let Some(argv) = crate::config::notification_command()? {
                    return Ok(Self::Hook(argv));
                }
                if preference.eq_ignore_ascii_case("command") {
                    return Err("请先配置通知命令 notify_command 或 TG_NOTIFY_CMD".into());
                }
                Ok(Self::automatic(
                    std::env::consts::OS,
                    detected.filter(|_| terminal),
                    !remote && desktop,
                    mux,
                ))
            }
            _ => Err("TG_NOTIFY_BACKEND 值无效".into()),
        }
    }

    fn automatic(os: &str, osc: Option<Protocol>, local_desktop: bool, mux: Mux) -> Self {
        if os == "windows" {
            return Self::Native;
        }
        if let Some(protocol) = osc {
            return Self::Terminal(Terminal { protocol, mux });
        }
        if os == "linux" && local_desktop {
            return Self::Native;
        }
        Self::Terminal(Terminal {
            protocol: Protocol::Bell,
            mux,
        })
    }

    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Terminal(terminal) => match terminal.protocol {
                Protocol::Osc99 => "OSC 99",
                Protocol::Osc777 => "OSC 777",
                Protocol::Osc9 => "OSC 9",
                Protocol::Bell => "Terminal BEL",
            },
            Self::Native if cfg!(windows) => "Windows Toast",
            Self::Native => "Linux D-Bus",
            Self::Hook(_) => "Custom command",
            Self::Off => "off",
        }
    }

    pub(super) fn defaults_on(&self) -> bool {
        match self {
            Self::Off => false,
            Self::Terminal(Terminal {
                protocol: Protocol::Bell,
                ..
            }) => io::stdout().is_terminal(),
            _ => true,
        }
    }

    pub(super) fn replaces_by_chat(&self) -> bool {
        matches!(self, Self::Terminal(_) | Self::Hook(_))
    }
}

fn detect_protocol(program: &str, term: &str, present: impl Fn(&str) -> bool) -> Option<Protocol> {
    if present("WT_SESSION")
        || present("GNOME_TERMINAL_SCREEN")
        || present("GNOME_TERMINAL_SERVICE")
        || program == "alacritty"
        || term.contains("alacritty")
    {
        return None;
    }
    if program == "ghostty" || term.contains("ghostty") || present("GHOSTTY_RESOURCES_DIR") {
        // Ghostty 1.4 parses OSC 99 without displaying it. OSC 777 is implemented.
        return Some(Protocol::Osc777);
    }
    if program == "kitty" || term.contains("kitty") || present("KITTY_WINDOW_ID") {
        return Some(Protocol::Osc99);
    }
    if program == "iterm.app" || present("ITERM_SESSION_ID") {
        return Some(Protocol::Osc9);
    }
    if program == "wezterm"
        || present("WEZTERM_PANE")
        || term.starts_with("foot")
        || present("VTE_VERSION")
        || term.starts_with("rxvt")
    {
        return Some(Protocol::Osc777);
    }
    None
}

fn field(text: &str, bytes: usize, osc: bool) -> String {
    let mut field = String::new();
    for ch in text.chars().take(2048) {
        let ch = if ch.is_control() || (osc && matches!(ch, ';' | '\u{2028}' | '\u{2029}')) {
            ' '
        } else {
            ch
        };
        if field.len() + ch.len_utf8() > bytes {
            break;
        }
        field.push(ch);
    }
    field
}

impl Terminal {
    #[cfg(test)]
    pub(super) fn test_osc99() -> Self {
        Self {
            protocol: Protocol::Osc99,
            mux: Mux::None,
        }
    }

    pub(super) fn write(
        self,
        writer: &mut impl Write,
        command: &Command,
        account: &str,
    ) -> io::Result<bool> {
        let sequence = match command {
            Command::Show {
                chat,
                title,
                body,
                silent,
                ..
            } => {
                let budget = if self.mux == Mux::Screen { 90 } else { 480 };
                let title = field(title, budget, true);
                let body = field(body, budget, true);
                match self.protocol {
                    Protocol::Osc99 => {
                        let sound = if *silent { ":s=c2lsZW50" } else { "" };
                        format!(
                            "\x1b]99;i={account}-{chat}:p=title:d=0{sound};{title}\x07\x1b]99;i={account}-{chat}:p=body{sound};{body}\x07"
                        )
                    }
                    Protocol::Osc777 => format!("\x1b]777;notify;{title};{body}\x07"),
                    Protocol::Osc9 => format!("\x1b]9;{title} · {body}\x07"),
                    Protocol::Bell if !silent => "\x07".to_owned(),
                    Protocol::Bell => return Ok(false),
                }
            }
            Command::Remove { chat, .. } if self.protocol == Protocol::Osc99 => {
                format!("\x1b]99;i={account}-{chat}:p=close;\x07")
            }
            _ => return Ok(false),
        };
        if self.protocol == Protocol::Bell {
            writer.write_all(sequence.as_bytes())?;
        } else {
            match self.mux {
                Mux::None => writer.write_all(sequence.as_bytes())?,
                Mux::Tmux => crate::terminal::write_tmux_passthrough(writer, &sequence)?,
                Mux::Screen => {
                    // An inner BEL ends each OSC, leaving ST for Screen's DCS.
                    for osc in sequence.split_inclusive('\x07') {
                        write!(writer, "\x1bP{osc}\x1b\\")?;
                    }
                }
            }
        }
        Ok(true)
    }
}

fn payload(command: &Command) -> Result<Vec<u8>, String> {
    let (op, chat, title, body, silent) = match command {
        Command::Show {
            chat,
            title,
            body,
            silent,
            ..
        } => (
            "show",
            *chat,
            field(title, 264, false),
            field(body, 720, false),
            *silent,
        ),
        Command::Remove { chat, .. } => ("remove", *chat, String::new(), String::new(), true),
        Command::Clear => ("clear", 0, String::new(), String::new(), true),
    };
    let mut bytes = serde_json::to_vec(&serde_json::json!({
        "op":op,"chat_id":chat,"title":title,"body":body,"silent":silent
    }))
    .map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > 4096 {
        return Err("通知命令输入超过 4 KiB".into());
    }
    Ok(bytes)
}

pub(super) fn run_hook(
    argv: &[String],
    command: &Command,
    timeout: Duration,
) -> Result<(), String> {
    let bytes = payload(command)?;
    let mut child = Process::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法启动通知命令：{error}"))?;
    let deadline = Instant::now() + timeout;
    let result = (|| {
        // This is one bounded write to a fresh pipe, below the pipe capacity
        // on the supported desktop platforms, even if the command never reads.
        child
            .stdin
            .take()
            .expect("piped stdin")
            .write_all(&bytes)
            .map_err(|error| format!("无法写入通知命令：{error}"))?;
        loop {
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("通知命令退出：{status}"))
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("通知命令超时".into());
            }
            std::thread::sleep(remaining.min(Duration::from_millis(20)));
        }
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notification() -> Command {
        Command::Show {
            group: 5,
            chat: -123,
            title: "标题;\x1b]52\x07\u{009b}\u{009c}".into(),
            body: "内容;\0\u{007f}\u{2028}\u{009d}".into(),
            silent: true,
        }
    }

    #[test]
    fn terminal_detection_prefers_implemented_protocols() {
        for (program, term, marker, expected) in [
            ("kitty", "", "", Some(Protocol::Osc99)),
            ("ghostty", "", "", Some(Protocol::Osc777)),
            ("", "foot-extra", "", Some(Protocol::Osc777)),
            ("WezTerm", "", "", None), // environment values are lowercased before detection
            ("wezterm", "", "", Some(Protocol::Osc777)),
            ("iterm.app", "", "", Some(Protocol::Osc9)),
            ("", "xterm", "VTE_VERSION", Some(Protocol::Osc777)),
            ("", "xterm", "GNOME_TERMINAL_SCREEN", None),
            ("", "xterm", "WT_SESSION", None),
            ("", "alacritty", "", None),
        ] {
            assert_eq!(
                detect_protocol(program, term, |key| key == marker),
                expected
            );
        }
    }

    #[test]
    fn automatic_notifications_use_winrt_on_windows_even_with_osc() {
        for osc in [None, Some(Protocol::Osc777)] {
            for local_desktop in [true, false] {
                assert_eq!(
                    Backend::automatic("windows", osc, local_desktop, Mux::Tmux),
                    Backend::Native
                );
            }
        }
        assert_eq!(
            Backend::automatic("linux", None, true, Mux::None),
            Backend::Native
        );
        for os in ["linux", "macos"] {
            assert_eq!(
                Backend::automatic(os, Some(Protocol::Osc777), true, Mux::None),
                Backend::Terminal(Terminal {
                    protocol: Protocol::Osc777,
                    mux: Mux::None,
                })
            );
        }
    }

    #[test]
    fn osc_protocols_sanitize_payloads_and_use_the_chat_id_for_replacement() {
        let mut bytes = Vec::new();
        for (protocol, expected) in [
            (
                Protocol::Osc99,
                "\x1b]99;i=account--123:p=title:d=0:s=c2lsZW50;标题  ]52   \x07\x1b]99;i=account--123:p=body:s=c2lsZW50;内容     \x07",
            ),
            (
                Protocol::Osc777,
                "\x1b]777;notify;标题  ]52   ;内容     \x07",
            ),
            (Protocol::Osc9, "\x1b]9;标题  ]52    · 内容     \x07"),
        ] {
            bytes.clear();
            let terminal = Terminal {
                protocol,
                mux: Mux::None,
            };
            assert!(
                terminal
                    .write(&mut bytes, &notification(), "account")
                    .unwrap()
            );
            assert_eq!(bytes, expected.as_bytes());
        }
        let terminal = Terminal {
            protocol: Protocol::Osc99,
            mux: Mux::None,
        };
        bytes.clear();
        terminal
            .write(
                &mut bytes,
                &Command::Remove {
                    group: 999,
                    chat: -123,
                },
                "account",
            )
            .unwrap();
        assert_eq!(bytes, b"\x1b]99;i=account--123:p=close;\x07");
        let bell = Terminal {
            protocol: Protocol::Bell,
            mux: Mux::None,
        };
        bytes.clear();
        assert!(!bell.write(&mut bytes, &notification(), "account").unwrap());
        assert!(bytes.is_empty());
    }

    #[test]
    fn multiplexers_forward_escape_strings_with_distinct_terminators() {
        let command = Command::Remove { group: 1, chat: 2 };
        for (mux, expected) in [
            (
                Mux::Tmux,
                "\x1bPtmux;\x1b\x1b]99;i=abc-2:p=close;\x07\x1b\\",
            ),
            (Mux::Screen, "\x1bP\x1b]99;i=abc-2:p=close;\x07\x1b\\"),
        ] {
            let mut bytes = Vec::new();
            Terminal {
                protocol: Protocol::Osc99,
                mux,
            }
            .write(&mut bytes, &command, "abc")
            .unwrap();
            assert_eq!(bytes, expected.as_bytes());
        }
    }

    #[cfg(unix)]
    #[test]
    fn external_hook_gets_message_only_on_stdin_and_reaps_a_timed_out_process() {
        let output =
            std::env::temp_dir().join(format!("teleaf-hook-{}-test.json", std::process::id()));
        let argv = [
            "/bin/sh".into(),
            "-c".into(),
            "test \"$#\" -eq 1 && cat > \"$1\"".into(),
            "test-hook".into(),
            output.to_string_lossy().into_owned(),
        ];
        let text = "$HOME $(false) `false` \"quote\" \\slash";
        let command = Command::Show {
            group: 5,
            chat: -15,
            title: "通知".into(),
            body: text.into(),
            silent: true,
        };
        run_hook(&argv, &command, Duration::from_secs(2)).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        std::fs::remove_file(output).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
            serde_json::json!({
                "op":"show","chat_id":-15,"title":"通知","body":text,"silent":true
            })
        );
        let timeout = run_hook(
            &["/bin/sleep".into(), "5".into()],
            &command,
            Duration::from_millis(35),
        )
        .unwrap_err();
        assert!(timeout.contains("超时"));
    }
}
