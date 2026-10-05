//! Small UI-facing projection of TDLib updates. No raw JSON is retained.

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde_json::Value;

#[derive(Clone)]
pub struct Chat {
    pub id: i64,
    pub title: String,
    pub unread: i64,
    pub preview: String,
    pub read_outbox: i64,
    orders: HashMap<ChatList, i64>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub enum ChatList {
    #[default]
    Main,
    Archive,
    Folder(i32),
}

impl ChatList {
    pub fn from_json(value: &Value) -> Option<Self> {
        match kind(value) {
            "chatListMain" => Some(Self::Main),
            "chatListArchive" => Some(Self::Archive),
            "chatListFolder" => Some(Self::Folder(integer(value.get("chat_folder_id")) as i32)),
            _ => None,
        }
    }
    pub fn json(self) -> Value {
        match self {
            Self::Main => serde_json::json!({"@type":"chatListMain"}),
            Self::Archive => serde_json::json!({"@type":"chatListArchive"}),
            Self::Folder(id) => serde_json::json!({"@type":"chatListFolder","chat_folder_id":id}),
        }
    }
}

pub struct ChatFolder {
    pub list: ChatList,
    pub name: String,
}

#[derive(Clone, Default)]
pub struct Message {
    pub id: i64,
    pub text: String,
    pub outgoing: bool,
    pub sender: Option<Sender>,
    pub author_signature: String,
    pub media: Option<MediaRef>,
    pub info: MessageInfo,
}

#[derive(Clone, Default)]
pub struct MessageInfo {
    pub text_message: bool,
    pub stamp: Option<crate::calendar::Stamp>,
    pub edited: bool,
    pub pinned: bool,
    pub sending: Sending,
    pub reply: Option<Reply>,
}

#[derive(Clone, Default)]
pub enum Sending {
    #[default]
    Sent,
    Pending,
    Failed {
        can_retry: bool,
        reason: String,
    },
}

#[derive(Clone)]
pub struct Reply {
    pub chat_id: i64,
    pub message_id: i64,
    pub excerpt: String,
}

impl Message {
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.text.capacity()
            + self.author_signature.capacity()
            + self
                .info
                .reply
                .as_ref()
                .map_or(0, |reply| reply.excerpt.capacity())
            + match &self.info.sending {
                Sending::Failed { reason, .. } => reason.capacity(),
                _ => 0,
            }
            + self.media.as_ref().map_or(0, |media| {
                media.path.as_ref().map_or(0, String::capacity)
                    + media
                        .detail
                        .as_ref()
                        .and_then(|file| file.path.as_ref())
                        .map_or(0, String::capacity)
            })
    }
    pub fn retryable(&self) -> bool {
        matches!(
            self.info.sending,
            Sending::Failed {
                can_retry: true,
                ..
            }
        )
    }
    pub fn status(&self, read_outbox: i64) -> String {
        let mut parts = Vec::with_capacity(4);
        if let Some(stamp) = self.info.stamp {
            parts.push(stamp.time());
        }
        if self.info.pinned {
            parts.push("置顶".into());
        }
        if self.info.edited {
            parts.push("已编辑".into());
        }
        match &self.info.sending {
            Sending::Pending => parts.push("发送中".into()),
            Sending::Failed { .. } => parts.push("发送失败".into()),
            Sending::Sent if self.outgoing => {
                parts.push(if self.id > 0 && self.id <= read_outbox {
                    "✓✓ 已读".into()
                } else {
                    "✓ 已发送".into()
                })
            }
            _ => {}
        }
        parts.join(" · ")
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Sender {
    User(i64),
    Chat(i64),
}

impl Sender {
    fn parse(value: &Value) -> Option<Self> {
        match kind(value) {
            "messageSenderUser" => Some(Self::User(value.get("user_id")?.as_i64()?)),
            "messageSenderChat" => Some(Self::Chat(value.get("chat_id")?.as_i64()?)),
            _ => None,
        }
    }
    fn request(self) -> Value {
        match self {
            Self::User(id) => {
                serde_json::json!({"@type":"getUser","user_id":id,"@extra":"sender-name"})
            }
            Self::Chat(id) => {
                serde_json::json!({"@type":"getChat","chat_id":id,"@extra":"sender-name"})
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Sticker,
    Document,
    Video,
}

#[derive(Clone)]
pub struct MediaRef {
    pub file_id: i32,
    pub path: Option<String>,
    pub kind: MediaKind,
    pub detail: Option<MediaFile>,
}

#[derive(Clone)]
pub struct MediaFile {
    pub file_id: i32,
    pub path: Option<String>,
}

impl MediaRef {
    pub fn detail_file(&self) -> (i32, Option<&str>) {
        self.detail
            .as_ref()
            .map(|file| (file.file_id, file.path.as_deref()))
            .unwrap_or((self.file_id, self.path.as_deref()))
    }
}

#[derive(Clone)]
pub struct Sticker {
    pub file_id: i32,
    pub emoji: String,
    pub width: i32,
    pub height: i32,
    pub preview: MediaRef,
}

type ChatOrder = BTreeSet<(Reverse<i64>, Reverse<i64>)>;

#[derive(Default)]
pub struct Store {
    chats: HashMap<i64, Chat>,
    ordered: HashMap<ChatList, ChatOrder>,
    pub selected_list: ChatList,
    pub folders: Vec<ChatFolder>,
    main_list_position: usize,
    pub exhausted_lists: std::collections::HashSet<ChatList>,
    pub active_chat: Option<i64>,
    pub messages: Vec<Message>,
    pub search_results: Vec<Message>,
    pub recent_stickers: Vec<Sticker>,
    pub favorite_stickers: Vec<Sticker>,
    pub installed_sticker_sets: Vec<(i64, String)>,
    pub sticker_sets: VecDeque<(i64, Vec<Sticker>)>,
    pub sticker_results: Vec<Sticker>,
    pub sticker_search_tag: String,
    pub history_exhausted: bool,
    sender_names: HashMap<Sender, String>,
    recent_senders: VecDeque<Sender>,
    requested_senders: HashSet<Sender>,
}

impl Store {
    pub fn chat_ids(&self) -> impl Iterator<Item = i64> + '_ {
        self.ordered
            .get(&self.selected_list)
            .into_iter()
            .flat_map(|ordered| ordered.iter().map(|(_, id)| id.0))
    }

    pub fn chat_lists(&self) -> Vec<(ChatList, String)> {
        let mut lists: Vec<_> = self
            .folders
            .iter()
            .map(|folder| (folder.list, folder.name.clone()))
            .collect();
        lists.insert(
            self.main_list_position.min(lists.len()),
            (ChatList::Main, "全部".into()),
        );
        lists.push((ChatList::Archive, "归档".into()));
        lists
    }

    pub fn list_name(&self) -> String {
        self.chat_lists()
            .into_iter()
            .find(|(list, _)| *list == self.selected_list)
            .map(|(_, name)| name)
            .unwrap_or_else(|| "全部".into())
    }

    pub fn select_list(&mut self, list: ChatList) -> bool {
        if self.selected_list == list
            || !self
                .chat_lists()
                .iter()
                .any(|(candidate, _)| *candidate == list)
        {
            return false;
        }
        self.selected_list = list;
        true
    }

    pub fn chat(&self, id: i64) -> Option<&Chat> {
        self.chats.get(&id)
    }

    pub fn chat_title(&self, id: i64) -> String {
        self.chats
            .get(&id)
            .map(|chat| chat.title.as_str())
            .filter(|title| !title.is_empty())
            .or_else(|| self.sender_names.get(&Sender::Chat(id)).map(String::as_str))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("会话 {id}"))
    }

    pub fn sender_label(&self, message: &Message) -> String {
        let name = if message.outgoing {
            "我".into()
        } else {
            match message.sender {
                Some(Sender::Chat(id)) => self.chat_title(id),
                Some(sender @ Sender::User(id)) => self
                    .sender_names
                    .get(&sender)
                    .cloned()
                    .unwrap_or_else(|| format!("用户 {id}")),
                None => "对方".into(),
            }
        };
        if message.author_signature.is_empty() {
            name
        } else {
            format!("{name} · {}", message.author_signature)
        }
    }

    // Only resolve senders in the bounded active message window. Never fetch group members.
    pub fn sender_requests(&mut self) -> Vec<(Sender, Value)> {
        if self.requested_senders.len() > 1024 {
            self.requested_senders.retain(|sender| {
                self.messages
                    .iter()
                    .chain(&self.search_results)
                    .any(|message| message.sender == Some(*sender))
                    || self.active_chat.map(Sender::Chat) == Some(*sender)
            });
        }
        let active = self
            .active_chat
            .filter(|id| {
                !self
                    .chats
                    .get(id)
                    .is_some_and(|chat| !chat.title.is_empty())
            })
            .map(Sender::Chat);
        let mut requests = Vec::new();
        for sender in active.into_iter().chain(
            self.messages
                .iter()
                .chain(&self.search_results)
                .filter(|message| !message.outgoing)
                .filter_map(|message| message.sender),
        ) {
            if self.sender_names.contains_key(&sender)
                || matches!(sender, Sender::Chat(id) if self.chats.get(&id).is_some_and(|chat| !chat.title.is_empty()))
            {
                continue;
            }
            if self.requested_senders.insert(sender) {
                requests.push((sender, sender.request()));
                if requests.len() == 8 {
                    break;
                }
            }
        }
        requests
    }

    pub fn retry_sender(&mut self, sender: Sender) {
        self.requested_senders.remove(&sender);
    }

    fn save_sender(&mut self, sender: Sender, name: String) -> bool {
        if name.is_empty() {
            return false;
        }
        self.requested_senders.remove(&sender);
        let changed = self.sender_names.get(&sender) != Some(&name);
        self.sender_names.insert(sender, name);
        self.recent_senders.retain(|old| *old != sender);
        self.recent_senders.push_back(sender);
        while self.recent_senders.len() > 1024 {
            if let Some(old) = self.recent_senders.pop_front() {
                self.sender_names.remove(&old);
            }
        }
        changed
            && (self
                .messages
                .iter()
                .chain(&self.search_results)
                .any(|message| message.sender == Some(sender))
                || self.active_chat.map(Sender::Chat) == Some(sender))
    }

    pub fn open(&mut self, id: i64) {
        if self.active_chat != Some(id) {
            self.active_chat = Some(id);
            self.messages.clear();
            self.search_results.clear();
            self.history_exhausted = false;
            self.requested_senders.clear();
        }
    }

    pub fn oldest_message_id(&self) -> Option<i64> {
        self.messages.first().map(|message| message.id)
    }

    pub fn apply(&mut self, update: &Value) -> bool {
        let update_kind = kind(update);
        let visible_change = matches!(
            update_kind,
            "updateChatFolders"
                | "updateNewChat"
                | "updateChatTitle"
                | "updateChatReadInbox"
                | "updateChatReadOutbox"
                | "updateChatPosition"
                | "updateChatLastMessage"
                | "updateChatDraftMessage"
                | "updateNewMessage"
                | "updateMessageSendSucceeded"
                | "updateMessageSendFailed"
                | "updateMessageContent"
                | "updateMessageEdited"
                | "updateMessageIsPinned"
                | "updateDeleteMessages"
                | "messages"
                | "message"
                | "foundChatMessages"
                | "updateFile"
                | "file"
                | "stickers"
                | "stickerSets"
                | "stickerSet"
        );
        match update_kind {
            "updateUser" | "user" => {
                let user = if update_kind == "user" {
                    Some(update)
                } else {
                    update.get("user")
                };
                if let Some(user) = user
                    && let Some(id) = user.get("id").and_then(Value::as_i64)
                {
                    let name = [
                        string(user.get("first_name")),
                        string(user.get("last_name")),
                    ]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                    return self.save_sender(
                        Sender::User(id),
                        if name.is_empty() {
                            if user.pointer("/type/@type").and_then(Value::as_str)
                                == Some("userTypeDeleted")
                            {
                                "已注销账号".into()
                            } else {
                                format!("用户 {id}")
                            }
                        } else {
                            name
                        },
                    );
                }
            }
            "chat" => {
                if let Some(id) = update.get("id").and_then(Value::as_i64) {
                    let title = string(update.get("title"));
                    if let Some(chat) = self.chats.get_mut(&id) {
                        chat.title = title.clone();
                    }
                    return self.save_sender(Sender::Chat(id), title);
                }
            }
            "updateChatFolders" => {
                self.folders = update
                    .get("chat_folders")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|folder| {
                        let id = folder.get("id")?.as_i64()? as i32;
                        let name = folder
                            .pointer("/name/text/text")
                            .or_else(|| folder.pointer("/name/text"))
                            .and_then(Value::as_str)
                            .or_else(|| folder.get("title").and_then(Value::as_str))
                            .unwrap_or("分组");
                        Some(ChatFolder {
                            list: ChatList::Folder(id),
                            name: name.to_owned(),
                        })
                    })
                    .collect();
                self.main_list_position =
                    integer(update.get("main_chat_list_position")).max(0) as usize;
                let lists = self.chat_lists();
                self.ordered
                    .retain(|list, _| lists.iter().any(|(candidate, _)| candidate == list));
                self.exhausted_lists
                    .retain(|list| lists.iter().any(|(candidate, _)| candidate == list));
                if !lists.iter().any(|(list, _)| *list == self.selected_list) {
                    self.selected_list = ChatList::Main;
                }
            }
            "error" if integer(update.get("code")) == 404 => {
                if let Some(list) = update
                    .get("@extra")
                    .and_then(Value::as_str)
                    .and_then(|extra| extra.strip_prefix("load-chats:"))
                    .and_then(|json| serde_json::from_str::<Value>(json).ok())
                    .as_ref()
                    .and_then(ChatList::from_json)
                {
                    self.exhausted_lists.insert(list);
                    return true;
                }
            }
            "updateNewChat" => {
                if let Some(chat) = update.get("chat") {
                    self.add_chat(chat);
                }
            }
            "updateChatTitle" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.title = string(update.get("title"));
                }
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64) {
                    self.save_sender(Sender::Chat(id), string(update.get("title")));
                }
            }
            "updateChatReadInbox" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.unread = integer(update.get("unread_count"));
                }
            }
            "updateChatReadOutbox" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.read_outbox = integer(update.get("last_read_outbox_message_id"));
                }
            }
            "updateChatPosition" => {
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(position) = update.get("position")
                    && let Some(list) = position.get("list").and_then(ChatList::from_json)
                {
                    self.set_order(id, list, integer(position.get("order")));
                }
            }
            "updateChatLastMessage" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.preview = preview(update.get("last_message"));
                }
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(positions) = update.get("positions").and_then(Value::as_array)
                {
                    self.replace_positions(id, positions);
                }
            }
            "updateChatDraftMessage" => {
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(positions) = update.get("positions").and_then(Value::as_array)
                {
                    self.replace_positions(id, positions);
                }
            }
            "message" => self.add_message(update, false),
            "updateNewMessage" | "updateMessageSendSucceeded" => {
                if kind(update) == "updateMessageSendSucceeded"
                    && update["message"]["chat_id"].as_i64() == self.active_chat
                    && let Some(old_id) = update.get("old_message_id")
                {
                    let old_id = integer(Some(old_id));
                    self.messages.retain(|message| message.id != old_id);
                }
                if let Some(message) = update.get("message") {
                    self.add_message(message, false);
                }
            }
            "updateMessageSendFailed" => {
                if update["message"]["chat_id"].as_i64() == self.active_chat {
                    let old_id = integer(update.get("old_message_id"));
                    self.messages.retain(|message| message.id != old_id);
                    self.add_message(&update["message"], false);
                    if let Some(message) = self
                        .messages
                        .iter_mut()
                        .find(|message| message.id == integer(update["message"].get("id")))
                        && !matches!(message.info.sending, Sending::Failed { .. })
                    {
                        message.info.sending = Sending::Failed {
                            can_retry: false,
                            reason: bounded_text(update.pointer("/error/message"), 160),
                        };
                    }
                }
            }
            "updateMessageEdited" | "updateMessageIsPinned" => {
                if update["chat_id"].as_i64() == self.active_chat {
                    let id = integer(update.get("message_id"));
                    for message in self
                        .messages
                        .iter_mut()
                        .chain(&mut self.search_results)
                        .filter(|m| m.id == id)
                    {
                        if update_kind == "updateMessageEdited" {
                            message.info.edited = integer(update.get("edit_date")) > 0;
                        } else {
                            message.info.pinned = update["is_pinned"] == true;
                        }
                    }
                }
            }
            "updateMessageContent" => {
                let id = integer(update.get("message_id"));
                if update.get("chat_id").and_then(Value::as_i64) == self.active_chat {
                    for message in self
                        .messages
                        .iter_mut()
                        .chain(&mut self.search_results)
                        .filter(|message| message.id == id)
                    {
                        message.text = content_text(update.get("new_content"));
                        message.info.text_message = kind(&update["new_content"]) == "messageText";
                        message.media = update.get("new_content").and_then(media_ref);
                    }
                    trim_messages(&mut self.messages, 6 * 1024 * 1024, false);
                    trim_messages(&mut self.search_results, 2 * 1024 * 1024, true);
                }
            }
            "updateDeleteMessages" => {
                if update.get("chat_id").and_then(Value::as_i64) == self.active_chat
                    && let Some(ids) = update.get("message_ids").and_then(Value::as_array)
                {
                    self.messages
                        .retain(|message| !ids.iter().any(|id| integer(Some(id)) == message.id));
                    self.search_results
                        .retain(|message| !ids.iter().any(|id| integer(Some(id)) == message.id));
                }
            }
            "messages" => {
                let extra = update.get("@extra").and_then(Value::as_str).unwrap_or("");
                if extra
                    .strip_prefix("resend:")
                    .and_then(|id| id.parse::<i64>().ok())
                    == self.active_chat
                {
                    for message in update["messages"].as_array().into_iter().flatten() {
                        self.add_message(message, false);
                    }
                }
                if let Some(id) = extra
                    .strip_prefix("history:")
                    .and_then(|s| s.parse::<i64>().ok())
                    && Some(id) == self.active_chat
                    && let Some(messages) = update.get("messages").and_then(Value::as_array)
                {
                    let oldest_before = self.oldest_message_id();
                    for message in messages {
                        self.add_message(message, true);
                    }
                    self.history_exhausted = messages.is_empty()
                        || oldest_before.is_some_and(|oldest| {
                            self.oldest_message_id().is_some_and(|new| new >= oldest)
                        });
                }
            }
            "foundChatMessages" => {
                let extra = update.get("@extra").and_then(Value::as_str).unwrap_or("");
                if let Some(id) = extra
                    .strip_prefix("search:")
                    .and_then(|value| value.parse::<i64>().ok())
                    && Some(id) == self.active_chat
                {
                    self.search_results = update
                        .get("messages")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(parse_message)
                        .take(100)
                        .collect();
                    trim_messages(&mut self.search_results, 2 * 1024 * 1024, true);
                }
            }
            "updateFile" => {
                if let Some(file) = update.get("file") {
                    return self.update_file(file);
                }
            }
            "file" => return self.update_file(update),
            "stickers" => {
                let extra = update["@extra"].as_str().unwrap_or("");
                let items = || {
                    update["stickers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(parse_sticker)
                        .take(256)
                        .collect()
                };
                match extra {
                    "recent-stickers" => self.recent_stickers = items(),
                    "favorite-stickers" => self.favorite_stickers = items(),
                    _ if !self.sticker_search_tag.is_empty()
                        && extra == self.sticker_search_tag =>
                    {
                        self.sticker_results = items()
                    }
                    _ => return false,
                }
            }
            "stickerSets" if update["@extra"].as_str() == Some("installed-sticker-sets") => {
                self.installed_sticker_sets = update["sets"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        Some((
                            s["id"]
                                .as_i64()
                                .or_else(|| s["id"].as_str()?.parse().ok())?,
                            string(s.get("title")),
                        ))
                    })
                    .take(256)
                    .collect();
            }
            "stickerSet" => {
                if let Some(id) = update["id"]
                    .as_i64()
                    .or_else(|| update["id"].as_str()?.parse().ok())
                {
                    let items = update["stickers"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(parse_sticker)
                        .take(256)
                        .collect();
                    self.sticker_sets.retain(|(key, _)| *key != id);
                    self.sticker_sets.push_back((id, items));
                    while self.sticker_sets.len() > 4 {
                        self.sticker_sets.pop_front();
                    }
                }
            }
            _ => {}
        }
        visible_change
    }

    fn chat_mut(&mut self, update: &Value) -> Option<&mut Chat> {
        let id = update.get("chat_id")?.as_i64()?;
        self.chats.get_mut(&id)
    }

    fn add_chat(&mut self, value: &Value) {
        let Some(id) = value.get("id").and_then(Value::as_i64) else {
            return;
        };
        self.replace_positions(id, &[]);
        self.chats.insert(
            id,
            Chat {
                id,
                title: string(value.get("title")),
                unread: integer(value.get("unread_count")),
                preview: preview(value.get("last_message")),
                read_outbox: integer(value.get("last_read_outbox_message_id")),
                orders: HashMap::new(),
            },
        );
        self.save_sender(Sender::Chat(id), string(value.get("title")));
        if let Some(positions) = value.get("positions").and_then(Value::as_array) {
            self.replace_positions(id, positions);
        }
    }

    fn replace_positions(&mut self, id: i64, positions: &[Value]) {
        let previous: Vec<_> = self
            .chats
            .get(&id)
            .map(|chat| chat.orders.keys().copied().collect())
            .unwrap_or_default();
        for list in previous {
            self.set_order(id, list, 0);
        }
        for position in positions {
            if let Some(list) = position.get("list").and_then(ChatList::from_json) {
                self.set_order(id, list, integer(position.get("order")));
            }
        }
    }

    fn set_order(&mut self, id: i64, list: ChatList, order: i64) {
        let Some(chat) = self.chats.get_mut(&id) else {
            return;
        };
        if let Some(previous) = chat.orders.remove(&list)
            && let Some(ordered) = self.ordered.get_mut(&list)
        {
            ordered.remove(&(Reverse(previous), Reverse(id)));
        }
        if order > 0 {
            chat.orders.insert(list, order);
            self.ordered
                .entry(list)
                .or_default()
                .insert((Reverse(order), Reverse(id)));
        }
    }

    fn add_message(&mut self, value: &Value, from_history: bool) {
        if value.get("chat_id").and_then(Value::as_i64) != self.active_chat {
            return;
        }
        let Some(message) = parse_message(value) else {
            return;
        };
        match self
            .messages
            .binary_search_by_key(&message.id, |item| item.id)
        {
            Ok(index) => {
                self.messages[index] = message;
            }
            Err(index) => self.messages.insert(index, message),
        }
        // Keep one bounded window; when browsing older history, retain the older edge.
        trim_messages(&mut self.messages, 6 * 1024 * 1024, from_history);
    }

    fn update_file(&mut self, file: &Value) -> bool {
        let id = integer(file.get("id")) as i32;
        let path = local_path(file);
        let mut changed = false;
        for message in self
            .messages
            .iter_mut()
            .chain(self.search_results.iter_mut())
        {
            if let Some(media) = &mut message.media {
                if media.file_id == id && media.path != path {
                    media.path = path.clone();
                    changed = true;
                }
                if let Some(detail) = &mut media.detail
                    && detail.file_id == id
                    && detail.path != path
                {
                    detail.path = path.clone();
                    changed = true;
                }
            }
        }
        for sticker in self
            .recent_stickers
            .iter_mut()
            .chain(&mut self.favorite_stickers)
            .chain(&mut self.sticker_results)
            .chain(self.sticker_sets.iter_mut().flat_map(|(_, items)| items))
        {
            if sticker.preview.file_id == id && sticker.preview.path != path {
                sticker.preview.path = path.clone();
                changed = true;
            }
        }
        changed
    }
}

fn parse_message(value: &Value) -> Option<Message> {
    Some(Message {
        id: value.get("id")?.as_i64()?,
        text: content_text(value.get("content")),
        outgoing: value
            .get("is_outgoing")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        sender: value.get("sender_id").and_then(Sender::parse),
        author_signature: bounded_text(value.get("author_signature"), 128),
        media: value.get("content").and_then(media_ref),
        info: MessageInfo {
            text_message: kind(&value["content"]) == "messageText",
            stamp: crate::calendar::local(integer(value.get("date"))),
            edited: integer(value.get("edit_date")) > 0,
            pinned: value["is_pinned"] == true,
            sending: match kind(&value["sending_state"]) {
                "messageSendingStatePending" => Sending::Pending,
                "messageSendingStateFailed" => Sending::Failed {
                    can_retry: value["sending_state"]["can_retry"] == true,
                    reason: bounded_text(
                        value
                            .pointer("/sending_state/error/message")
                            .or_else(|| value.pointer("/sending_state/error_message")),
                        160,
                    ),
                },
                _ => Sending::Sent,
            },
            reply: value
                .get("reply_to")
                .filter(|reply| kind(reply) == "messageReplyToMessage")
                .map(|reply| Reply {
                    chat_id: reply["chat_id"]
                        .as_i64()
                        .filter(|id| *id != 0)
                        .unwrap_or(integer(value.get("chat_id"))),
                    message_id: integer(reply.get("message_id")),
                    excerpt: reply
                        .pointer("/quote/text/text")
                        .filter(|v| v.as_str().is_some_and(|s| !s.is_empty()))
                        .map(|text| bounded_text(Some(text), 256))
                        .unwrap_or_else(|| {
                            content_text(reply.get("content"))
                                .chars()
                                .take(256)
                                .collect()
                        }),
                }),
        },
    })
}

fn bounded_text(value: Option<&Value>, limit: usize) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(limit)
        .collect()
}

fn trim_messages(messages: &mut Vec<Message>, budget: usize, keep_oldest: bool) {
    let mut bytes: usize = messages.iter().map(Message::retained_bytes).sum();
    while messages.len() > 500 || bytes > budget {
        let message = if keep_oldest {
            messages.pop()
        } else if !messages.is_empty() {
            Some(messages.remove(0))
        } else {
            None
        };
        let Some(message) = message else {
            break;
        };
        bytes -= message.retained_bytes();
    }
}

fn media_ref(content: &Value) -> Option<MediaRef> {
    let detail = if kind(content) == "messagePhoto" {
        content
            .pointer("/photo/sizes")?
            .as_array()?
            .iter()
            .max_by_key(|size| integer(size.get("width")) * integer(size.get("height")).max(1))
            .and_then(|size| size.get("photo"))
            .map(|file| MediaFile {
                file_id: integer(file.get("id")) as i32,
                path: local_path(file),
            })
    } else {
        None
    };
    let (kind, file) = match kind(content) {
        "messagePhoto" => {
            let sizes = content.pointer("/photo/sizes")?.as_array()?;
            let size = sizes
                .iter()
                .rev()
                .find(|size| integer(size.get("width")) <= 640)
                .or_else(|| sizes.first())?;
            (MediaKind::Photo, size.get("photo")?)
        }
        "messageSticker" => {
            let sticker = content.get("sticker")?;
            let format = sticker.pointer("/format/@type").and_then(Value::as_str);
            let file = if format == Some("stickerFormatWebp") {
                sticker.get("sticker")?
            } else {
                sticker.pointer("/thumbnail/file")?
            };
            (MediaKind::Sticker, file)
        }
        "messageDocument" => (MediaKind::Document, content.pointer("/document/document")?),
        "messageVideo" => (MediaKind::Video, content.pointer("/video/video")?),
        _ => return None,
    };
    Some(MediaRef {
        file_id: integer(file.get("id")) as i32,
        path: local_path(file),
        kind,
        detail: detail.filter(|detail| detail.file_id != integer(file.get("id")) as i32),
    })
}

fn parse_sticker(value: &Value) -> Option<Sticker> {
    let file = value.get("sticker")?;
    let file_id = integer(file.get("id")) as i32;
    if file_id <= 0 {
        return None;
    }
    let format = value.pointer("/format/@type").and_then(Value::as_str);
    let preview_file = if format == Some("stickerFormatWebp") {
        file
    } else {
        value.pointer("/thumbnail/file").unwrap_or(file)
    };
    Some(Sticker {
        file_id,
        emoji: string(value.get("emoji")),
        width: integer(value.get("width")) as i32,
        height: integer(value.get("height")) as i32,
        preview: MediaRef {
            file_id: integer(preview_file.get("id")) as i32,
            path: local_path(preview_file),
            kind: MediaKind::Sticker,
            detail: None,
        },
    })
}

fn local_path(file: &Value) -> Option<String> {
    let local = file.get("local")?;
    if local
        .get("is_downloading_completed")
        .and_then(Value::as_bool)
        != Some(true)
    {
        return None;
    }
    local
        .get("path")?
        .as_str()
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn kind(value: &Value) -> &str {
    value.get("@type").and_then(Value::as_str).unwrap_or("")
}

fn string(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_owned()
}

fn integer(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| value.as_i64().or_else(|| value.as_str()?.parse().ok()))
        .unwrap_or(0)
}

fn preview(message: Option<&Value>) -> String {
    message
        .map(|message| content_text(message.get("content")))
        .unwrap_or_default()
}

fn content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    match kind(content) {
        "messageText" => string(content.pointer("/text/text")),
        "messagePhoto" => format!("[图片] {}", string(content.pointer("/caption/text"))),
        "messageSticker" => format!("[贴纸] {}", string(content.pointer("/sticker/emoji"))),
        "messageVideo" => format!("[视频] {}", string(content.pointer("/caption/text"))),
        "messageVoiceNote" => "[语音消息]".into(),
        "messageDocument" => format!("[文件] {}", string(content.pointer("/document/file_name"))),
        other if !other.is_empty() => format!("[{other}]"),
        _ => "[未知消息]".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn content_changes_and_deletions_update_history_and_search() {
        let mut store = Store::default();
        store.open(7);
        let message = json!({"id": 30, "chat_id": 7, "content": {
            "@type": "messageText", "text": {"text": "old"}
        }});
        store.apply(&json!({"@type":"messages", "@extra":"history:7", "messages":[message]}));
        store.apply(
            &json!({"@type":"foundChatMessages", "@extra":"search:7", "messages":[message]}),
        );
        let mut edit = json!({"@type":"updateMessageContent", "chat_id":8, "message_id":30,
            "new_content":{"@type":"messagePhoto", "caption":{"text":"new"},
                "photo":{"sizes":[{"width":320,"photo":{"id":42}}]}}});
        store.apply(&edit);
        assert_eq!(store.search_results[0].text, "old");
        edit["chat_id"] = json!(7);
        store.apply(&edit);
        for message in store.messages.iter().chain(&store.search_results) {
            assert_eq!(message.text, "[图片] new");
            assert!(!message.info.text_message);
            assert_eq!(message.media.as_ref().unwrap().file_id, 42);
        }
        store.apply(&json!({"@type":"updateDeleteMessages", "chat_id":8, "message_ids":[30]}));
        assert_eq!(store.search_results.len(), 1);
        store.apply(&json!({"@type":"updateDeleteMessages", "chat_id":7, "message_ids":[30]}));
        assert!(store.messages.is_empty());
        assert!(store.search_results.is_empty());
    }

    #[test]
    fn editing_large_messages_keeps_history_and_search_within_byte_budgets() {
        let mut store = Store::default();
        store.open(7);
        store.messages = (1..=100)
            .map(|id| Message {
                id,
                text: "x".repeat(60_000),
                ..Message::default()
            })
            .collect();
        store.search_results = store.messages.iter().take(34).cloned().collect();
        store.apply(
            &json!({"@type":"updateMessageContent", "chat_id":7, "message_id":1,
            "new_content":{"@type":"messageText", "text":{"text":"y".repeat(600_000)}}}),
        );
        assert!(
            store
                .messages
                .iter()
                .map(Message::retained_bytes)
                .sum::<usize>()
                <= 6 * 1024 * 1024
        );
        assert!(
            store
                .search_results
                .iter()
                .map(Message::retained_bytes)
                .sum::<usize>()
                <= 2 * 1024 * 1024
        );
    }

    #[test]
    fn sending_transitions_and_read_receipts_preserve_text_and_ignore_other_chats() {
        let mut store = Store::default();
        store.apply(&json!({"@type":"updateNewChat","chat":{"id":1,"title":"chat"}}));
        store.open(1);
        let message = json!({"id":10,"chat_id":1,"date":1700000000,"is_outgoing":true,
            "sending_state":{"@type":"messageSendingStatePending"},
            "content":{"@type":"messageText","text":{"text":"原始正文"}}});
        store.apply(&json!({"@type":"updateNewMessage","message":message}));
        assert!(store.messages[0].status(0).contains("发送中"));
        let mut failed = message.clone();
        failed["id"] = json!(11);
        failed["sending_state"] = json!({"@type":"messageSendingStateFailed","can_retry":true,"error":{"message":"临时断网"}});
        store.apply(
            &json!({"@type":"updateMessageSendFailed","old_message_id":10,"message":failed}),
        );
        assert_eq!(store.messages.len(), 1);
        assert_eq!(store.messages[0].text, "原始正文");
        assert!(store.messages[0].retryable());
        let mut unrelated = failed.clone();
        unrelated["chat_id"] = json!(2);
        store.apply(
            &json!({"@type":"updateMessageSendSucceeded","old_message_id":11,"message":unrelated}),
        );
        assert_eq!(store.messages[0].id, 11);
        failed["id"] = json!(12);
        failed["sending_state"] = Value::Null;
        store.apply(
            &json!({"@type":"updateMessageSendSucceeded","old_message_id":11,"message":failed}),
        );
        assert!(store.messages[0].status(0).contains("已发送"));
        store.apply(
            &json!({"@type":"updateChatReadOutbox","chat_id":1,"last_read_outbox_message_id":12}),
        );
        assert!(
            store.messages[0]
                .status(store.chat(1).unwrap().read_outbox)
                .contains("已读")
        );
        store.apply(&json!({"@type":"updateMessageEdited","chat_id":1,"message_id":12,"edit_date":1700000001}));
        assert!(store.messages[0].info.edited);
    }

    #[test]
    fn long_unicode_history_obeys_byte_budget_and_quotes_remain_small() {
        let mut store = Store::default();
        store.open(1);
        for id in 1..=500 {
            store.apply(
                &json!({"@type":"updateNewMessage","message":{"id":id,"chat_id":1,
                "content":{"@type":"messageText","text":{"text":"👋".repeat(4096)}},
                "reply_to":{"@type":"messageReplyToMessage","chat_id":0,"message_id":id-1,
                    "quote":{"text":{"text":"👋".repeat(1000)}}}}}),
            );
        }
        assert!(store.messages.len() < 500);
        assert!(
            store
                .messages
                .iter()
                .map(Message::retained_bytes)
                .sum::<usize>()
                <= 6 * 1024 * 1024
        );
        let reply = store.messages.last().unwrap().info.reply.as_ref().unwrap();
        assert_eq!(reply.chat_id, 1);
        assert_eq!(reply.excerpt.chars().count(), 256);
        assert!(reply.excerpt.len() <= 1024);
        assert_eq!(store.messages.last().unwrap().id, 500);
    }

    #[test]
    fn photos_keep_separate_thumbnail_and_detail_paths_without_progress_redraws() {
        let mut store = Store::default();
        store.open(1);
        store.apply(&json!({"@type":"updateNewMessage","message":{"id":1,"chat_id":1,"content":{"@type":"messagePhoto","photo":{"sizes":[
            {"width":320,"height":160,"photo":{"id":42,"local":{"is_downloading_completed":true,"path":"/thumbnail.png"}}},
            {"width":2560,"height":1280,"photo":{"id":43,"local":{"is_downloading_completed":false}}}
        ]}}}}));
        let media = store.messages[0].media.as_ref().unwrap();
        assert_eq!(media.file_id, 42);
        assert_eq!(media.detail_file(), (43, None));
        assert!(!store.apply(&json!({"@type":"updateFile","file":{"id":43,"local":{"is_downloading_completed":false,"downloaded_size":100}}})));
        assert!(!store.apply(&json!({"@type":"updateFile","file":{"id":999,"local":{"is_downloading_completed":true,"path":"/unrelated.png"}}})));
        assert!(store.apply(&json!({"@type":"updateFile","file":{"id":43,"local":{"is_downloading_completed":true,"path":"/detail.png"}}})));
        let media = store.messages[0].media.as_ref().unwrap();
        assert_eq!(media.detail_file(), (43, Some("/detail.png")));
        assert_eq!(media.path.as_deref(), Some("/thumbnail.png"));
        assert!(!store.apply(&json!({"@type":"updateFile","file":{"id":43,"local":{"is_downloading_completed":true,"path":"/detail.png"}}})));
    }

    #[test]
    fn group_senders_resolve_users_anonymous_chats_and_renames() {
        let mut store = Store::default();
        store.open(-100);
        store.apply(
            &json!({"@type":"updateUser","user":{"id":7,"first_name":"小林","last_name":"同学"}}),
        );
        store.apply(&json!({"@type":"messages","@extra":"history:-100","messages":[
            {"id":1,"chat_id":-100,"sender_id":{"@type":"messageSenderUser","user_id":7},"content":{"@type":"messageText","text":{"text":"你好"}}},
            {"id":2,"chat_id":-100,"sender_id":{"@type":"messageSenderChat","chat_id":-100},"author_signature":"管理员","content":{"@type":"messageText","text":{"text":"公告"}}},
            {"id":3,"chat_id":-100,"sender_id":{"@type":"messageSenderUser","user_id":8},"content":{"@type":"messageText","text":{"text":"未缓存成员"}}}
        ]}));
        assert_eq!(store.sender_label(&store.messages[0]), "小林 同学");
        assert_eq!(store.sender_label(&store.messages[2]), "用户 8");
        let requests = store.sender_requests();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .any(|(_, request)| request["@type"] == "getChat" && request["chat_id"] == -100)
        );
        assert!(
            requests
                .iter()
                .any(|(_, request)| request["@type"] == "getUser" && request["user_id"] == 8)
        );
        assert!(
            store.sender_requests().is_empty(),
            "missing senders must not flood requests"
        );
        assert!(store.apply(&json!({"@type":"chat","id":-100,"title":"开发群"})));
        assert_eq!(store.chat_title(-100), "开发群");
        assert_eq!(store.sender_label(&store.messages[1]), "开发群 · 管理员");
        assert!(store.apply(&json!({"@type":"user","id":8,"first_name":"陈同学","last_name":""})));
        assert_eq!(store.sender_label(&store.messages[2]), "陈同学");
        assert!(store.apply(
            &json!({"@type":"updateUser","user":{"id":7,"first_name":"林","last_name":""}})
        ));
        assert_eq!(store.sender_label(&store.messages[0]), "林");
        store.apply(&json!({"@type":"updateChatTitle","chat_id":-100,"title":"群改名"}));
        assert_eq!(store.chat_title(-100), "群改名");
    }

    #[test]
    fn sender_metadata_is_bounded_and_irrelevant_users_do_not_redraw() {
        let mut store = Store::default();
        for id in 1..=3000 {
            assert!(
                !store.apply(&json!({"@type":"updateUser","user":{"id":id,"first_name":"成员"}}))
            );
        }
        assert_eq!(store.sender_names.len(), 1024);
        assert_eq!(store.recent_senders.len(), 1024);
        store.open(1);
        store.apply(&json!({"@type":"updateNewMessage","message":{"id":1,"chat_id":1,"sender_id":{"@type":"messageSenderUser","user_id":1}}}));
        assert!(
            store
                .sender_requests()
                .iter()
                .any(|(_, request)| request["user_id"] == 1)
        );
        assert!(store.apply(&json!({"@type":"user","id":1,"first_name":"恢复的名字"})));
        assert_eq!(store.sender_label(&store.messages[0]), "恢复的名字");
    }

    #[test]
    fn folders_track_independent_order_removal_archive_and_server_names() {
        let mut store = Store::default();
        store.apply(&json!({"@type":"updateChatFolders","main_chat_list_position":1,"chat_folders":[{"id":7,"name":{"text":{"text":"工作"}}},{"id":8,"title":"朋友"}]}));
        assert_eq!(
            store.chat_lists(),
            vec![
                (ChatList::Folder(7), "工作".into()),
                (ChatList::Main, "全部".into()),
                (ChatList::Folder(8), "朋友".into()),
                (ChatList::Archive, "归档".into())
            ]
        );
        for (id, order, folder_order) in [(1, 100, 10), (2, 50, 20)] {
            store.apply(&json!({"@type":"updateNewChat","chat":{"id":id,"title":"chat","positions":[{"list":{"@type":"chatListMain"},"order":order.to_string()},{"list":{"@type":"chatListFolder","chat_folder_id":7},"order":folder_order.to_string()}]}}));
        }
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![1, 2]);
        store.select_list(ChatList::Folder(7));
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![2, 1]);
        store.apply(&json!({"@type":"updateChatPosition","chat_id":2,"position":{"list":{"@type":"chatListFolder","chat_folder_id":7},"order":"0"}}));
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![1]);
        store.apply(&json!({"@type":"updateChatLastMessage","chat_id":1,"positions":[{"list":{"@type":"chatListArchive"},"order":"999"}]}));
        assert_eq!(store.chat_ids().count(), 0);
        store.select_list(ChatList::Archive);
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![1]);
        store.select_list(ChatList::Main);
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![2]);
        store.select_list(ChatList::Folder(7));
        store.apply(
            &json!({"@type":"updateChatFolders","chat_folders":[],"main_chat_list_position":0}),
        );
        assert_eq!(store.selected_list, ChatList::Main);
        assert!(!store.select_list(ChatList::Folder(7)));
    }

    #[test]
    fn exhausted_chat_list_is_scoped_to_its_load_request() {
        let mut store = Store::default();
        let request = crate::actions::load_chat_list(ChatList::Folder(7));
        assert_eq!(
            request["chat_list"],
            json!({"@type":"chatListFolder","chat_folder_id":7})
        );
        assert!(store.apply(&json!({"@type":"error","code":404,"@extra":request["@extra"]})));
        assert!(store.exhausted_lists.contains(&ChatList::Folder(7)));
        assert!(!store.exhausted_lists.contains(&ChatList::Main));
    }

    #[test]
    fn updates_keep_chat_order_and_bound_active_messages() {
        let mut store = Store::default();
        for (id, order) in [(1, "10"), (2, "20")] {
            store.apply(&json!({
                "@type": "updateNewChat",
                "chat": {"id": id, "title": format!("chat {id}"), "unread_count": 0,
                    "positions": [{"list": {"@type": "chatListMain"}, "order": order}]}
            }));
        }
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![2, 1]);
        store.open(2);
        store.apply(&json!({
            "@type": "messages", "@extra": "history:2",
            "messages": [{"id": 5, "chat_id": 2, "is_outgoing": false,
                "content": {"@type": "messageText", "text": {"text": "hello"}}}]
        }));
        assert_eq!(store.messages.len(), 1);
        assert_eq!(store.messages[0].text, "hello");
        store.apply(&json!({
            "@type": "updateChatPosition", "chat_id": 1,
            "position": {"list": {"@type": "chatListMain"}, "order": "30"}
        }));
        assert_eq!(store.chat_ids().collect::<Vec<_>>(), vec![1, 2]);
        for id in 6..=610 {
            store.apply(&json!({
                "@type": "updateNewMessage",
                "message": {"id": id, "chat_id": 2, "content": {
                    "@type": "messageText", "text": {"text": "more"}
                }}
            }));
        }
        assert_eq!(store.messages.len(), 500);
        assert_eq!(store.messages.first().map(|message| message.id), Some(111));
        assert_eq!(store.messages.last().map(|message| message.id), Some(610));
    }

    #[test]
    fn media_paths_and_history_are_scoped_to_open_chat() {
        let mut store = Store::default();
        store.open(7);
        let photo = json!({
            "id": 30, "chat_id": 7, "content": {"@type": "messagePhoto",
                "photo": {"sizes": [{"width": 320, "photo": {"id": 8,
                    "local": {"is_downloading_completed": false, "path": ""}}}]}}
        });
        store.apply(&json!({"@type": "messages", "@extra": "history:7", "messages": [photo]}));
        assert_eq!(store.messages[0].media.as_ref().map(|m| m.file_id), Some(8));
        store.apply(&json!({"@type": "updateFile", "file": {"id": 8,
            "local": {"is_downloading_completed": true, "path": "/tmp/example.png"}}}));
        assert_eq!(
            store.messages[0]
                .media
                .as_ref()
                .and_then(|m| m.path.as_deref()),
            Some("/tmp/example.png")
        );
        store.apply(&json!({"@type": "messages", "@extra": "history:8", "messages": []}));
        assert!(!store.history_exhausted);
        store.apply(&json!({"@type": "messages", "@extra": "history:7", "messages": []}));
        assert!(store.history_exhausted);
    }
}
