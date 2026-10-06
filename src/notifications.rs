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
            let message = &notification["type"]["message"];
            if latest.is_some()
                || date < self.started
                || now.saturating_sub(date) > 120
                || notification["type"]["@type"] != "notificationTypeNewMessage"
                || message["is_outgoing"].as_bool() != Some(false)
            {
                continue;
            }
            latest = Some((id, message));
        }
        if let Some((id, message)) = latest {
            self.displayed.retain(|(existing, _)| *existing != group);
            self.displayed.push_back((group, id));
            while self.displayed.len() > 128 {
                self.displayed.pop_front();
            }
            let title = store
                .chat(chat)
                .map_or("Telegram", |chat| chat.title.as_str());
            commands.push(Command::Show {
                group,
                title: clean(title, 80),
                body: clean(&message_body(message), 200),
                silent: update["notification_sound_id"].as_i64().unwrap_or(0) == 0,
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

fn clean(text: &str, limit: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    // Bound the input before scanning graphemes, including pathological combining sequences.
    let text: String = text
        .chars()
        .take(2048)
        .filter(|c| !c.is_control())
        .collect();
    text.graphemes(true).take(limit).collect()
}

fn message_body(message: &Value) -> String {
    let content = &message["content"];
    match content["@type"].as_str() {
        Some("messageText") => clean(content["text"]["text"].as_str().unwrap_or("新消息"), 200),
        Some("messagePhoto") => format!(
            "图片 {}",
            clean(content["caption"]["text"].as_str().unwrap_or(""), 160)
        ),
        Some("messageVideo") => "视频".into(),
        Some("messageSticker") => format!(
            "贴纸 {}",
            clean(content["sticker"]["emoji"].as_str().unwrap_or(""), 16)
        ),
        Some("messageDocument") => format!(
            "文件 {}",
            clean(content["document"]["file_name"].as_str().unwrap_or(""), 160)
        ),
        Some("messageVoiceNote") => "语音消息".into(),
        _ => "新消息".into(),
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
    }
}
