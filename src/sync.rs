//! Correlates TDLib responses with the current view and its outstanding requests.
use std::collections::VecDeque;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::store::{ChatList, ChatPeer, MessageScope};
use crate::{App, InputMode, TdWorker, actions, send_request};

type Response = (Option<Value>, bool);

#[derive(Default)]
pub struct Sync {
    generation: u64,
    mention: Option<(Option<i64>, String)>,
    instant_view: Option<(String, String)>,
    callback: Option<String>,
    topics: Option<String>,
    thread_info: Option<String>,
    thread_history: Option<String>,
    navigation: Option<Navigation>,
    quick_query: String,
    quick_generation: u64,
    quick_ids: Vec<i64>,
    quick_fetch: VecDeque<i64>,
    list_hydration: Vec<(i64, u64)>,
    history: Option<String>,
    newer: Option<(String, i64)>,
    search: Option<String>,
    jump: Option<String>,
    pinned: Option<String>,
    chat: Option<String>,
    open_session: Option<String>,
    enter_unread: bool,
    unread_context: Option<(String, i64)>,
    read: ReadSync,
    details_generation: u64,
    details_requested: bool,
    mention_clear: Option<(i64, u64)>,
    presence_request: Option<(i64, u64)>,
    presence_updated: bool,
    summary_peer: Option<ChatPeer>,
    chat_list_load: Option<String>,
    toggles: Vec<ChatToggle>,
    draft: DraftSync,
    media: Vec<FileOperation>,
}

enum Navigation {
    OpenMention(i64),
    OpenDiscussion { chat: i64, root: i64 },
}

#[derive(Default)]
struct ReadSync {
    visible: Vec<i64>,
    deadline: Option<Instant>,
    reported: i64,
    retried: bool,
    pending: Option<(u64, i64)>,
    stable: bool,
    initialized: bool,
    at_bottom: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Toggle {
    Mute,
    Pin(ChatList),
}

struct ChatToggle {
    chat: i64,
    setting: Toggle,
    generation: u64,
    previous: Option<bool>,
}

#[derive(Default)]
struct DraftSync {
    fingerprint: u64,
    deadline: Option<Instant>,
    pending: VecDeque<(i64, u64)>,
    failed: VecDeque<i64>,
    cloud_update_seen: bool,
}

struct FileOperation {
    id: i32,
    generation: u64,
    cancel: bool,
    started: bool,
    previous_download: Option<u64>,
}

fn tag(request: &mut Value, generation: u64) -> String {
    let tag = format!(
        "{}:{generation}",
        request["@extra"].as_str().expect("request tag")
    );
    request["@extra"] = json!(tag);
    tag
}

impl App {
    pub fn next_sync_generation(&mut self) -> u64 {
        self.sync.generation = self.sync.generation.wrapping_add(1);
        self.sync.generation
    }

    pub fn close_quick_switcher(&mut self) {
        self.quick_switcher = None;
        self.clear_global_search();
        self.reset_quick_search();
        self.store.global_chat_results.shrink_to_fit();
        self.sync.quick_ids.shrink_to_fit();
        self.sync.quick_fetch.shrink_to_fit();
        self.sync.quick_query.shrink_to_fit();
    }

    pub fn reset_quick_search(&mut self) {
        self.store.global_chat_results.clear();
        self.sync.quick_ids.clear();
        self.sync.quick_fetch.clear();
        self.sync.quick_query.clear();
        self.sync.quick_generation = self.next_sync_generation();
    }

    pub fn service_quick_search(&mut self, worker: &TdWorker) -> bool {
        let query = self
            .quick_switcher
            .as_ref()
            .filter(|sw| !sw.global_messages)
            .map(|switcher| switcher.query.trim())
            .unwrap_or_default();
        let mut changed = false;
        if query != self.sync.quick_query {
            changed = !self.store.global_chat_results.is_empty();
            let query = query.to_owned();
            self.reset_quick_search();
            self.sync.quick_query = query;
            if !self.sync.quick_query.is_empty() {
                let mut request = actions::search_chats(self.sync.quick_query.clone());
                request["@extra"] =
                    json!(format!("quick-switch:{}:local", self.sync.quick_generation));
                changed |= !send_request(self, worker, request);
            }
        }
        while let Some(id) = self.sync.quick_fetch.pop_front() {
            let request = json!({"@type":"getChat","chat_id":id,
                "@extra":format!("quick-switch:{}:chat:{id}", self.sync.quick_generation)});
            if !send_request(self, worker, request) {
                self.sync.quick_fetch.clear();
                changed = true;
                break;
            }
        }
        changed
    }

    fn receive_quick_search(&mut self, value: &Value, parts: &[&str]) -> Response {
        let generation = parts.first().and_then(|part| part.parse::<u64>().ok());
        if generation != Some(self.sync.quick_generation)
            || !self.quick_switcher.as_ref().is_some_and(|sw| {
                !sw.global_messages
                    && sw.query.trim() == self.sync.quick_query
                    && !self.sync.quick_query.is_empty()
            })
        {
            return (None, false);
        }
        if let [_, "chat", id] = parts
            && let Ok(id) = id.parse::<i64>()
            && self.sync.quick_ids.contains(&id)
        {
            let changed = self.add_quick_result(id, value);
            return (None, changed);
        }
        let mut changed = false;
        if value["@type"] == "chats" {
            for id in value["chat_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_i64)
            {
                if self.sync.quick_ids.len() == 32 {
                    break;
                }
                if !self.sync.quick_ids.contains(&id) {
                    self.sync.quick_ids.push(id);
                    if self.store.chat(id).is_some() {
                        changed |= self.add_quick_result(id, &Value::Null);
                    } else {
                        self.sync.quick_fetch.push_back(id);
                    }
                }
            }
        }
        let next = match parts {
            [_, "local"] => Some(("searchChatsOnServer", "server")),
            [_, "server"] => Some(("searchPublicChats", "public")),
            _ => None,
        };
        let request = next
            .filter(|_| self.sync.quick_ids.len() < 32)
            .map(|(kind, stage)| {
                let mut request = json!({"@type":kind, "query":self.sync.quick_query,
                "@extra":format!("quick-switch:{}:{stage}", self.sync.quick_generation)});
                if stage == "server" {
                    request["limit"] = json!(32 - self.sync.quick_ids.len());
                }
                request
            });
        (request, changed)
    }

    pub fn hydrate_positioned_chat(&mut self, value: &Value) -> Option<Value> {
        let chat = value["chat_id"].as_i64()?;
        if self.store.chat(chat).is_some()
            || self.sync.list_hydration.len() >= 128
            || self.sync.list_hydration.iter().any(|(id, _)| *id == chat)
        {
            return None;
        }
        let nonzero_order = |position: &Value| {
            let order = &position["order"];
            order
                .as_i64()
                .or_else(|| order.as_str()?.parse().ok())
                .is_some_and(|n| n > 0)
        };
        let listed = match value["@type"].as_str()? {
            "updateChatPosition" => nonzero_order(&value["position"]),
            "updateChatLastMessage" | "updateChatDraftMessage" => value["positions"]
                .as_array()
                .is_some_and(|positions| positions.iter().any(nonzero_order)),
            _ => false,
        };
        if !listed {
            return None;
        }
        let generation = self.next_sync_generation();
        self.sync.list_hydration.push((chat, generation));
        Some(json!({"@type":"getChat","chat_id":chat,
            "@extra":format!("list-chat:{chat}:{generation}")}))
    }

    fn add_quick_result(&mut self, id: i64, value: &Value) -> bool {
        if self
            .store
            .global_chat_results
            .iter()
            .any(|item| item.id == id)
        {
            return false;
        }
        let Some(summary) = self.store.remote_chat_summary(id, value) else {
            return false;
        };
        self.store.global_chat_results.push(summary);
        self.store
            .global_chat_results
            .sort_by_key(|item| self.sync.quick_ids.iter().position(|id| *id == item.id));
        true
    }

    pub fn reset_chat_requests(&mut self) {
        self.sync.mention = None;
        self.sync.callback = None;
        self.sync.topics = None;
        self.sync.thread_info = None;
        self.sync.thread_history = None;
        self.sync.navigation = None;
    }

    pub fn search_mention(&mut self, worker: &TdWorker, handle: &str) {
        let mut request = actions::search_public_chat(handle);
        let extra = tag(&mut request, self.next_sync_generation());
        self.sync.mention = Some((self.store.active_chat, extra));
        if send_request(self, worker, request) {
            self.notice = Some(format!("正在查找 {handle}…"));
            self.notice_error = false;
        }
    }

    pub fn request_callback(&mut self, worker: &TdWorker, message: i64, data: &str) -> bool {
        let Some(chat) = self.store.active_chat else {
            return false;
        };
        if self.sync.callback.is_some() {
            return false;
        }
        let mut request = actions::callback_query(chat, message, data);
        self.sync.callback = Some(tag(&mut request, self.next_sync_generation()));
        send_request(self, worker, request)
    }

    pub fn request_forum_topics(&mut self, worker: &TdWorker, chat: i64) {
        let mut request = actions::get_forum_topics(chat);
        self.sync.topics = Some(tag(&mut request, self.next_sync_generation()));
        send_request(self, worker, request);
    }

    pub fn park_composer(&mut self) {
        if self.store.message_scope.is_none() {
            self.save_draft();
        } else {
            self.draft.clear();
            self.suspended_input = None;
            self.input_mode = InputMode::Off;
            self.draft_cursor = usize::MAX;
            self.composer_focus = false;
        }
    }

    pub fn scope_outgoing_request(&self, request: &mut Value) {
        if let Some(scope) = self.store.message_scope
            && matches!(
                request["@type"].as_str(),
                Some(
                    "sendMessage"
                        | "sendMessageAlbum"
                        | "searchChatMessages"
                        | "setChatDraftMessage"
                )
            )
        {
            request["topic_id"] = scope.json();
        }
    }

    fn reset_scoped_timeline(&mut self) {
        self.close_preview();
        self.media.switch_chat();
        self.sync.mention = None;
        self.sync.history = None;
        self.sync.newer = None;
        self.sync.search = None;
        self.sync.jump = None;
        self.cancel_unread_context();
        self.sync.thread_info = None;
        self.sync.thread_history = None;
        self.sync.callback = None;
        self.sync.read = ReadSync::default();
        self.selected_message = None;
        self.timeline_anchor = None;
        self.timeline_heights.clear();
        self.quote_back = None;
        self.pending_messages = 0;
        self.selection = None;
        self.drag = None;
        self.clear_message_search();
        self.multi_select.clear();
        self.message_check = None;
        self.reset_draft_snapshot();
        self.focus_messages = true;
    }

    pub fn scoped_history_request(&mut self, chat: i64, from: i64) -> Value {
        let scope = self.store.message_scope.expect("scoped history");
        let mut request = actions::scoped_history(chat, scope, from);
        self.sync.thread_history = Some(tag(&mut request, self.next_sync_generation()));
        self.history_loading = true;
        request
    }

    pub fn open_forum_topic(&mut self, worker: &TdWorker, chat: i64, topic: i64, title: String) {
        if self.store.active_chat != Some(chat) || self.store.chat(chat).is_none() {
            return;
        }
        if let Some(request) = self.flush_draft(Instant::now(), true) {
            send_request(self, worker, request);
        }
        self.park_composer();
        self.store
            .set_message_scope(Some(MessageScope::Forum(topic)));
        self.store
            .chat_mut_by_id(chat)
            .expect("forum chat")
            .active_topic = Some((topic, title));
        self.reset_scoped_timeline();
        let request = self.scoped_history_request(chat, 0);
        send_request(self, worker, request);
    }

    pub fn exit_forum_topic(&mut self, worker: &TdWorker) {
        let Some(chat) = self.store.active_chat else {
            return;
        };
        if !matches!(self.store.message_scope, Some(MessageScope::Forum(_))) {
            return;
        }
        if let Some(request) = self.flush_draft(Instant::now(), true) {
            send_request(self, worker, request);
        }
        self.park_composer();
        self.store.set_message_scope(None);
        self.restore_draft(chat);
        self.reset_scoped_timeline();
        let request = self.history_request(chat, 0);
        send_request(self, worker, request);
    }

    pub fn open_discussion(&mut self, worker: &TdWorker, post_id: i64) -> bool {
        let Some(source_chat) = self.store.active_chat else {
            return false;
        };
        if self
            .active_thread
            .as_ref()
            .is_some_and(|thread| thread.discussion_chat.is_some())
        {
            return false;
        }
        let Some(post) = self
            .active_messages()
            .iter()
            .find(|message| message.id == post_id)
        else {
            return false;
        };
        let title: String = post.text.chars().take(48).collect();
        self.active_thread = Some(crate::ThreadView {
            source_chat,
            post_id,
            thread_id: post.info.thread_id,
            discussion_chat: None,
            title: if title.is_empty() {
                "评论串".into()
            } else {
                title
            },
        });
        let mut request = actions::get_message_thread(source_chat, post_id);
        self.sync.thread_info = Some(tag(&mut request, self.next_sync_generation()));
        if send_request(self, worker, request) {
            self.notice = Some("正在加载评论串…".into());
            self.notice_error = false;
        }
        true
    }

    pub fn exit_discussion(&mut self, worker: &TdWorker) {
        let Some(thread) = self.active_thread.clone() else {
            return;
        };
        self.sync.thread_info = None;
        self.sync.thread_history = None;
        self.sync.navigation = None;
        if thread.discussion_chat.is_none() {
            self.active_thread = None;
            return;
        }
        self.selected_chat = Some(thread.source_chat);
        for request in self.open_selected() {
            if request["@type"] != "getChatHistory" {
                send_request(self, worker, request);
            }
        }
        self.jump_to(worker, thread.post_id);
    }

    pub fn service_navigation(&mut self, worker: &TdWorker) -> bool {
        match self.sync.navigation.take() {
            Some(Navigation::OpenMention(chat)) => {
                self.close_overlays();
                self.selected_chat = Some(chat);
                for request in self.open_selected() {
                    send_request(self, worker, request);
                }
                true
            }
            Some(Navigation::OpenDiscussion { chat, root }) => {
                let Some(thread) = self.active_thread.as_ref().filter(|thread| {
                    Some(thread.source_chat) == self.store.active_chat
                        && thread.discussion_chat.is_none()
                }) else {
                    return false;
                };
                let source = thread.source_chat;
                if let Some(request) = self.flush_draft(Instant::now(), true) {
                    send_request(self, worker, request);
                }
                self.park_composer();
                self.close_overlays();
                self.reset_chat_requests();
                self.sync.pinned = None;
                self.sync.chat = None;
                self.sync.open_session = None;
                self.sync.details_requested = false;
                self.sync.details_generation = self.next_sync_generation();
                self.store.open(chat);
                self.store
                    .set_message_scope(Some(MessageScope::Discussion(root)));
                self.reset_scoped_timeline();
                if let Some(thread) = &mut self.active_thread {
                    thread.discussion_chat = Some(chat);
                    thread.thread_id = root;
                }
                if source != chat {
                    send_request(
                        self,
                        worker,
                        json!({"@type":"closeChat","chat_id":source,"@extra":"close-session"}),
                    );
                    let mut open = json!({"@type":"openChat","chat_id":chat,"@extra":format!("open-session:{chat}")});
                    self.sync.open_session = Some(tag(&mut open, self.next_sync_generation()));
                    send_request(self, worker, open);
                }
                let request = self.scoped_history_request(chat, 0);
                send_request(self, worker, request);
                true
            }
            None => false,
        }
    }

    fn receive_chat_feature(&mut self, value: &Value, extra: &str) -> Option<Response> {
        let mut parts = extra.split(':');
        let route = parts.next()?;
        let kind = value["@type"].as_str().unwrap_or_default();
        if route == "mention" {
            let Some((source, _)) = self.sync.mention.take_if(|(_, pending)| pending == extra)
            else {
                return Some((None, false));
            };
            if source != self.store.active_chat {
                return Some((None, false));
            }
            if kind == "chat"
                && let Some(chat) = value["id"].as_i64().filter(|id| *id != 0)
            {
                if self.store.chat(chat).is_none() {
                    self.store.add_chat(value);
                } else {
                    self.store.merge_open_chat(chat, value, true);
                }
                self.sync.navigation = Some(Navigation::OpenMention(chat));
                self.notice = Some("已找到提及的会话".into());
                self.notice_error = false;
            } else {
                self.sync_error(value, "找不到提及的会话");
            }
            return Some((None, true));
        }
        if !matches!(
            route,
            "callback" | "topics" | "thread-info" | "thread-history"
        ) {
            return None;
        }
        let chat = parts.next().and_then(|id| id.parse::<i64>().ok());
        if chat.is_none() || chat != self.store.active_chat {
            return Some((None, false));
        }
        let pending = match route {
            "callback" => &mut self.sync.callback,
            "topics" => &mut self.sync.topics,
            "thread-info" => &mut self.sync.thread_info,
            _ => &mut self.sync.thread_history,
        };
        if pending.as_deref() != Some(extra) {
            return Some((None, false));
        }
        *pending = None;
        let chat = chat.expect("active chat");
        match route {
            "callback" if kind == "callbackQueryAnswer" => {
                let text = value["text"].as_str().unwrap_or_default();
                let text: String = text
                    .chars()
                    .map(|ch| if ch.is_control() { ' ' } else { ch })
                    .take(160)
                    .collect();
                let url = value["url"].as_str().filter(|url| valid_callback_url(url));
                let alert = value["show_alert"] == true;
                let prefix = if alert { "Bot 提醒：" } else { "" };
                self.notice = Some(match (text.trim(), url) {
                    ("", None) => "Bot 已响应".into(),
                    ("", Some(url)) => format!("{prefix}Bot 返回链接：{url}"),
                    (text, None) => format!("{prefix}{text}"),
                    (text, Some(url)) => format!("{prefix}{text} · 链接：{url}"),
                });
                self.notice_error = alert;
            }
            "callback" => self.sync_error(value, "Bot 按钮操作失败"),
            "topics" if !self.show_topics => return Some((None, false)),
            "topics" if kind == "forumTopics" => {
                self.store.update_topics(chat, value);
                self.topic_cursor = 0;
            }
            "topics" => self.sync_error(value, "无法读取话题列表"),
            "thread-info" => {
                let post = parts.next().and_then(|post| post.parse::<i64>().ok());
                if !self.active_thread.as_ref().is_some_and(|thread| {
                    thread.source_chat == chat
                        && Some(thread.post_id) == post
                        && thread.discussion_chat.is_none()
                }) {
                    return Some((None, false));
                }
                if kind == "messageThreadInfo"
                    && let (Some(discussion), Some(root)) = (
                        value["chat_id"].as_i64().filter(|id| *id != 0),
                        value["message_thread_id"].as_i64().filter(|id| *id > 0),
                    )
                {
                    self.sync.navigation = Some(Navigation::OpenDiscussion {
                        chat: discussion,
                        root,
                    });
                } else {
                    self.active_thread = None;
                    self.sync_error(value, "无法打开评论串");
                }
            }
            "thread-history" => {
                self.history_loading = false;
                if kind == "messages"
                    && let Some(messages) = value["messages"].as_array()
                {
                    self.store.merge_scoped_history(messages);
                    self.timeline_heights.clear();
                    if self
                        .selected_message
                        .is_none_or(|id| !self.store.messages.iter().any(|msg| msg.id == id))
                    {
                        self.selected_message =
                            self.store.messages.last().map(|message| message.id);
                    }
                    self.notice = None;
                    self.notice_error = false;
                } else {
                    self.sync_error(value, "无法加载话题或评论历史");
                }
            }
            _ => unreachable!(),
        }
        Some((None, true))
    }

    pub fn sync_open(&mut self, id: i64, requests: &mut Vec<Value>) {
        self.sync.history = None;
        self.sync.newer = None;
        self.sync.search = None;
        self.sync.jump = None;
        self.sync.pinned = None;
        self.sync.chat = None;
        self.sync.open_session = None;
        let mut open =
            json!({"@type":"openChat", "chat_id":id, "@extra":format!("open-session:{id}")});
        self.sync.open_session = Some(tag(&mut open, self.next_sync_generation()));
        requests.push(open);
        self.sync.read = ReadSync::default();
        self.sync.enter_unread = !self.demo;
        self.sync.unread_context = None;
        self.sync.draft.cloud_update_seen = false;
        self.sync.details_generation = self.next_sync_generation();
        self.sync.details_requested = false;
        self.sync.summary_peer = None;
        self.reset_draft_snapshot();
        let mut request =
            json!({"@type":"getChat","chat_id":id,"@extra":format!("open-chat:{id}")});
        self.sync.chat = Some(tag(&mut request, self.next_sync_generation()));
        if !self.demo || self.store.chat(id).is_none() {
            requests.push(request);
        }
        if !self.demo {
            requests.push(self.pinned_request(id));
        }
        if self.show_chat_info {
            requests.extend(self.chat_info_requests());
        }
        requests.push(self.history_request(id, 0));
    }

    pub fn history_request(&mut self, chat: i64, from: i64) -> Value {
        let mut request = actions::history(chat, from);
        self.sync.history = Some(tag(&mut request, self.next_sync_generation()));
        self.history_loading = true;
        request
    }

    pub fn load_newer_near_bottom(&mut self, worker: &TdWorker) -> bool {
        if !self.store.context_window
            || self.show_search
            || self.history_loading
            || self.sync.jump.is_some()
            || self.timeline_max.saturating_sub(self.timeline_top) > 3
        {
            return false;
        }
        let (Some(chat), Some(from)) = (
            self.store.active_chat,
            self.store.messages.last().map(|m| m.id),
        ) else {
            return false;
        };
        let mut request = self.store.message_scope.map_or_else(
            || actions::history(chat, from),
            |scope| actions::scoped_history(chat, scope, from),
        );
        request["offset"] = json!(-50);
        request["@extra"] = json!(format!("newer:{chat}"));
        let tag = tag(&mut request, self.next_sync_generation());
        if send_request(self, worker, request) {
            self.sync.newer = Some((tag, from));
            self.history_loading = true;
        }
        true
    }

    fn receive_newer(&mut self, value: &Value, extra: &str) -> Response {
        let Some((_, from)) = self.sync.newer.take_if(|(pending, _)| pending == extra) else {
            return (None, false);
        };
        self.history_loading = false;
        if let Some(messages) = value["messages"].as_array() {
            let advanced = self.store.extend_context(messages, from);
            if advanced {
                if self.timeline_anchor.is_none() {
                    self.timeline_anchor = Some((from, 0));
                }
                self.timeline_heights.clear();
                if self
                    .selected_message
                    .is_none_or(|id| !self.store.messages.iter().any(|msg| msg.id == id))
                {
                    self.selected_message = self.store.messages.first().map(|msg| msg.id);
                }
            }
        } else if value["@type"] == "error" {
            self.sync_error(value, "无法加载较新消息");
        }
        (None, true)
    }

    pub fn tag_search(&mut self, request: &mut Value) {
        self.sync.search = Some(tag(request, self.next_sync_generation()));
        self.sync.jump = None;
        if self.sync.newer.take().is_some() {
            self.history_loading = false;
        }
        self.cancel_unread_context();
    }

    pub fn clear_message_search(&mut self) {
        self.show_search = false;
        self.store.search_results = Vec::new();
        self.search_query = String::new();
        self.sync.search = None;
    }

    pub fn jump_to(&mut self, worker: &TdWorker, target: i64) -> bool {
        let Some(chat) = self.store.active_chat else {
            return false;
        };
        self.clear_message_search();
        self.sync.jump = None;
        if self.sync.newer.take().is_some() {
            self.history_loading = false;
        }
        self.cancel_unread_context();
        self.selection = None;
        self.focus_messages = true;
        self.composer_focus = false;
        if self
            .store
            .messages
            .iter()
            .any(|message| message.id == target)
        {
            self.reveal_jump(target);
        } else {
            let mut request = actions::jump_to_message(chat, target);
            if let Some(scope) = self.store.message_scope {
                let extra = request["@extra"].clone();
                request = actions::scoped_history(chat, scope, target);
                request["offset"] = json!(-20);
                request["@extra"] = extra;
            }
            self.sync.history = None;
            self.sync.thread_history = None;
            self.history_loading = false;
            self.sync.jump = Some(tag(&mut request, self.next_sync_generation()));
            send_request(self, worker, request);
        }
        true
    }

    fn reveal_jump(&mut self, target: i64) {
        self.selected_message = Some(target);
        self.reveal_message = true;
        self.timeline_heights.clear();
    }

    pub fn return_to_latest(&mut self, worker: &TdWorker) {
        self.sync.jump = None;
        if self.sync.newer.take().is_some() {
            self.history_loading = false;
        }
        self.cancel_unread_context();
        if self.store.context_window
            && let Some(chat) = self.store.active_chat
        {
            self.store.context_window = false;
            self.store.history_exhausted = false;
            self.store.messages.clear();
            self.timeline_heights.clear();
            let request = if self.store.message_scope.is_some() {
                self.scoped_history_request(chat, 0)
            } else {
                self.history_request(chat, 0)
            };
            send_request(self, worker, request);
        }
    }

    fn pinned_request(&mut self, chat: i64) -> Value {
        let mut request = actions::search(chat, String::new());
        request["limit"] = json!(1);
        request["filter"] = json!({"@type":"searchMessagesFilterPinned"});
        request["@extra"] = json!(format!("pinned:{chat}"));
        self.sync.pinned = Some(tag(&mut request, self.next_sync_generation()));
        request
    }

    fn receive_jump(&mut self, value: &Value, chat: i64, target: i64) -> Response {
        self.sync.jump = None;
        let found = match value["@type"].as_str() {
            Some("messages") => value["messages"]
                .as_array()
                .is_some_and(|messages| self.store.replace_context(messages, target)),
            Some("message") => self
                .store
                .replace_context(std::slice::from_ref(value), target),
            _ => false,
        };
        if found {
            self.sync.history = None;
            self.history_loading = false;
            self.timeline_anchor = Some((target, 0));
            self.reveal_jump(target);
            return (None, true);
        }
        if value["@type"] == "messages" {
            // History may omit the anchor. Ask for the exact message once.
            let mut request = json!({"@type":"getMessage","chat_id":chat,"message_id":target,
                "@extra":format!("jump:{chat}:{target}")});
            self.sync.jump = Some(tag(&mut request, self.next_sync_generation()));
            return (Some(request), false);
        }
        self.sync_error(value, "无法定位消息");
        (None, true)
    }

    pub fn sync_error(&mut self, value: &Value, label: &str) {
        let reason: String = value["message"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(160)
            .collect();
        self.notice = Some(if reason.is_empty() {
            label.to_owned()
        } else {
            format!("{label}：{reason}")
        });
        self.notice_error = true;
    }

    fn pending_instant_view(&mut self) -> Option<Value> {
        if self.sync.instant_view.is_some() {
            return None;
        }
        let reader = self.instant_view.as_ref()?;
        if !matches!(reader.state, crate::instant_view::State::Loading) {
            return None;
        }
        let url = reader.url.clone();
        let mut request = actions::get_instant_view(&url);
        let extra = tag(&mut request, self.next_sync_generation());
        self.sync.instant_view = Some((extra, url));
        Some(request)
    }

    pub fn open_instant_view(&mut self, worker: &TdWorker, url: String) -> bool {
        if !crate::text::web_url(&url) {
            return false;
        }
        self.close_overlays();
        self.notice = None;
        self.notice_error = false;
        self.instant_view = Some(crate::instant_view::Reader::new(url));
        if let Some(request) = self.pending_instant_view() {
            send_request(self, worker, request);
        }
        true
    }

    pub fn fail_instant_view(&mut self) {
        self.sync.instant_view = None;
        if let Some(reader) = &mut self.instant_view
            && matches!(reader.state, crate::instant_view::State::Loading)
        {
            reader.state = crate::instant_view::State::Failed;
        }
    }

    pub fn receive_instant_view(&mut self, value: &Value, extra: &str) -> Response {
        use crate::instant_view::{InstantViewArticle, State};

        if !self
            .sync
            .instant_view
            .as_ref()
            .is_some_and(|(tag, _)| tag == extra)
        {
            return (None, false);
        }
        let (_, url) = self.sync.instant_view.take().unwrap();
        let Some(reader) = self.instant_view.as_mut() else {
            return (None, false);
        };
        if reader.url != url {
            return (self.pending_instant_view(), false);
        }
        reader.state = InstantViewArticle::parse(value).map_or(State::Failed, State::Ready);
        (None, true)
    }

    pub fn receive_sync(&mut self, value: &Value) -> Option<Response> {
        let extra = value["@extra"].as_str().unwrap_or_default();
        if extra.starts_with("instant-view:") {
            return Some(self.receive_instant_view(value, extra));
        }
        if let Some(response) = self.receive_chat_feature(value, extra) {
            return Some(response);
        }
        let mut fields = extra.split(':');
        let route = fields.next().unwrap_or_default();
        if route == "close-session" {
            return Some((None, false));
        }
        if route == "quick-switch" {
            let mut parts = [""; 3];
            let mut count = 0;
            for part in fields {
                if count == parts.len() {
                    return Some((None, false));
                }
                parts[count] = part;
                count += 1;
            }
            return Some(self.receive_quick_search(value, &parts[..count]));
        }
        if matches!(
            route,
            "history"
                | "search"
                | "jump"
                | "pinned"
                | "open-chat"
                | "open-session"
                | "unread"
                | "newer"
        ) {
            let chat = fields.next().and_then(|id| id.parse::<i64>().ok());
            if chat != self.store.active_chat || chat.is_none() {
                return Some((None, false));
            }
            let chat = chat.expect("active chat");
            if route == "newer" {
                return Some(self.receive_newer(value, extra));
            }
            if route == "unread" {
                return Some(self.receive_unread_context(value, extra, chat));
            }
            let pending = match route {
                "history" => &mut self.sync.history,
                "search" => &mut self.sync.search,
                "jump" => &mut self.sync.jump,
                "pinned" => &mut self.sync.pinned,
                "open-session" => &mut self.sync.open_session,
                _ => &mut self.sync.chat,
            };
            if pending.as_deref() != Some(extra) {
                return Some((None, false));
            }
            *pending = None;
            match route {
                "jump" => {
                    let target = fields.next().and_then(|id| id.parse::<i64>().ok())?;
                    return Some(self.receive_jump(value, chat, target));
                }
                "history" => self.history_loading = false,
                "search" if !self.show_search => return Some((None, false)),
                "pinned" => {
                    let changed = matches!(
                        value["@type"].as_str(),
                        Some("foundChatMessages" | "messages")
                    ) && self.store.set_pin_banner(
                        chat,
                        value["messages"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .find(|msg| msg["chat_id"].as_i64() == Some(chat)),
                    );
                    return Some((None, changed));
                }
                "open-session" => {
                    let failed = value["@type"] == "error";
                    if failed {
                        self.sync_error(value, "无法打开会话");
                    }
                    return Some((None, failed));
                }
                "open-chat" => {
                    if value["@type"] == "chat" && value["id"].as_i64() == Some(chat) {
                        let old_peer = self.store.chat(chat).and_then(|chat| chat.peer);
                        self.store
                            .merge_open_chat(chat, value, !self.sync.draft.cloud_update_seen);
                        if old_peer != self.store.chat(chat).and_then(|chat| chat.peer) {
                            self.sync.details_generation = self.next_sync_generation();
                            self.sync.details_requested = false;
                        }
                        if !self.sync.draft.cloud_update_seen
                            && value.get("draft_message").is_some()
                        {
                            self.accept_cloud_draft(chat, &value["draft_message"]);
                        }
                        return Some((None, true));
                    }
                    return Some((None, false));
                }
                _ => {}
            }
            if value["@type"] == "error" {
                if route == "history" {
                    self.sync.enter_unread = false;
                }
                self.sync_error(
                    value,
                    if route == "history" {
                        "无法加载聊天记录"
                    } else {
                        "消息搜索失败"
                    },
                );
                return Some((None, true));
            }
        }
        if let Some(response) = self.receive_daily_sync(value, route, fields) {
            return Some(response);
        }
        if value["@type"] == "updateMessageIsPinned"
            && let Some(chat) = value["chat_id"].as_i64()
        {
            let changed = self.store.apply(value);
            let request = (!self.demo && Some(chat) == self.store.active_chat)
                .then(|| self.pinned_request(chat));
            return Some((request, changed));
        }
        None
    }
}

impl App {
    pub fn locate_first_unread(&mut self) -> Option<Value> {
        if !std::mem::take(&mut self.sync.enter_unread) {
            return None;
        }
        let chat = self.store.active_chat.and_then(|id| self.store.chat(id))?;
        let (id, marker, unread) = (chat.id, chat.read_inbox, chat.unread);
        if unread <= 0 {
            return None;
        }
        let incoming = self
            .store
            .messages
            .iter()
            .filter(|message| !message.outgoing && message.id > marker);
        if marker > 0
            && self.store.oldest_message_id().is_some_and(|id| id > marker)
            && incoming.count() < unread as usize
        {
            let mut request = actions::jump_to_message(id, marker);
            request["@extra"] = json!(format!("unread:{id}"));
            let tag = tag(&mut request, self.next_sync_generation());
            self.sync.unread_context = Some((tag, marker));
            self.history_loading = true;
            return Some(request);
        }
        self.anchor_unread(marker);
        None
    }

    fn anchor_unread(&mut self, marker: i64) {
        if let Some(message) = self
            .store
            .messages
            .iter()
            .find(|message| !message.outgoing && message.id > marker)
        {
            self.timeline_anchor = Some((message.id, 0));
            self.reveal_jump(message.id);
        }
    }

    fn cancel_unread_context(&mut self) {
        self.sync.enter_unread = false;
        if self.sync.unread_context.take().is_some() {
            self.history_loading = false;
        }
    }

    fn receive_unread_context(&mut self, value: &Value, extra: &str, chat: i64) -> Response {
        let Some((pending, marker)) = self.sync.unread_context.as_ref() else {
            return (None, false);
        };
        if pending != extra {
            return (None, false);
        }
        let marker = *marker;
        self.cancel_unread_context();
        if self.store.chat(chat).is_some_and(|chat| chat.unread == 0) {
            return (None, false);
        }
        let marker = self
            .store
            .chat(chat)
            .map_or(marker, |chat| marker.max(chat.read_inbox));
        if let Some(messages) = value["messages"].as_array()
            && let Some(target) = messages
                .iter()
                .filter(|message| {
                    message["chat_id"].as_i64() == Some(chat) && message["is_outgoing"] != true
                })
                .filter_map(|message| message["id"].as_i64())
                .filter(|id| *id > marker)
                .min()
        {
            self.store.replace_context(messages, target);
        }
        self.anchor_unread(marker);
        (None, true)
    }

    pub fn record_viewport(&mut self, visible: Vec<i64>, now: Instant) {
        let read = &mut self.sync.read;
        let at_bottom = self.timeline_top >= self.timeline_max;
        if !read.initialized || read.visible != visible || read.at_bottom != at_bottom {
            read.initialized = true;
            read.at_bottom = at_bottom;
            read.stable = false;
            read.visible = visible;
            read.retried = false;
            read.deadline = Some(now + Duration::from_millis(300));
        }
    }

    pub fn hide_viewport(&mut self) {
        self.sync.read.initialized = false;
        self.sync.read.stable = false;
        self.sync.read.deadline = None;
    }

    pub fn restart_viewport_stability(&mut self, now: Instant) {
        self.sync.read.stable = false;
        self.sync.read.deadline = self
            .sync
            .read
            .initialized
            .then_some(now + Duration::from_millis(300));
    }

    pub fn report_viewport(&mut self, worker: &TdWorker, now: Instant) -> bool {
        let Some(chat) = self.store.active_chat else {
            self.sync.read.deadline = None;
            return false;
        };
        if self.demo || self.has_overlay() || self.show_search || self.history_loading {
            self.hide_viewport();
            return false;
        }
        if !self.terminal_focused {
            self.sync.read.stable = false;
            self.sync.read.deadline = None;
            return false;
        }
        let read = &mut self.sync.read;
        if read.pending.is_some() {
            return false;
        }
        if read.deadline.is_none_or(|deadline| deadline > now) {
            return self.maybe_clear_mentions(worker);
        }
        read.deadline = None;
        read.stable = true;
        let read_through = self
            .store
            .chat(chat)
            .map_or(0, |chat| chat.read_inbox)
            .max(read.reported);
        let ids: Vec<_> = read
            .visible
            .iter()
            .copied()
            .filter(|id| *id > read_through)
            .collect();
        let Some(&newest) = ids.last() else {
            return self.maybe_clear_mentions(worker);
        };
        let generation = self.next_sync_generation();
        let mut request = actions::view_messages(chat, ids);
        request["@extra"] = json!(format!("view:{chat}:{generation}"));
        if send_request(self, worker, request) {
            self.sync.read.pending = Some((generation, newest));
            false
        } else {
            true
        }
    }

    fn maybe_clear_mentions(&mut self, worker: &TdWorker) -> bool {
        let read = &self.sync.read;
        if !read.stable
            || self.store.context_window
            || self.store.message_scope.is_some()
            || self.timeline_top < self.timeline_max
        {
            return false;
        }
        let Some(chat) = self.store.active_chat.and_then(|id| self.store.chat(id)) else {
            return false;
        };
        let latest = chat
            .latest_message_id
            .max(self.store.messages.last().map_or(0, |message| message.id));
        let at_latest = self
            .store
            .messages
            .last()
            .is_some_and(|message| message.id >= latest)
            || read.visible.last().is_some_and(|id| *id >= latest);
        let empty_read_chat =
            read.visible.is_empty() && self.store.messages.is_empty() && chat.unread == 0;
        let read_confirmed = chat.unread == 0 || read.reported.max(chat.read_inbox) >= latest;
        if (at_latest || empty_read_chat) && read_confirmed {
            return self.clear_mentions(worker);
        }
        false
    }

    pub fn clear_mentions(&mut self, worker: &TdWorker) -> bool {
        let Some(chat) = self.store.active_chat.and_then(|id| self.store.chat(id)) else {
            return false;
        };
        if chat.mention_count <= 0 || chat.mentions_requested || self.sync.mention_clear.is_some() {
            return false;
        }
        let id = chat.id;
        let generation = self.next_sync_generation();
        let mut request = actions::read_all_chat_mentions(id);
        request["@extra"] = json!(format!("clear-mentions:{id}:{generation}"));
        if send_request(self, worker, request) {
            self.sync.mention_clear = Some((id, generation));
            self.store.chat_mut_by_id(id).unwrap().mentions_requested = true;
            return false;
        }
        self.sync.read.stable = false;
        true
    }

    pub fn load_chat_list(&mut self, worker: &TdWorker, list: ChatList) -> bool {
        if self.store.exhausted_lists.contains(&list) {
            return false;
        }
        let request = actions::load_chat_list(list);
        let tag = request["@extra"].as_str().unwrap();
        if self.sync.chat_list_load.is_some() {
            return false;
        }
        let tag = tag.to_owned();
        if send_request(self, worker, request) {
            self.sync.chat_list_load = Some(tag);
        }
        true
    }

    pub fn pending_presence_user(&self) -> Option<i64> {
        self.sync.presence_request.map(|(user, _)| user)
    }

    pub fn request_chat_summary(&mut self, worker: &TdWorker) {
        if self.demo {
            return;
        }
        let Some((chat, peer)) = self.store.active_chat.and_then(|id| {
            self.store
                .chat(id)
                .and_then(|chat| chat.peer.map(|peer| (id, peer)))
        }) else {
            return;
        };
        if self.sync.summary_peer == Some(peer) {
            return;
        }
        let request = match peer {
            ChatPeer::Private(id) => {
                if self.store.user_status(id) != crate::store::UserStatus::Unknown {
                    self.sync.summary_peer = Some(peer);
                    return;
                }
                if self.sync.presence_request.is_some() || self.store.sender_pending(id) {
                    return;
                }
                let generation = self.next_sync_generation();
                let request = json!({"@type":"getUser", "user_id":id, "@extra":format!("presence:{id}:{generation}")});
                if send_request(self, worker, request) {
                    self.sync.presence_request = Some((id, generation));
                    self.sync.presence_updated = false;
                    self.sync.summary_peer = Some(peer);
                }
                return;
            }
            ChatPeer::BasicGroup(id) => json!({"@type":"getBasicGroup", "basic_group_id":id}),
            ChatPeer::Supergroup(id) => json!({"@type":"getSupergroup", "supergroup_id":id}),
        };
        let mut request = request;
        request["@extra"] = json!(format!("details:{chat}:{}", self.sync.details_generation));
        if send_request(self, worker, request) {
            self.sync.summary_peer = Some(peer);
        }
    }

    pub fn next_sync_deadline(&self, include_viewport: bool) -> Option<Instant> {
        let read = &self.sync.read;
        [
            self.store.typing_deadline(),
            self.sync.draft.deadline,
            self.global_search_deadline(),
            self.proxy_deadline(),
            read.deadline
                .filter(|_| include_viewport && read.pending.is_none()),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn refresh_visible_chat_info(&mut self, worker: &TdWorker) {
        if self.show_chat_info && !self.sync.details_requested {
            self.request_chat_info(worker);
        }
    }

    pub fn request_chat_info(&mut self, worker: &TdWorker) {
        self.request_chat_summary(worker);
        for request in self.chat_info_requests() {
            send_request(self, worker, request);
        }
    }

    fn chat_info_requests(&mut self) -> Vec<Value> {
        let Some(chat) = self.store.active_chat else {
            return Vec::new();
        };
        let Some(peer) = self.store.chat(chat).and_then(|chat| chat.peer) else {
            return Vec::new();
        };
        self.sync.details_requested = true;
        let generation = self.sync.details_generation;
        let (field, id, full) = match peer {
            ChatPeer::Private(id) => ("user_id", id, "getUserFullInfo"),
            ChatPeer::BasicGroup(id) => ("basic_group_id", id, "getBasicGroupFullInfo"),
            ChatPeer::Supergroup(id) => ("supergroup_id", id, "getSupergroupFullInfo"),
        };
        vec![json!({"@type":full, field:id, "@extra":format!("details:{chat}:{generation}")})]
    }

    fn apply_chat_details(
        &mut self,
        chat_id: i64,
        peer: ChatPeer,
        value: &Value,
        kind: &str,
    ) -> bool {
        let Some(chat) = self.store.chat_mut_by_id(chat_id) else {
            return false;
        };
        match (peer, kind) {
            (ChatPeer::BasicGroup(id), "basicGroup") if value["id"].as_i64() == Some(id) => {
                chat.member_count = value["member_count"]
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok());
            }
            (ChatPeer::Private(_), "userFullInfo") => {
                chat.bio = limited(value["bio"]["text"].as_str(), 512);
            }
            (ChatPeer::BasicGroup(_), "basicGroupFullInfo") => {
                chat.bio = limited(value["description"].as_str(), 512);
                if let Some(members) = value["members"].as_array() {
                    chat.member_count = u32::try_from(members.len()).ok();
                }
            }
            (ChatPeer::Supergroup(_), "supergroupFullInfo") => {
                chat.bio = limited(value["description"].as_str(), 512);
                if let Some(count) = value["member_count"].as_i64().filter(|count| *count > 0) {
                    chat.member_count = u32::try_from(count).ok();
                }
            }
            (ChatPeer::Supergroup(id), "supergroup") if value["id"].as_i64() == Some(id) => {
                chat.username = chat_username(value);
                if let Some(is_forum) = value["is_forum"].as_bool() {
                    chat.is_forum = is_forum;
                }
                if let Some(count) = value["member_count"].as_i64().filter(|count| *count > 0) {
                    chat.member_count = u32::try_from(count).ok();
                }
            }
            _ => return false,
        }
        true
    }

    pub fn toggle_chat_setting(&mut self, worker: &TdWorker, pin: bool) {
        let Some(chat) = self.store.active_chat else {
            return;
        };
        let Some(details) = self.store.chat(chat) else {
            return;
        };
        let list = self.store.selected_list;
        let (setting, previous) = if pin {
            (Toggle::Pin(list), self.store.chat_pinned(chat, list))
        } else {
            (Toggle::Mute, details.muted)
        };
        if self.sync.toggles.len() >= 16
            || self
                .sync
                .toggles
                .iter()
                .any(|pending| pending.chat == chat && pending.setting == setting)
        {
            self.notice = Some("会话设置正在同步，请稍后再试".into());
            return;
        }
        let mut request = if pin {
            actions::toggle_chat_pin(list, chat, !previous)
        } else {
            let mut request = actions::toggle_chat_mute(chat, !previous);
            details
                .notifications
                .preserve(&mut request["notification_settings"]);
            request
        };
        let generation = self.next_sync_generation();
        tag(&mut request, generation);
        if send_request(self, worker, request) {
            self.sync.toggles.push(ChatToggle {
                chat,
                setting,
                generation,
                previous: Some(previous),
            });
            self.set_chat_setting(chat, setting, !previous);
            self.notice = None;
            self.notice_error = false;
        }
    }

    fn set_chat_setting(&mut self, chat: i64, setting: Toggle, enabled: bool) {
        match setting {
            Toggle::Mute => {
                if let Some(chat) = self.store.chat_mut_by_id(chat) {
                    chat.muted = enabled;
                }
            }
            Toggle::Pin(list) => self.store.set_chat_pinned(chat, list, enabled),
        }
    }

    fn receive_daily_sync(
        &mut self,
        value: &Value,
        route: &str,
        fields: std::str::Split<'_, char>,
    ) -> Option<Response> {
        let kind = value["@type"].as_str().unwrap_or_default();
        let pair = || {
            let mut fields = fields.clone();
            Some((
                fields.next()?.parse::<i64>().ok()?,
                fields.next()?.parse::<u64>().ok()?,
            ))
        };
        if self.sync.chat_list_load.as_deref() == value["@extra"].as_str()
            && self.sync.chat_list_load.is_some()
        {
            self.sync.chat_list_load = None;
        }
        if matches!(
            route,
            "view"
                | "details"
                | "presence"
                | "clear-mentions"
                | "mute"
                | "pin"
                | "draft"
                | "media-download"
                | "media-cancel"
                | "list-chat"
        ) {
            let Some((id, generation)) = pair() else {
                return Some((None, false));
            };
            let active = self.store.active_chat == Some(id);
            let changed = match route {
                "presence" if self.sync.presence_request == Some((id, generation)) => {
                    self.sync.presence_request = None;
                    if kind == "user" && value["id"].as_i64() == Some(id) {
                        self.store
                            .apply_user(value, id, !self.sync.presence_updated)
                    } else {
                        false
                    }
                }
                "clear-mentions" if self.sync.mention_clear == Some((id, generation)) => {
                    self.sync.mention_clear = None;
                    if kind == "error" {
                        if let Some(chat) = self.store.chat_mut_by_id(id) {
                            chat.mentions_requested = false;
                        }
                        if active {
                            self.sync.read.stable = false;
                        }
                        self.sync_error(value, "无法清除未读提及");
                        true
                    } else {
                        false
                    }
                }
                "view" if active => {
                    if let Some((pending, newest)) = self.sync.read.pending
                        && pending == generation
                    {
                        self.sync.read.pending = None;
                        if kind == "ok" {
                            self.sync.read.reported = newest;
                        } else if kind == "error"
                            && !self.sync.read.retried
                            && self.sync.read.deadline.is_none()
                            && !self.sync.read.visible.is_empty()
                        {
                            self.sync.read.retried = true;
                            self.sync.read.deadline = Some(Instant::now() + Duration::from_secs(3));
                        }
                    }
                    false
                }
                "details" if active && generation == self.sync.details_generation => {
                    if kind == "error" {
                        self.sync_error(value, "无法加载部分会话详情");
                        true
                    } else {
                        self.store
                            .chat(id)
                            .and_then(|chat| chat.peer)
                            .is_some_and(|peer| self.apply_chat_details(id, peer, value, kind))
                    }
                }
                "list-chat" => {
                    let Some(index) = self
                        .sync
                        .list_hydration
                        .iter()
                        .position(|pending| *pending == (id, generation))
                    else {
                        return Some((None, false));
                    };
                    self.sync.list_hydration.swap_remove(index);
                    if kind != "chat"
                        || value["id"].as_i64() != Some(id)
                        || self.store.chat(id).is_some()
                    {
                        return Some((None, false));
                    }
                    self.store.add_chat(value);
                    if self.selected_chat.is_none() {
                        self.selected_chat = self.store.chat_ids().next();
                    }
                    true
                }
                "mute" | "pin" => self.receive_toggle(value, id, generation),
                "draft" => self.receive_draft(value, id, generation),
                "media-download" | "media-cancel" => {
                    self.receive_media(value, id as i32, generation)
                }
                _ => false,
            };
            return Some((None, changed));
        }
        if let Some((id, _)) = self.sync.presence_request
            && ((kind == "updateUserStatus" && value["user_id"].as_i64() == Some(id))
                || (kind == "updateUser"
                    && value["user"]["id"].as_i64() == Some(id)
                    && value["user"].get("status").is_some()))
        {
            self.sync.presence_updated = true;
        }
        if let Some(chat) = value["chat_id"].as_i64() {
            if kind == "updateChatDraftMessage" {
                if self.store.active_chat == Some(chat) {
                    self.sync.draft.cloud_update_seen = true;
                }
                self.accept_cloud_draft(chat, &value["draft_message"]);
            }
            for pending in self
                .sync
                .toggles
                .iter_mut()
                .filter(|pending| pending.chat == chat)
            {
                match pending.setting {
                    Toggle::Mute if kind == "updateChatNotificationSettings" => {
                        pending.previous = None
                    }
                    Toggle::Pin(list) => {
                        let position_list =
                            |position: &Value| position.get("list").and_then(ChatList::from_json);
                        if (kind == "updateChatPosition"
                            && position_list(&value["position"]) == Some(list))
                            || (matches!(kind, "updateChatDraftMessage" | "updateChatLastMessage")
                                && value["positions"].is_array())
                        {
                            pending.previous = None;
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn receive_toggle(&mut self, value: &Value, chat: i64, generation: u64) -> bool {
        let Some(index) = self
            .sync
            .toggles
            .iter()
            .position(|pending| pending.chat == chat && pending.generation == generation)
        else {
            return false;
        };
        let pending = self.sync.toggles.remove(index);
        if value["@type"] != "error" {
            return false;
        }
        if let Some(previous) = pending.previous {
            self.set_chat_setting(chat, pending.setting, previous);
        }
        if self.store.active_chat == Some(chat) {
            self.sync_error(value, "无法更新会话设置");
        }
        true
    }

    fn composer_draft(&self) -> (&str, Option<i64>) {
        let (mode, text) = if let Some((mode, text, _)) = &self.suspended_input {
            (*mode, text.as_str())
        } else {
            (self.input_mode, self.draft.as_str())
        };
        match mode {
            InputMode::Send => (text, None),
            InputMode::Reply(id) => (text, Some(id)),
            _ => ("", None),
        }
    }

    fn draft_fingerprint(&self) -> u64 {
        let mut hash = DefaultHasher::new();
        self.composer_draft().hash(&mut hash);
        hash.finish()
    }

    pub fn reset_draft_snapshot(&mut self) {
        self.sync.draft.fingerprint = self.draft_fingerprint();
        self.sync.draft.deadline = None;
    }

    pub fn observe_draft(&mut self, now: Instant) {
        if self.demo || self.store.active_chat.is_none() {
            return;
        }
        let fingerprint = self.draft_fingerprint();
        if fingerprint != self.sync.draft.fingerprint {
            self.sync.draft.fingerprint = fingerprint;
            self.sync.draft.deadline = Some(now + Duration::from_millis(1500));
        }
    }

    pub fn flush_draft(&mut self, now: Instant, force: bool) -> Option<Value> {
        let chat = self.store.active_chat?;
        if force {
            self.observe_draft(now);
        }
        if self.demo
            || (!self
                .sync
                .draft
                .deadline
                .is_some_and(|deadline| force || deadline <= now)
                && !(force && self.sync.draft.failed.contains(&chat)))
        {
            return None;
        }
        self.sync.draft.deadline = None;
        let generation = self.next_sync_generation();
        let (text, reply) = self.composer_draft();
        let mut request = actions::set_draft(chat, text, reply);
        self.scope_outgoing_request(&mut request);
        tag(&mut request, generation);
        self.sync.draft.pending.retain(|(id, _)| *id != chat);
        if self.sync.draft.pending.len() == 16 {
            self.sync.draft.pending.pop_front();
        }
        self.sync.draft.pending.push_back((chat, generation));
        self.sync.draft.failed.retain(|id| *id != chat);
        Some(request)
    }

    fn receive_draft(&mut self, value: &Value, chat: i64, generation: u64) -> bool {
        let Some(index) = self
            .sync
            .draft
            .pending
            .iter()
            .position(|pending| *pending == (chat, generation))
        else {
            return false;
        };
        self.sync.draft.pending.remove(index);
        if value["@type"] == "error" {
            if self.sync.draft.failed.len() == 16 {
                self.sync.draft.failed.pop_front();
            }
            self.sync.draft.failed.push_back(chat);
            if self.store.active_chat == Some(chat) {
                self.sync_error(value, "草稿同步失败");
                return true;
            }
        }
        false
    }

    fn accept_cloud_draft(&mut self, chat: i64, value: &Value) {
        if self.demo
            || self.sync.draft.pending.iter().any(|(id, _)| *id == chat)
            || self.sync.draft.failed.contains(&chat)
        {
            return;
        }
        let content = value
            .pointer("/input_message_text/text/text")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let reply = value
            .pointer("/reply_to/message_id")
            .and_then(Value::as_i64)
            .filter(|id| *id > 0);
        if self.store.active_chat != Some(chat) {
            self.drafts.retain(|(id, saved)| {
                *id != chat
                    || saved.suspended.is_some()
                    || (saved.text == content
                        && match saved.mode {
                            InputMode::Reply(id) => reply == Some(id),
                            InputMode::Send => reply.is_none(),
                            _ => true,
                        })
            });
            return;
        }
        if self.store.message_scope.is_some()
            || self.composer_focus
            || self.sync.draft.deadline.is_some()
            || self.suspended_input.is_some()
            || !matches!(
                self.input_mode,
                InputMode::Off | InputMode::Send | InputMode::Reply(_)
            )
        {
            return;
        }
        self.draft = limited(Some(content), 65536);
        self.draft_cursor = usize::MAX;
        self.input_mode = reply.map_or(
            if self.draft.is_empty() {
                InputMode::Off
            } else {
                InputMode::Send
            },
            InputMode::Reply,
        );
        self.reset_draft_snapshot();
    }

    pub fn control_large_file(&mut self, worker: &TdWorker, id: i32, active: bool) {
        let previous = self
            .sync
            .media
            .iter()
            .position(|operation| operation.id == id);
        if previous.is_some_and(|index| self.sync.media[index].cancel || !active) {
            return;
        }
        if !active
            && self
                .sync
                .media
                .iter()
                .filter(|op| !op.cancel || op.previous_download.is_some())
                .count()
                >= 3
        {
            self.sync_error(&Value::Null, "下载任务已满，请稍后重试");
            return;
        }
        if previous.is_none() && self.sync.media.len() >= 16 {
            self.sync_error(&Value::Null, "下载任务正在同步，请稍后重试");
            return;
        }
        let generation = self.next_sync_generation();
        let request = if active {
            json!({"@type":"cancelDownloadFile","file_id":id,"only_if_pending":false,
                "@extra":format!("media-cancel:{id}:{generation}")})
        } else {
            let mut request = actions::download(id);
            request["@extra"] = json!(format!("media-download:{id}:{generation}"));
            request
        };
        if send_request(self, worker, request) {
            let previous = previous.map(|index| self.sync.media.remove(index));
            self.sync.media.push(FileOperation {
                id,
                generation,
                cancel: active,
                started: previous.as_ref().is_some_and(|op| op.started),
                previous_download: previous.map(|op| op.generation),
            });
            self.failed_files.remove(&id);
            self.store.set_downloading(id, !active);
        }
    }

    fn receive_media(&mut self, value: &Value, id: i32, generation: u64) -> bool {
        let Some(index) = self
            .sync
            .media
            .iter()
            .position(|op| op.id == id && op.generation == generation)
        else {
            return false;
        };
        let cancel = self.sync.media[index].cancel;
        if value["@type"] == "error" {
            let mut op = self.sync.media.remove(index);
            if cancel {
                if let Some(previous) = op.previous_download.take() {
                    op.generation = previous;
                    op.cancel = false;
                    self.sync.media.push(op);
                }
                self.store.set_downloading(id, true);
            } else {
                self.mark_file_failed(id);
                self.store.set_downloading(id, false);
            }
            self.sync_error(
                value,
                if cancel {
                    "取消下载失败"
                } else {
                    "下载失败"
                },
            );
            return true;
        }
        if cancel {
            self.sync.media.remove(index);
            self.store.set_downloading(id, false);
            return true;
        }
        if value["@type"] == "file" && value["id"].as_i64() == Some(i64::from(id)) {
            let terminal = value.pointer("/local/is_downloading_completed")
                == Some(&Value::Bool(true))
                || value.pointer("/local/can_be_downloaded") == Some(&Value::Bool(false));
            if terminal {
                return self.store.update_file(value) | self.observe_large_file(value);
            }
            self.sync.media[index].started |=
                value.pointer("/local/is_downloading_active") == Some(&Value::Bool(true));
        }
        false
    }

    pub fn large_file_pending(&self, id: i32) -> bool {
        self.sync.media.iter().any(|op| op.id == id)
    }

    pub fn cancelling_large_file(&self, id: i32) -> bool {
        self.sync.media.iter().any(|op| op.id == id && op.cancel)
    }

    pub fn observe_large_file(&mut self, file: &Value) -> bool {
        let Some(id) = file["id"].as_i64().and_then(|id| i32::try_from(id).ok()) else {
            return false;
        };
        let Some(index) = self.sync.media.iter().position(|op| op.id == id) else {
            return false;
        };
        let local = &file["local"];
        if local["is_downloading_completed"] == true {
            self.sync.media.remove(index);
            self.failed_files.remove(&id);
            return true;
        }
        if local["is_downloading_active"] == true {
            self.sync.media[index].started = true;
        } else if !self.sync.media[index].cancel
            && (self.sync.media[index].started || local["can_be_downloaded"] == false)
        {
            self.sync.media.remove(index);
            self.mark_file_failed(id);
            return true;
        }
        false
    }

    pub fn mark_file_failed(&mut self, id: i32) {
        if self.failed_files.len() >= 128 {
            self.failed_files.clear();
        }
        self.failed_files.insert(id);
    }

    pub fn request_queue_failed(&mut self, extra: &str) {
        if extra.starts_with("draft:")
            || extra.starts_with("history:")
            || extra.starts_with("jump:")
            || extra.starts_with("unread:")
            || extra.starts_with("list-chat:")
            || [
                "mention:",
                "callback:",
                "topics:",
                "thread-info:",
                "thread-history:",
                "newer:",
            ]
            .iter()
            .any(|prefix| extra.starts_with(prefix))
        {
            self.receive_sync(&json!({"@type":"error", "@extra":extra}));
        }
    }
}

fn valid_callback_url(url: &str) -> bool {
    url.len() <= 256
        && !url.chars().any(|ch| ch.is_control() || ch.is_whitespace())
        && url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .and_then(|rest| rest.split(['/', '?', '#']).next())
            .is_some_and(|host| {
                !host.is_empty() && !host.contains('@') && !host.contains(char::from(92))
            })
}

fn limited(value: Option<&str>, max: usize) -> String {
    value.unwrap_or_default().chars().take(max).collect()
}

pub(crate) fn chat_username(value: &Value) -> String {
    limited(
        value
            .pointer("/usernames/active_usernames/0")
            .or_else(|| value.get("username"))
            .and_then(Value::as_str),
        64,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{MediaKind, Store};
    use crate::tdlib::{TdCommand, TdEvent};
    use crate::{QuickSwitcher, interaction, ui};
    use std::sync::mpsc::Receiver;

    fn request(commands: &Receiver<TdCommand>) -> Value {
        let TdCommand::Request(value) = commands.try_recv().expect("TDLib request") else {
            panic!("unexpected TDLib command");
        };
        value
    }

    fn deliver(app: &mut App, sent: &Value, mut response: Value) -> Response {
        response["@extra"] = sent["@extra"].clone();
        app.apply(TdEvent::Update(response))
    }

    fn message(chat: i64, id: i64) -> Value {
        json!({"@type":"message", "chat_id":chat, "id":id, "content":{
            "@type":"messageText", "text":{"text":format!("消息 {id}")}
        }})
    }

    fn live_app() -> App {
        let mut app = ui::tests::fixture();
        app.demo = false;
        app.composer_focus = false;
        app.reset_draft_snapshot();
        app
    }

    #[test]
    fn instant_view_keeps_one_request_in_flight_and_ignores_closed_or_stale_results() {
        use crate::instant_view::State;
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        assert!(app.open_instant_view(&worker, "https://telegra.ph/first".into()));
        let first = request(&requests);
        assert!(app.open_instant_view(&worker, "https://graph.org/second".into()));
        assert!(requests.try_recv().is_err());
        let (next, changed) = deliver(&mut app, &first, json!({"@type":"error"}));
        assert!(!changed);
        let next = next.unwrap();
        assert_eq!(next["url"], "https://graph.org/second");
        assert_ne!(first["@extra"], next["@extra"]);
        assert!(matches!(
            app.instant_view.as_ref().unwrap().state,
            State::Loading
        ));
        assert!(!deliver(&mut app, &first, json!({"@type":"error"})).1);
        app.close_overlays();
        assert!(!deliver(&mut app, &next, json!({"@type":"error"})).1);
        assert!(app.instant_view.is_none());

        let (disconnected, requests) = TdWorker::test_pair();
        drop(requests);
        assert!(app.open_instant_view(&disconnected, "https://graph.org/third".into()));
        assert!(matches!(
            app.instant_view.as_ref().unwrap().state,
            State::Failed
        ));
    }

    #[test]
    fn quick_search_hydrates_and_opens_a_chat_outside_the_folder() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.quick_switcher = Some(QuickSwitcher {
            query: "public_name".into(),
            ..Default::default()
        });
        app.service_quick_search(&worker);
        let local = request(&commands);
        assert_eq!(local["@type"], "searchChats");
        let (server, _) = deliver(&mut app, &local, json!({"@type":"chats","chat_ids":[]}));
        let server = server.unwrap();
        assert_eq!(server["@type"], "searchChatsOnServer");
        let (public, changed) = deliver(
            &mut app,
            &server,
            json!({"@type":"chats","chat_ids":[90,90]}),
        );
        assert!(!changed);
        assert_eq!(public.unwrap()["@type"], "searchPublicChats");
        app.service_quick_search(&worker);
        let fetch = request(&commands);
        assert_eq!(fetch["chat_id"], 90);
        deliver(
            &mut app,
            &fetch,
            json!({"@type":"chat","id":90,"title":"频道标题"}),
        );
        // Username/server ranking is authoritative even when the title does not match.
        assert_eq!(app.store.quick_switch_entries("public_name")[0].0, 90);
        assert!(app.store.chat(90).is_none());
        interaction::perform_target(&mut app, &worker, ui::Target::QuickSwitchChat(90));
        assert_eq!(app.store.active_chat, Some(90));
        assert_eq!(request(&commands)["@type"], "closeChat");
        assert_eq!(request(&commands)["@type"], "openChat");
        let get_chat = request(&commands);
        deliver(
            &mut app,
            &get_chat,
            json!({"@type":"chat","id":90,"title":"频道标题"}),
        );
        assert_eq!(app.store.chat_title(90), "频道标题");
        assert_eq!(
            request(&commands)["filter"]["@type"],
            "searchMessagesFilterPinned"
        );
        assert_eq!(request(&commands)["@type"], "getChatHistory");
    }

    #[test]
    fn quick_search_caps_results_and_discards_superseded_queries_and_hydration() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.quick_switcher = Some(QuickSwitcher {
            query: "one".into(),
            ..Default::default()
        });
        app.service_quick_search(&worker);
        let old = request(&commands);
        deliver(&mut app, &old, json!({"@type":"chats","chat_ids":[1,90]}));
        app.service_quick_search(&worker);
        let old_chat = request(&commands);
        app.quick_switcher.as_mut().unwrap().query = "two".into();
        app.service_quick_search(&worker);
        let new = request(&commands);
        assert_eq!(
            deliver(
                &mut app,
                &old_chat,
                json!({"@type":"chat","id":90,"title":"已过期"})
            ),
            (None, false)
        );
        assert_eq!(
            deliver(&mut app, &old, json!({"@type":"chats","chat_ids":[3]})),
            (None, false)
        );
        assert!(app.store.global_chat_results.is_empty());
        let ids: Vec<_> = (100..150).collect();
        assert_eq!(
            deliver(&mut app, &new, json!({"@type":"chats","chat_ids":ids})),
            (None, false)
        );
        app.service_quick_search(&worker);
        for id in 100..132 {
            let fetch = request(&commands);
            assert_eq!(fetch["chat_id"], id);
            deliver(
                &mut app,
                &fetch,
                json!({"@type":"chat","id":id,"title":"搜索结果"}),
            );
        }
        assert!(commands.try_recv().is_err());
        assert_eq!(app.store.global_chat_results.len(), 32);
    }

    #[test]
    fn search_chat_updates_cannot_accumulate_outside_lists_and_promotion_rehydrates() {
        let mut app = live_app();
        for id in 100..200 {
            app.apply(TdEvent::Update(
                json!({"@type":"updateNewChat","chat":{"id":id,"title":"公开会话"}}),
            ));
        }
        assert!(app.store.chat(100).is_none());
        assert!(app.store.chat(199).is_some());
        assert!(app.store.chat(1).is_some());
        let (lookup, _) = app.apply(TdEvent::Update(json!({"@type":"updateChatPosition",
            "chat_id":100,"position":{"list":{"@type":"chatListMain"},"order":"200"}})));
        let lookup = lookup.unwrap();
        assert_eq!(lookup["chat_id"], 100);
        deliver(
            &mut app,
            &lookup,
            json!({"@type":"chat","id":100,"title":"加入列表",
            "positions":[{"list":{"@type":"chatListMain"},"order":"200"}]}),
        );
        assert_eq!(app.store.chat_ids().next(), Some(100));
    }

    #[test]
    fn group_typing_resolves_names_clears_only_the_sender_and_expires() {
        let mut store = Store::default();
        store.apply(&json!({"@type":"updateNewChat","chat":{"id":-10,"type":{
            "@type":"chatTypeBasicGroup","basic_group_id":10
        }}}));
        let typing = |id| {
            json!({"@type":"updateChatAction","chat_id":-10,
            "sender_id":{"@type":"messageSenderUser","user_id":id},
            "action":{"@type":"chatActionTyping"}})
        };
        assert!(store.apply(&typing(1)));
        assert_eq!(store.sender_requests(None)[0].1["user_id"], 1);
        store.apply(&json!({"@type":"updateUser","user":{"id":1,"first_name":"小林"}}));
        assert_eq!(store.chat(-10).unwrap().typing, "小林 正在输入…");
        store.apply(&typing(2));
        store.apply(
            &json!({"@type":"updateNewMessage", "message":{"chat_id":-10,
            "sender_id":{"@type":"messageSenderUser","user_id":2}}}),
        );
        assert_eq!(store.chat(-10).unwrap().typing, "小林 正在输入…");
        let deadline = store.typing_deadline().unwrap();
        assert!(!store.expire_typing(deadline - Duration::from_millis(1)));
        assert!(store.expire_typing(deadline));
        assert!(store.chat(-10).unwrap().typing.is_empty());
        assert!(store.typing_deadline().is_none());
    }

    #[test]
    fn jumps_reject_old_generations_and_history_after_reopening_the_same_chat() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        let old_history = app.history_request(1, 0);
        interaction::perform_target(&mut app, &worker, ui::Target::ReferencedMessage(30));
        let first = request(&commands);
        assert!(app.quote_back.is_some());
        app.jump_to(&worker, 40);
        let last = request(&commands);
        assert_eq!(
            deliver(
                &mut app,
                &first,
                json!({"@type":"messages","messages":[message(1,30)]})
            ),
            (None, false)
        );
        assert!(
            deliver(
                &mut app,
                &last,
                json!({"@type":"messages","messages":[message(1,41),message(1,40),message(1,39)]})
            )
            .1
        );
        assert_eq!(app.selected_message, Some(40));
        assert!(app.reveal_message);
        assert_eq!(
            app.store.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            [39, 40, 41]
        );
        app.selected_chat = Some(2);
        app.open_selected();
        app.selected_chat = Some(1);
        app.open_selected();
        assert_eq!(
            deliver(
                &mut app,
                &old_history,
                json!({"@type":"messages","messages":[message(1,99)]})
            ),
            (None, false)
        );
        assert!(app.store.messages.is_empty());
        assert!(app.history_loading);
    }

    #[test]
    fn missing_jump_anchor_uses_exact_message_and_bottom_reloads_latest() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.jump_to(&worker, 30);
        let sent = request(&commands);
        let (fallback, _) = deliver(
            &mut app,
            &sent,
            json!({"@type":"messages","messages":[message(1,31)]}),
        );
        let fallback = fallback.unwrap();
        assert_eq!(fallback["@type"], "getMessage");
        assert_eq!(fallback["message_id"], 30);
        assert!(deliver(&mut app, &fallback, message(1, 30)).1);
        assert_eq!(app.selected_message, Some(30));
        app.return_to_latest(&worker);
        assert_eq!(request(&commands)["from_message_id"], 0);
        assert!(!app.store.context_window);
    }

    #[test]
    fn context_window_keeps_target_within_both_budgets() {
        let mut app = live_app();
        let text = "x".repeat(65_536);
        let mut values: Vec<_> = (1..=600).rev().map(|id| message(1, id)).collect();
        assert!(app.store.replace_context(&values, 300));
        assert_eq!(app.store.messages.len(), 500);
        for value in &mut values {
            value["content"]["text"]["text"] = json!(text);
        }
        assert!(app.store.replace_context(&values, 300));
        assert!(app.store.messages.iter().any(|message| message.id == 300));
        assert!(
            app.store
                .messages
                .iter()
                .map(|message| message.retained_bytes())
                .sum::<usize>()
                <= 6 * 1024 * 1024
        );
    }

    #[test]
    fn unpin_invalidates_the_previous_lookup_and_loads_the_next_pin() {
        let mut app = live_app();
        let first = app.pinned_request(1);
        let (next, _) = app.apply(TdEvent::Update(json!({"@type":"updateMessageIsPinned",
            "chat_id":1,"message_id":1,"is_pinned":true})));
        assert_eq!(
            app.store
                .chat(1)
                .unwrap()
                .pinned_message
                .as_ref()
                .unwrap()
                .id,
            1
        );
        assert_eq!(
            deliver(
                &mut app,
                &first,
                json!({"@type":"foundChatMessages","messages":[]})
            ),
            (None, false)
        );
        let (unpin, _) = app.apply(TdEvent::Update(json!({"@type":"updateMessageIsPinned",
            "chat_id":1,"message_id":1,"is_pinned":false})));
        assert!(app.store.chat(1).unwrap().pinned_message.is_none());
        assert_eq!(
            deliver(
                &mut app,
                &next.unwrap(),
                json!({"@type":"foundChatMessages","messages":[message(1,1)]})
            ),
            (None, false)
        );
        assert!(
            deliver(
                &mut app,
                &unpin.unwrap(),
                json!({"@type":"foundChatMessages","messages":[message(1,2)]})
            )
            .1
        );
        assert_eq!(
            app.store
                .chat(1)
                .unwrap()
                .pinned_message
                .as_ref()
                .unwrap()
                .id,
            2
        );
    }

    #[test]
    fn viewport_reads_wait_and_use_only_the_latest_visible_unobscured_ids() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        let now = Instant::now();
        app.record_viewport(vec![1, 2], now);
        assert!(!app.report_viewport(&worker, now));
        app.record_viewport(vec![8, 9], now + Duration::from_millis(250));
        app.report_viewport(&worker, now + Duration::from_millis(300));
        assert!(commands.try_recv().is_err());
        app.show_help = true;
        app.report_viewport(&worker, now + Duration::from_millis(550));
        assert!(commands.try_recv().is_err());
        app.show_help = false;
        app.record_viewport(vec![9, 10], now + Duration::from_millis(600));
        app.report_viewport(&worker, now + Duration::from_millis(900));
        let read = request(&commands);
        assert_eq!(read["message_ids"], json!([9, 10]));
        deliver(&mut app, &read, json!({"@type":"ok"}));
        app.record_viewport(vec![10, 11], now + Duration::from_millis(1000));
        app.report_viewport(&worker, now + Duration::from_millis(1300));
        assert_eq!(request(&commands)["message_ids"], json!([11]));
    }

    #[test]
    fn rendered_viewport_excludes_offscreen_messages_and_modal_frames() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut app = live_app();
        for id in 3..=50 {
            app.store
                .apply(&json!({"@type":"updateNewMessage","message":message(1,id)}));
        }
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        assert!(app.visible_read_ids.contains(&50));
        assert!(!app.visible_read_ids.contains(&1));
        assert!(app.visible_read_ids.len() < app.store.messages.len());
        app.show_help = true;
        terminal.draw(|frame| ui::draw(frame, &mut app)).unwrap();
        assert!(app.visible_read_ids.is_empty());
    }

    #[test]
    fn opening_with_unread_outside_latest_history_fetches_the_boundary() {
        let mut app = live_app();
        app.selected_chat = Some(2);
        app.store
            .apply(&json!({"@type":"updateChatReadInbox","chat_id":2,
            "last_read_inbox_message_id":100,"unread_count":80}));
        let initial = app.open_selected().pop().unwrap();
        let (unread, _) = deliver(
            &mut app,
            &initial,
            json!({"@type":"messages",
            "messages":[message(2,180),message(2,179)]}),
        );
        let unread = unread.unwrap();
        assert_eq!(unread["from_message_id"], 100);
        assert!(app.history_loading);
        deliver(
            &mut app,
            &unread,
            json!({"@type":"messages","messages":[
                message(2,102),message(2,101),message(2,100)
            ]}),
        );
        assert_eq!(app.timeline_anchor, Some((101, 0)));
        assert_eq!(app.selected_message, Some(101));
        assert!(app.store.context_window);
        assert!(!app.history_loading);
    }

    #[test]
    fn mentions_clear_once_after_latest_read_or_stable_mentions_only_view() {
        for unread in [0, 1] {
            let mut app = live_app();
            let (worker, commands) = TdWorker::test_pair();
            let now = Instant::now();
            let chat = app.store.chat_mut_by_id(1).unwrap();
            chat.mention_count = 2;
            chat.unread = unread;
            chat.latest_message_id = 2;
            app.timeline_max = 1;
            app.record_viewport(if unread == 0 { vec![] } else { vec![2] }, now);
            app.report_viewport(&worker, now + Duration::from_millis(200));
            assert!(commands.try_recv().is_err());
            app.report_viewport(&worker, now + Duration::from_millis(301));
            if unread > 0 {
                let view = request(&commands);
                assert_eq!(view["@type"], "viewMessages");
                assert!(commands.try_recv().is_err());
                deliver(&mut app, &view, json!({"@type":"ok"}));
            }
            assert!(commands.try_recv().is_err(), "scrolled back must not clear");
            app.timeline_top = 1;
            app.record_viewport(
                if unread == 0 { vec![] } else { vec![2] },
                now + Duration::from_millis(310),
            );
            app.report_viewport(&worker, now + Duration::from_millis(611));
            let clear = request(&commands);
            assert_eq!(clear["@type"], "readAllChatMentions");
            assert_eq!(clear["chat_id"], 1);
            app.clear_mentions(&worker);
            deliver(&mut app, &clear, json!({"@type":"ok"}));
            app.report_viewport(&worker, now + Duration::from_secs(1));
            assert!(commands.try_recv().is_err());
            assert_eq!(app.store.chat(1).unwrap().mention_count, 2);
            app.apply(TdEvent::Update(json!({"@type":"updateChatUnreadMentionCount","chat_id":1,"unread_mention_count":0})));
            assert_eq!(app.store.chat(1).unwrap().mention_count, 0);
            assert!(
                !crate::menu::items(&app)
                    .any(|item| item.action == crate::ui::Action::ClearMentions)
            );
        }
    }

    #[test]
    fn private_summary_fetch_is_shared_and_only_one_request_is_in_flight() {
        let mut app = live_app();
        app.store.add_chat(
            &json!({"id":1,"title":"小林","type":{"@type":"chatTypePrivate","user_id":5}}),
        );
        let (worker, commands) = TdWorker::test_pair();
        app.request_chat_summary(&worker);
        let user = request(&commands);
        assert_eq!(user["@type"], "getUser");
        app.request_chat_summary(&worker);
        app.request_chat_info(&worker);
        assert_eq!(request(&commands)["@type"], "getUserFullInfo");
        assert!(commands.try_recv().is_err());
        deliver(
            &mut app,
            &user,
            json!({"@type":"user","id":5,"status":{"@type":"userStatusRecently"}}),
        );
        app.request_chat_summary(&worker);
        assert!(commands.try_recv().is_err());
    }

    #[test]
    fn private_details_respect_live_status_and_chat_generation() {
        let mut app = live_app();
        app.store.add_chat(
            &json!({"id":1,"title":"小林","type":{"@type":"chatTypePrivate","user_id":5}}),
        );
        let (worker, commands) = TdWorker::test_pair();
        app.request_chat_info(&worker);
        let user = request(&commands);
        let full = request(&commands);
        assert_eq!(user["user_id"], 5);
        assert_eq!(full["@type"], "getUserFullInfo");
        app.apply(TdEvent::Update(
            json!({"@type":"updateUserStatus","user_id":5,
            "status":{"@type":"userStatusOnline","expires":crate::store::unix_now()+100}}),
        ));
        deliver(
            &mut app,
            &user,
            json!({"@type":"user","id":5,"usernames":{"active_usernames":["xiaolin"]},
            "status":{"@type":"userStatusOffline"}}),
        );
        assert!(app.store.user_status(5).is_online(crate::store::unix_now()));
        assert_eq!(app.store.chat(1).unwrap().username, "xiaolin");
        app.selected_chat = Some(2);
        app.open_selected();
        assert_eq!(
            deliver(
                &mut app,
                &full,
                json!({"@type":"userFullInfo","bio":{"text":"过期简介"}})
            ),
            (None, false)
        );
        assert!(app.store.chat(1).unwrap().bio.is_empty());
    }

    #[test]
    fn mute_and_pin_errors_roll_back_only_their_own_optimistic_setting() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.store.apply(
            &json!({"@type":"updateChatNotificationSettings","chat_id":1,
            "notification_settings":{"sound_id":"345","use_default_sound":false}}),
        );
        app.toggle_chat_setting(&worker, false);
        let mute = request(&commands);
        assert!(app.store.chat(1).unwrap().muted);
        assert_eq!(mute["notification_settings"]["sound_id"], "345");
        assert_eq!(mute["notification_settings"]["use_default_sound"], false);
        app.toggle_chat_setting(&worker, true);
        let pin = request(&commands);
        assert!(app.store.chat_pinned(1, ChatList::Main));
        let auth_message = app.auth.message.clone();
        deliver(&mut app, &mute, json!({"@type":"error","message":"拒绝"}));
        assert!(!app.store.chat(1).unwrap().muted);
        assert!(app.store.chat(1).unwrap().pinned);
        app.store.select_list(ChatList::Archive);
        deliver(
            &mut app,
            &pin,
            json!({"@type":"error","message":"置顶数量已满"}),
        );
        assert!(!app.store.chat_pinned(1, ChatList::Main));
        assert_eq!(app.auth.message, auth_message);
        assert!(app.notice_error);
    }

    #[test]
    fn draft_updates_protect_edits_and_debounce_latest_text_then_clear() {
        let mut app = live_app();
        let now = Instant::now();
        let cloud = |text: &str| {
            TdEvent::Update(json!({"@type":"updateChatDraftMessage","chat_id":1,
            "draft_message":{"input_message_text":{"text":{"text":text}}}}))
        };
        app.apply(cloud("云端"));
        assert_eq!(app.draft, "云端");
        app.composer_focus = true;
        app.draft = "编辑中".into();
        app.observe_draft(now);
        app.apply(cloud("外部更改"));
        assert_eq!(app.draft, "编辑中");
        assert!(
            app.flush_draft(now + Duration::from_millis(1499), false)
                .is_none()
        );
        app.draft.push('！');
        app.observe_draft(now + Duration::from_millis(1499));
        assert!(
            app.flush_draft(now + Duration::from_millis(1500), false)
                .is_none()
        );
        let edited = app
            .flush_draft(now + Duration::from_millis(2999), false)
            .unwrap();
        assert_eq!(
            edited["draft_message"]["input_message_text"]["text"]["text"],
            "编辑中！"
        );
        app.draft.clear();
        let clear = app
            .flush_draft(now + Duration::from_millis(3000), true)
            .unwrap();
        assert!(clear["draft_message"].is_null());
        assert_eq!(
            deliver(
                &mut app,
                &edited,
                json!({"@type":"error","message":"旧请求失败"})
            ),
            (None, false)
        );
        assert_eq!(
            deliver(&mut app, &clear, json!({"@type":"ok"})),
            (None, false)
        );
        assert!(app.draft.is_empty());
    }

    #[test]
    fn switching_chats_flushes_composer_before_close_and_protects_pending_draft() {
        let mut app = live_app();
        app.input_mode = InputMode::Reply(3);
        app.draft = "回复草稿".into();
        app.selected_chat = Some(2);
        let requests = app.open_selected();
        let draft = &requests[0];
        assert_eq!(draft["@type"], "setChatDraftMessage");
        assert_eq!(draft["draft_message"]["reply_to"]["message_id"], 3);
        assert_eq!(requests[1]["@type"], "closeChat");
        app.apply(TdEvent::Update(
            json!({"@type":"updateChatDraftMessage","chat_id":1,
            "draft_message":null}),
        ));
        app.selected_chat = Some(1);
        app.open_selected();
        assert_eq!(app.draft, "回复草稿");
        assert!(app.input_mode == InputMode::Reply(3));
    }

    #[test]
    fn newer_pages_merge_and_resume_live_messages_at_the_latest_known_id() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.store
            .replace_context(&[message(1, 10), message(1, 11)], 11);
        app.store.chat_mut_by_id(1).unwrap().latest_message_id = 15;
        assert!(app.load_newer_near_bottom(&worker));
        let first = request(&commands);
        assert_eq!(first["offset"], -50);
        assert_eq!(first["from_message_id"], 11);
        deliver(
            &mut app,
            &first,
            json!({"@type":"messages","messages":[message(1,13),message(1,12)]}),
        );
        assert!(app.store.context_window);
        assert!(app.load_newer_near_bottom(&worker));
        let second = request(&commands);
        assert_eq!(second["from_message_id"], 13);
        assert!(
            !deliver(
                &mut app,
                &first,
                json!({"@type":"messages","messages":[message(1,99)]})
            )
            .1
        );
        deliver(
            &mut app,
            &second,
            json!({"@type":"messages","messages":[message(1,15),message(1,14)]}),
        );
        assert!(!app.store.context_window);
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":message(1,16)}),
        ));
        assert_eq!(
            app.store
                .messages
                .iter()
                .map(|msg| msg.id)
                .collect::<Vec<_>>(),
            [10, 11, 12, 13, 14, 15, 16]
        );
        app.store.replace_context(&[message(1, 11)], 11);
        app.load_newer_near_bottom(&worker);
        let old = request(&commands);
        app.selected_chat = Some(2);
        app.open_selected();
        assert!(
            !deliver(
                &mut app,
                &old,
                json!({"@type":"messages","messages":[message(1,12)]})
            )
            .1
        );
    }

    #[test]
    fn viewport_errors_retry_once_without_a_new_frame() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        let now = Instant::now();
        app.record_viewport(vec![7], now);
        app.report_viewport(&worker, now + Duration::from_millis(300));
        let first = request(&commands);
        deliver(&mut app, &first, json!({"@type":"error"}));
        let retry = app.next_sync_deadline(true).unwrap();
        app.report_viewport(&worker, retry - Duration::from_millis(1));
        assert!(commands.try_recv().is_err());
        app.report_viewport(&worker, retry);
        let second = request(&commands);
        assert_eq!(second["message_ids"], json!([7]));
        assert_ne!(first["@extra"], second["@extra"]);
        deliver(&mut app, &second, json!({"@type":"error"}));
        assert!(app.next_sync_deadline(true).is_none());
    }

    #[test]
    fn send_message_updates_the_draft_snapshot() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        let now = Instant::now();
        app.input_mode = InputMode::Send;
        app.draft = "sent".into();
        app.observe_draft(now);
        assert!(crate::submit_composer(&mut app, &worker));
        assert_eq!(
            request(&commands)["input_message_content"]["clear_draft"],
            true
        );
        app.observe_draft(now);
        assert!(
            app.flush_draft(now + Duration::from_secs(3), false)
                .is_none()
        );
    }

    #[test]
    fn open_chat_merges_cached_metadata_and_dispatches_visible_details() {
        let mut app = live_app();
        app.show_chat_info = true;
        app.store.add_chat(
            &json!({"id":2, "type":{"@type":"chatTypePrivate","user_id":42},
            "positions":[{"list":{"@type":"chatListMain"},"order":"300","is_pinned":true}]}),
        );
        app.selected_chat = Some(2);
        let requests = app.open_selected();
        assert!(
            requests
                .iter()
                .any(|req| req["@type"] == "getUserFullInfo" && req["user_id"] == 42)
        );
        let sent = requests
            .iter()
            .find(|req| req["@type"] == "getChat")
            .unwrap();
        deliver(
            &mut app,
            sent,
            json!({"@type":"chat","id":2,"unread_count":2,
            "last_read_inbox_message_id":20,"last_read_outbox_message_id":18,
            "notification_settings":{"mute_for":99}, "pinned_message":{"id":16,"excerpt":"pinned"},
            "draft_message":{"input_message_text":{"text":{"text":"cloud"}}},"positions":[]}),
        );
        let chat = app.store.chat(2).unwrap();
        assert_eq!(
            (chat.unread, chat.read_inbox, chat.read_outbox),
            (2, 20, 18)
        );
        assert!(chat.pinned && chat.muted);
        assert_eq!(chat.pinned_message.as_ref().unwrap().id, 16);
        assert_eq!(app.store.chat_ids().next(), Some(2));
        assert_eq!(app.draft, "cloud");
    }

    #[test]
    fn thumbnail_and_manual_download_quotas_are_independent() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.requested_files.extend(10..14);
        for id in 20..23 {
            app.control_large_file(&worker, id, false);
            let sent = request(&commands);
            assert_eq!(sent["file_id"], id);
            deliver(
                &mut app,
                &sent,
                json!({"@type":"file","id":id,"local":{"is_downloading_active":true}}),
            );
        }
        app.control_large_file(&worker, 24, false);
        assert!(commands.try_recv().is_err());
        app.requested_files.remove(&10);
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":{"id":60,"chat_id":1,
            "content":{"@type":"messagePhoto","photo":{"sizes":[{"photo":{"id":30}}]}}}}),
        ));
        app.visible_media.push(30);
        assert_eq!(app.visible_media_download().unwrap()["file_id"], 30);
        app.apply(TdEvent::Update(
            json!({"@type":"updateFile","file":{"id":20,
            "local":{"is_downloading_completed":true,"path":"/finished"}}}),
        ));
        app.control_large_file(&worker, 24, false);
        assert_eq!(request(&commands)["file_id"], 24);
    }

    #[test]
    fn media_reaction_and_quote_clicks_only_select_during_multiselect() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        for target in [
            ui::Target::Media(2),
            ui::Target::Reaction(2, 0),
            ui::Target::Quote(2, 90),
        ] {
            app.multi_select = vec![1];
            interaction::perform_target(&mut app, &worker, target);
            assert_eq!(app.multi_select, [1, 2]);
            assert!(app.preview_message.is_none() && app.quote_back.is_none());
            assert!(commands.try_recv().is_err());
        }
    }

    #[test]
    fn remote_media_preview_and_exact_title_precede_weak_local_match() {
        let mut app = live_app();
        app.store.chat_mut_by_id(1).unwrap().preview = "旧记录包含 资料".into();
        let remote = app.store.remote_chat_summary(12, &json!({"@type":"chat","id":12,"title":"资料",
            "last_message":{"content":{"@type":"messageDocument","document":{"file_name":"报告.pdf"}}}})).unwrap();
        assert_eq!(remote.subtitle, "[文件] 报告.pdf");
        app.store.global_chat_results.push(remote);
        let entries = app.store.quick_switch_entries("资料");
        assert_eq!((entries[0].0, entries[1].0), (12, 1));
    }

    #[test]
    fn shared_media_ignores_old_queries_and_wrong_chat_results() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.input_mode = InputMode::Search;
        app.parity.filter = crate::parity::MediaFilter::Photo;
        assert!(crate::submit_composer(&mut app, &worker));
        let first = request(&commands);
        assert_eq!(first["filter"]["@type"], "searchMessagesFilterPhoto");
        assert_eq!(first["query"], "");
        app.input_mode = InputMode::Search;
        app.parity.filter = crate::parity::MediaFilter::Voice;
        assert!(crate::submit_composer(&mut app, &worker));
        let second = request(&commands);
        assert_eq!(second["filter"]["@type"], "searchMessagesFilterVoiceNote");
        assert!(
            !deliver(
                &mut app,
                &first,
                json!({"@type":"foundChatMessages","messages":[message(1,9)]})
            )
            .1
        );
        assert!(app.store.search_results.is_empty());
        let found: Vec<_> = std::iter::once(message(2, 900))
            .chain((100..165).map(|id| message(1, id)))
            .collect();
        deliver(
            &mut app,
            &second,
            json!({"@type":"foundChatMessages","messages":found}),
        );
        assert_eq!(app.store.search_results.len(), 50);
        assert_eq!(app.store.search_results[0].id, 100);
        interaction::perform_target(&mut app, &worker, ui::Target::Message(100));
        assert_eq!(request(&commands)["from_message_id"], 100);
    }

    #[test]
    fn global_message_search_debounces_rejects_stale_and_opens_cross_chat_context() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        let now = Instant::now();
        app.quick_switcher = Some(QuickSwitcher {
            query: "old".into(),
            global_messages: true,
            ..Default::default()
        });
        app.service_global_search(&worker, now);
        app.service_global_search(&worker, now + Duration::from_millis(249));
        assert!(commands.try_recv().is_err());
        app.service_global_search(&worker, now + Duration::from_millis(250));
        let first = request(&commands);
        assert_eq!(first["@type"], "searchMessages");
        app.quick_switcher.as_mut().unwrap().query = "new".into();
        app.service_global_search(&worker, now + Duration::from_millis(300));
        assert!(
            !deliver(
                &mut app,
                &first,
                json!({"@type":"foundMessages","messages":[message(2,10)]})
            )
            .1
        );
        assert!(app.quick_entries().is_empty());
        app.service_global_search(&worker, now + Duration::from_millis(550));
        let second = request(&commands);
        assert_eq!(second["query"], "new");
        let messages: Vec<_> = (100..145).map(|id| message(2, id)).collect();
        deliver(
            &mut app,
            &second,
            json!({"@type":"foundMessages","messages":messages}),
        );
        assert_eq!(app.quick_entries().len(), 32);
        interaction::perform_target(&mut app, &worker, ui::Target::GlobalSearchHit(2, 100));
        assert_eq!(app.store.active_chat, Some(2));
        let mut sent = Vec::new();
        while let Ok(TdCommand::Request(command)) = commands.try_recv() {
            sent.push(command);
        }
        assert!(
            sent.iter()
                .any(|command| command["@type"] == "openChat" && command["chat_id"] == 2)
        );
        let jump = sent.last().unwrap();
        assert_eq!(jump["from_message_id"], 100);
        deliver(
            &mut app,
            jump,
            json!({"@type":"messages","messages":[message(2,100),message(2,99)]}),
        );
        assert!(app.store.context_window);
        assert_eq!(app.selected_message, Some(100));
    }

    #[test]
    fn batch_forward_is_bounded_and_skips_protected_messages() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.multi_select = (1..=6).rev().collect();
        interaction::perform(&mut app, &worker, ui::Action::MultiForward);
        app.selected_chat = Some(2);
        let first: Vec<_> = (0..4).map(|_| request(&commands)).collect();
        assert!(commands.try_recv().is_err());
        interaction::perform(&mut app, &worker, ui::Action::Confirm);
        assert!(commands.try_recv().is_err());
        deliver(
            &mut app,
            &first[0],
            json!({"@type":"messageProperties","can_be_forwarded":false}),
        );
        app.service_batch(&worker);
        let fifth = request(&commands);
        assert!(commands.try_recv().is_err());
        for request in first.iter().skip(1).chain([&fifth]) {
            deliver(
                &mut app,
                request,
                json!({"@type":"messageProperties","can_be_forwarded":true}),
            );
        }
        app.service_batch(&worker);
        let sixth = request(&commands);
        deliver(
            &mut app,
            &sixth,
            json!({"@type":"messageProperties","can_be_forwarded":true}),
        );
        assert!(app.batch_prompt().unwrap().contains("1 条无法转发"));
        interaction::perform(&mut app, &worker, ui::Action::Confirm);
        let forward = request(&commands);
        assert_eq!(forward["@type"], "forwardMessages");
        assert_eq!(forward["chat_id"], 2);
        assert_eq!(forward["message_ids"], json!([1, 2, 3, 4, 5]));
    }

    #[test]
    fn batch_delete_distinguishes_revoke_and_self_and_ignores_cancelled_generation() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.multi_select = vec![1, 2, 3];
        interaction::perform(&mut app, &worker, ui::Action::MultiDelete);
        let first: Vec<_> = (0..3).map(|_| request(&commands)).collect();
        app.close_overlays();
        interaction::perform(&mut app, &worker, ui::Action::MultiDelete);
        let second: Vec<_> = (0..3).map(|_| request(&commands)).collect();
        assert!(
            !deliver(
                &mut app,
                &first[2],
                json!({"@type":"messageProperties","can_be_deleted_for_all_users":true})
            )
            .1
        );
        deliver(
            &mut app,
            &second[0],
            json!({"@type":"messageProperties","can_be_deleted_for_all_users":true}),
        );
        deliver(
            &mut app,
            &second[1],
            json!({"@type":"messageProperties","can_be_deleted_only_for_self":true}),
        );
        deliver(
            &mut app,
            &second[2],
            json!({"@type":"messageProperties","can_be_deleted_only_for_self":false}),
        );
        let prompt = app.batch_prompt().unwrap();
        assert!(prompt.contains("1 条仅为自己删除") && prompt.contains("1 条无法删除"));
        interaction::perform(&mut app, &worker, ui::Action::Confirm);
        let revoke = request(&commands);
        let own = request(&commands);
        assert_eq!(revoke["revoke"], true);
        assert_eq!(revoke["message_ids"], json!([1]));
        assert_eq!(own["revoke"], false);
        assert_eq!(own["message_ids"], json!([2]));
    }

    #[test]
    fn reactions_scope_to_current_message_cap_choices_and_fall_back_on_error() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.input_mode = InputMode::React(1);
        app.request_reactions(&worker, 1);
        let first = request(&commands);
        app.input_mode = InputMode::React(2);
        app.request_reactions(&worker, 2);
        let second = request(&commands);
        assert!(
            !deliver(
                &mut app,
                &first,
                json!({"@type":"availableReactions","top_reactions":[]})
            )
            .1
        );
        assert_eq!(app.reaction_choices().len(), 8);
        deliver(&mut app, &second, json!({"@type":"error"}));
        assert_eq!(app.reaction_choices().len(), 8);
        app.request_reactions(&worker, 2);
        let third = request(&commands);
        let choices: Vec<_> = (1..30).map(|id| json!({"type":{"@type":"reactionTypeCustomEmoji","custom_emoji_id":id.to_string()}})).collect();
        deliver(
            &mut app,
            &third,
            json!({"@type":"availableReactions","top_reactions":choices}),
        );
        assert_eq!(app.reaction_choices().len(), 24);
        app.parity.reaction_page = 1;
        assert!(app.pick_reaction(&worker, 0));
        assert_eq!(request(&commands)["reaction_type"]["custom_emoji_id"], "9");
    }

    #[test]
    fn storage_statistics_are_once_per_open_and_cleanup_requires_confirmation() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.show_settings = true;
        app.service_storage(&worker);
        let stats = request(&commands);
        app.service_storage(&worker);
        app.confirm_storage_cleanup(&worker);
        assert!(commands.try_recv().is_err());
        deliver(
            &mut app,
            &stats,
            json!({"@type":"storageStatisticsFast","files_size":8192}),
        );
        assert_eq!(app.parity.storage_size, Some(8192));
        interaction::perform(&mut app, &worker, ui::Action::CleanStorage);
        assert!(commands.try_recv().is_err());
        interaction::perform(&mut app, &worker, ui::Action::ConfirmStorage);
        let cleanup = request(&commands);
        assert_eq!(cleanup["@type"], "optimizeStorage");
        deliver(
            &mut app,
            &cleanup,
            json!({"@type":"storageStatistics","size":0}),
        );
        assert_eq!(app.parity.storage_size, Some(0));
    }

    #[test]
    fn card_download_can_cancel_then_retry_and_ignores_the_superseded_response() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.store.apply(&json!({"@type":"updateNewMessage","message":{"id":4,"chat_id":1,
            "content":{"@type":"messageDocument","document":{"file_name":"test.zip","document":{"id":42,"size":8000}}}}}));
        assert!(
            app.store
                .messages
                .last()
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .kind
                == MediaKind::Document
        );
        app.selected_message = Some(4);
        interaction::perform(&mut app, &worker, ui::Action::Preview);
        let first = request(&commands);
        assert_eq!(first["synchronous"], false);
        interaction::perform(&mut app, &worker, ui::Action::Preview);
        let cancel = request(&commands);
        assert_eq!(cancel["@type"], "cancelDownloadFile");
        assert!(
            !deliver(
                &mut app,
                &first,
                json!({"@type":"error","message":"cancelled"})
            )
            .1
        );
        assert!(!app.failed_files.contains(&42));
        deliver(&mut app, &cancel, json!({"@type":"ok"}));
        interaction::perform(&mut app, &worker, ui::Action::Preview);
        let retried = request(&commands);
        deliver(
            &mut app,
            &retried,
            json!({"@type":"error","message":"download failed"}),
        );
        assert!(app.failed_files.contains(&42));
        interaction::perform(&mut app, &worker, ui::Action::Preview);
        assert!(!app.failed_files.contains(&42));
        let successful = request(&commands);
        deliver(
            &mut app,
            &successful,
            json!({"@type":"file","id":42,
            "local":{"path":"/test.zip","is_downloading_completed":true}}),
        );
        assert_eq!(
            app.store
                .messages
                .last()
                .unwrap()
                .media
                .as_ref()
                .unwrap()
                .path
                .as_deref(),
            Some("/test.zip")
        );
    }

    #[test]
    fn global_search_hydrates_unloaded_chat_titles_and_ignores_stale_replies() {
        let mut app = App::new();
        let (worker, commands) = TdWorker::test_pair();
        let now = Instant::now();
        app.quick_switcher = Some(crate::QuickSwitcher {
            query: "release".into(),
            cursor: 7,
            selected: 0,
            scroll: 0,
            global_messages: true,
        });
        app.service_global_search(&worker, now);
        app.service_global_search(&worker, now + Duration::from_millis(250));
        let req = request(&commands);
        assert_eq!(req["@type"], "searchMessages");
        app.receive_parity(&json!({
            "@type": "foundMessages",
            "@extra": req["@extra"],
            "messages": [{"id": 500, "chat_id": 777, "content": {"@type": "messageText", "text": {"text": "release notes"}}}]
        }));
        assert_eq!(app.quick_entries()[0].1, "会话 777");
        app.service_global_search(&worker, now + Duration::from_millis(260));
        let hydrate = request(&commands);
        assert_eq!(hydrate["@type"], "getChat");
        assert_eq!(hydrate["chat_id"], 777);
        assert_eq!(
            app.receive_parity(&json!({
                "@type": "chat",
                "id": 777,
                "title": "归档项目群",
                "@extra": hydrate["@extra"],
            })),
            Some(true)
        );
        assert_eq!(app.quick_entries()[0].1, "归档项目群");
        // Stale hydration reply with wrong generation is ignored.
        assert_eq!(
            app.receive_parity(&json!({
                "@type": "chat",
                "id": 777,
                "title": "旧标题",
                "@extra": "global-chat:0:777",
            })),
            Some(false)
        );
        assert_eq!(app.quick_entries()[0].1, "归档项目群");
    }

    #[test]
    fn custom_emoji_reactions_render_and_toggle_and_empty_all_search_is_ignored() {
        let mut app = App::new();
        let (worker, commands) = TdWorker::test_pair();
        app.store
            .apply(&json!({"@type":"updateNewChat","chat":{"id":1,"title":"测试"}}));
        app.store.open(1);
        app.store.apply(&json!({
            "@type": "updateNewMessage",
            "message": {
                "id": 10,
                "chat_id": 1,
                "is_outgoing": false,
                "content": {"@type": "messageText", "text": {"text": "hi"}},
                "interaction_info": {
                    "reactions": {
                        "reactions": [{
                            "type": {"@type": "reactionTypeCustomEmoji", "custom_emoji_id": "555"},
                            "total_count": 2,
                            "is_chosen": true
                        }]
                    }
                }
            }
        }));
        let reaction = &app.store.messages[0].info.extras().reactions[0];
        assert_eq!(reaction.emoji, "✦");
        assert_eq!(reaction.custom_emoji_id, Some(555));
        assert!(reaction.chosen);
        interaction::perform_target(&mut app, &worker, ui::Target::Reaction(10, 0));
        let req = request(&commands);
        assert_eq!(req["@type"], "removeMessageReaction");
        assert_eq!(req["reaction_type"]["@type"], "reactionTypeCustomEmoji");
        assert_eq!(req["reaction_type"]["custom_emoji_id"], "555");

        // Empty search is ignored for MediaFilter::All but allowed for Photo.
        app.input_mode = crate::InputMode::Search;
        app.draft.clear();
        app.parity.filter = crate::parity::MediaFilter::All;
        assert!(app.submit_input().is_none());
        app.parity.filter = crate::parity::MediaFilter::Photo;
        let search_req = app.submit_input().expect("photo filter allows empty query");
        assert_eq!(search_req["filter"]["@type"], "searchMessagesFilterPhoto");
    }
    #[test]
    fn latest_mention_opens_the_returned_unlisted_chat_and_errors_stay_in_chat() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        app.search_mention(&worker, "@first");
        let first = request(&commands);
        app.search_mention(&worker, "@second");
        let second = request(&commands);
        assert_eq!(second["username"], "second");
        assert_eq!(
            deliver(
                &mut app,
                &first,
                json!({"@type":"chat","id":80,"title":"过期"})
            ),
            (None, false)
        );
        assert!(app.store.chat(80).is_none());
        deliver(
            &mut app,
            &second,
            json!({"@type":"chat","id":81,"title":"新会话"}),
        );
        assert!(app.service_navigation(&worker));
        assert_eq!(app.store.active_chat, Some(81));
        assert_eq!(app.store.chat_title(81), "新会话");
        assert_eq!(request(&commands)["@type"], "closeChat");
        assert_eq!(request(&commands)["chat_id"], 81);
        app.search_mention(&worker, "@missing");
        let missing = commands
            .try_iter()
            .filter_map(|command| match command {
                TdCommand::Request(value) if value["@type"] == "searchPublicChat" => Some(value),
                _ => None,
            })
            .next()
            .unwrap();
        let previous_auth = app.auth.message.clone();
        deliver(
            &mut app,
            &missing,
            json!({"@type":"error","message":"USERNAME_NOT_OCCUPIED"}),
        );
        assert!(app.notice.as_deref().unwrap().contains("找不到提及"));
        assert_eq!(app.auth.message, previous_auth);
    }

    #[test]
    fn callback_accepts_one_request_and_scopes_alert_and_url_to_chat() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        assert!(app.request_callback(&worker, 2, "YWNr"));
        let sent = request(&commands);
        assert_eq!(sent["payload"]["data"], "YWNr");
        assert!(!app.request_callback(&worker, 2, "YWNr"));
        assert!(commands.try_recv().is_err());
        deliver(
            &mut app,
            &sent,
            json!({"@type":"callbackQueryAnswer","show_alert":true,
            "text":"提".repeat(200),"url":"https://example.org/bot"}),
        );
        let notice = app.notice.as_deref().unwrap();
        assert!(notice.starts_with("Bot 提醒："));
        assert_eq!(notice.chars().filter(|ch| *ch == '提').count(), 161);
        assert!(notice.contains("https://example.org/bot") && app.notice_error);
        assert!(!valid_callback_url("file:///tmp/example"));
        assert!(!valid_callback_url("https://"));
        app.request_callback(&worker, 2, "YWNr");
        let old = request(&commands);
        app.selected_chat = Some(2);
        app.open_selected();
        assert_eq!(
            deliver(&mut app, &old, json!({"@type":"error","message":"stale"})),
            (None, false)
        );
        assert!(app.request_callback(&worker, 1, "YWNr"));
    }

    #[test]
    fn forum_topics_are_bounded_sorted_and_scope_history_live_messages_and_sends() {
        let mut app = live_app();
        app.store.chat_mut_by_id(1).unwrap().is_forum = true;
        let (worker, commands) = TdWorker::test_pair();
        app.show_topics = true;
        app.request_forum_topics(&worker, 1);
        let old = request(&commands);
        app.request_forum_topics(&worker, 1);
        let list = request(&commands);
        assert!(!deliver(&mut app, &old, json!({"@type":"forumTopics","topics":[]})).1);
        let topics: Vec<_> = (1..70)
            .map(|id| {
                json!({
                    "info":{"forum_topic_id":id,"name":format!("话题 {id}")},"order":id.to_string(),
                    "is_pinned":id == 11
                })
            })
            .collect();
        deliver(
            &mut app,
            &list,
            json!({"@type":"forumTopics","topics":topics}),
        );
        let topics = &app.store.chat(1).unwrap().topics;
        assert_eq!(topics.len(), 64);
        assert_eq!((topics[0].thread_id, topics[1].thread_id), (11, 64));
        interaction::perform_target(&mut app, &worker, ui::Target::TopicEntry(0));
        let history = request(&commands);
        assert_eq!(history["@type"], "getForumTopicHistory");
        assert_eq!(history["forum_topic_id"], 11);
        let forum_message = |msg, topic| {
            let mut value = message(1, msg);
            value["topic_id"] = json!({"@type":"messageTopicForum","forum_topic_id":topic});
            value
        };
        deliver(
            &mut app,
            &history,
            json!({"@type":"messages","messages":[forum_message(200,12),forum_message(100,11)]}),
        );
        assert_eq!(
            app.store.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            [100]
        );
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":forum_message(101,12)}),
        ));
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":forum_message(102,11)}),
        ));
        assert_eq!(
            app.store.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            [100, 102]
        );
        let older = app.load_older().unwrap();
        assert_eq!(older["@type"], "getForumTopicHistory");
        assert_eq!(older["from_message_id"], 100);
        app.input_mode = InputMode::Send;
        app.draft = "话题消息".into();
        assert_eq!(
            app.submit_input().unwrap()["topic_id"],
            json!({"@type":"messageTopicForum","forum_topic_id":11})
        );
        app.reset_draft_snapshot();
        app.exit_forum_topic(&worker);
        assert_eq!(request(&commands)["@type"], "getChatHistory");
        assert!(app.store.chat(1).unwrap().active_topic.is_none());
        assert!(
            !deliver(
                &mut app,
                &older,
                json!({"@type":"messages","messages":[forum_message(90,11)]})
            )
            .1
        );
    }

    #[test]
    fn discussion_resolves_linked_group_filters_other_threads_and_returns_to_original_post() {
        let mut app = live_app();
        let (worker, commands) = TdWorker::test_pair();
        assert!(app.open_discussion(&worker, 2));
        let info = request(&commands);
        assert_eq!(info["@type"], "getMessageThread");
        assert_eq!(info["chat_id"], 1);
        assert!(commands.try_recv().is_err());
        app.input_mode = InputMode::Send;
        app.draft = "频道草稿".into();
        assert!(app.submit_input().is_none());
        app.reset_draft_snapshot();
        deliver(
            &mut app,
            &info,
            json!({"@type":"messageThreadInfo","chat_id":88,"message_thread_id":900}),
        );
        assert!(app.service_navigation(&worker));
        assert_eq!(request(&commands)["@type"], "closeChat");
        let open = request(&commands);
        assert_eq!(open["@type"], "openChat");
        assert_eq!(open["chat_id"], 88);
        let history = request(&commands);
        assert_eq!(history["@type"], "getMessageThreadHistory");
        assert_eq!(history["chat_id"], 88);
        assert_eq!(history["message_id"], 900);
        let comment = |msg, root| {
            let mut value = message(88, msg);
            value["topic_id"] = json!({"@type":"messageTopicThread","message_thread_id":root});
            value
        };
        deliver(
            &mut app,
            &history,
            json!({"@type":"messages","messages":[message(1,2),comment(902,777),comment(901,900),message(88,900)]}),
        );
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":comment(903,777)}),
        ));
        app.apply(TdEvent::Update(
            json!({"@type":"updateNewMessage","message":comment(904,900)}),
        ));
        assert_eq!(
            app.store.messages.iter().map(|m| m.id).collect::<Vec<_>>(),
            [900, 901, 904]
        );
        let older = app.load_older().unwrap();
        assert_eq!(older["from_message_id"], 900);
        app.input_mode = InputMode::Reply(901);
        app.draft = "评论".into();
        let sent = app.submit_input().unwrap();
        assert_eq!(sent["chat_id"], 88);
        assert_eq!(
            sent["topic_id"],
            json!({"@type":"messageTopicThread","message_thread_id":900})
        );
        assert_eq!(sent["reply_to"]["message_id"], 901);
        app.reset_draft_snapshot();
        app.exit_discussion(&worker);
        assert_eq!(app.store.active_chat, Some(1));
        assert!(app.active_thread.is_none());
        assert_eq!(app.draft, "频道草稿");
        let jump = commands
            .try_iter()
            .find_map(|command| match command {
                TdCommand::Request(value) if value["@type"] == "getChatHistory" => Some(value),
                _ => None,
            })
            .unwrap();
        assert_eq!(jump["from_message_id"], 2);
        assert!(
            !deliver(
                &mut app,
                &older,
                json!({"@type":"messages","messages":[comment(899,900)]})
            )
            .1
        );
        deliver(
            &mut app,
            &jump,
            json!({"@type":"messages","messages":[message(1,2)]}),
        );
        assert_eq!(app.selected_message, Some(2));
    }
}
