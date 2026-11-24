mod actions;
mod attachments;
mod auth;
mod calendar;
mod cli;
mod clipboard;
mod config;
mod demo;
mod instant_view;
mod interaction;
mod markdown;
mod media;
mod menu;
mod notifications;
mod parity;
mod proxy;
mod quick_message;
mod resample;
mod selection;
mod stickers;
mod store;
mod sync;
mod tdlib;
mod terminal;
mod text;
mod theme;
mod ui;

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{self, Stdout};
use std::process::Command;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, Event, KeyCode,
    KeyEventKind, KeyModifiers,
};
use crossterm::execute;
use crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use serde_json::{Value, json};

use auth::AuthFlow;
use media::MediaManager;
use store::{Message, Store};
use tdlib::{TdEvent, TdWorker};

#[derive(Clone, Copy, PartialEq, Eq)]
enum InputMode {
    Off,
    Send,
    Reply(i64),
    Edit(i64),
    Search,
    React(i64),
}

struct SavedDraft {
    mode: InputMode,
    text: String,
    cursor: usize,
    suspended: Option<(InputMode, String, usize)>,
}

struct ReadingPosition {
    anchor: Option<(i64, usize)>,
    selected: Option<i64>,
}

#[derive(Default)]
struct QuickSwitcher {
    query: String,
    global_messages: bool,
    cursor: usize,
    selected: usize,
    scroll: usize,
}

const QUICK_REACTIONS: [&str; 8] = ["👍", "❤️", "😂", "😮", "😢", "🔥", "🎉", "👏"];

const MOUSE_CAPTURE: &str = "\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1006h";
const MOUSE_RELEASE: &str = "\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l";

fn disable_mouse_capture(writer: &mut impl io::Write) -> io::Result<()> {
    // Disable only mouse reporting; keep raw keyboard input and resize events.
    #[cfg(windows)]
    terminal::disable_console_mouse()?;
    execute!(writer, crossterm::style::Print(MOUSE_RELEASE))
}

fn enable_mouse_capture(writer: &mut impl io::Write) -> io::Result<()> {
    #[cfg(windows)]
    execute!(writer, crossterm::event::EnableMouseCapture)?;
    // A single-mode terminal must receive the final enable AFTER disabling 1003.
    execute!(writer, crossterm::style::Print(MOUSE_CAPTURE))
}

struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalGuard {
    fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)
            .and_then(|_| enable_mouse_capture(&mut stdout))
        {
            let _ = execute!(
                stdout,
                DisableBracketedPaste,
                DisableMouseCapture,
                LeaveAlternateScreen
            );
            let _ = disable_raw_mode();
            return Err(error);
        }

        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(Self { terminal }),
            Err(error) => {
                let _ = execute!(
                    io::stdout(),
                    DisableBracketedPaste,
                    DisableMouseCapture,
                    LeaveAlternateScreen
                );
                let _ = disable_raw_mode();
                Err(error)
            }
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            self.terminal.backend_mut(),
            EndSynchronizedUpdate,
            DisableBracketedPaste,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        // On Windows DisableMouseCapture restores the mode saved AFTER raw mode
        // was enabled. Restore cooked keyboard input last.
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}

struct LinkPicker {
    items: Vec<store::LinkItem>,
    selected: usize,
}

struct InlinePicker {
    message: i64,
    selected: usize,
}

#[derive(Clone)]
struct ThreadView {
    source_chat: i64,
    post_id: i64,
    thread_id: i64,
    discussion_chat: Option<i64>,
    title: String,
}

struct App {
    demo: bool,
    status: String,
    tdlib_path: Option<String>,
    tdlib_connected: bool,
    connection_label: String,
    notice: Option<String>,
    notice_error: bool,
    auth: AuthFlow,
    store: Store,
    sync: sync::Sync,
    parity: parity::Parity,
    proxy: proxy::ProxyState,
    link_picker: Option<LinkPicker>,
    inline_picker: Option<InlinePicker>,
    instant_view: Option<instant_view::Reader>,
    show_topics: bool,
    topic_cursor: usize,
    active_thread: Option<ThreadView>,
    media: MediaManager,
    selected_chat: Option<i64>,
    selected_message: Option<i64>,
    focus_messages: bool,
    preview_message: Option<i64>,
    sticker_picker: bool,
    sticker_cursor: usize,
    sticker_panel: stickers::Panel,
    attachments: Option<attachments::Picker>,
    clipboard: clipboard::State,
    notifications: notifications::State,
    show_help: bool,
    show_settings: bool,
    show_chat_info: bool,
    chat_info_modal: bool,
    quick_switcher: Option<QuickSwitcher>,
    collapsed_pin_chat: Option<i64>,
    multi_select: Vec<i64>,
    confirm_delete_many: bool,
    forward_many: bool,
    composer_width: usize,
    settings_focus: Option<ui::Action>,
    action_menu: Option<usize>,
    menu_scope: menu::Scope,
    menu_generation: u64,
    message_check: Option<menu::Check>,
    quick_message: quick_message::State,
    timeline_anchor: Option<(i64, usize)>,
    timeline_top: usize,
    timeline_max: usize,
    timeline_rows: Vec<(i64, usize)>,
    quote_back: Option<ReadingPosition>,
    timeline_heights: HashMap<i64, usize>,
    timeline_width: u16,
    reveal_message: bool,
    pending_messages: usize,
    chat_offset: usize,
    chat_visible: usize,
    reveal_chat: bool,
    mouse_enabled: bool,
    folder_offset: usize,
    folder_cursor: usize,
    show_folders: bool,
    visible_media: Vec<i32>,
    visible_read_ids: Vec<i64>,
    selection: Option<selection::Selection>,
    drag: Option<interaction::Drag>,
    last_click: Option<(ui::Target, u16, u16, Instant)>,
    scrollbars: Vec<(ui::Pane, ratatui::layout::Rect, usize)>,
    preview_view: media::View,
    mouse_press: Option<(crossterm::event::MouseButton, ui::Target)>,
    composer_focus: bool,
    draft_cursor: usize,
    drafts: VecDeque<(i64, SavedDraft)>,
    suspended_input: Option<(InputMode, String, usize)>,
    modal_scroll: u16,
    modal_visible: usize,
    hit_targets: Vec<(ratatui::layout::Rect, ui::Target)>,
    search_query: String,
    show_search: bool,
    confirm_delete: Option<i64>,
    forward_message: Option<i64>,
    requested_files: HashSet<i32>,
    failed_files: HashSet<i32>,
    history_loading: bool,
    input_mode: InputMode,
    draft: String,
    update_count: u64,
    last_update: String,
}

impl App {
    fn new() -> Self {
        Self::with_parts(AuthFlow::new(), MediaManager::new())
    }

    fn with_parts(auth: AuthFlow, media: MediaManager) -> Self {
        Self {
            demo: false,
            status: "正在连接 TDLib…".into(),
            tdlib_path: None,
            tdlib_connected: false,
            connection_label: "连接中".into(),
            notice: None,
            notice_error: false,
            auth,
            store: Store::default(),
            sync: sync::Sync::default(),
            parity: parity::Parity::default(),
            proxy: proxy::ProxyState::default(),
            link_picker: None,
            inline_picker: None,
            instant_view: None,
            show_topics: false,
            topic_cursor: 0,
            active_thread: None,
            media,
            selected_chat: None,
            selected_message: None,
            focus_messages: false,
            preview_message: None,
            sticker_picker: false,
            sticker_cursor: 0,
            sticker_panel: stickers::Panel::default(),
            attachments: None,
            clipboard: clipboard::State::default(),
            notifications: notifications::State::default(),
            show_help: false,
            show_settings: false,
            show_chat_info: false,
            chat_info_modal: false,
            quick_switcher: None,
            collapsed_pin_chat: None,
            multi_select: Vec::new(),
            confirm_delete_many: false,
            forward_many: false,
            composer_width: 40,
            action_menu: None,
            menu_scope: menu::Scope::default(),
            menu_generation: 0,
            message_check: None,
            quick_message: quick_message::State::default(),
            timeline_anchor: None,
            timeline_top: 0,
            timeline_max: 0,
            timeline_rows: Vec::new(),
            quote_back: None,
            timeline_heights: HashMap::new(),
            timeline_width: 0,
            reveal_message: false,
            pending_messages: 0,
            chat_offset: 0,
            chat_visible: 1,
            reveal_chat: true,
            mouse_enabled: true,
            settings_focus: None,
            folder_offset: 0,
            folder_cursor: 0,
            show_folders: false,
            visible_media: Vec::new(),
            visible_read_ids: Vec::new(),
            mouse_press: None,
            selection: None,
            drag: None,
            last_click: None,
            scrollbars: Vec::new(),
            preview_view: media::View::default(),
            composer_focus: true,
            draft_cursor: usize::MAX,
            drafts: VecDeque::new(),
            suspended_input: None,
            modal_scroll: 0,
            modal_visible: 1,
            hit_targets: Vec::new(),
            search_query: String::new(),
            show_search: false,
            confirm_delete: None,
            forward_message: None,
            requested_files: HashSet::new(),
            failed_files: HashSet::new(),
            history_loading: false,
            input_mode: InputMode::Off,
            draft: String::new(),
            update_count: 0,
            last_update: "尚无更新".into(),
        }
    }

    fn apply(&mut self, event: TdEvent) -> (Option<Value>, bool) {
        match event {
            TdEvent::Connected { version, path } => {
                self.status = format!("TDLib {version} 已连接");
                self.tdlib_path = Some(path.display().to_string());
                self.tdlib_connected = true;
                (None, true)
            }
            TdEvent::Error(message) => {
                self.status = message;
                self.tdlib_connected = false;
                self.fail_instant_view();
                (None, true)
            }
            TdEvent::Update(value) => {
                if let Some(result) = quick_message::receive(self, &value) {
                    return result;
                }
                quick_message::observe(self, &value);
                if let Some(changed) = menu::receive(self, &value) {
                    return (None, changed);
                }
                let menu_changed = menu::invalidate(self, &value);
                if let Some((request, changed)) = self.receive_proxy(&value) {
                    return (request, changed || menu_changed);
                }
                if let Some(changed) = self.receive_parity(&value) {
                    return (None, changed || menu_changed);
                }
                if let Some((request, changed)) = self.receive_sync(&value) {
                    return (request, changed || menu_changed);
                }
                let was_ready = self.auth.state == "authorizationStateReady";
                self.update_count += 1;
                self.last_update = value
                    .get("@type")
                    .and_then(|kind| kind.as_str())
                    .unwrap_or("未知事件")
                    .to_owned();
                self.notifications.update(
                    &value,
                    &self.store,
                    self.auth.state == "authorizationStateReady" && !self.demo,
                );
                if self.last_update == "updateConnectionState" {
                    let connection = value.pointer("/state/@type").and_then(Value::as_str);
                    self.proxy_connection_state(connection.unwrap_or_default(), Instant::now());
                    self.connection_label = match connection {
                        Some("connectionStateReady") => "在线",
                        Some("connectionStateUpdating") => "同步中",
                        Some("connectionStateWaitingForNetwork") => "等待网络",
                        Some("connectionStateConnectingToProxy") => "连接代理",
                        _ => "连接中",
                    }
                    .into();
                    return (None, true);
                }
                let media_changed =
                    self.last_update == "updateFile" && self.observe_large_file(&value["file"]);
                if self.last_update == "updateFile" || self.last_update == "file" {
                    let file = if self.last_update == "updateFile" {
                        value.get("file")
                    } else {
                        Some(&value)
                    };
                    if let Some(file) = file
                        && file
                            .pointer("/local/is_downloading_completed")
                            .and_then(Value::as_bool)
                            == Some(true)
                        && let Some(id) = file.get("id").and_then(Value::as_i64)
                    {
                        self.requested_files.remove(&(id as i32));
                        self.failed_files.remove(&(id as i32));
                    }
                }
                if self.last_update == "error" && value["@extra"].as_str() == Some("sender-name") {
                    // Keep the ID fallback when metadata is unavailable; don't interrupt login.
                    return (None, false);
                }
                if self.last_update == "error"
                    && let Some(id) = value["@extra"]
                        .as_str()
                        .and_then(|extra| extra.strip_prefix("download:"))
                        .and_then(|id| id.parse::<i32>().ok())
                {
                    self.requested_files.remove(&id);
                    self.mark_file_failed(id);
                    return (None, true);
                }
                // Invalidate only affected layout heights; file and chat updates keep them.
                match self.last_update.as_str() {
                    "messages" | "foundChatMessages" => self.timeline_heights.clear(),
                    "updateMessageContent" | "updateMessageSendFailed" => {
                        let id = value["message_id"]
                            .as_i64()
                            .or_else(|| value["old_message_id"].as_i64());
                        if let Some(id) = id {
                            self.timeline_heights.remove(&id);
                        }
                    }
                    "updateNewMessage" | "updateMessageSendSucceeded" => {
                        if let Some(id) = value["message"]["id"].as_i64() {
                            self.timeline_heights.remove(&id);
                        }
                        if self.timeline_anchor.is_some()
                            && !self.show_search
                            && self.store.message_in_window(&value["message"])
                            && self.last_update == "updateNewMessage"
                        {
                            self.pending_messages = self.pending_messages.saturating_add(1);
                        }
                    }
                    _ => {}
                }
                if matches!(
                    self.last_update.as_str(),
                    "updateMessageContent"
                        | "updateDeleteMessages"
                        | "updateMessageSendSucceeded"
                        | "foundChatMessages"
                ) {
                    self.selection = None;
                    self.drag = None;
                }
                let previous_list = self.store.selected_list;
                if matches!(
                    self.last_update.as_str(),
                    "updateMessageSendSucceeded" | "updateMessageSendFailed"
                ) && value["message"]["chat_id"].as_i64() == self.store.active_chat
                    && let (Some(old), Some(new)) = (
                        value["old_message_id"].as_i64(),
                        value["message"]["id"].as_i64(),
                    )
                {
                    if self.selected_message == Some(old) {
                        self.selected_message = Some(new);
                    }
                    if self.preview_message == Some(old) {
                        self.preview_message = Some(new);
                    }
                    if let Some((id, offset)) = self.timeline_anchor
                        && id == old
                    {
                        self.timeline_anchor = Some((new, offset));
                    }
                }
                let store_changed = self.store.apply(&value) || media_changed;
                if self.last_update == "updateFile"
                    && let Some(id) = value["file"]["id"].as_i64()
                    && self.large_file_pending(id as i32)
                    && !self.cancelling_large_file(id as i32)
                {
                    self.store.set_downloading(id as i32, true);
                }
                let list_request = self.hydrate_positioned_chat(&value);
                let unread_request = if self.last_update == "messages"
                    && value["@extra"]
                        .as_str()
                        .is_some_and(|extra| extra.starts_with("history:"))
                {
                    self.locate_first_unread()
                } else {
                    None
                };
                stickers::on_response(self, &value);
                if self.last_update == "updateChatFolders" {
                    self.folder_offset = 0;
                    if previous_list != self.store.selected_list {
                        self.chat_offset = 0;
                        self.reveal_chat = true;
                    }
                }
                if self.selected_chat.is_none()
                    || (self.selected_chat != self.store.active_chat
                        && !self
                            .store
                            .chat_ids()
                            .any(|id| Some(id) == self.selected_chat))
                {
                    self.selected_chat = self.store.chat_ids().next();
                }
                if self.selected_message.is_none()
                    || !self
                        .active_messages()
                        .iter()
                        .any(|message| Some(message.id) == self.selected_message)
                {
                    self.selected_message = self.active_messages().last().map(|message| message.id);
                }
                let auth_request = self.auth.on_update(&value);
                if value["@extra"].as_str() == Some("refresh-favorites") {
                    return (
                        Some(json!({"@type":"getFavoriteStickers","@extra":"favorite-stickers"})),
                        true,
                    );
                }
                if self.sticker_picker {
                    let request = match self.last_update.as_str() {
                        "updateFavoriteStickers" => Some(
                            json!({"@type":"getFavoriteStickers","@extra":"favorite-stickers"}),
                        ),
                        "updateRecentStickers" if value["is_attached"] != true => {
                            Some(actions::recent_stickers())
                        }
                        "updateInstalledStickerSets"
                            if value.pointer("/sticker_type/@type").and_then(Value::as_str)
                                == Some("stickerTypeRegular") =>
                        {
                            Some(
                                json!({"@type":"getInstalledStickerSets","sticker_type":{"@type":"stickerTypeRegular"},"@extra":"installed-sticker-sets"}),
                            )
                        }
                        _ => None,
                    };
                    if request.is_some() {
                        return (request, true);
                    }
                }
                let auth_changed = matches!(
                    self.last_update.as_str(),
                    "updateAuthorizationState" | "error"
                );
                if self.auth.state == "authorizationStateReady"
                    && self.last_update == "updateAuthorizationState"
                {
                    return (Some(actions::load_chats()), true);
                }
                (
                    unread_request.or(list_request).or(auth_request),
                    !was_ready || store_changed || auth_changed || menu_changed,
                )
            }
        }
    }

    fn move_selected(&mut self, delta: isize) {
        let ids: Vec<_> = self.store.chat_ids().collect();
        if ids.is_empty() {
            return;
        }
        let current = self
            .selected_chat
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0);
        let next = current.saturating_add_signed(delta).min(ids.len() - 1);
        self.selected_chat = Some(ids[next]);
        self.reveal_chat = true;
    }

    fn has_overlay(&self) -> bool {
        self.proxy.open
            || self.link_picker.is_some()
            || self.inline_picker.is_some()
            || self.instant_view.is_some()
            || self.show_topics
            || self.show_help
            || self.show_settings
            || self.show_folders
            || self.quick_switcher.is_some()
            || (self.show_chat_info && self.chat_info_modal)
            || self.action_menu.is_some()
            || self.sticker_picker
            || self.attachments.is_some()
            || self.confirm_delete.is_some()
            || self.forward_message.is_some()
            || self.preview_message.is_some()
    }

    fn close_overlays(&mut self) {
        self.clipboard.cancel();
        self.quick_message.cancel_preparation();
        if self.sticker_picker {
            stickers::close(self);
        }
        if let Some(check) = &mut self.message_check {
            check.cancel_pending();
        }
        self.mouse_press = None;
        self.drag = None;
        self.hit_targets.clear();
        self.proxy.open = false;
        self.proxy.form = None;
        self.link_picker = None;
        self.inline_picker = None;
        self.instant_view = None;
        self.show_topics = false;
        self.show_folders = false;
        self.show_help = false;
        self.show_settings = false;
        self.quick_switcher = None;
        self.clear_global_search();
        self.cancel_batch();
        self.reset_storage_view();
        if self.chat_info_modal {
            self.show_chat_info = false;
        }
        self.settings_focus = None;
        self.action_menu = None;
        self.sticker_picker = false;
        self.attachments = None;
        self.confirm_delete = None;
        self.confirm_delete_many = false;
        if self.forward_message.take().is_some() {
            self.forward_many = false;
            self.selected_chat = self.store.active_chat;
            self.reveal_chat = true;
        }
        self.preview_message = None;
        self.media.last_error = None;
        self.modal_scroll = 0;
    }

    fn toggle_multi_select_message(&mut self, id: i64) {
        if let Some(pos) = self
            .multi_select
            .iter()
            .position(|existing| *existing == id)
        {
            self.multi_select.remove(pos);
        } else if self.multi_select.len() < 100 {
            self.multi_select.push(id);
        }
    }

    fn local_draft_preview(&self, chat_id: i64) -> Option<&str> {
        if self.store.active_chat == Some(chat_id) && !self.draft.is_empty() {
            return Some(&self.draft);
        }
        self.drafts
            .iter()
            .find(|(id, saved)| *id == chat_id && !saved.text.is_empty())
            .map(|(_, saved)| saved.text.as_str())
    }

    fn open_selected(&mut self) -> Vec<Value> {
        let Some(id) = self.selected_chat else {
            return Vec::new();
        };
        let previous = self.store.active_chat;
        if previous == Some(id)
            && self.active_thread.is_none()
            && self.store.message_scope.is_none()
        {
            self.focus_messages = true;
            return Vec::new();
        }
        self.selection = None;
        self.drag = None;
        self.last_click = None;
        self.media.last_error = None;
        let mut requests: Vec<_> = self.flush_draft(Instant::now(), true).into_iter().collect();
        self.park_composer();
        self.clipboard.cancel();
        if previous == Some(id) {
            self.store.set_message_scope(None);
        } else {
            self.store.open(id);
        }
        self.cancel_batch();
        self.clear_reactions();
        self.parity.filter = parity::MediaFilter::All;
        self.quick_message.cancel_preparation();
        self.message_check = None;
        self.restore_draft(id);
        self.selected_message = None;
        self.preview_message = None;
        self.show_search = false;
        self.show_topics = false;
        self.active_thread = None;
        self.reset_chat_requests();
        self.link_picker = None;
        self.inline_picker = None;
        self.instant_view = None;
        self.multi_select.clear();
        self.confirm_delete_many = false;
        self.forward_many = false;
        self.focus_messages = true;
        self.timeline_anchor = None;
        self.quote_back = None;
        self.pending_messages = 0;
        self.visible_media.clear();
        self.history_loading = true;
        self.timeline_heights.clear();
        if let Some(previous) = previous
            && previous != id
        {
            requests
                .push(json!({"@type": "closeChat", "chat_id": previous, "@extra":"close-session"}));
        }
        self.sync_open(id, &mut requests);
        requests
    }

    fn save_draft(&mut self) {
        if let Some(id) = self.store.active_chat {
            self.drafts.retain(|(chat, ..)| *chat != id);
            if !self.draft.is_empty() || self.suspended_input.is_some() {
                self.drafts.push_back((
                    id,
                    SavedDraft {
                        mode: self.input_mode,
                        text: std::mem::take(&mut self.draft),
                        cursor: self.draft_cursor,
                        suspended: self.suspended_input.take(),
                    },
                ));
            }
            while self.drafts.len() > 16
                || self
                    .drafts
                    .iter()
                    .map(|(_, saved)| {
                        saved.text.len()
                            + saved
                                .suspended
                                .as_ref()
                                .map_or(0, |(_, text, _)| text.len())
                    })
                    .sum::<usize>()
                    > 524288
            {
                self.drafts.pop_front();
            }
        }
        self.draft.clear();
        self.input_mode = InputMode::Off;
        self.draft_cursor = usize::MAX;
    }

    fn restore_draft(&mut self, id: i64) {
        if let Some(index) = self.drafts.iter().position(|(chat, ..)| *chat == id) {
            let (_, saved) = self.drafts.remove(index).expect("draft");
            self.input_mode = saved.mode;
            self.draft = saved.text;
            self.draft_cursor = saved.cursor;
            self.suspended_input = saved.suspended;
        }
        self.composer_focus = self.input_mode != InputMode::Off;
    }

    fn active_messages(&self) -> &[Message] {
        if self.show_search {
            &self.store.search_results
        } else {
            &self.store.messages
        }
    }

    fn selected_message(&self) -> Option<&Message> {
        let selected = self.selected_message?;
        self.active_messages()
            .iter()
            .find(|message| message.id == selected)
    }

    fn move_message(&mut self, delta: isize) {
        self.reveal_message = true;
        let messages = self.active_messages();
        if messages.is_empty() {
            return;
        }
        let Some(mut current) = self
            .selected_message
            .and_then(|id| messages.iter().position(|message| message.id == id))
        else {
            let last = store::album_range(messages, messages.len() - 1).start;
            self.selected_message = Some(messages[last].id);
            return;
        };
        for _ in 0..delta.unsigned_abs() {
            let group = store::album_range(messages, current);
            current = if delta < 0 && group.start > 0 {
                store::album_range(messages, group.start - 1).start
            } else if delta > 0 && group.end < messages.len() {
                group.end
            } else {
                break;
            };
        }
        self.selected_message = Some(messages[current].id);
    }

    fn selected_album(&self) -> Option<std::ops::Range<usize>> {
        let messages = self.active_messages();
        let index = messages
            .iter()
            .position(|message| Some(message.id) == self.selected_message)?;
        let range = store::album_range(messages, index);
        (range.len() > 1).then_some(range)
    }

    fn step_album(&mut self, delta: isize) {
        let Some(group) = self.selected_album() else {
            return;
        };
        let messages = &self.active_messages()[group];
        let current = messages
            .iter()
            .position(|message| Some(message.id) == self.selected_message)
            .unwrap();
        let next = current.saturating_add_signed(delta).min(messages.len() - 1);
        self.selected_message = Some(messages[next].id);
        self.selection = None;
        self.reveal_message = true;
    }

    fn submit_input(&mut self) -> Option<Value> {
        let chat_id = self.store.active_chat?;
        let text: String = if self.input_mode == InputMode::Search {
            self.draft.trim().chars().take(256).collect()
        } else {
            self.draft.trim().to_owned()
        };
        if text.is_empty()
            && !(self.input_mode == InputMode::Search
                && self.parity.filter != parity::MediaFilter::All)
        {
            return None;
        }
        if self
            .active_thread
            .as_ref()
            .is_some_and(|thread| thread.discussion_chat.is_none() || thread.thread_id <= 0)
        {
            return None;
        }
        let mut request = match self.input_mode {
            InputMode::Off => return None,
            InputMode::Send => actions::send_text(chat_id, text, None),
            InputMode::Reply(message_id) => actions::send_text(chat_id, text, Some(message_id)),
            InputMode::Edit(message_id) => actions::edit_text(chat_id, message_id, text),
            InputMode::Search => {
                let mut request = actions::search(chat_id, text);
                request["filter"] = json!({"@type":self.parity.filter.tdlib_type()});
                request
            }
            InputMode::React(message_id) => actions::react(chat_id, message_id, text),
        };
        self.scope_outgoing_request(&mut request);
        Some(request)
    }

    fn load_older(&mut self) -> Option<Value> {
        if self.store.history_exhausted || self.history_loading {
            return None;
        }
        let chat = self.store.active_chat?;
        let from = self.store.oldest_message_id()?;
        Some(if self.store.message_scope.is_some() {
            self.scoped_history_request(chat, from)
        } else {
            self.history_request(chat, from)
        })
    }

    fn preview_selected(&mut self) -> Option<Value> {
        let message = self.selected_message()?;
        let media = message.media.as_ref()?;
        let (file_id, path) = media.detail_file();
        let missing = path.is_none();
        self.preview_message = Some(message.id);
        self.selection = None;
        self.preview_view = media::View::default();
        self.drag = None;
        if missing {
            if self.requested_files.len() >= 4 || !self.requested_files.insert(file_id) {
                return None;
            }
            self.failed_files.remove(&file_id);
            return Some(actions::download(file_id));
        }
        None
    }

    fn visible_media_download(&mut self) -> Option<Value> {
        if self.requested_files.len() >= 4 {
            return None;
        }
        let id = self.visible_media.iter().copied().find(|id| {
            *id > 0
                && !self.requested_files.contains(id)
                && !self.failed_files.contains(id)
                && self.active_messages().iter().any(|message| {
                    message
                        .media
                        .as_ref()
                        .is_some_and(|media| media.file_id == *id && media.path.is_none())
                })
        })?;
        self.requested_files
            .insert(id)
            .then(|| actions::download(id))
    }

    fn open_selected_external(&mut self) {
        let Some(path) = self
            .selected_message()
            .and_then(|message| message.media.as_ref())
            .and_then(|media| media.detail_file().1)
        else {
            self.notice = Some("文件尚未下载；先按 v 下载".into());
            return;
        };
        let launcher = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "explorer"
        } else {
            "xdg-open"
        };
        match Command::new(launcher).arg(path).spawn() {
            Ok(_) => self.notice = Some("已请求系统打开文件".into()),
            Err(error) => self.notice = Some(format!("无法打开文件：{error}")),
        }
    }

    fn save_selected_file(&mut self) {
        let Some(message) = self.selected_message() else {
            self.notice = Some("未选中含媒体的消息".into());
            return;
        };
        let Some(path) = message
            .media
            .as_ref()
            .and_then(|media| media.detail_file().1)
            .map(str::to_owned)
        else {
            self.notice = Some("文件尚未下载；先按 v 下载".into());
            return;
        };
        let suggested = message
            .info
            .media_meta
            .as_ref()
            .map(|meta| meta.file_name.as_str())
            .filter(|name| !name.is_empty())
            .map(str::to_owned);
        match export_file_to_downloads(&path, suggested.as_deref()) {
            Ok(dest) => {
                self.notice = Some(format!("已保存到 {}", dest.display()));
                self.notice_error = false;
            }
            Err(error) => {
                self.notice = Some(error);
                self.notice_error = true;
            }
        }
    }

    fn step_preview(&mut self, delta: isize) -> Option<Value> {
        let current = self.preview_message?;
        let media_ids: Vec<i64> = self
            .active_messages()
            .iter()
            .filter(|m| {
                m.media.as_ref().is_some_and(|media| {
                    matches!(
                        media.kind,
                        store::MediaKind::Photo | store::MediaKind::Sticker
                    )
                })
            })
            .map(|m| m.id)
            .collect();
        let pos = media_ids.iter().position(|id| *id == current)?;
        let next = pos.checked_add_signed(delta)?;
        let target = *media_ids.get(next)?;
        self.selected_message = Some(target);
        self.reveal_message = true;
        self.preview_selected()
    }
}

fn export_file_to_downloads(
    source: &str,
    suggested_name: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let source_path = std::path::Path::new(source);
    if !source_path.is_file() {
        return Err("源文件不存在或已被清理".into());
    }
    let downloads = directories::UserDirs::new()
        .and_then(|dirs| dirs.download_dir().map(std::path::Path::to_path_buf))
        .or_else(|| directories::UserDirs::new().map(|dirs| dirs.home_dir().join("Downloads")))
        .ok_or_else(|| "无法确定下载目录".to_owned())?;
    export_file(source_path, suggested_name, &downloads)
}

fn export_file(
    source: &std::path::Path,
    suggested_name: Option<&str>,
    downloads: &std::path::Path,
) -> Result<std::path::PathBuf, String> {
    std::fs::create_dir_all(downloads).map_err(|e| format!("无法创建下载目录：{e}"))?;
    let mut input = std::fs::File::open(source).map_err(|e| format!("无法读取源文件：{e}"))?;
    let raw_name = suggested_name
        .and_then(|name| std::path::Path::new(name).file_name())
        .or_else(|| source.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("teleaf-file");
    let mut safe_name = String::new();
    for ch in raw_name.chars() {
        let ch = if ch.is_control()
            || matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        {
            '_'
        } else {
            ch
        };
        if safe_name.len() + ch.len_utf8() > 180 {
            break;
        }
        safe_name.push(ch);
    }
    let safe_name = safe_name.trim_end_matches(['.', ' ']);
    let safe_name = if safe_name.is_empty() {
        "teleaf-file"
    } else {
        safe_name
    };
    let path = std::path::Path::new(safe_name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("teleaf-file");
    let extension = path.extension().and_then(|ext| ext.to_str());
    for suffix in 0..=999 {
        let candidate = if suffix == 0 {
            safe_name.to_owned()
        } else if let Some(ext) = extension {
            format!("{stem} ({suffix}).{ext}")
        } else {
            format!("{stem} ({suffix})")
        };
        let target = downloads.join(candidate);
        let mut output = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
        {
            Ok(output) => output,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("无法创建目标文件：{error}")),
        };
        if let Err(error) = io::copy(&mut input, &mut output) {
            drop(output);
            let _ = std::fs::remove_file(&target);
            return Err(format!("保存文件失败：{error}"));
        }
        return Ok(target);
    }
    Err("目标目录中同名文件过多".into())
}

fn render(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    let synchronized = terminal::sync_output();
    if synchronized {
        execute!(terminal.backend_mut(), BeginSynchronizedUpdate)?;
    }
    let draw_result = terminal.draw(|frame| ui::draw(frame, app)).map(|_| ());
    let visible = std::mem::take(&mut app.visible_read_ids);
    app.record_viewport(visible, Instant::now());
    let end_result = if synchronized {
        execute!(terminal.backend_mut(), EndSynchronizedUpdate)
    } else {
        Ok(())
    };
    draw_result.and(end_result)
}

fn drain_events(receiver: &Receiver<TdEvent>, app: &mut App, worker: &TdWorker) -> bool {
    let mut changed = false;
    // A busy update stream must not starve keyboard input or drawing.
    for _ in 0..256 {
        match receiver.try_recv() {
            Ok(event) => {
                let was_connected = app.tdlib_connected;
                let was_ready = app.auth.state == "authorizationStateReady";
                let (request, redraw) = app.apply(event);
                if !was_connected && app.tdlib_connected && !app.demo {
                    app.bootstrap_env_proxy(worker);
                }
                app.initialize_proxy_list(worker);
                if !was_ready && app.auth.state == "authorizationStateReady" && !app.demo {
                    for request in app.notifications.requests() {
                        changed |= !send_request(app, worker, request);
                    }
                }
                if let Some(request) = request {
                    changed |= !send_request(app, worker, request);
                }
                changed |= redraw;
                changed |= app.service_navigation(worker);
                app.refresh_visible_chat_info(worker);
                changed |= app.service_batch(worker);
                changed |= menu::complete_pending(app, worker);
            }
            Err(TryRecvError::Empty) => return changed,
            Err(TryRecvError::Disconnected) => return changed,
        }
    }
    changed
}

fn send_request(app: &mut App, worker: &TdWorker, mut request: Value) -> bool {
    if matches!(
        request["@type"].as_str(),
        Some("sendMessage" | "sendMessageAlbum")
    ) && request["chat_id"].as_i64() == app.store.active_chat
    {
        if app
            .active_thread
            .as_ref()
            .is_some_and(|thread| thread.discussion_chat.is_none())
        {
            app.notice = Some("评论串仍在加载，请稍后发送".into());
            app.notice_error = true;
            return false;
        }
        app.scope_outgoing_request(&mut request);
    }
    let extra = &request["@extra"];
    let quick = extra["kind"] == "quick-message";
    let tag = extra.as_str().unwrap_or_default().to_owned();
    if let Err(error) = worker.request(request) {
        app.request_queue_failed(&tag);
        if tag.starts_with("instant-view:") {
            app.receive_instant_view(&json!({"@type":"error","@extra":tag}), &tag);
        }
        if tag.starts_with("proxy-") {
            app.receive_proxy(&json!({"@type":"error","@extra":tag}));
        }
        if quick {
            app.quick_message.request_failed();
        }
        app.notice = Some(error);
        app.notice_error = true;
        false
    } else {
        true
    }
}

fn start_clipboard_paste(app: &mut App) {
    if app.auth.setup.is_some() || app.auth.state != "authorizationStateReady" {
        return;
    }
    if app.store.active_chat.is_none() {
        app.notice = Some("请先打开一个会话".into());
        return;
    }
    if (app.has_overlay() && app.attachments.is_none())
        || !matches!(
            app.input_mode,
            InputMode::Off | InputMode::Send | InputMode::Reply(_)
        )
    {
        app.notice = Some("请在聊天输入区或附件窗口粘贴".into());
        return;
    }
    if app
        .clipboard
        .start(app.store.active_chat.unwrap(), app.input_mode)
    {
        app.notice = Some("正在读取剪贴板…（Esc 取消）".into());
    }
}

fn accept_clipboard(
    app: &mut App,
    chat: i64,
    mode: InputMode,
    result: Result<clipboard::Content, String>,
) {
    if app.auth.setup.is_some()
        || app.auth.state != "authorizationStateReady"
        || app.store.active_chat != Some(chat)
        || app.input_mode != mode
        || (app.has_overlay() && app.attachments.is_none())
    {
        return;
    }
    let result = result.and_then(|content| {
        let (paths, image) = match content {
            clipboard::Content::Text(text) => {
                paste_text(app, &text);
                app.notice = Some("已粘贴剪贴板文字".into());
                return Ok(());
            }
            clipboard::Content::Files(paths) => (paths, None),
            clipboard::Content::Image(image) => (vec![image.path.clone()], Some(image)),
        };
        if let Some(image) = &image
            && !app.clipboard.can_stage(
                app.attachments
                    .as_ref()
                    .map_or(&[], |p| p.clipboard_images.as_slice()),
                image,
            )
        {
            return Err("本次运行的剪贴板暂存已达 256 MiB；请改用已保存的文件".into());
        }
        if app.attachments.is_none() {
            interaction::open_attachments(app, attachments::photo_paths(&paths));
        }
        let picker = app.attachments.as_mut().unwrap();
        picker.add(paths)?;
        if let Some(image) = image {
            picker.clipboard_images.push(image);
        }
        app.notice = Some("已加入附件；确认后按 F8 发送".into());
        Ok(())
    });
    if let Err(error) = result {
        if let Some(picker) = &mut app.attachments {
            picker.error = Some(error.clone());
        }
        app.notice = Some(error);
        app.notice_error = true;
    }
}

fn paste_text(app: &mut App, text: &str) {
    if app.proxy.open {
        if let Some(form) = &mut app.proxy.form {
            let trimmed = text.trim();
            if let Some(parsed) = proxy::parse_proxy_url(trimmed) {
                *form = proxy::ProxyForm::from_parsed(parsed);
                return;
            }
            let field = form.field;
            if let Some((buf, cursor, max_chars)) = form.active_text_mut() {
                let mut clean: String = text
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(max_chars)
                    .collect();
                if field == 2 {
                    clean.retain(|c| c.is_ascii_digit());
                }
                text::insert(buf, cursor, &clean, max_chars * 4);
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
        return;
    }
    if app.show_settings || app.show_help {
        return;
    }
    if app.auth.setup.is_some() || app.auth.state != "authorizationStateReady" {
        app.auth.paste(text);
    } else if let Some(switcher) = &mut app.quick_switcher {
        let clean: String = text.chars().filter(|c| !c.is_control()).take(128).collect();
        text::insert(&mut switcher.query, &mut switcher.cursor, &clean, 128);
        switcher.selected = 0;
        switcher.scroll = 0;
    } else if app.attachments.is_some() {
        if let Some(picker) = &mut app.attachments {
            if picker.focus == attachments::Focus::Caption {
                let clean: String = text
                    .chars()
                    .filter(|c| !c.is_control() || *c == '\n')
                    .take(4096)
                    .collect();
                text::insert(
                    &mut picker.caption,
                    &mut picker.caption_cursor,
                    &clean,
                    4096,
                );
            } else {
                picker.paste_paths(text);
            }
        }
    } else if app.sticker_picker && app.sticker_panel.search_focus {
        let clean: String = text.chars().filter(|c| !c.is_control()).take(256).collect();
        text::insert(
            &mut app.sticker_panel.query,
            &mut app.sticker_panel.query_cursor,
            &clean,
            256,
        );
    } else if !app.has_overlay()
        && matches!(
            app.input_mode,
            InputMode::Off | InputMode::Send | InputMode::Reply(_)
        )
        && app.store.active_chat.is_some()
        && let Some(paths) = attachments::pasted_files(text)
    {
        interaction::open_attachments(app, attachments::photo_paths(&paths));
        if let Some(picker) = &mut app.attachments {
            picker.paste_paths(text);
        }
    } else if app.store.active_chat.is_some() && !app.has_overlay() {
        if app.input_mode == InputMode::Off {
            app.input_mode = InputMode::Send;
            app.composer_focus = true;
        }
        app.composer_focus = true;
        selection::remove_from_draft(app);
        let text: String = text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .take(65536usize.saturating_sub(app.draft.chars().count()))
            .collect();
        if matches!(
            app.input_mode,
            InputMode::Send | InputMode::Reply(_) | InputMode::Edit(_)
        ) {
            text::insert(&mut app.draft, &mut app.draft_cursor, &text, 262144);
        } else {
            text::insert(&mut app.draft, &mut app.draft_cursor, text.trim(), 262144);
        }
    }
}

fn submit_composer(app: &mut App, worker: &TdWorker) -> bool {
    app.selection = None;
    if matches!(app.input_mode, InputMode::Send | InputMode::Reply(_))
        && let Some(paths) = attachments::pasted_files(&app.draft)
    {
        interaction::open_attachments(app, attachments::photo_paths(&paths));
        let picker = app.attachments.as_mut().unwrap();
        picker.consume_path_draft = true;
        if let Err(error) = picker.add(paths) {
            picker.error = Some(error);
        }
        return true;
    }
    if let Some(mut request) = app.submit_input() {
        if app.input_mode == InputMode::Search {
            app.tag_search(&mut request);
        }
        if !send_request(app, worker, request) {
            return false;
        }
        let keep_composer = matches!(app.input_mode, InputMode::Send | InputMode::Reply(_));
        if app.input_mode == InputMode::Search {
            app.search_query = app.draft.trim().chars().take(256).collect();
            app.show_search = true;
            app.store.search_results.clear();
            app.selected_message = None;
            app.timeline_heights.clear();
        }
        app.timeline_anchor = None;
        app.pending_messages = 0;
        interaction::finish_input(app);
        if keep_composer {
            app.reset_draft_snapshot();
        }
        if keep_composer && app.input_mode == InputMode::Off {
            app.input_mode = InputMode::Send;
            app.composer_focus = true;
            app.focus_messages = true;
        }
        true
    } else {
        false
    }
}

fn handle_settings_key(app: &mut App, worker: &TdWorker, code: KeyCode) {
    if app.parity.storage_confirmation {
        match code {
            KeyCode::Enter | KeyCode::Char('y') => app.confirm_storage_cleanup(worker),
            KeyCode::Esc | KeyCode::Char('n') => app.parity.storage_confirmation = false,
            _ => {}
        }
        return;
    }
    let actions = &[
        ui::Action::ApiSetup,
        ui::Action::OpenLink,
        ui::Action::ToggleMouse,
        ui::Action::ToggleNotifications,
    ][..if notifications::supported() { 4 } else { 3 }];
    let page = app.modal_visible.saturating_sub(1).max(1) as u16;
    match code {
        KeyCode::Esc | KeyCode::Char('s') => app.close_overlays(),
        KeyCode::Char('t') => {
            interaction::perform(app, worker, ui::Action::CycleTheme);
        }
        KeyCode::Char('c') => {
            interaction::perform(app, worker, ui::Action::CleanStorage);
        }
        KeyCode::Char('p') => {
            interaction::perform(app, worker, ui::Action::ProxySettings);
        }
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
            let previous = app
                .settings_focus
                .and_then(|action| actions.iter().position(|a| *a == action));
            let backwards = matches!(code, KeyCode::BackTab | KeyCode::Left);
            let index = previous.map_or(if backwards { actions.len() - 1 } else { 0 }, |index| {
                (index + if backwards { actions.len() - 1 } else { 1 }) % actions.len()
            });
            app.settings_focus = Some(actions[index]);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(action) = app.settings_focus {
                interaction::perform(app, worker, action);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.modal_scroll = app.modal_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.modal_scroll = app.modal_scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.modal_scroll = app.modal_scroll.saturating_add(page);
        }
        KeyCode::PageUp => {
            app.modal_scroll = app.modal_scroll.saturating_sub(page);
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut App, code: KeyCode) {
    let page = app.modal_visible.saturating_sub(1).max(1) as u16;
    match code {
        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('s') | KeyCode::F(1) => {
            app.show_settings = false;
            app.show_help = false;
            app.modal_scroll = 0;
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.modal_scroll = app.modal_scroll.saturating_add(1);
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.modal_scroll = app.modal_scroll.saturating_sub(1);
        }
        KeyCode::PageDown => {
            app.modal_scroll = app.modal_scroll.saturating_add(page);
        }
        KeyCode::PageUp => {
            app.modal_scroll = app.modal_scroll.saturating_sub(page);
        }
        _ => {}
    }
}

fn handle_ready_key(app: &mut App, worker: &TdWorker, code: KeyCode) -> bool {
    if code == KeyCode::Esc {
        app.quick_message.cancel_preparation();
        app.clipboard.cancel();
    }
    if code == KeyCode::Esc
        && let Some(check) = &mut app.message_check
    {
        check.cancel_pending();
    }
    app.notice = None;
    app.notice_error = false;
    app.media.last_error = None;
    app.auth.clear_message();
    if code == KeyCode::Esc && !app.has_overlay() && selection::active(app) {
        app.selection = None;
        return false;
    }
    if app.quick_switcher.is_some() {
        let entries = app.quick_entries();
        let page = app.modal_visible.max(1);
        match code {
            KeyCode::Esc => app.close_overlays(),
            KeyCode::Tab | KeyCode::BackTab => {
                let sw = app.quick_switcher.as_mut().unwrap();
                sw.global_messages = !sw.global_messages;
                sw.selected = 0;
                sw.scroll = 0;
                app.reset_quick_search();
                app.clear_global_search();
            }
            KeyCode::Up => {
                if let Some(sw) = &mut app.quick_switcher {
                    sw.selected = sw.selected.saturating_sub(1);
                }
            }
            KeyCode::Down => {
                if let Some(sw) = &mut app.quick_switcher {
                    sw.selected = (sw.selected + 1).min(entries.len().saturating_sub(1));
                }
            }
            KeyCode::PageUp => {
                if let Some(sw) = &mut app.quick_switcher {
                    sw.selected = sw.selected.saturating_sub(page);
                }
            }
            KeyCode::PageDown => {
                if let Some(sw) = &mut app.quick_switcher {
                    sw.selected = (sw.selected + page).min(entries.len().saturating_sub(1));
                }
            }
            KeyCode::Enter => {
                let selected = app
                    .quick_switcher
                    .as_ref()
                    .map_or(0, |sw| sw.selected)
                    .min(entries.len().saturating_sub(1));
                if let Some(&(chat_id, ..)) = entries.get(selected) {
                    if app.global_message_mode() {
                        if let Some((chat, message)) = app.global_hit_key(chat_id as usize) {
                            app.open_global_hit(worker, chat, message);
                        }
                    } else {
                        interaction::perform_target(
                            app,
                            worker,
                            ui::Target::QuickSwitchChat(chat_id),
                        );
                    }
                }
            }
            key => {
                if let Some(sw) = &mut app.quick_switcher
                    && text::edit(&mut sw.query, &mut sw.cursor, key)
                {
                    sw.query.truncate(text::cursor(&sw.query, 128));
                    sw.cursor = sw.cursor.min(sw.query.len());
                    sw.selected = 0;
                    sw.scroll = 0;
                }
            }
        }
        return false;
    }
    if app.show_folders {
        let lists = app.store.chat_lists();
        let page = app.modal_visible.max(1);
        match code {
            KeyCode::Esc => app.close_overlays(),
            KeyCode::Up | KeyCode::Char('k') => {
                app.folder_cursor = app.folder_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.folder_cursor = (app.folder_cursor + 1).min(lists.len().saturating_sub(1));
            }
            KeyCode::PageUp => {
                app.folder_cursor = app.folder_cursor.saturating_sub(page);
            }
            KeyCode::PageDown => {
                app.folder_cursor = (app.folder_cursor + page).min(lists.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                interaction::perform_target(
                    app,
                    worker,
                    ui::Target::Folder(lists[app.folder_cursor.min(lists.len() - 1)].0),
                );
            }
            _ => {}
        }
        return false;
    }
    if !app.has_overlay()
        && matches!(code, KeyCode::Char('[') | KeyCode::Char(']'))
        && !(app.composer_focus && app.input_mode != InputMode::Off)
    {
        if app.focus_messages && app.selected_album().is_some() {
            app.step_album(if code == KeyCode::Char('[') { -1 } else { 1 });
            return false;
        }
        let lists = app.store.chat_lists();
        let index = lists
            .iter()
            .position(|(list, _)| *list == app.store.selected_list)
            .unwrap_or(0);
        let next = index
            .saturating_add_signed(if code == KeyCode::Char('[') { -1 } else { 1 })
            .min(lists.len() - 1);
        interaction::perform_target(app, worker, ui::Target::Folder(lists[next].0));
        return false;
    }
    if app.show_help {
        handle_help_key(app, code);
        return false;
    }
    if app.show_settings {
        handle_settings_key(app, worker, code);
        return false;
    }
    if let Some(index) = app.action_menu {
        let count = menu::items(app).count();
        match code {
            KeyCode::Esc => app.action_menu = None,
            KeyCode::Up | KeyCode::Char('k') => app.action_menu = Some(index.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => {
                app.action_menu = Some((index + 1).min(count.saturating_sub(1)))
            }
            KeyCode::Enter => {
                let action = menu::items(app).nth(index).map(|item| item.action);
                if let Some(action) = action {
                    menu::execute(app, worker, action);
                }
            }
            KeyCode::Char(key) => {
                let action = menu::items(app)
                    .find(|item| item.key == key)
                    .map(|item| item.action);
                if let Some(action) = action {
                    menu::execute(app, worker, action);
                }
            }
            _ => {}
        }
        return false;
    }
    if let Some(reader) = &mut app.instant_view {
        let page = reader.page.saturating_sub(1).max(1) as isize;
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                interaction::perform(app, worker, ui::Action::CloseInstantView);
            }
            KeyCode::Char('o') => {
                interaction::perform(app, worker, ui::Action::OpenInstantViewBrowser);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                reader.scroll_by(-1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                reader.scroll_by(1);
            }
            KeyCode::PageUp => {
                reader.scroll_by(-page);
            }
            KeyCode::PageDown => {
                reader.scroll_by(page);
            }
            KeyCode::Home => {
                reader.set_scroll(0);
            }
            KeyCode::End => {
                reader.set_scroll(usize::MAX);
            }
            _ => {}
        }
        return false;
    }
    if let Some(picker) = &app.inline_picker {
        let message = picker.message;
        let entry = app.active_messages().iter().find(|item| item.id == message);
        let count = entry.map_or(0, |item| item.inline_buttons().count());
        let selected = picker.selected.min(count.saturating_sub(1));
        match code {
            KeyCode::Esc => app.close_overlays(),
            KeyCode::Up | KeyCode::Char('k') => {
                app.inline_picker.as_mut().unwrap().selected = selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.inline_picker.as_mut().unwrap().selected =
                    (selected + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter => {
                if let Some((row, col, _)) =
                    entry.and_then(|item| item.inline_buttons().nth(selected))
                {
                    interaction::perform_target(
                        app,
                        worker,
                        ui::Target::InlineButton(message, row, col),
                    );
                } else {
                    app.close_overlays();
                }
            }
            _ => {}
        }
        return false;
    }
    if let Some(picker) = &mut app.link_picker {
        let len = picker.items.len();
        match code {
            KeyCode::Esc => app.link_picker = None,
            KeyCode::Up | KeyCode::Char('k') => {
                picker.selected = picker.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if len > 0 => {
                picker.selected = (picker.selected + 1).min(len - 1);
            }
            KeyCode::Enter => {
                let idx = picker.selected;
                interaction::perform_target(app, worker, ui::Target::LinkEntry(idx));
            }
            _ => {}
        }
        return false;
    }
    if app.show_topics {
        let count = app
            .store
            .active_chat
            .and_then(|id| app.store.chat(id))
            .map_or(0, |chat| chat.topics.len());
        match code {
            KeyCode::Esc => app.show_topics = false,
            KeyCode::Up | KeyCode::Char('k') => {
                app.topic_cursor = app.topic_cursor.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') if count > 0 => {
                app.topic_cursor = (app.topic_cursor + 1).min(count - 1);
            }
            KeyCode::Enter => {
                let idx = app.topic_cursor;
                interaction::perform_target(app, worker, ui::Target::TopicEntry(idx));
            }
            _ => {}
        }
        return false;
    }
    if app.confirm_delete.is_some() {
        if matches!(code, KeyCode::Char('y') | KeyCode::Enter) {
            interaction::perform(app, worker, ui::Action::Confirm);
        } else if matches!(code, KeyCode::Esc | KeyCode::Char('n')) {
            app.confirm_delete = None;
            app.confirm_delete_many = false;
            app.cancel_batch();
        }
        return false;
    }
    if app.forward_message.is_some() {
        match code {
            KeyCode::Esc => app.close_overlays(),
            KeyCode::Up | KeyCode::Char('k') => app.move_selected(-1),
            KeyCode::Down | KeyCode::Char('j') => app.move_selected(1),
            KeyCode::Enter => {
                interaction::perform(app, worker, ui::Action::Confirm);
            }
            _ => {}
        }
        return false;
    }
    if app.attachments.is_some() {
        if code == KeyCode::F(8) {
            interaction::perform(app, worker, ui::Action::Confirm);
        } else if code == KeyCode::Esc {
            app.attachments = None;
        } else if let Some(picker) = &mut app.attachments {
            picker.key(code);
        }
        return false;
    }
    if app.sticker_picker {
        stickers::key(app, worker, code);
        return false;
    }
    if app.preview_message.is_some() {
        match code {
            KeyCode::Esc | KeyCode::Char('v') => app.preview_message = None,
            KeyCode::Char('o') => app.open_selected_external(),
            KeyCode::Char('E') => {
                interaction::perform(app, worker, ui::Action::SaveFileAs);
            }
            KeyCode::Char('[') => {
                interaction::perform(app, worker, ui::Action::PreviewPrev);
            }
            KeyCode::Char(']') => {
                interaction::perform(app, worker, ui::Action::PreviewNext);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                interaction::perform(app, worker, ui::Action::ZoomIn);
            }
            KeyCode::Char('-') => {
                interaction::perform(app, worker, ui::Action::ZoomOut);
            }
            KeyCode::Char('0') => {
                interaction::perform(app, worker, ui::Action::ZoomReset);
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                interaction::pan_key(app, code);
            }
            _ => {}
        }
        return false;
    }
    if app.input_mode != InputMode::Off && app.composer_focus {
        if matches!(app.input_mode, InputMode::React(_))
            && matches!(code, KeyCode::PageUp | KeyCode::PageDown)
        {
            let pages = app.reaction_choices().len().div_ceil(8);
            app.parity.reaction_page = app
                .parity
                .reaction_page
                .saturating_add_signed(if code == KeyCode::PageUp { -1 } else { 1 })
                .min(pages.saturating_sub(1));
            return false;
        }
        if let InputMode::React(_) = app.input_mode
            && app.draft.is_empty()
            && let KeyCode::Char(c @ '1'..='8') = code
        {
            let index = usize::from(c as u8 - b'1');
            interaction::perform(app, worker, ui::Action::QuickReact(index));
            return false;
        }
        match code {
            KeyCode::Esc => {
                interaction::finish_input(app);
            }
            KeyCode::Tab => {
                app.composer_focus = false;
                app.focus_messages = true;
            }
            KeyCode::Enter => {
                submit_composer(app, worker);
            }
            KeyCode::Up => {
                app.selection = None;
                app.draft_cursor = text::move_vertical(
                    &app.draft,
                    app.draft_cursor,
                    app.composer_width.max(1),
                    -1,
                );
            }
            KeyCode::Down => {
                app.selection = None;
                app.draft_cursor =
                    text::move_vertical(&app.draft, app.draft_cursor, app.composer_width.max(1), 1);
            }
            key => {
                let removed =
                    if matches!(key, KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Delete) {
                        selection::remove_from_draft(app)
                    } else {
                        app.selection = None;
                        false
                    };
                if !(removed && matches!(key, KeyCode::Backspace | KeyCode::Delete)) {
                    text::edit(&mut app.draft, &mut app.draft_cursor, key);
                }
            }
        }
        return false;
    }
    match code {
        KeyCode::Char('q') => return true,
        KeyCode::Char('?') => app.show_help = true,
        KeyCode::Char('s') => app.show_settings = true,
        KeyCode::Char(' ') => {
            if !app.multi_select.is_empty() && app.focus_messages {
                if let Some(id) = app.selected_message {
                    app.toggle_multi_select_message(id);
                }
            } else {
                interaction::perform(app, worker, ui::Action::ChatMenu);
            }
        }
        KeyCode::Esc if !app.multi_select.is_empty() => {
            app.multi_select.clear();
        }
        KeyCode::Esc if app.show_chat_info => {
            app.show_chat_info = false;
        }
        KeyCode::Esc if app.show_search => {
            app.show_search = false;
            app.selected_message = app.store.messages.last().map(|message| message.id);
            app.timeline_anchor = None;
            app.timeline_heights.clear();
        }
        KeyCode::Esc if app.active_thread.is_some() => app.exit_discussion(worker),
        KeyCode::Esc if matches!(app.store.message_scope, Some(store::MessageScope::Forum(_))) => {
            app.exit_forum_topic(worker);
        }
        KeyCode::Esc => {
            if app.focus_messages {
                app.focus_messages = false;
            }
        }
        KeyCode::Tab => app.focus_messages = !app.focus_messages,
        KeyCode::Up | KeyCode::Char('k') => {
            if app.focus_messages {
                app.move_message(-1);
            } else {
                app.move_selected(-1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.focus_messages {
                app.move_message(1);
                if !app.show_search
                    && (app.selected_message == app.store.messages.last().map(|msg| msg.id)
                        || app
                            .selected_album()
                            .is_some_and(|group| group.end == app.store.messages.len()))
                {
                    app.load_newer_near_bottom(worker);
                }
            } else {
                app.move_selected(1);
            }
        }
        KeyCode::Char('o')
            if app.focus_messages
                && app
                    .selected_message()
                    .and_then(|message| message.media.as_ref())
                    .is_some_and(|media| media.kind == store::MediaKind::Photo) =>
        {
            interaction::perform(app, worker, ui::Action::Preview);
        }
        KeyCode::Enter if !app.focus_messages => {
            for request in app.open_selected() {
                send_request(app, worker, request);
            }
        }
        KeyCode::Enter if app.focus_messages && app.show_search && app.multi_select.is_empty() => {
            interaction::perform(app, worker, ui::Action::JumpToMessage);
        }
        KeyCode::Enter | KeyCode::Char('v') if app.focus_messages => {
            if app
                .selected_message()
                .is_some_and(|message| message.media.is_some())
            {
                interaction::perform(app, worker, ui::Action::Preview);
            } else if app.selected_message.is_some() {
                menu::open_message(app, worker);
            }
        }
        KeyCode::PageUp if app.focus_messages => {
            ui::scroll_timeline(app, -5);
        }
        KeyCode::PageDown if app.focus_messages => {
            ui::scroll_timeline(app, 5);
            app.load_newer_near_bottom(worker);
        }
        KeyCode::End if app.focus_messages => {
            interaction::perform(app, worker, ui::Action::Bottom);
        }
        KeyCode::Char(key) => {
            if let Some(action) = ui::action_for_key(key) {
                interaction::perform(app, worker, action);
            }
        }
        _ => {}
    }
    false
}

fn run(demo: bool) -> io::Result<()> {
    let mut terminal = TerminalGuard::new()?;
    let mut worker = if demo {
        TdWorker::spawn_demo()
    } else {
        TdWorker::spawn()
    };
    let mut app = if demo {
        App::with_parts(AuthFlow::empty(), MediaManager::new())
    } else {
        App::new()
    };
    app.demo = demo;
    let mut dirty = true;
    let mut capture = true;
    let mut last_draw = Instant::now() - Duration::from_secs(1);

    'running: loop {
        if dirty {
            app.observe_draft(Instant::now());
        }
        if let Some(request) = app.flush_draft(Instant::now(), false) {
            dirty |= !send_request(&mut app, &worker, request);
        }
        dirty |= app.service_quick_search(&worker);
        dirty |= app.service_global_search(&worker, Instant::now());
        dirty |= app.service_storage(&worker);
        dirty |= app.store.expire_typing(Instant::now());
        dirty |= app.expire_proxy_connection(Instant::now());
        dirty |= drain_events(&worker.events, &mut app, &worker);
        for (sender, request) in app.store.sender_requests() {
            if !send_request(&mut app, &worker, request) {
                app.store.retry_sender(sender);
            }
        }
        if let Some(request) = app.visible_media_download() {
            let file_id = request
                .get("file_id")
                .and_then(Value::as_i64)
                .unwrap_or_default() as i32;
            if !send_request(&mut app, &worker, request) {
                app.requested_files.remove(&file_id);
            }
        }
        stickers::download_visible(&mut app, &worker);
        dirty |= app.media.poll();
        if let Some((chat, mode, result)) = app.clipboard.poll() {
            accept_clipboard(&mut app, chat, mode, result);
            dirty = true;
        }
        if let Some(error) = app.notifications.poll_error() {
            app.notice = Some(error);
            app.notice_error = true;
            if app.auth.state == "authorizationStateReady" && !app.demo {
                for request in app.notifications.requests() {
                    send_request(&mut app, &worker, request);
                }
            }
            dirty = true;
        }
        if app.auth.take_restart() {
            worker.shutdown();
            app = App::new();
            app.mouse_enabled = capture;
            worker = TdWorker::spawn();
            dirty = true;
        }
        if capture != app.mouse_enabled {
            if app.mouse_enabled {
                enable_mouse_capture(terminal.terminal.backend_mut())?;
            } else {
                disable_mouse_capture(terminal.terminal.backend_mut())?;
            }
            capture = app.mouse_enabled;
            app.mouse_press = None;
            app.drag = None;
        }
        if last_draw.elapsed() >= Duration::from_millis(33) {
            dirty |= interaction::tick(&mut app);
        }
        if dirty && last_draw.elapsed() >= Duration::from_millis(33) {
            app.observe_draft(Instant::now());
            render(&mut terminal.terminal, &mut app)?;
            dirty = false;
            last_draw = Instant::now();
        }

        if !dirty {
            dirty |= app.report_viewport(&worker, Instant::now());
        }
        let timeout = if dirty || app.drag.is_some() {
            Duration::from_millis(10)
        } else {
            Duration::from_millis(100)
        };
        let timeout = app.next_sync_deadline(!dirty).map_or(timeout, |deadline| {
            timeout.min(deadline.saturating_duration_since(Instant::now()))
        });
        if event::poll(timeout)? {
            for _ in 0..64 {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        if key.code == KeyCode::Esc {
                            app.clipboard.cancel();
                        }
                        if key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            if app.auth.setup.is_none()
                                && app.auth.state == "authorizationStateReady"
                                && selection::active(&app)
                            {
                                interaction::perform(&mut app, &worker, ui::Action::Copy);
                                dirty = true;
                                last_draw = Instant::now() - Duration::from_secs(1);
                                break;
                            }
                            break 'running;
                        }
                        if key.code == KeyCode::Char('q')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            break 'running;
                        }
                        if key.code == KeyCode::F(2) {
                            let url = if app.auth.setup.is_some() || app.show_settings {
                                "https://my.telegram.org/apps"
                            } else {
                                app.auth
                                    .confirmation_link
                                    .as_deref()
                                    .unwrap_or("https://my.telegram.org/apps")
                            };
                            if let Err(error) = open_url(url) {
                                app.notice = Some(error);
                            }
                        } else if key.code == KeyCode::F(6) {
                            app.mouse_enabled = !app.mouse_enabled;
                        } else if key.code == KeyCode::F(3) && app.demo {
                            app.notice = Some("离线演示无需 API 配置".into());
                        } else if key.code == KeyCode::F(3) {
                            app.close_overlays();
                            app.auth.begin_setup();
                            app.show_settings = false;
                            app.show_help = false;
                        } else if key.code == KeyCode::F(9) {
                            if app.proxy.open {
                                app.proxy.open = false;
                                app.proxy.form = None;
                            } else {
                                app.open_proxy_modal(&worker);
                            }
                        } else if app.proxy.open {
                            app.handle_proxy_key(&worker, key.code);
                        } else if key.code == KeyCode::F(4) {
                            let was_open = app.show_settings;
                            app.close_overlays();
                            app.show_settings = !was_open;
                        } else if key.code == KeyCode::F(1) {
                            let was_open = app.show_help;
                            app.close_overlays();
                            app.show_help = !was_open;
                        } else if app.show_settings {
                            handle_settings_key(&mut app, &worker, key.code);
                        } else if app.show_help {
                            handle_help_key(&mut app, key.code);
                        } else if key.code == KeyCode::Char('k')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.auth.setup.is_none()
                            && app.auth.state == "authorizationStateReady"
                        {
                            interaction::perform(&mut app, &worker, ui::Action::QuickSwitch);
                        } else if key.code == KeyCode::Char('f')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.input_mode == InputMode::Search
                        {
                            interaction::perform(&mut app, &worker, ui::Action::CycleMediaFilter);
                        } else if key.code == KeyCode::Char('w')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            if let Some(sw) = &mut app.quick_switcher {
                                if text::delete_prev_word(&mut sw.query, &mut sw.cursor) {
                                    sw.selected = 0;
                                    sw.scroll = 0;
                                }
                            } else if app.composer_focus
                                && app.input_mode != InputMode::Off
                                && !app.has_overlay()
                            {
                                selection::remove_from_draft(&mut app);
                                text::delete_prev_word(&mut app.draft, &mut app.draft_cursor);
                            }
                        } else if key.code == KeyCode::Char('u')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                        {
                            if app.auth.setup.is_some()
                                || app.auth.state != "authorizationStateReady"
                            {
                                app.auth.clear_input();
                            } else if let Some(sw) = &mut app.quick_switcher {
                                sw.query.clear();
                                sw.cursor = 0;
                                sw.selected = 0;
                                sw.scroll = 0;
                            } else if let Some(picker) = &mut app.attachments {
                                match picker.focus {
                                    attachments::Focus::Path => {
                                        picker.path.clear();
                                        picker.path_cursor = 0;
                                    }
                                    attachments::Focus::Caption => {
                                        picker.caption.clear();
                                        picker.caption_cursor = 0;
                                    }
                                    attachments::Focus::Browser => {}
                                }
                            } else if app.sticker_picker {
                                app.sticker_panel.query.clear();
                                app.sticker_panel.query_cursor = 0;
                            } else {
                                app.draft.clear();
                                app.selection = None;
                                app.draft_cursor = usize::MAX;
                            }
                        } else if key.code == KeyCode::Char('a')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.composer_focus
                            && app.input_mode != InputMode::Off
                            && !app.has_overlay()
                            && app.auth.setup.is_none()
                            && app.auth.state == "authorizationStateReady"
                        {
                            app.selection = Some(selection::Selection {
                                anchor: selection::Point {
                                    source: selection::Source::Composer,
                                    byte: 0,
                                },
                                head: selection::Point {
                                    source: selection::Source::Composer,
                                    byte: app.draft.len(),
                                },
                            });
                        } else if key.code == KeyCode::F(7)
                            || (matches!(key.code, KeyCode::Char('v' | 'V'))
                                && key.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            start_clipboard_paste(&mut app);
                        } else if key.code == KeyCode::Char('o')
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.auth.setup.is_none()
                            && app.auth.state == "authorizationStateReady"
                        {
                            if app.attachments.is_none() {
                                interaction::perform(&mut app, &worker, ui::Action::File);
                            }
                        } else if key.code == KeyCode::Enter
                            && key.modifiers.contains(KeyModifiers::CONTROL)
                            && app.attachments.is_some()
                        {
                            interaction::perform(&mut app, &worker, ui::Action::Confirm);
                        } else if key.modifiers.contains(KeyModifiers::CONTROL) {
                            // Ignore other control combinations rather than inserting their letters.
                        } else if app.auth.setup.is_none()
                            && app.auth.state == "authorizationStateReady"
                        {
                            if key.code == KeyCode::Enter
                                && key.modifiers.contains(KeyModifiers::ALT)
                                && app.attachments.is_some()
                            {
                                if let Some(picker) = &mut app.attachments
                                    && picker.focus == attachments::Focus::Caption
                                {
                                    text::insert(
                                        &mut picker.caption,
                                        &mut picker.caption_cursor,
                                        "\n",
                                        4096,
                                    );
                                }
                            } else if key.code == KeyCode::Enter
                                && key.modifiers.contains(KeyModifiers::ALT)
                                && matches!(
                                    app.input_mode,
                                    InputMode::Send | InputMode::Reply(_) | InputMode::Edit(_)
                                )
                            {
                                selection::remove_from_draft(&mut app);
                                text::insert(&mut app.draft, &mut app.draft_cursor, "\n", 262144);
                            } else if key.code == KeyCode::Backspace
                                && key.modifiers.contains(KeyModifiers::ALT)
                                && app.composer_focus
                                && app.input_mode != InputMode::Off
                                && !app.has_overlay()
                            {
                                selection::remove_from_draft(&mut app);
                                text::delete_prev_word(&mut app.draft, &mut app.draft_cursor);
                            } else if matches!(key.code, KeyCode::Left | KeyCode::Right)
                                && key.modifiers.contains(KeyModifiers::ALT)
                                && app.composer_focus
                                && app.input_mode != InputMode::Off
                                && !app.has_overlay()
                            {
                                app.selection = None;
                                app.draft_cursor = if key.code == KeyCode::Left {
                                    text::prev_word(&app.draft, app.draft_cursor)
                                } else {
                                    text::next_word(&app.draft, app.draft_cursor)
                                };
                            } else if handle_ready_key(&mut app, &worker, key.code) {
                                break 'running;
                            }
                        } else {
                            if key.code == KeyCode::Esc {
                                if !app.auth.cancel_setup() {
                                    break 'running;
                                }
                            } else if key.code == KeyCode::Char('q') && !app.auth.has_input() {
                                break 'running;
                            } else if key.code == KeyCode::Char('s') && !app.auth.has_input() {
                                app.show_settings = true;
                            } else if let Some(request) = app.auth.key(key.code)
                                && let Err(error) = worker.request(request)
                            {
                                app.status = error;
                            }
                        }
                        dirty = true;
                    }
                    Event::Paste(text) => {
                        paste_text(&mut app, &text);
                        dirty = true;
                    }
                    Event::Resize(_, _) => {
                        app.mouse_press = None;
                        app.drag = None;
                        app.last_click = None;
                        app.scrollbars.clear();
                        app.hit_targets.clear();
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        let changed = interaction::mouse(&mut app, &worker, mouse);
                        dirty |= changed;
                        if changed && matches!(mouse.kind, crossterm::event::MouseEventKind::Up(_))
                        {
                            last_draw = Instant::now() - Duration::from_secs(1);
                            break;
                        }
                    }
                    _ => {}
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }

    if let Some(request) = app.flush_draft(Instant::now(), true) {
        send_request(&mut app, &worker, request);
    }
    worker.shutdown();
    Ok(())
}

fn open_url(url: &str) -> Result<(), String> {
    let launcher = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    Command::new(launcher)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法打开浏览器：{error}"))
}

fn main() {
    let demo = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::DismissNotification) => return,
        Ok(cli::Command::TestNotification) => {
            match notifications::test_notification() {
                Ok(()) => {
                    println!("已请求显示 Teleaf 测试通知；请检查系统通知中心和勿扰设置。")
                }
                Err(error) => {
                    eprintln!("Teleaf: {error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Ok(cli::Command::Help) => {
            println!("{}", cli::HELP);
            return;
        }
        Ok(cli::Command::Version) => {
            println!("Teleaf {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(cli::Command::Check) => {
            match tdlib::runtime_info() {
                Ok((version, path)) => println!(
                    "Teleaf {}\nTDLib {version}\n{}",
                    env!("CARGO_PKG_VERSION"),
                    path.display()
                ),
                Err(error) => {
                    eprintln!("Teleaf: {error}");
                    std::process::exit(1);
                }
            }
            return;
        }
        Ok(cli::Command::Run { demo }) => demo,
        Err(error) => {
            eprintln!("Teleaf: {error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(demo) {
        eprintln!("Teleaf: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod interaction_tests {
    #[test]
    fn export_file_uses_distinct_names_without_overwriting() {
        let directory =
            std::env::temp_dir().join(format!("teleaf-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("source");
        let downloads = directory.join("downloads");
        std::fs::write(&source, b"first").unwrap();
        let first = super::export_file(&source, Some("report.pdf"), &downloads).unwrap();
        std::fs::write(&source, b"second").unwrap();
        let second = super::export_file(&source, Some("report.pdf"), &downloads).unwrap();
        assert_eq!(first.file_name().unwrap(), "report.pdf");
        assert_eq!(second.file_name().unwrap(), "report (1).pdf");
        assert_eq!(std::fs::read(first).unwrap(), b"first");
        assert_eq!(std::fs::read(second).unwrap(), b"second");
        std::fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn sending_and_replying_keep_composer_focused_for_the_next_message() {
        for reply in [false, true] {
            let mut app = super::ui::tests::fixture();
            let (worker, requests) = super::TdWorker::test_pair();
            app.input_mode = if reply {
                super::InputMode::Reply(2)
            } else {
                super::InputMode::Send
            };
            app.composer_focus = true;
            app.draft = "第一条".into();
            super::handle_ready_key(&mut app, &worker, super::KeyCode::Enter);
            assert!(app.draft.is_empty());
            assert!(app.input_mode == super::InputMode::Send);
            assert!(app.composer_focus);
            assert!(app.focus_messages);
            assert!(requests.try_recv().is_ok());
            assert!(!super::handle_ready_key(
                &mut app,
                &worker,
                super::KeyCode::Char('q')
            ));
            assert_eq!(app.draft, "q");
            super::interaction::perform(&mut app, &worker, super::ui::Action::Submit);
            let super::tdlib::TdCommand::Request(request) = requests.try_recv().unwrap() else {
                panic!()
            };
            assert_eq!(request["input_message_content"]["text"]["text"], "q");
            assert!(request["reply_to"].is_null());
            assert!(app.composer_focus);
            super::handle_ready_key(&mut app, &worker, super::KeyCode::Esc);
            assert!(app.input_mode == super::InputMode::Off);
        }
    }

    #[test]
    fn switching_chats_cancels_clipboard_even_when_returning_to_the_same_chat() {
        let mut app = super::ui::tests::fixture();
        let image = super::clipboard::test_image();
        let path = image.path.clone();
        app.clipboard = super::clipboard::State::queued(
            1,
            super::InputMode::Off,
            super::clipboard::Content::Image(image),
        );
        app.selected_chat = Some(2);
        app.open_selected();
        app.selected_chat = Some(1);
        app.open_selected();
        assert!(app.clipboard.poll().is_none());
        assert!(app.attachments.is_none());
        assert!(!path.exists());
    }

    #[test]
    fn deleting_selected_search_result_selects_a_remaining_message() {
        let mut app = super::ui::tests::fixture();
        app.show_search = true;
        app.store.search_results = app.store.messages.clone();
        app.selected_message = Some(2);
        app.apply(super::TdEvent::Update(serde_json::json!({
            "@type":"updateDeleteMessages", "chat_id":1, "message_ids":[2]
        })));
        assert_eq!(app.selected_message, Some(1));
        assert_eq!(app.active_messages().len(), 1);
    }

    #[test]
    fn folder_modal_paste_does_not_change_hidden_composer() {
        let mut app = super::ui::tests::fixture();
        app.show_folders = true;
        app.draft = "保留草稿".into();
        app.draft_cursor = 3;
        super::paste_text(&mut app, "不应插入");
        assert_eq!(app.draft, "保留草稿");
        assert_eq!(app.draft_cursor, 3);
        assert!(app.input_mode == super::InputMode::Off);
        assert!(app.attachments.is_none());
    }

    #[test]
    fn settings_keyboard_works_when_mouse_is_disabled_before_and_after_login() {
        use super::*;
        for ready in [false, true] {
            let mut app = ui::tests::fixture();
            let (worker, _) = TdWorker::test_pair();
            if !ready {
                app.auth.state = "authorizationStateWaitPhoneNumber".into();
                app.auth.begin_setup();
            }
            interaction::perform(&mut app, &worker, ui::Action::Settings);
            interaction::perform(&mut app, &worker, ui::Action::ToggleMouse);
            assert!(!app.mouse_enabled);
            handle_settings_key(&mut app, &worker, KeyCode::Tab);
            if notifications::supported() {
                assert_eq!(app.settings_focus, Some(ui::Action::ToggleNotifications));
                handle_settings_key(&mut app, &worker, KeyCode::Tab);
            }
            assert_eq!(app.settings_focus, Some(ui::Action::ApiSetup));
            handle_settings_key(&mut app, &worker, KeyCode::BackTab);
            if notifications::supported() {
                assert_eq!(app.settings_focus, Some(ui::Action::ToggleNotifications));
                handle_settings_key(&mut app, &worker, KeyCode::BackTab);
            }
            assert_eq!(app.settings_focus, Some(ui::Action::ToggleMouse));
            handle_settings_key(&mut app, &worker, KeyCode::Enter);
            assert!(app.mouse_enabled);
            handle_settings_key(&mut app, &worker, KeyCode::Char(' '));
            assert!(!app.mouse_enabled);
            handle_settings_key(&mut app, &worker, KeyCode::Esc);
            assert!(!app.show_settings);
            assert_eq!(app.settings_focus, None);
            if ready {
                handle_ready_key(&mut app, &worker, KeyCode::Char('i'));
                handle_ready_key(&mut app, &worker, KeyCode::Char('好'));
                assert_eq!(app.draft, "好");
            } else {
                app.auth.key(KeyCode::Char('1'));
                assert_eq!(app.auth.setup.as_ref().unwrap().api_id, "1");
            }
        }
    }
    use super::*;
    use tdlib::TdCommand;

    #[test]
    fn clipboard_image_requires_confirmation_preserves_reply_and_cleans_up() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Reply(2);
        app.draft = "保留草稿".into();
        let image = clipboard::test_image();
        let path = image.path.clone();
        accept_clipboard(
            &mut app,
            1,
            InputMode::Reply(2),
            Ok(clipboard::Content::Image(image)),
        );
        assert!(requests.try_recv().is_err(), "paste must not send");
        assert!(path.exists());
        assert_eq!(app.attachments.as_ref().unwrap().reply_to, Some(2));
        assert_eq!(app.draft, "保留草稿");
        interaction::perform(&mut app, &worker, ui::Action::Cancel);
        assert!(!path.exists(), "cancel removes staged pixels");
        let image = clipboard::test_image();
        let path = image.path.clone();
        accept_clipboard(
            &mut app,
            1,
            InputMode::Reply(2),
            Ok(clipboard::Content::Image(image)),
        );
        interaction::perform(&mut app, &worker, ui::Action::Confirm);
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("send request")
        };
        assert_eq!(request["reply_to"]["message_id"], 2);
        assert_eq!(
            request["input_message_content"]["@type"],
            "inputMessagePhoto"
        );
        assert!(app.attachments.is_none());
        assert_eq!(app.draft, "保留草稿");
        assert!(path.exists(), "TDLib can read source after submission");
        drop(app);
        assert!(!path.exists());
    }

    #[test]
    fn stale_clipboard_results_are_discarded_and_plain_text_uses_regular_paste() {
        let mut app = ui::tests::fixture();
        let image = clipboard::test_image();
        let path = image.path.clone();
        accept_clipboard(
            &mut app,
            2,
            InputMode::Off,
            Ok(clipboard::Content::Image(image)),
        );
        assert!(app.attachments.is_none());
        assert!(!path.exists());
        accept_clipboard(
            &mut app,
            1,
            InputMode::Off,
            Ok(clipboard::Content::Text("文字\n👩‍💻".into())),
        );
        assert_eq!(app.draft, "文字\n👩‍💻");
        assert!(app.input_mode == InputMode::Send);
        let image = clipboard::test_image();
        let path = image.path.clone();
        accept_clipboard(
            &mut app,
            1,
            InputMode::Off,
            Ok(clipboard::Content::Image(image)),
        );
        assert!(app.attachments.is_none());
        assert!(
            !path.exists(),
            "reply/input changes invalidate pending paste"
        );
    }

    #[test]
    fn menu_reply_and_delete_confirmation_preserve_requests() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        handle_ready_key(&mut app, &worker, KeyCode::Enter);
        assert_eq!(app.action_menu, Some(0));
        handle_ready_key(&mut app, &worker, KeyCode::Enter);
        assert!(app.input_mode == InputMode::Reply(2));
        app.draft = "回复内容".into();
        handle_ready_key(&mut app, &worker, KeyCode::Enter);
        let TdCommand::Request(request) = requests.try_recv().expect("reply request") else {
            panic!("not a request")
        };
        assert_eq!(request["reply_to"]["message_id"], 2);
        assert_eq!(request["input_message_content"]["text"]["text"], "回复内容");
        handle_ready_key(&mut app, &worker, KeyCode::Esc);
        handle_ready_key(&mut app, &worker, KeyCode::Char('d'));
        handle_ready_key(&mut app, &worker, KeyCode::Esc);
        assert!(requests.try_recv().is_err());
        handle_ready_key(&mut app, &worker, KeyCode::Char('d'));
        handle_ready_key(&mut app, &worker, KeyCode::Enter);
        let TdCommand::Request(request) = requests.try_recv().expect("delete request") else {
            panic!("not a request")
        };
        assert_eq!(request["@type"], "deleteMessages");
    }

    #[test]
    fn opening_chat_focuses_messages_and_escape_returns_to_chats() {
        let mut app = ui::tests::fixture();
        let (worker, _requests) = TdWorker::test_pair();
        app.focus_messages = false;
        app.selected_chat = Some(2);
        handle_ready_key(&mut app, &worker, KeyCode::Enter);
        assert!(app.focus_messages);
        assert_eq!(app.store.active_chat, Some(2));
        assert!(!handle_ready_key(&mut app, &worker, KeyCode::Esc));
        assert!(!app.focus_messages);
    }
    #[test]
    fn ordinary_terminal_path_input_opens_review_and_only_clears_after_submission() {
        let path = std::env::temp_dir().join(format!("tg-input-path-{}.txt", std::process::id()));
        std::fs::write(&path, b"local attachment").unwrap();
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Reply(2);
        app.draft = path.to_string_lossy().into_owned();
        assert!(submit_composer(&mut app, &worker));
        assert!(app.attachments.is_some());
        assert!(requests.try_recv().is_err());
        interaction::perform(&mut app, &worker, ui::Action::Cancel);
        assert_eq!(app.draft, path.to_string_lossy());
        submit_composer(&mut app, &worker);
        interaction::perform(&mut app, &worker, ui::Action::Confirm);
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(
            request["input_message_content"]["@type"],
            "inputMessageDocument"
        );
        assert_eq!(request["reply_to"]["message_id"], 2);
        assert!(app.draft.is_empty());
        assert!(app.attachments.is_none());
        std::fs::remove_file(path).unwrap();
    }
}
