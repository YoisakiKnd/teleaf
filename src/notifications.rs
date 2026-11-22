//! TDLib decides which messages notify; the UI never polls chats for alerts.
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::store::Store;

#[derive(Debug)]
enum Command {
    Show {
        group: i64,
        title: String,
        body: String,
        silent: bool,
    },
    Remove(i64),
    Clear,
}

pub struct State {
    pub enabled: bool,
    started: i64,
    seen: VecDeque<(i64, i64)>,
    displayed: VecDeque<(i64, i64)>,
    worker: Option<std::sync::mpsc::SyncSender<Command>>,
    errors: Option<std::sync::mpsc::Receiver<String>>,
    account: String,
    #[cfg(target_os = "linux")]
    history: std::sync::Arc<std::sync::Mutex<native::History>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            started: timestamp(),
            seen: VecDeque::new(),
            displayed: VecDeque::new(),
            worker: None,
            errors: None,
            #[cfg(target_os = "linux")]
            history: Default::default(),
            account: {
                use std::hash::{Hash, Hasher};
                let mut hash = std::collections::hash_map::DefaultHasher::new();
                crate::config::Config::data_dir().ok().hash(&mut hash);
                format!("{:016x}", hash.finish())
            },
        }
    }
}

impl State {
    pub fn requests(&self) -> Vec<Value> {
        if !supported() {
            return vec![];
        }
        [("notification_group_size_max", 1), ("notification_group_count_max", if self.enabled { 10 } else { 0 })]
            .into_iter().map(|(name, value)| serde_json::json!({
                "@type":"setOption", "name":name, "value":{"@type":"optionValueInteger", "value":value},
                "@extra":"notification-options"
            })).collect()
    }
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled && supported();
        self.seen.clear();
        self.displayed.clear();
        self.started = timestamp();
        if self.worker.is_some() {
            self.dispatch(Command::Clear);
        }
    }

    pub fn update(&mut self, update: &Value, store: &Store, ready: bool) {
        if ready {
            if self.enabled && update["@type"] == "updateActiveNotifications" {
                // Clear previous-run toasts instead of replaying the startup backlog.
                self.dispatch(Command::Clear);
                return;
            }
            for command in self.commands(update, store, timestamp()) {
                self.dispatch(command);
            }
        }
    }

    fn commands(&mut self, update: &Value, store: &Store, now: i64) -> Vec<Command> {
        if !self.enabled
            || update["@type"] != "updateNotificationGroup"
            || !matches!(
                update["type"]["@type"].as_str(),
                Some("notificationGroupTypeMessages" | "notificationGroupTypeMentions")
            )
        {
            return vec![];
        }
        let Some(group) = update["notification_group_id"]
            .as_i64()
            .filter(|id| *id > 0)
        else {
            return vec![];
        };
        let Some(chat) = update["chat_id"].as_i64() else {
            return vec![];
        };
        let mut commands = vec![];
        if let Some((_, last)) = self.displayed.iter().find(|(id, _)| *id == group)
            && update["removed_notification_ids"]
                .as_array()
                .is_some_and(|ids| ids.iter().any(|id| id.as_i64() == Some(*last)))
        {
            self.displayed.retain(|(id, _)| *id != group);
            commands.push(Command::Remove(group));
        }
        let is_mention = update["type"]["@type"].as_str() == Some("notificationGroupTypeMentions");
        let total_count = update["total_count"].as_i64().unwrap_or(0).max(0);
        let sound_id = update["notification_sound_id"]
            .as_i64()
            .or_else(|| update["notification_sound_id"].as_str()?.parse().ok())
            .unwrap_or(0);
        let mut latest = None;
        for notification in update["added_notifications"]
            .as_array()
            .into_iter()
            .flatten()
            .rev()
            .take(256)
        {
            let Some(id) = notification["id"].as_i64().filter(|id| *id > 0) else {
                continue;
            };
            if self.seen.contains(&(group, id)) {
                continue;
            }
            self.seen.push_back((group, id));
            if self.seen.len() > 256 {
                self.seen.pop_front();
            }
            let date = notification["date"].as_i64().unwrap_or(0);
            if latest.is_some() || date < self.started || now.saturating_sub(date) > 120 {
                continue;
            }
            let notif_type = &notification["type"];
            let is_silent = notification["is_silent"] == true;
            match notif_type["@type"].as_str() {
                Some("notificationTypeNewMessage") => {
                    let message = &notif_type["message"];
                    if message["is_outgoing"].as_bool() != Some(false) {
                        continue;
                    }
                    latest = Some((id, is_silent, NotificationPayload::Message(message)));
                }
                Some("notificationTypeNewPushMessage") => {
                    if notif_type["is_outgoing"] == true {
                        continue;
                    }
                    latest = Some((id, is_silent, NotificationPayload::Push(notif_type)));
                }
                _ => continue,
            }
        }
        if let Some((id, is_silent, payload)) = latest {
            self.displayed.retain(|(existing, _)| *existing != group);
            self.displayed.push_back((group, id));
            while self.displayed.len() > 128 {
                self.displayed.pop_front();
            }
            let chat_info = store.chat(chat);
            let base_title = store.notification_chat_title(chat).unwrap_or("Telegram");
            let topic_title = match payload {
                NotificationPayload::Message(message) => {
                    chat_info.filter(|c| c.is_forum).and_then(|c| {
                        let topic_id = message
                            .pointer("/topic_id/forum_topic_id")
                            .and_then(Value::as_i64)?;
                        c.topics
                            .iter()
                            .find(|topic| topic.thread_id == topic_id)
                            .map(|topic| topic.title.as_str())
                            .filter(|title| !title.is_empty())
                    })
                }
                NotificationPayload::Push(_) => None,
            };
            let title = format_title(base_title, topic_title, is_mention, total_count);
            let preview_hidden = chat_info.is_some_and(|c| c.notifications.preview_hidden());
            let body = if preview_hidden {
                if total_count > 1 {
                    format!("收到 {total_count} 条新消息")
                } else {
                    "收到一条新消息".into()
                }
            } else {
                match payload {
                    NotificationPayload::Message(message) => {
                        let content = message_body(message);
                        let sender = store.notification_sender(chat, message);
                        format_body(sender.as_deref(), &content)
                    }
                    NotificationPayload::Push(push) => {
                        let content = push_message_body(&push["content"]);
                        let sender = push["sender_name"]
                            .as_str()
                            .map(str::trim)
                            .filter(|s| !s.is_empty() && *s != base_title);
                        format_body(sender, &content)
                    }
                }
            };
            commands.push(Command::Show {
                group,
                title: clean(&title, 80),
                body: clean(&body, 200),
                silent: sound_id == 0 || is_silent,
            });
        }
        commands
    }

    fn dispatch(&mut self, command: Command) {
        use std::sync::mpsc::{self, TrySendError};
        let command = if let Some(worker) = &self.worker {
            match worker.try_send(command) {
                Ok(()) | Err(TrySendError::Full(_)) => return,
                Err(TrySendError::Disconnected(command)) => command,
            }
        } else {
            command
        };
        let (sender, receiver) = mpsc::sync_channel(16);
        let (errors, error_receiver) = mpsc::sync_channel(1);
        let thread_errors = errors.clone();
        let account = self.account.clone();
        #[cfg(target_os = "linux")]
        let history = self.history.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("teleaf-notifications".into())
            .spawn(move || {
                #[cfg(target_os = "linux")]
                let result = native::run(receiver, &account, &history);
                #[cfg(not(target_os = "linux"))]
                let result = native::run(receiver, &account);
                if let Err(error) = result {
                    let _ = thread_errors.try_send(format!("桌面通知不可用：{error}"));
                }
            })
        {
            let _ = errors.try_send(format!("无法启动通知线程：{error}"));
            self.errors = Some(error_receiver);
            return;
        }
        let _ = sender.try_send(command);
        self.worker = Some(sender);
        self.errors = Some(error_receiver);
    }

    pub fn poll_error(&mut self) -> Option<String> {
        if let Some(error) = self
            .errors
            .as_ref()
            .and_then(|errors| errors.try_recv().ok())
        {
            self.enabled = false;
            self.worker = None;
            return Some(error);
        }
        None
    }
}

fn timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub const fn supported() -> bool {
    cfg!(any(windows, target_os = "macos", target_os = "linux"))
}

fn default_enabled() -> bool {
    default_for(
        supported(),
        std::env::var("TG_NOTIFICATIONS").ok().as_deref(),
        std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some(),
        !cfg!(target_os = "linux")
            || std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            || std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some(),
    )
}

fn default_for(supported: bool, preference: Option<&str>, remote: bool, desktop: bool) -> bool {
    supported && preference != Some("0") && (preference == Some("1") || (!remote && desktop))
}

pub fn test_notification() -> Result<(), String> {
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    sender
        .send(Command::Show {
            group: 1,
            title: "Teleaf 通知测试".into(),
            body: "桌面通知已连接。此测试不登录账号或发送消息。".into(),
            silent: true,
        })
        .map_err(|e| e.to_string())?;
    drop(sender);
    #[cfg(target_os = "linux")]
    return native::run(receiver, "teleaf-test", &Default::default());
    #[cfg(not(target_os = "linux"))]
    native::run(receiver, "teleaf-test")
}

enum NotificationPayload<'a> {
    Message(&'a Value),
    Push(&'a Value),
}

fn format_title(base: &str, topic: Option<&str>, is_mention: bool, total_count: i64) -> String {
    let mut title = clean(base, 48);
    if title.is_empty() {
        title = "Telegram".into();
    }
    if let Some(topic) = topic {
        let topic = clean(topic, 24);
        if !topic.is_empty() {
            title.push_str(" · #");
            title.push_str(&topic);
        }
    }
    if is_mention {
        title.push_str(" · @提及");
    }
    if total_count > 1 {
        title.push_str(&format!("（{total_count} 条新消息）"));
    }
    title
}

fn format_body(sender: Option<&str>, content: &str) -> String {
    let content = clean(content, 200);
    let content = if content.is_empty() {
        "新消息".to_owned()
    } else {
        content
    };
    if let Some(sender) = sender {
        let sender = clean(sender, 40);
        if !sender.is_empty() {
            return format!("{sender}：{content}");
        }
    }
    content
}

fn clean(text: &str, limit: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    // Bound the input before scanning graphemes, including pathological combining sequences.
    // Convert newlines/tabs to spaces so multi-line messages remain readable.
    let mut normalized = String::new();
    let mut pending_space = false;
    for ch in text.chars().take(2048) {
        if matches!(ch, '\n' | '\r' | '\t' | ' ') {
            if !normalized.is_empty() {
                pending_space = true;
            }
        } else if !ch.is_control() {
            if pending_space {
                normalized.push(' ');
                pending_space = false;
            }
            normalized.push(ch);
        }
    }
    normalized.graphemes(true).take(limit).collect()
}

fn masked_formatted_text(value: &Value, limit: usize) -> String {
    let raw = value["text"].as_str().unwrap_or_default();
    if raw.is_empty() {
        return String::new();
    }
    let Some(entities) = value["entities"].as_array() else {
        return clean(raw, limit);
    };
    let mut spoiler_ranges: Vec<(usize, usize)> = Vec::new();
    for entity in entities.iter().take(64) {
        if entity.pointer("/type/@type").and_then(Value::as_str) != Some("textEntityTypeSpoiler") {
            continue;
        }
        let offset = entity["offset"].as_u64().unwrap_or(0) as usize;
        let len = entity["length"].as_u64().unwrap_or(0) as usize;
        if len > 0 {
            spoiler_ranges.push((offset, offset.saturating_add(len)));
        }
    }
    if spoiler_ranges.is_empty() {
        return clean(raw, limit);
    }
    spoiler_ranges.sort_unstable();
    let mut out = String::with_capacity(raw.len().min(512));
    let mut utf16_pos = 0usize;
    let mut in_spoiler = false;
    for ch in raw.chars().take(2048) {
        let next_pos = utf16_pos + ch.len_utf16();
        let covered = spoiler_ranges
            .iter()
            .any(|&(start, end)| utf16_pos < end && next_pos > start);
        if covered {
            if !in_spoiler {
                out.push_str("██");
                in_spoiler = true;
            }
        } else {
            in_spoiler = false;
            out.push(ch);
        }
        utf16_pos = next_pos;
    }
    clean(&out, limit)
}

fn format_duration(seconds: i64) -> String {
    let s = seconds.max(0);
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

fn join_label(label: &str, detail: &str) -> String {
    let detail = clean(detail, 160);
    if detail.is_empty() {
        label.to_owned()
    } else {
        format!("{label} {detail}")
    }
}

fn message_body(message: &Value) -> String {
    let content = &message["content"];
    let base = match content["@type"].as_str() {
        Some("messageText") => {
            let text = masked_formatted_text(&content["text"], 200);
            if text.is_empty() {
                "新消息".into()
            } else {
                text
            }
        }
        Some("messagePhoto") => {
            join_label("[图片]", &masked_formatted_text(&content["caption"], 160))
        }
        Some("messageVideo") => {
            let dur = content
                .pointer("/video/duration")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let tag = if dur > 0 {
                format!("[视频 {}]", format_duration(dur))
            } else {
                "[视频]".into()
            };
            join_label(&tag, &masked_formatted_text(&content["caption"], 160))
        }
        Some("messageAnimation") => {
            join_label("[GIF]", &masked_formatted_text(&content["caption"], 160))
        }
        Some("messageVideoNote") => {
            let dur = content
                .pointer("/video_note/duration")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if dur > 0 {
                format!("[视频消息 {}]", format_duration(dur))
            } else {
                "[视频消息]".into()
            }
        }
        Some("messageVoiceNote") => {
            let dur = content
                .pointer("/voice_note/duration")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let tag = if dur > 0 {
                format!("[语音 {}]", format_duration(dur))
            } else {
                "[语音消息]".into()
            };
            join_label(&tag, &masked_formatted_text(&content["caption"], 160))
        }
        Some("messageAudio") => {
            let performer = content
                .pointer("/audio/performer")
                .and_then(Value::as_str)
                .unwrap_or("");
            let title = content
                .pointer("/audio/title")
                .and_then(Value::as_str)
                .unwrap_or("");
            let name = match (performer.trim(), title.trim()) {
                ("", "") => content
                    .pointer("/audio/file_name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                ("", t) => t.to_owned(),
                (p, "") => p.to_owned(),
                (p, t) => format!("{p} - {t}"),
            };
            join_label("[音频]", &name)
        }
        Some("messageDocument") => {
            let file_name = content
                .pointer("/document/file_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let caption = masked_formatted_text(&content["caption"], 120);
            let detail = match (clean(file_name, 80).as_str(), caption.as_str()) {
                ("", "") => String::new(),
                (name, "") => name.to_owned(),
                ("", cap) => cap.to_owned(),
                (name, cap) => format!("{name} · {cap}"),
            };
            join_label("[文件]", &detail)
        }
        Some("messageSticker") => {
            let emoji = content
                .pointer("/sticker/emoji")
                .and_then(Value::as_str)
                .unwrap_or("");
            join_label("[贴纸]", emoji)
        }
        Some("messagePoll") => {
            let question = content
                .pointer("/poll/question/text")
                .or_else(|| content.pointer("/poll/question"))
                .and_then(Value::as_str)
                .unwrap_or("");
            join_label("[投票]", question)
        }
        Some("messageLocation") => "[位置]".into(),
        Some("messageVenue") => join_label(
            "[位置]",
            content
                .pointer("/venue/title")
                .and_then(Value::as_str)
                .unwrap_or(""),
        ),
        Some("messageContact") => {
            let first = content
                .pointer("/contact/first_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            let last = content
                .pointer("/contact/last_name")
                .and_then(Value::as_str)
                .unwrap_or("");
            join_label("[联系人]", &format!("{first} {last}"))
        }
        Some("messageCall") => "[通话]".into(),
        Some("messagePinMessage") => "置顶了一条消息".into(),
        _ => "新消息".into(),
    };
    if message.get("forward_info").is_some_and(Value::is_object) {
        format!("[转发] {base}")
    } else {
        base
    }
}

fn push_message_body(content: &Value) -> String {
    match content["@type"].as_str() {
        Some("pushMessageContentText") => {
            let text = clean(content["text"].as_str().unwrap_or(""), 200);
            if text.is_empty() {
                "新消息".into()
            } else {
                text
            }
        }
        Some("pushMessageContentPhoto") => "[图片]".into(),
        Some("pushMessageContentVideo") => "[视频]".into(),
        Some("pushMessageContentAnimation") => "[GIF]".into(),
        Some("pushMessageContentVideoNote") => "[视频消息]".into(),
        Some("pushMessageContentVoiceNote") => "[语音消息]".into(),
        Some("pushMessageContentAudio") => {
            join_label("[音频]", content["title"].as_str().unwrap_or(""))
        }
        Some("pushMessageContentDocument") => join_label(
            "[文件]",
            content["document"]["file_name"].as_str().unwrap_or(""),
        ),
        Some("pushMessageContentSticker") => {
            join_label("[贴纸]", content["emoji"].as_str().unwrap_or(""))
        }
        Some("pushMessageContentPoll") => {
            join_label("[投票]", content["question"].as_str().unwrap_or(""))
        }
        Some("pushMessageContentLocation") => "[位置]".into(),
        Some("pushMessageContentContact") => {
            join_label("[联系人]", content["name"].as_str().unwrap_or(""))
        }
        _ => "收到一条新消息".into(),
    }
}

#[cfg(windows)]
mod native;
#[cfg(target_os = "linux")]
#[path = "notifications/linux.rs"]
mod native;
#[cfg(target_os = "macos")]
#[path = "notifications/macos.rs"]
mod native;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod native {
    pub(super) fn run(_: std::sync::mpsc::Receiver<super::Command>, _: &str) -> Result<(), String> {
        Err("这个平台暂不支持桌面通知".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn remote_headless_and_explicit_notification_preferences() {
        assert!(default_for(true, None, false, true));
        assert!(!default_for(true, None, true, true));
        assert!(!default_for(true, None, false, false));
        assert!(default_for(true, Some("1"), true, false));
        assert!(!default_for(true, Some("0"), false, true));
        assert!(!default_for(false, Some("1"), false, true));
        let state = State::default();
        assert!(state.worker.is_none() && state.errors.is_none());
    }

    fn fixture() -> (State, Store, Value) {
        let state = State {
            enabled: true,
            started: 100,
            ..State::default()
        };
        let mut store = Store::default();
        store.apply(&json!({"@type":"updateNewChat", "chat":{"id":7,"title":"测试群"}}));
        let update = json!({"@type":"updateNotificationGroup", "notification_group_id":3,
            "type":{"@type":"notificationGroupTypeMessages"}, "chat_id":7,
            "notification_sound_id":0, "removed_notification_ids":[],
            "added_notifications":[{"id":1,"date":105,"type":{
                "@type":"notificationTypeNewMessage", "message":{"is_outgoing":false,
                    "content":{"@type":"messageText","text":{"text":"你好 <&>"}}}}}]});
        (state, store, update)
    }

    #[test]
    fn tdlib_notifications_deduplicate_and_withdraw_read_messages() {
        let (mut state, store, mut update) = fixture();
        let commands = state.commands(&update, &store, 110);
        assert!(
            matches!(&commands[..], [Command::Show { group:3, title, body, silent:true }]
            if title == "测试群" && body == "你好 <&>")
        );
        assert!(state.commands(&update, &store, 110).is_empty());
        update["removed_notification_ids"] = json!([1]);
        update["added_notifications"] = json!([]);
        assert!(matches!(
            &state.commands(&update, &store, 110)[..],
            [Command::Remove(3)]
        ));
        assert!(state.commands(&update, &store, 110).is_empty());
    }

    #[test]
    fn history_outgoing_disabled_and_non_message_updates_do_not_notify() {
        let (state, store, update) = fixture();
        for variant in 0..6 {
            let mut state = State {
                enabled: true,
                started: state.started,
                ..State::default()
            };
            let mut update = update.clone();
            match variant {
                0 => update["added_notifications"][0]["date"] = json!(99),
                1 => {
                    update["added_notifications"][0]["type"]["message"]["is_outgoing"] = json!(true)
                }
                2 => state.enabled = false,
                3 => update["@type"] = json!("updateActiveNotifications"),
                4 => update["type"]["@type"] = json!("notificationGroupTypeCalls"),
                _ => {
                    update["added_notifications"][0]["type"]["@type"] =
                        json!("notificationTypeNewSecretChat")
                }
            }
            assert!(state.commands(&update, &store, 110).is_empty());
        }
        let (mut state, _, update) = fixture();
        assert!(state.commands(&update, &store, 500).is_empty());
    }

    #[test]
    fn mentions_notify_and_options_enable_the_tdlib_api() {
        let (mut state, store, mut update) = fixture();
        update["type"]["@type"] = json!("notificationGroupTypeMentions");
        assert!(matches!(
            &state.commands(&update, &store, 110)[..],
            [Command::Show { .. }]
        ));
        if supported() {
            let requests = state.requests();
            assert_eq!(requests[0]["name"], "notification_group_size_max");
            assert_eq!(requests[0]["value"]["value"], 1);
            assert_eq!(requests[1]["value"]["value"], 10);
            state.enabled = false;
            assert_eq!(state.requests()[1]["value"]["value"], 0);
        } else {
            assert!(state.requests().is_empty());
        }
    }

    #[test]
    fn groups_coalesce_bursts_and_state_text_remain_bounded() {
        let (mut state, store, mut update) = fixture();
        let mut second = update["added_notifications"][0].clone();
        second["id"] = json!(2);
        second["type"]["message"]["content"]["text"]["text"] = json!("最新消息");
        update["added_notifications"]
            .as_array_mut()
            .unwrap()
            .push(second);
        assert!(
            matches!(&state.commands(&update, &store, 110)[..], [Command::Show { body, .. }] if body == "最新消息")
        );
        assert!(state.commands(&update, &store, 110).is_empty());
        for group in 4..1000 {
            update["notification_group_id"] = json!(group);
            let _ = state.commands(&update, &store, 110);
        }
        assert!(state.seen.len() <= 256);
        assert!(state.displayed.len() <= 128);
        assert!(clean(&"界\u{301}".repeat(100_000), 200).chars().count() <= 400);
        assert!(!clean("字\u{1b}\n符", 200).contains('\u{1b}'));
        assert_eq!(clean("第一行\n第二行\t第三列", 200), "第一行 第二行 第三列");
    }

    #[test]
    fn sound_id_string_silent_flag_group_sender_topic_and_mentions_format_clearly() {
        let mut store = Store::default();
        store.apply(
            &json!({"@type":"updateUser","user":{"id":101,"first_name":"小林","last_name":""}}),
        );
        store.apply(&json!({
            "@type": "updateNewChat",
            "chat": {
                "id": 7,
                "title": "技术群",
                "type": {"@type": "chatTypeSupergroup", "supergroup_id": 70},
                "is_forum": true,
                "topics": [{"info": {"message_thread_id": 42, "name": "发布"}, "unread_count": 3}]
            }
        }));
        store.apply(&json!({
            "@type": "updateNewChat",
            "chat": {
                "id": 8,
                "title": "小林",
                "type": {"@type": "chatTypePrivate", "user_id": 101}
            }
        }));
        let mut state = State {
            enabled: true,
            started: 100,
            ..State::default()
        };
        // TDLib encodes int64 notification_sound_id as a JSON string.
        let update = json!({
            "@type": "updateNotificationGroup",
            "notification_group_id": 10,
            "type": {"@type": "notificationGroupTypeMentions"},
            "chat_id": 7,
            "total_count": 3,
            "notification_sound_id": "5123456789",
            "removed_notification_ids": [],
            "added_notifications": [{
                "id": 1,
                "date": 105,
                "is_silent": false,
                "type": {
                    "@type": "notificationTypeNewMessage",
                    "message": {
                        "is_outgoing": false,
                        "sender_id": {"@type": "messageSenderUser", "user_id": 101},
                        "topic_id": {"@type": "messageTopicForum", "forum_topic_id": 42},
                        "content": {
                            "@type": "messageText",
                            "text": {
                                "text": "答案是 秘密内容！",
                                "entities": [{"offset": 4, "length": 4, "type": {"@type": "textEntityTypeSpoiler"}}]
                            }
                        }
                    }
                }
            }]
        });
        let cmds = state.commands(&update, &store, 110);
        assert!(matches!(
            &cmds[..],
            [Command::Show { group: 10, title, body, silent: false }]
                if title == "技术群 · #发布 · @提及（3 条新消息）" && body == "小林：答案是 ██！"
        ));

        // Private chat omits duplicate sender prefix, and is_silent forces silent=true.
        let dm = json!({
            "@type": "updateNotificationGroup",
            "notification_group_id": 11,
            "type": {"@type": "notificationGroupTypeMessages"},
            "chat_id": 8,
            "total_count": 1,
            "notification_sound_id": "5123456789",
            "removed_notification_ids": [],
            "added_notifications": [{
                "id": 2,
                "date": 106,
                "is_silent": true,
                "type": {
                    "@type": "notificationTypeNewMessage",
                    "message": {
                        "is_outgoing": false,
                        "sender_id": {"@type": "messageSenderUser", "user_id": 101},
                        "content": {"@type": "messageText", "text": {"text": "晚安\n明天见"}}
                    }
                }
            }]
        });
        let cmds = state.commands(&dm, &store, 110);
        assert!(matches!(
            &cmds[..],
            [Command::Show { group: 11, title, body, silent: true }]
                if title == "小林" && body == "晚安 明天见"
        ));
    }

    #[test]
    fn privacy_preview_hidden_push_messages_and_rich_media_bodies() {
        let mut store = Store::default();
        store.apply(&json!({
            "@type": "updateNewChat",
            "chat": {
                "id": 9,
                "title": "私密群",
                "notification_settings": {
                    "use_default_show_preview": false,
                    "show_preview": false
                }
            }
        }));
        let mut state = State {
            enabled: true,
            started: 100,
            ..State::default()
        };
        let hidden = json!({
            "@type": "updateNotificationGroup",
            "notification_group_id": 20,
            "type": {"@type": "notificationGroupTypeMessages"},
            "chat_id": 9,
            "total_count": 2,
            "notification_sound_id": "0",
            "removed_notification_ids": [],
            "added_notifications": [{
                "id": 1,
                "date": 105,
                "type": {
                    "@type": "notificationTypeNewMessage",
                    "message": {
                        "is_outgoing": false,
                        "content": {"@type": "messageText", "text": {"text": "绝密正文不应泄露"}}
                    }
                }
            }]
        });
        let cmds = state.commands(&hidden, &store, 110);
        assert!(matches!(
            &cmds[..],
            [Command::Show { body, .. }] if body == "收到 2 条新消息"
        ));

        // Push message notification format.
        store.apply(&json!({"@type":"updateNewChat","chat":{"id":10,"title":"工作群"}}));
        let push = json!({
            "@type": "updateNotificationGroup",
            "notification_group_id": 21,
            "type": {"@type": "notificationGroupTypeMessages"},
            "chat_id": 10,
            "notification_sound_id": "1",
            "removed_notification_ids": [],
            "added_notifications": [{
                "id": 3,
                "date": 107,
                "type": {
                    "@type": "notificationTypeNewPushMessage",
                    "sender_name": "陈同学",
                    "content": {"@type": "pushMessageContentPhoto"}
                }
            }]
        });
        let cmds = state.commands(&push, &store, 110);
        assert!(matches!(
            &cmds[..],
            [Command::Show { title, body, silent: false, .. }]
                if title == "工作群" && body == "陈同学：[图片]"
        ));

        // Rich media previews have no trailing spaces and include duration / forward context.
        assert_eq!(
            message_body(&json!({"content":{"@type":"messagePhoto"}})),
            "[图片]"
        );
        assert_eq!(
            message_body(
                &json!({"content":{"@type":"messageVideo","video":{"duration":75},"caption":{"text":"演示"}}})
            ),
            "[视频 1:15] 演示"
        );
        assert_eq!(
            message_body(
                &json!({"content":{"@type":"messageVoiceNote","voice_note":{"duration":18}}})
            ),
            "[语音 0:18]"
        );
        assert_eq!(
            message_body(
                &json!({"content":{"@type":"messageDocument","document":{"file_name":"report.pdf"}}})
            ),
            "[文件] report.pdf"
        );
        assert_eq!(
            message_body(
                &json!({"content":{"@type":"messagePoll","poll":{"question":{"text":"周几开会？"}}}})
            ),
            "[投票] 周几开会？"
        );
        assert_eq!(
            message_body(&json!({
                "forward_info": {"origin": {"@type": "messageOriginHiddenUser", "sender_name": "某人"}},
                "content": {"@type": "messageText", "text": {"text": "公告"}}
            })),
            "[转发] 公告"
        );
    }
}
