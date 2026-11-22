//! Bounded TDLib proxy management, environment proxy detection, and form state.
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{App, TdWorker, send_request, text};

const MAX_PROXIES: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProxyKind {
    #[default]
    Socks5,
    Http,
    Mtproto,
}

impl ProxyKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Socks5 => "SOCKS5",
            Self::Http => "HTTP",
            Self::Mtproto => "MTProto",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Socks5 => Self::Http,
            Self::Http => Self::Mtproto,
            Self::Mtproto => Self::Socks5,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Socks5 => Self::Mtproto,
            Self::Http => Self::Socks5,
            Self::Mtproto => Self::Http,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyEntry {
    pub id: i32,
    pub server: String,
    pub port: u16,
    pub kind: ProxyKind,
    pub enabled: bool,
    pub ping_ms: Option<u32>,
    pub pinging: bool,
    pub ping_error: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ParsedProxy {
    pub kind: ProxyKind,
    pub server: String,
    pub port: u16,
    pub user_or_secret: String,
    pub password: String,
}

#[derive(Clone)]
pub struct ProxyForm {
    pub kind: ProxyKind,
    pub server: String,
    pub server_cursor: usize,
    pub port: String,
    pub port_cursor: usize,
    pub user_or_secret: String,
    pub user_cursor: usize,
    pub password: String,
    pub password_cursor: usize,
    pub field: usize,
    pub error: Option<String>,
}

impl Default for ProxyForm {
    fn default() -> Self {
        Self {
            kind: ProxyKind::Socks5,
            server: "127.0.0.1".into(),
            server_cursor: "127.0.0.1".len(),
            port: "7890".into(),
            port_cursor: "7890".len(),
            user_or_secret: String::new(),
            user_cursor: 0,
            password: String::new(),
            password_cursor: 0,
            field: 1,
            error: None,
        }
    }
}

impl ProxyForm {
    pub fn from_parsed(parsed: ParsedProxy) -> Self {
        let port = parsed.port.to_string();
        Self {
            kind: parsed.kind,
            server_cursor: parsed.server.len(),
            server: parsed.server,
            port_cursor: port.len(),
            port,
            user_cursor: parsed.user_or_secret.len(),
            user_or_secret: parsed.user_or_secret,
            password_cursor: parsed.password.len(),
            password: parsed.password,
            field: 1,
            error: None,
        }
    }

    pub fn field_count(&self) -> usize {
        match self.kind {
            ProxyKind::Mtproto => 4,
            ProxyKind::Socks5 | ProxyKind::Http => 5,
        }
    }

    pub fn active_text_mut(&mut self) -> Option<(&mut String, &mut usize, usize)> {
        match self.field {
            1 => Some((&mut self.server, &mut self.server_cursor, 128)),
            2 => Some((&mut self.port, &mut self.port_cursor, 5)),
            3 => Some((&mut self.user_or_secret, &mut self.user_cursor, 128)),
            4 if self.kind != ProxyKind::Mtproto => {
                Some((&mut self.password, &mut self.password_cursor, 128))
            }
            _ => None,
        }
    }

    pub fn validate(&self) -> Result<ParsedProxy, String> {
        let server: String = self.server.trim().chars().take(128).collect();
        if !valid_proxy_host(&server) {
            return Err("请填写有效的代理服务器地址（如 127.0.0.1）".into());
        }
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .ok_or_else(|| "端口应为 1–65535 之间的数字".to_owned())?;
        let user_or_secret: String = self.user_or_secret.trim().chars().take(128).collect();
        if self.kind == ProxyKind::Mtproto && user_or_secret.is_empty() {
            return Err("MTProto 代理必须填写 Secret".into());
        }
        let password: String = if self.kind == ProxyKind::Mtproto {
            String::new()
        } else {
            self.password.chars().take(128).collect()
        };
        Ok(ParsedProxy {
            kind: self.kind,
            server,
            port,
            user_or_secret,
            password,
        })
    }
}

#[derive(Default)]
pub struct ProxyState {
    pub open: bool,
    pub entries: Vec<ProxyEntry>,
    pub selected: usize,
    pub form: Option<ProxyForm>,
    pub loading: bool,
    pub status: Option<String>,
    pub status_error: bool,
    startup: Option<ProxyIntent>,
    bootstrapped: bool,
    paused: bool,
    initialized: bool,
    list: Option<(u64, Option<ProxyIntent>)>,
    change: Option<(u64, ProxyChange)>,
    ping: Option<(u64, i32)>,
    connecting_since: Option<Instant>,
}

struct ProxyIntent {
    parsed: ParsedProxy,
    automatic: Option<&'static str>,
}

enum ProxyChange {
    Save(Option<&'static str>),
    Enable(ProxyEntry, Option<&'static str>),
    Disable,
    Remove(i32),
}

type Response = (Option<Value>, bool);

impl ProxyState {
    pub fn active_summary(&self) -> Option<String> {
        let entry = self.entries.iter().find(|entry| entry.enabled)?;
        Some(format!(
            "{} {}:{}",
            entry.kind.label(),
            entry.server,
            entry.port
        ))
    }

    pub fn selected_entry(&self) -> Option<&ProxyEntry> {
        self.entries.get(self.selected)
    }
}

pub fn detect_env_proxy() -> Option<(&'static str, ParsedProxy)> {
    env_proxy(|name| std::env::var(name).ok())
}

fn env_proxy(mut env: impl FnMut(&str) -> Option<String>) -> Option<(&'static str, ParsedProxy)> {
    [
        "TG_PROXY",
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ]
    .into_iter()
    .find_map(|name| Some((name, parse_proxy_url(&env(name)?)?)))
}

fn startup_proxy(
    mut env: impl FnMut(&str) -> Option<String>,
    system: impl FnOnce() -> Option<ParsedProxy>,
) -> Option<ProxyIntent> {
    if env("TG_NO_PROXY").as_deref() == Some("1") {
        return None;
    }
    let (source, parsed) =
        env_proxy(env).or_else(|| system().map(|parsed| ("macOS 系统代理", parsed)))?;
    Some(ProxyIntent {
        parsed,
        automatic: Some(source),
    })
}

#[cfg(target_os = "macos")]
fn system_proxy() -> Option<ParsedProxy> {
    let output = std::process::Command::new("/usr/sbin/scutil")
        .arg("--proxy")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_system_proxy(std::str::from_utf8(&output.stdout).ok()?)
}

#[cfg(not(target_os = "macos"))]
fn system_proxy() -> Option<ParsedProxy> {
    None
}

#[cfg(any(target_os = "macos", test))]
fn parse_system_proxy(output: &str) -> Option<ParsedProxy> {
    // Entries inside __SCOPED__ apply to particular interfaces. Use the top-level settings.
    let get = |key: &str| {
        let mut depth: u8 = 0;
        for line in output.lines().map(str::trim) {
            if depth == 1
                && let Some((name, value)) = line.split_once(" : ")
                && name == key
            {
                return Some(value.trim());
            }
            if line.ends_with('{') {
                depth = depth.saturating_add(1);
            } else if line == "}" {
                depth = depth.saturating_sub(1);
            }
        }
        None
    };
    for (prefix, kind) in [
        ("SOCKS", ProxyKind::Socks5),
        ("HTTPS", ProxyKind::Http),
        ("HTTP", ProxyKind::Http),
    ] {
        if get(&format!("{prefix}Enable")) != Some("1") {
            continue;
        }
        if let (Some(server), Some(port)) = (
            get(&format!("{prefix}Proxy")).filter(|host| valid_proxy_host(host)),
            get(&format!("{prefix}Port"))
                .and_then(|port| port.parse::<u16>().ok())
                .filter(|port| *port > 0),
        ) {
            return Some(ParsedProxy {
                kind,
                server: server.into(),
                port,
                user_or_secret: String::new(),
                password: String::new(),
            });
        }
    }
    None
}

fn valid_proxy_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 128
        && (host.parse::<std::net::IpAddr>().is_ok()
            || host
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'_')))
}

pub fn parse_proxy_url(raw: &str) -> Option<ParsedProxy> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.len() > 2048 {
        return None;
    }
    if let Some(query) = trimmed
        .strip_prefix("tg://proxy?")
        .or_else(|| trimmed.strip_prefix("https://t.me/proxy?"))
        .or_else(|| trimmed.strip_prefix("http://t.me/proxy?"))
    {
        let mut server = String::new();
        let mut port = 0u16;
        let mut secret = String::new();
        for pair in query.split('&') {
            let Some((key, val)) = pair.split_once('=') else {
                continue;
            };
            match key {
                "server" => server = percent_decode(val).chars().take(128).collect(),
                "port" => port = val.parse().unwrap_or(0),
                "secret" => secret = percent_decode(val).chars().take(128).collect(),
                _ => {}
            }
        }
        if valid_proxy_host(&server) && port > 0 && !secret.is_empty() {
            return Some(ParsedProxy {
                kind: ProxyKind::Mtproto,
                server,
                port,
                user_or_secret: secret,
                password: String::new(),
            });
        }
        return None;
    }

    let (kind, rest) = if let Some(rest) = trimmed
        .strip_prefix("socks5://")
        .or_else(|| trimmed.strip_prefix("socks5h://"))
        .or_else(|| trimmed.strip_prefix("socks://"))
    {
        (ProxyKind::Socks5, rest)
    } else if let Some(rest) = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
    {
        (ProxyKind::Http, rest)
    } else if !trimmed.contains("://") {
        (ProxyKind::Http, trimmed)
    } else {
        return None;
    };

    let authority = rest.split('/').next()?.trim();
    let (auth, hostport) = match authority.rsplit_once('@') {
        Some((a, hp)) => (Some(a), hp),
        None => (None, authority),
    };
    let (server, port_str) = if let Some(bracketed) = hostport.strip_prefix('[') {
        let (host, after) = bracketed.split_once(']')?;
        let port = after.strip_prefix(':')?;
        (host, port)
    } else {
        hostport.rsplit_once(':')?
    };
    let server: String = server.trim().chars().take(128).collect();
    let port = port_str.trim().parse::<u16>().ok().filter(|p| *p > 0)?;
    if !valid_proxy_host(&server) {
        return None;
    }
    let (user, pass) = match auth {
        Some(a) => match a.split_once(':') {
            Some((u, p)) => (
                percent_decode(u).chars().take(128).collect(),
                percent_decode(p).chars().take(128).collect(),
            ),
            None => (percent_decode(a).chars().take(128).collect(), String::new()),
        },
        None => (String::new(), String::new()),
    };
    Some(ParsedProxy {
        kind,
        server,
        port,
        user_or_secret: user,
        password: pass,
    })
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

pub fn proxy_type_json(parsed: &ParsedProxy) -> Value {
    match parsed.kind {
        ProxyKind::Socks5 => json!({
            "@type": "proxyTypeSocks5",
            "username": parsed.user_or_secret,
            "password": parsed.password
        }),
        ProxyKind::Http => json!({
            "@type": "proxyTypeHttp",
            "username": parsed.user_or_secret,
            "password": parsed.password,
            "http_only": false
        }),
        ProxyKind::Mtproto => json!({
            "@type": "proxyTypeMtproto",
            "secret": parsed.user_or_secret
        }),
    }
}

pub fn parse_proxy_entry(value: &Value) -> Option<ProxyEntry> {
    let id = value["id"].as_i64().and_then(|id| i32::try_from(id).ok())?;
    let proxy = value.get("proxy").unwrap_or(value);
    let server: String = proxy["server"]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(128)
        .collect();
    if !valid_proxy_host(&server) {
        return None;
    }
    let port = proxy["port"]
        .as_u64()
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(0);
    let kind = match proxy.pointer("/type/@type").and_then(Value::as_str) {
        Some("proxyTypeSocks5") => ProxyKind::Socks5,
        Some("proxyTypeHttp") => ProxyKind::Http,
        Some("proxyTypeMtproto") => ProxyKind::Mtproto,
        _ => return None,
    };
    Some(ProxyEntry {
        id,
        server,
        port,
        kind,
        enabled: value["is_enabled"] == true,
        ping_ms: None,
        pinging: false,
        ping_error: None,
    })
}

impl App {
    pub fn open_proxy_modal(&mut self, worker: &TdWorker) {
        self.close_overlays();
        self.proxy.open = true;
        self.proxy.form = None;
        self.refresh_proxies(worker);
    }

    pub fn refresh_proxies(&mut self, worker: &TdWorker) {
        if self.proxy.change.is_none()
            && self
                .proxy
                .list
                .as_ref()
                .is_none_or(|(_, intent)| intent.is_none())
        {
            let request = self.proxy_list_request(None);
            send_request(self, worker, request);
        }
    }

    fn proxy_list_request(&mut self, intent: Option<ProxyIntent>) -> Value {
        let generation = self.next_sync_generation();
        self.proxy.list = Some((generation, intent));
        self.proxy.loading = true;
        let mut request = crate::actions::get_proxies();
        request["@extra"] = json!(format!("proxy-list:{generation}"));
        request
    }

    pub fn bootstrap_env_proxy(&mut self, worker: &TdWorker) {
        if self.demo || self.proxy.bootstrapped {
            return;
        }
        self.proxy.bootstrapped = true;
        self.proxy.startup = startup_proxy(|name| std::env::var(name).ok(), system_proxy);
        if self.proxy.startup.is_some() {
            self.proxy.paused = send_request(self, worker, network_type("networkTypeNone"));
        }
    }

    pub fn initialize_proxy_list(&mut self, worker: &TdWorker) {
        // The persisted proxy list is loaded when TDLib opens its database.
        if !self.proxy.bootstrapped
            || self.proxy.initialized
            || matches!(
                self.auth.state.as_str(),
                "" | "authorizationStateWaitTdlibParameters"
                    | "authorizationStateWaitEncryptionKey"
            )
        {
            return;
        }
        self.proxy.initialized = true;
        let intent = self.proxy.startup.take();
        if self.proxy.change.is_none()
            && self
                .proxy
                .list
                .as_ref()
                .is_none_or(|(_, pending)| pending.is_none())
        {
            let request = self.proxy_list_request(intent);
            send_request(self, worker, request);
        }
    }

    fn resume_proxy_network(&mut self) -> Option<Value> {
        std::mem::take(&mut self.proxy.paused).then(|| network_type("networkTypeOther"))
    }

    pub fn proxy_connection_state(&mut self, state: &str, now: Instant) {
        match state {
            "connectionStateConnectingToProxy" => {
                self.proxy.connecting_since.get_or_insert(now);
            }
            "connectionStateReady" | "connectionStateUpdating" => {
                self.proxy.connecting_since = None;
                if self.proxy.status.as_deref() == Some(PROXY_TIMEOUT) {
                    self.proxy_feedback("已通过代理连接 Telegram", false);
                }
            }
            _ => {
                self.proxy.connecting_since = None;
            }
        }
    }

    pub fn proxy_deadline(&self) -> Option<Instant> {
        self.proxy
            .connecting_since
            .map(|since| since + Duration::from_secs(15))
    }

    pub fn expire_proxy_connection(&mut self, now: Instant) -> bool {
        if self
            .proxy_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            self.proxy.connecting_since = None;
            self.proxy_feedback(PROXY_TIMEOUT, true);
            return true;
        }
        false
    }

    fn proxy_feedback(&mut self, message: impl Into<String>, error: bool) {
        let message = message.into();
        self.notice = Some(message.clone());
        self.notice_error = error;
        self.proxy.status = Some(message);
        self.proxy.status_error = error;
    }

    pub fn start_add_proxy(&mut self, from_env: bool) {
        if from_env {
            if let Some((source, parsed)) = detect_env_proxy() {
                self.proxy.form = Some(ProxyForm::from_parsed(parsed));
                self.proxy.status = Some(format!(
                    "已从环境变量 {source} 预填代理，按 Enter 保存并启用"
                ));
                self.proxy.status_error = false;
            } else {
                self.proxy.status =
                    Some("未检测到 TG_PROXY / ALL_PROXY / HTTPS_PROXY / HTTP_PROXY".into());
                self.proxy.status_error = true;
            }
        } else {
            self.proxy.form = Some(ProxyForm::default());
            self.proxy.status = None;
            self.proxy.status_error = false;
        }
    }

    pub fn submit_proxy_form(&mut self, worker: &TdWorker) -> bool {
        let Some(form) = self.proxy.form.as_mut() else {
            return false;
        };
        let parsed = match form.validate() {
            Ok(parsed) => parsed,
            Err(error) => {
                form.error = Some(error);
                return true;
            }
        };
        if self.proxy.change.is_some()
            || self
                .proxy
                .list
                .as_ref()
                .is_some_and(|(_, intent)| intent.is_some())
        {
            return true;
        }
        self.proxy.startup = None;
        let request = self.proxy_list_request(Some(ProxyIntent {
            parsed,
            automatic: None,
        }));
        send_request(self, worker, request);
        true
    }

    fn proxy_change_request(&mut self, mut request: Value, change: ProxyChange) -> Value {
        let generation = self.next_sync_generation();
        request["@extra"] = json!(format!("proxy-change:{generation}"));
        self.proxy.change = Some((generation, change));
        request
    }

    fn send_proxy_change(&mut self, worker: &TdWorker, request: Value, change: ProxyChange) {
        if self.proxy.change.is_some()
            || self
                .proxy
                .list
                .as_ref()
                .is_some_and(|(_, intent)| intent.is_some())
        {
            return;
        }
        self.proxy.startup = None;
        self.proxy.list = None;
        self.proxy.loading = false;
        let request = self.proxy_change_request(request, change);
        send_request(self, worker, request);
    }

    pub fn toggle_selected_proxy(&mut self, worker: &TdWorker) {
        let Some(entry) = self.proxy.selected_entry().cloned() else {
            return;
        };
        if entry.enabled {
            self.disable_all_proxies(worker);
        } else {
            self.send_proxy_change(
                worker,
                crate::actions::enable_proxy(entry.id),
                ProxyChange::Enable(entry, None),
            );
        }
    }

    pub fn disable_all_proxies(&mut self, worker: &TdWorker) {
        self.send_proxy_change(
            worker,
            crate::actions::disable_proxy(),
            ProxyChange::Disable,
        );
    }

    pub fn remove_selected_proxy(&mut self, worker: &TdWorker) {
        if let Some(id) = self.proxy.selected_entry().map(|entry| entry.id) {
            self.send_proxy_change(
                worker,
                crate::actions::remove_proxy(id),
                ProxyChange::Remove(id),
            );
        }
    }

    pub fn ping_selected_proxy(&mut self, worker: &TdWorker) {
        if self.proxy.ping.is_some() {
            return;
        }
        let Some(entry) = self.proxy.entries.get_mut(self.proxy.selected) else {
            return;
        };
        let id = entry.id;
        entry.pinging = true;
        entry.ping_error = None;
        let generation = self.next_sync_generation();
        self.proxy.ping = Some((generation, id));
        let mut request = crate::actions::ping_proxy(id);
        request["@extra"] = json!(format!("proxy-ping:{generation}"));
        send_request(self, worker, request);
    }

    pub fn handle_proxy_key(&mut self, worker: &TdWorker, code: crossterm::event::KeyCode) -> bool {
        use crossterm::event::KeyCode;
        if let Some(form) = self.proxy.form.as_mut() {
            form.error = None;
            match code {
                KeyCode::Esc => {
                    self.proxy.form = None;
                }
                KeyCode::Tab | KeyCode::Down => {
                    form.field = (form.field + 1) % form.field_count();
                }
                KeyCode::BackTab | KeyCode::Up => {
                    let count = form.field_count();
                    form.field = (form.field + count - 1) % count;
                }
                KeyCode::Left if form.field == 0 => {
                    form.kind = form.kind.prev();
                    form.field = form.field.min(form.field_count() - 1);
                }
                KeyCode::Right | KeyCode::Char(' ') if form.field == 0 => {
                    form.kind = form.kind.next();
                    form.field = form.field.min(form.field_count() - 1);
                }
                KeyCode::Enter => {
                    return self.submit_proxy_form(worker);
                }
                key => {
                    let field = form.field;
                    if let Some((buf, cursor, max_chars)) = form.active_text_mut()
                        && text::edit(buf, cursor, key)
                    {
                        if field == 2 {
                            buf.retain(|ch| ch.is_ascii_digit());
                        }
                        if buf.chars().count() > max_chars {
                            let cut = buf
                                .char_indices()
                                .nth(max_chars)
                                .map_or(buf.len(), |(idx, _)| idx);
                            buf.truncate(cut);
                        }
                        *cursor = (*cursor).min(buf.len());
                    }
                }
            }
            return true;
        }

        match code {
            KeyCode::Esc | KeyCode::F(9) => {
                self.proxy.open = false;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.proxy.selected = self.proxy.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.proxy.entries.is_empty() {
                    self.proxy.selected =
                        (self.proxy.selected + 1).min(self.proxy.entries.len() - 1);
                }
            }
            KeyCode::Char('a') => self.start_add_proxy(false),
            KeyCode::Char('e') => self.start_add_proxy(true),
            KeyCode::Enter | KeyCode::Char(' ') => self.toggle_selected_proxy(worker),
            KeyCode::Char('t') => self.ping_selected_proxy(worker),
            KeyCode::Char('d') | KeyCode::Delete => self.remove_selected_proxy(worker),
            KeyCode::Char('0') => self.disable_all_proxies(worker),
            KeyCode::Char('r') => self.refresh_proxies(worker),
            _ => return false,
        }
        true
    }

    pub fn receive_proxy(&mut self, value: &Value) -> Option<Response> {
        let (route, generation) = value["@extra"].as_str()?.split_once(':')?;
        let generation = generation.parse::<u64>().ok();
        let kind = value["@type"].as_str().unwrap_or_default();
        match route {
            "proxy-list" => {
                let Some((_, intent)) = self.proxy.list.take_if(|(id, _)| Some(*id) == generation)
                else {
                    return Some((None, false));
                };
                self.proxy.loading = false;
                if !matches!(kind, "addedProxies" | "proxies") {
                    self.proxy_feedback("无法读取代理列表，按 F9 重试或切换直连", true);
                    return Some((None, true));
                }
                let entries = value["proxies"].as_array().map_or(&[][..], Vec::as_slice);
                let enabled = entries.iter().find(|entry| entry["is_enabled"] == true);
                let mut projection: Vec<_> = entries
                    .iter()
                    .filter_map(parse_proxy_entry)
                    .take(MAX_PROXIES)
                    .collect();
                if let Some(entry) = enabled.and_then(parse_proxy_entry)
                    && !projection.iter().any(|stored| stored.id == entry.id)
                {
                    if projection.len() == MAX_PROXIES {
                        projection.pop();
                    }
                    projection.push(entry);
                }
                for entry in &mut projection {
                    if let Some(old) = self.proxy.entries.iter().find(|old| old.id == entry.id) {
                        entry.ping_ms = old.ping_ms;
                        entry.ping_error.clone_from(&old.ping_error);
                    }
                    entry.pinging = self.proxy.ping.is_some_and(|(_, id)| id == entry.id);
                }
                self.proxy.entries = projection;
                self.proxy.selected = self
                    .proxy
                    .selected
                    .min(self.proxy.entries.len().saturating_sub(1));
                let Some(intent) = intent else {
                    return Some((None, true));
                };
                if intent.automatic.is_some_and(|name| name != "TG_PROXY") && enabled.is_some() {
                    return Some((self.resume_proxy_network(), true));
                }
                let parsed = intent.parsed;
                let same = entries
                    .iter()
                    .filter_map(|value| parse_proxy_entry(value).map(|entry| (entry, value)))
                    .filter(|(entry, _)| {
                        entry.kind == parsed.kind
                            && entry.port == parsed.port
                            && entry.server.eq_ignore_ascii_case(&parsed.server)
                    })
                    .max_by_key(|(entry, _)| entry.enabled);
                let request = if let Some((entry, value)) = same {
                    let credentials = value.get("proxy").unwrap_or(value).get("type");
                    let edit = intent.automatic.is_none()
                        && (!parsed.user_or_secret.is_empty() || !parsed.password.is_empty())
                        && credentials != Some(&proxy_type_json(&parsed));
                    if edit {
                        self.proxy_change_request(
                            crate::actions::save_proxy(&parsed, Some(entry.id)),
                            ProxyChange::Save(None),
                        )
                    } else {
                        self.proxy_change_request(
                            crate::actions::enable_proxy(entry.id),
                            ProxyChange::Enable(entry, intent.automatic),
                        )
                    }
                } else {
                    self.proxy_change_request(
                        crate::actions::save_proxy(&parsed, None),
                        ProxyChange::Save(intent.automatic),
                    )
                };
                Some((Some(request), true))
            }
            "proxy-change" => {
                let Some((_, change)) =
                    self.proxy.change.take_if(|(id, _)| Some(*id) == generation)
                else {
                    return Some((None, false));
                };
                let entry = match &change {
                    ProxyChange::Save(_) if matches!(kind, "addedProxy" | "proxy") => {
                        parse_proxy_entry(value).filter(|entry| entry.enabled)
                    }
                    ProxyChange::Enable(entry, _) if kind == "ok" => Some(entry.clone()),
                    _ => None,
                };
                if let Some(mut entry) = entry {
                    let source = match change {
                        ProxyChange::Save(source) | ProxyChange::Enable(_, source) => source,
                        _ => None,
                    };
                    let summary = format!("{} {}:{}", entry.kind.label(), entry.server, entry.port);
                    for existing in &mut self.proxy.entries {
                        existing.enabled = false;
                    }
                    entry.enabled = true;
                    let index = self
                        .proxy
                        .entries
                        .iter()
                        .position(|existing| existing.id == entry.id);
                    if let Some(index) = index {
                        self.proxy.entries[index] = entry;
                        self.proxy.selected = index;
                    } else {
                        if self.proxy.entries.len() == MAX_PROXIES {
                            self.proxy.entries.pop();
                        }
                        self.proxy.entries.push(entry);
                        self.proxy.selected = self.proxy.entries.len() - 1;
                    }
                    if source.is_none() {
                        self.proxy.form = None;
                    }
                    self.proxy_feedback(
                        source.map_or_else(
                            || format!("已启用代理 {summary}"),
                            |source| format!("已自动启用 {source} 代理 {summary}"),
                        ),
                        false,
                    );
                    return Some((self.resume_proxy_network(), true));
                }
                if kind == "ok" {
                    match change {
                        ProxyChange::Disable => {
                            for entry in &mut self.proxy.entries {
                                entry.enabled = false;
                            }
                            self.proxy.connecting_since = None;
                            self.proxy_feedback("已停用代理，改为直连", false);
                            return Some((self.resume_proxy_network(), true));
                        }
                        ProxyChange::Remove(id) => {
                            self.proxy.entries.retain(|entry| entry.id != id);
                            self.proxy.selected = self
                                .proxy
                                .selected
                                .min(self.proxy.entries.len().saturating_sub(1));
                            self.proxy_feedback("已删除代理", false);
                            return Some((None, true));
                        }
                        _ => {}
                    }
                }
                self.proxy_feedback("代理设置失败，按 F9 检查地址、端口和凭据，或切换直连", true);
                let resume = if self.proxy.entries.iter().any(|entry| entry.enabled) {
                    self.resume_proxy_network()
                } else {
                    None
                };
                Some((resume, true))
            }
            "proxy-ping" => {
                let Some((_, id)) = self.proxy.ping.take_if(|(id, _)| Some(*id) == generation)
                else {
                    return Some((None, false));
                };
                let latency = value["seconds"]
                    .as_f64()
                    .filter(|s| s.is_finite() && *s >= 0.0 && kind == "seconds");
                if let Some(entry) = self.proxy.entries.iter_mut().find(|entry| entry.id == id) {
                    entry.pinging = false;
                    entry.ping_ms = latency.map(|s| (s * 1000.0).round().max(1.0) as u32);
                    entry.ping_error = latency.is_none().then(|| "连接失败".into());
                }
                if latency.is_none() {
                    self.proxy_feedback("代理测速失败，按 F9 检查端口或切换直连", true);
                }
                Some((None, true))
            }
            "proxy-network" if kind == "error" => {
                self.proxy.paused = generation == Some(0);
                self.proxy_feedback("无法更新 TDLib 网络状态，按 F9 检查代理或切换直连", true);
                Some((None, true))
            }
            "proxy-network" => Some((None, false)),
            _ => None,
        }
    }
}

const PROXY_TIMEOUT: &str = "连接代理超时，按 F9 检查端口或切换直连";

fn network_type(kind: &str) -> Value {
    json!({"@type":"setNetworkType", "type":{"@type":kind},
        "@extra":if kind == "networkTypeNone" { "proxy-network:1" } else { "proxy-network:0" }})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_socks5_http_ipv6_and_mtproto_proxy_urls() {
        let socks = parse_proxy_url("socks5h://alice:p%40ss@127.0.0.1:7890").unwrap();
        assert_eq!(socks.kind, ProxyKind::Socks5);
        assert_eq!(socks.server, "127.0.0.1");
        assert_eq!(socks.port, 7890);
        assert_eq!(socks.user_or_secret, "alice");
        assert_eq!(socks.password, "p@ss");

        let http = parse_proxy_url("http://[::1]:10809/").unwrap();
        assert_eq!(http.kind, ProxyKind::Http);
        assert_eq!(http.server, "::1");
        assert_eq!(http.port, 10809);

        let mt =
            parse_proxy_url("tg://proxy?server=mt.example.com&port=443&secret=dd0123456789abcdef")
                .unwrap();
        assert_eq!(mt.kind, ProxyKind::Mtproto);
        assert_eq!(mt.server, "mt.example.com");
        assert_eq!(mt.port, 443);
        assert_eq!(mt.user_or_secret, "dd0123456789abcdef");

        assert!(parse_proxy_url("ftp://example.com:21").is_none());
        assert!(parse_proxy_url("http://127.0.0.1:0").is_none());
    }
    fn request(commands: &std::sync::mpsc::Receiver<crate::tdlib::TdCommand>) -> Value {
        let crate::tdlib::TdCommand::Request(value) = commands.try_recv().unwrap() else {
            panic!("expected TDLib request");
        };
        value
    }

    fn respond(app: &mut App, request: &Value, mut value: Value) -> Response {
        value["@extra"] = request["@extra"].clone();
        app.apply(crate::tdlib::TdEvent::Update(value))
    }

    fn saved_proxy(id: i32, port: u16, enabled: bool) -> Value {
        json!({"@type":"addedProxy","id":id,"is_enabled":enabled,
            "proxy":{"@type":"proxy","server":"127.0.0.1","port":port,
                "type":{"@type":"proxyTypeSocks5","username":"","password":""}}})
    }

    fn start(app: &mut App, worker: &TdWorker, source: &'static str) {
        app.demo = false;
        app.proxy.bootstrapped = true;
        app.proxy.paused = true;
        app.proxy.startup = Some(ProxyIntent {
            automatic: Some(source),
            parsed: parse_proxy_url("socks5://127.0.0.1:7890").unwrap(),
        });
        app.initialize_proxy_list(worker);
    }

    #[test]
    fn environment_policy_and_macos_proxy_ignore_opt_out_and_nested_overrides() {
        let system = r#"<dictionary> {
  ExceptionsList : <array> {
    0 : localhost
  }
  SOCKSEnable : 1
  SOCKSPort : 7890
  SOCKSProxy : 127.0.0.1
  __SCOPED__ : <dictionary> {
    HTTPEnable : 1
    HTTPPort : 9999
    HTTPProxy : localhost
  }
}"#;
        let parsed = parse_system_proxy(system).unwrap();
        assert_eq!(parsed.kind, ProxyKind::Socks5);
        assert_eq!(parsed.port, 7890);
        let disabled = startup_proxy(
            |key| match key {
                "TG_NO_PROXY" => Some("1".into()),
                "TG_PROXY" => Some("socks5://localhost:1080".into()),
                _ => None,
            },
            || panic!("do not inspect system if auto proxy is disabled"),
        );
        assert!(disabled.is_none());
        let intent = startup_proxy(
            |key| (key == "ALL_PROXY").then(|| "http://localhost:1080".into()),
            || panic!("the environment takes precedence"),
        )
        .unwrap();
        assert_eq!(intent.automatic, Some("ALL_PROXY"));
    }

    #[test]
    fn startup_waits_for_persisted_list_then_adds_or_reuses_environment_proxy() {
        let mut app = crate::ui::tests::fixture();
        let (worker, commands) = TdWorker::test_pair();
        app.auth.state = "authorizationStateWaitTdlibParameters".into();
        start(&mut app, &worker, "ALL_PROXY");
        assert!(commands.try_recv().is_err());
        app.auth.state = "authorizationStateWaitPhoneNumber".into();
        app.initialize_proxy_list(&worker);
        let list = request(&commands);
        let add = respond(
            &mut app,
            &list,
            json!({"@type":"addedProxies","proxies":[]}),
        )
        .0
        .unwrap();
        assert_eq!(add["@type"], "addProxy");
        assert_eq!(add["proxy"]["port"], 7890);
        assert_eq!(add["enable"], true);
        let resume = respond(&mut app, &add, saved_proxy(1, 7890, true))
            .0
            .unwrap();
        assert_eq!(resume["type"]["@type"], "networkTypeOther");
        assert!(
            app.notice
                .as_deref()
                .unwrap()
                .contains("已自动启用 ALL_PROXY")
        );
        assert_eq!(app.proxy.entries.len(), 1);

        let mut app = crate::ui::tests::fixture();
        start(&mut app, &worker, "TG_PROXY");
        let list = request(&commands);
        let enable = respond(
            &mut app,
            &list,
            json!({"@type":"addedProxies","proxies":[
                saved_proxy(1, 7890, false),saved_proxy(2, 8888, true)
            ]}),
        )
        .0
        .unwrap();
        assert_eq!(enable["@type"], "enableProxy");
        assert_eq!(enable["proxy_id"], 1);
        respond(&mut app, &enable, json!({"@type":"ok"}));
        assert!(app.proxy.entries[0].enabled);
        assert!(!app.proxy.entries[1].enabled);
    }

    #[test]
    fn fallback_environment_respects_an_enabled_proxy_even_beyond_the_display_limit() {
        let mut app = crate::ui::tests::fixture();
        let (worker, commands) = TdWorker::test_pair();
        start(&mut app, &worker, "HTTPS_PROXY");
        let list = request(&commands);
        let mut proxies: Vec<_> = (1..=32).map(|id| saved_proxy(id, 7890, false)).collect();
        proxies.push(saved_proxy(40, 8888, true));
        let resume = respond(
            &mut app,
            &list,
            json!({"@type":"addedProxies","proxies":proxies}),
        )
        .0
        .unwrap();
        assert_eq!(resume["@type"], "setNetworkType");
        assert_eq!(app.proxy.entries.len(), 32);
        assert_eq!(
            app.proxy.active_summary().as_deref(),
            Some("SOCKS5 127.0.0.1:8888")
        );
    }

    #[test]
    fn manual_duplicates_edit_in_place_and_errors_do_not_leak_or_replace_existing_proxy() {
        let mut app = crate::ui::tests::fixture();
        let (worker, commands) = TdWorker::test_pair();
        app.proxy.form = Some(ProxyForm::from_parsed(
            parse_proxy_url("socks5://alice:very-secret@127.0.0.1:7890").unwrap(),
        ));
        app.submit_proxy_form(&worker);
        let list = request(&commands);
        let edit = respond(
            &mut app,
            &list,
            json!({"@type":"addedProxies","proxies":[saved_proxy(1,7890,true)]}),
        )
        .0
        .unwrap();
        assert_eq!(edit["@type"], "editProxy");
        assert_eq!(edit["proxy_id"], 1);
        respond(
            &mut app,
            &edit,
            json!({"@type":"error","message":"invalid very-secret"}),
        );
        assert!(app.proxy.entries[0].enabled);
        assert!(app.proxy.form.is_some());
        assert!(app.proxy.status_error);
        assert!(!app.notice.as_deref().unwrap().contains("very-secret"));
        assert_eq!(
            respond(&mut app, &edit, saved_proxy(2, 7890, true)),
            (None, false)
        );
        assert_eq!(app.proxy.entries.len(), 1);
        app.ping_selected_proxy(&worker);
        let ping = request(&commands);
        respond(
            &mut app,
            &ping,
            json!({"@type":"error","message":"invalid very-secret"}),
        );
        assert!(!app.proxy.entries[0].pinging);
        assert!(!app.notice.as_deref().unwrap().contains("very-secret"));
    }

    #[test]
    fn connection_timeout_is_one_shot_and_recovers_on_ready() {
        let mut app = crate::ui::tests::fixture();
        let now = Instant::now();
        app.proxy_connection_state("connectionStateConnectingToProxy", now);
        assert!(!app.expire_proxy_connection(now + Duration::from_secs(14)));
        assert!(app.expire_proxy_connection(now + Duration::from_secs(15)));
        assert!(!app.expire_proxy_connection(now + Duration::from_secs(16)));
        assert!(app.notice_error);
        app.proxy_connection_state("connectionStateReady", now + Duration::from_secs(17));
        assert!(!app.notice_error);
        assert!(app.proxy_deadline().is_none());
    }
}
