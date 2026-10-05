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
    #[cfg(windows)]
    Clear,
}

pub struct State {
    pub enabled: bool,
    started: i64,
    seen: VecDeque<(i64, i64)>,
    displayed: VecDeque<(i64, i64)>,
    #[cfg(windows)]
    worker: Option<std::sync::mpsc::SyncSender<Command>>,
    #[cfg(windows)]
    errors: Option<std::sync::mpsc::Receiver<String>>,
    #[cfg(windows)]
    account: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            enabled: cfg!(windows) && std::env::var("TG_NOTIFICATIONS").as_deref() != Ok("0"),
            started: timestamp(),
            seen: VecDeque::new(),
            displayed: VecDeque::new(),
            #[cfg(windows)]
            worker: None,
            #[cfg(windows)]
            errors: None,
            #[cfg(windows)]
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
        if !cfg!(windows) {
            return vec![];
        }
        [("notification_group_size_max", 1), ("notification_group_count_max", if self.enabled { 10 } else { 0 })]
            .into_iter().map(|(name, value)| serde_json::json!({
                "@type":"setOption", "name":name, "value":{"@type":"optionValueInteger", "value":value},
                "@extra":"notification-options"
            })).collect()
    }
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled && cfg!(windows);
        self.seen.clear();
        self.displayed.clear();
        self.started = timestamp();
        #[cfg(windows)]
        if self.worker.is_some() {
            self.dispatch(Command::Clear);
        }
    }

    pub fn update(&mut self, update: &Value, store: &Store, ready: bool) {
        if ready {
            #[cfg(windows)]
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

    #[cfg(not(windows))]
    fn dispatch(&mut self, command: Command) {
        // No backend/dependency or helper process is loaded on other platforms.
        match command {
            Command::Show {
                group,
                title,
                body,
                silent,
            } => {
                let _ = (group, title, body, silent);
            }
            Command::Remove(group) => {
                let _ = group;
            }
        }
    }

    #[cfg(windows)]
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
        if let Err(error) = std::thread::Builder::new()
            .name("teleaf-notifications".into())
            .spawn(move || {
                if let Err(error) = native::run(receiver, &account) {
                    let _ = thread_errors
                        .try_send(format!("Windows 通知不可用：{error}；请检查系统通知设置"));
                }
            })
        {
            let _ = errors.try_send(format!("无法启动 Windows 通知线程：{error}"));
            self.errors = Some(error_receiver);
            return;
        }
        let _ = sender.try_send(command);
        self.worker = Some(sender);
        self.errors = Some(error_receiver);
    }

    pub fn poll_error(&mut self) -> Option<String> {
        #[cfg(windows)]
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

pub fn test_notification() -> Result<(), String> {
    #[cfg(windows)]
    {
        let (sender, receiver) = std::sync::mpsc::sync_channel(1);
        sender
            .send(Command::Show {
                group: 1,
                title: "Teleaf 通知测试".into(),
                body: "Windows 原生通知已连接。此测试不登录账号或发送消息。".into(),
                silent: true,
            })
            .map_err(|e| e.to_string())?;
        drop(sender);
        native::run(receiver, "teleaf-test")
    }
    #[cfg(not(windows))]
    Err("原生通知测试目前支持 Windows 10/11".into())
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn mentions_notify_and_windows_options_enable_the_tdlib_api() {
        let (mut state, store, mut update) = fixture();
        update["type"]["@type"] = json!("notificationGroupTypeMentions");
        assert!(matches!(
            &state.commands(&update, &store, 110)[..],
            [Command::Show { .. }]
        ));
        if cfg!(windows) {
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
