//! Small UI-facing projection of TDLib updates. No raw JSON is retained.

use std::time::{Duration, Instant};

use std::cmp::Reverse;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use serde_json::Value;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct PinnedBanner {
    pub id: i64,
    pub excerpt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageScope {
    Forum(i64),
    Discussion(i64),
}

impl MessageScope {
    pub fn json(self) -> Value {
        match self {
            Self::Forum(id) => serde_json::json!({"@type":"messageTopicForum","forum_topic_id":id}),
            Self::Discussion(id) => {
                serde_json::json!({"@type":"messageTopicThread","message_thread_id":id})
            }
        }
    }

    fn matches(self, message: &Value) -> bool {
        if let Self::Discussion(root) = self
            && message["id"].as_i64() == Some(root)
        {
            return true;
        }
        if let Some(scope) = message_scope(message) {
            return scope == self;
        }
        if message
            .get("topic_id")
            .is_some_and(|topic| !topic.is_null())
        {
            return false;
        }
        let id = match self {
            Self::Forum(id) | Self::Discussion(id) => id,
        };
        integer(message.get("message_thread_id")) == id
    }
}

fn message_scope(message: &Value) -> Option<MessageScope> {
    let topic = &message["topic_id"];
    match kind(topic) {
        "messageTopicForum" => topic["forum_topic_id"].as_i64().map(MessageScope::Forum),
        "messageTopicThread" => topic["message_thread_id"]
            .as_i64()
            .map(MessageScope::Discussion),
        _ => None,
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct ForumTopic {
    pub thread_id: i64,
    pub title: String,
    pub unread: i64,
    pub pinned: bool,
    pub order: i64,
    pub closed: bool,
    pub last_preview: String,
}

#[derive(Clone)]
pub struct Chat {
    pub id: i64,
    pub title: String,
    pub unread: i64,
    pub preview: String,
    pub read_outbox: i64,
    pub read_inbox: i64,
    pub latest_message_id: i64,
    pub last_stamp: Option<crate::calendar::Stamp>,
    pub pinned: bool,
    pub muted: bool,
    pub mention_count: i64,
    pub draft_preview: String,
    pub typing: String,
    pub subtitle: String,
    pub username: String,
    pub bio: String,
    pub pinned_message: Option<PinnedBanner>,
    pub is_forum: bool,
    pub topics: Vec<ForumTopic>,
    pub active_topic: Option<(i64, String)>,
    pub peer: Option<ChatPeer>,
    pub notifications: NotificationSettings,
    orders: HashMap<ChatList, i64>,
    pinned_lists: HashSet<ChatList>,
}

// Preserve the chat's existing overrides when changing only mute duration.
const NOTIFICATION_FLAGS: &[&str] = &[
    "use_default_sound",
    "show_preview",
    "use_default_show_preview",
    "mute_stories",
    "use_default_mute_stories",
    "use_default_story_sound",
    "show_story_sender",
    "use_default_show_story_sender",
    "show_story_poster",
    "use_default_show_story_poster",
    "disable_pinned_message_notifications",
    "use_default_disable_pinned_message_notifications",
    "disable_mention_notifications",
    "use_default_disable_mention_notifications",
];

#[derive(Clone, Copy, Default)]
pub struct NotificationSettings {
    present: u16,
    enabled: u16,
    sound: Option<i64>,
    story_sound: Option<i64>,
}

impl NotificationSettings {
    fn parse(value: &Value) -> Self {
        let mut settings = Self::default();
        for (index, key) in NOTIFICATION_FLAGS.iter().enumerate() {
            if let Some(enabled) = value[*key].as_bool() {
                settings.present |= 1 << index;
                settings.enabled |= u16::from(enabled) << index;
            }
        }
        let sound_id = |key: &str| {
            value[key]
                .as_i64()
                .or_else(|| value[key].as_str()?.parse().ok())
        };
        settings.sound = sound_id("sound_id");
        settings.story_sound = sound_id("story_sound_id");
        settings
    }

    pub fn preserve(self, value: &mut Value) {
        for (index, key) in NOTIFICATION_FLAGS.iter().enumerate() {
            if self.present & (1 << index) != 0 {
                value[*key] = Value::Bool(self.enabled & (1 << index) != 0);
            }
        }
        for (key, id) in [
            ("sound_id", self.sound),
            ("story_sound_id", self.story_sound),
        ] {
            if let Some(id) = id {
                value[key] = Value::String(id.to_string());
            }
        }
    }

    fn flag(self, key: &str) -> Option<bool> {
        let index = NOTIFICATION_FLAGS.iter().position(|flag| *flag == key)?;
        (self.present & (1 << index) != 0).then_some(self.enabled & (1 << index) != 0)
    }

    pub fn preview_hidden(self) -> bool {
        self.flag("use_default_show_preview") == Some(false)
            && self.flag("show_preview") == Some(false)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChatPeer {
    Private(i64),
    BasicGroup(i64),
    Supergroup(i64),
}

#[derive(Clone)]
pub struct ChatSummary {
    pub id: i64,
    pub username: String,
    pub title: String,
    pub subtitle: String,
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
    pub media_album_id: i64,
    pub thread_id: i64,
    pub reply_count: u32,
    pub can_get_thread: bool,
    extras: Option<Box<MessageExtras>>,
}

#[derive(Clone, Default)]
pub struct MessageExtras {
    pub reply: Option<Reply>,
    pub forwarded_from: Option<ForwardOrigin>,
    pub reactions: Vec<Reaction>,
    pub entities: Vec<TextEntity>,
    pub media_meta: Option<MediaMeta>,
    pub link_preview: Option<LinkPreview>,
    pub inline_keyboard: Vec<Vec<InlineButton>>,
}

impl MessageExtras {
    fn boxed(self) -> Option<Box<Self>> {
        (!self.is_empty()).then(|| Box::new(self))
    }

    fn is_empty(&self) -> bool {
        self.reply.is_none()
            && self.forwarded_from.is_none()
            && self.reactions.is_empty()
            && self.entities.is_empty()
            && self.media_meta.is_none()
            && self.link_preview.is_none()
            && self.inline_keyboard.is_empty()
    }
}

impl MessageInfo {
    pub fn extras(&self) -> &MessageExtras {
        static EMPTY: MessageExtras = MessageExtras {
            reply: None,
            forwarded_from: None,
            reactions: Vec::new(),
            entities: Vec::new(),
            media_meta: None,
            link_preview: None,
            inline_keyboard: Vec::new(),
        };
        self.extras.as_deref().unwrap_or(&EMPTY)
    }

    pub fn extras_mut(&mut self) -> &mut MessageExtras {
        self.extras.get_or_insert_with(Box::default)
    }

    pub fn prune_extras(&mut self) {
        if self.extras.as_ref().is_some_and(|extras| extras.is_empty()) {
            self.extras = None;
        }
    }
}

#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct LinkPreview {
    pub site_name: String,
    pub title: String,
    pub description: String,
    pub url: String,
    pub instant_view_version: i32,
}

impl LinkPreview {
    pub fn has_instant_view(&self) -> bool {
        crate::text::web_url(&self.url)
            && (self.instant_view_version > 0 || crate::text::is_telegraph(&self.url))
    }

    pub fn row_count(&self) -> usize {
        usize::from(!self.title.is_empty() || !self.site_name.is_empty())
            + usize::from(!self.description.is_empty())
            + usize::from(!self.url.is_empty())
            + usize::from(self.has_instant_view())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineButtonKind {
    Url(String),
    Callback(String),
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineButton {
    pub text: String,
    pub kind: InlineButtonKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkItem {
    Url(String),
    InstantView(String),
    Mention(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reaction {
    pub emoji: String,
    pub custom_emoji_id: Option<i64>,
    pub count: u32,
    pub chosen: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityKind {
    Bold,
    Italic,
    Underline,
    Code,
    Pre(String),
    BlockQuote,
    Strike,
    Spoiler,
    Url,
    TextUrl(String),
    Mention,
    BotCommand,
    Hashtag,
    Cashtag,
    EmailAddress,
    PhoneNumber,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEntity {
    pub start: usize,
    pub end: usize,
    pub kind: EntityKind,
}

#[derive(Clone, Default, Debug)]
pub struct MediaMeta {
    pub file_name: String,
    pub size_bytes: u64,
    pub downloaded_bytes: u64,
    pub duration_secs: u32,
    pub downloading: bool,
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
pub enum ForwardOrigin {
    Sender(Sender),
    Hidden(String),
}

#[derive(Clone)]
pub struct Reply {
    pub chat_id: i64,
    pub message_id: i64,
    pub excerpt: String,
}

pub fn album_range(messages: &[Message], index: usize) -> std::ops::Range<usize> {
    let message = &messages[index];
    let before = messages[..index]
        .iter()
        .rev()
        .take_while(|item| message.same_album(item))
        .count();
    let after = messages[index + 1..]
        .iter()
        .take_while(|item| message.same_album(item))
        .count();
    index - before..index + 1 + after
}

impl Message {
    pub fn retained_bytes(&self) -> usize {
        let extras = self.info.extras();
        std::mem::size_of::<Self>()
            + usize::from(self.info.extras.is_some()) * std::mem::size_of::<MessageExtras>()
            + self.text.capacity()
            + self.author_signature.capacity()
            + match &extras.forwarded_from {
                Some(ForwardOrigin::Hidden(name)) => name.capacity(),
                _ => 0,
            }
            + extras
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
            + extras.reactions.capacity() * std::mem::size_of::<Reaction>()
            + extras
                .reactions
                .iter()
                .map(|r| r.emoji.capacity())
                .sum::<usize>()
            + extras.entities.capacity() * std::mem::size_of::<TextEntity>()
            + extras
                .entities
                .iter()
                .map(|e| match &e.kind {
                    EntityKind::TextUrl(text) | EntityKind::Pre(text) => text.capacity(),
                    _ => 0,
                })
                .sum::<usize>()
            + extras
                .media_meta
                .as_ref()
                .map_or(0, |meta| meta.file_name.capacity())
            + extras.link_preview.as_ref().map_or(0, |lp| {
                lp.site_name.capacity()
                    + lp.title.capacity()
                    + lp.description.capacity()
                    + lp.url.capacity()
            })
            + extras.inline_keyboard.capacity() * std::mem::size_of::<Vec<InlineButton>>()
            + extras
                .inline_keyboard
                .iter()
                .map(|row| {
                    row.capacity() * std::mem::size_of::<InlineButton>()
                        + row
                            .iter()
                            .map(|btn| {
                                btn.text.capacity()
                                    + match &btn.kind {
                                        InlineButtonKind::Url(s)
                                        | InlineButtonKind::Callback(s) => s.capacity(),
                                        InlineButtonKind::Other => 0,
                                    }
                            })
                            .sum::<usize>()
                })
                .sum::<usize>()
    }
    pub fn same_album(&self, other: &Self) -> bool {
        self.info.media_album_id != 0 && self.info.media_album_id == other.info.media_album_id
    }

    pub fn inline_buttons(&self) -> impl Iterator<Item = (usize, usize, &InlineButton)> {
        self.info
            .extras()
            .inline_keyboard
            .iter()
            .enumerate()
            .flat_map(|(row, buttons)| {
                buttons
                    .iter()
                    .enumerate()
                    .map(move |(col, button)| (row, col, button))
            })
    }

    pub fn links_and_mentions(&self) -> Vec<LinkItem> {
        let mut items: Vec<LinkItem> = Vec::new();
        let mut push = |item: LinkItem| {
            if items.len() < 32 && !items.contains(&item) {
                items.push(item);
            }
        };
        let preview = self.info.extras().link_preview.as_ref();
        let add_url = |url: &str, push: &mut dyn FnMut(LinkItem)| {
            if crate::text::is_telegraph(url)
                || preview.is_some_and(|lp| lp.url == url && lp.has_instant_view())
            {
                push(LinkItem::InstantView(url.to_owned()));
            }
            push(LinkItem::Url(url.to_owned()));
        };
        for entity in &self.info.extras().entities {
            match &entity.kind {
                EntityKind::TextUrl(url) if !url.is_empty() => add_url(url, &mut push),
                EntityKind::Url => {
                    if let Some(raw) = self.text.get(entity.start..entity.end).map(str::trim)
                        && !raw.is_empty()
                    {
                        let url = if raw.contains("://") {
                            raw.to_owned()
                        } else {
                            format!("https://{raw}")
                        };
                        if url.len() <= crate::text::MAX_WEB_URL_BYTES {
                            add_url(&url, &mut push);
                        }
                    }
                }
                EntityKind::Mention => {
                    if let Some(raw) = self.text.get(entity.start..entity.end).map(str::trim)
                        && raw.starts_with('@')
                        && raw.len() > 1
                    {
                        push(LinkItem::Mention(raw.chars().take(64).collect()));
                    }
                }
                _ => {}
            }
        }
        if let Some(lp) = preview
            && !lp.url.is_empty()
        {
            add_url(&lp.url, &mut push);
        }
        items
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
    #[cfg(test)]
    pub fn status(&self, read_outbox: i64) -> String {
        self.status_formatted(read_outbox, false)
    }
    pub fn status_formatted(&self, read_outbox: i64, compact: bool) -> String {
        let mut parts = Vec::with_capacity(4);
        if let Some(stamp) = self.info.stamp {
            parts.push(stamp.time());
        }
        if self.info.pinned {
            parts.push(if compact { "顶" } else { "置顶" }.into());
        }
        if self.info.edited {
            parts.push(if compact { "编辑" } else { "已编辑" }.into());
        }
        match &self.info.sending {
            Sending::Pending => parts.push(if compact { "…" } else { "发送中" }.into()),
            Sending::Failed { .. } => parts.push(if compact { "失败" } else { "发送失败" }.into()),
            Sending::Sent if self.outgoing => {
                parts.push(if self.id > 0 && self.id <= read_outbox {
                    if compact { "✓✓" } else { "✓✓ 已读" }.into()
                } else {
                    if compact { "✓" } else { "✓ 已发送" }.into()
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
    Voice,
}

impl MediaKind {
    pub fn is_large_file(self) -> bool {
        matches!(self, Self::Document | Self::Video | Self::Voice)
    }
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

struct ChatAction {
    chat: i64,
    sender: Sender,
    label: &'static str,
    expires: Instant,
}

#[derive(Default)]
pub struct Store {
    chats: HashMap<i64, Chat>,
    unlisted_chats: VecDeque<i64>,
    ordered: HashMap<ChatList, ChatOrder>,
    pub selected_list: ChatList,
    pub folders: Vec<ChatFolder>,
    main_list_position: usize,
    pub exhausted_lists: std::collections::HashSet<ChatList>,
    pub active_chat: Option<i64>,
    pub message_scope: Option<MessageScope>,
    pub messages: Vec<Message>,
    pub search_results: Vec<Message>,
    pub global_chat_results: Vec<ChatSummary>,
    pub recent_stickers: Vec<Sticker>,
    pub favorite_stickers: Vec<Sticker>,
    pub installed_sticker_sets: Vec<(i64, String)>,
    pub sticker_sets: VecDeque<(i64, Vec<Sticker>)>,
    pub sticker_results: Vec<Sticker>,
    pub sticker_search_tag: String,
    pub history_exhausted: bool,
    pub context_window: bool,
    chat_actions: VecDeque<ChatAction>,
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
        for chat in self.chats.values_mut() {
            chat.pinned = chat.pinned_lists.contains(&list);
        }
        true
    }

    pub fn chat(&self, id: i64) -> Option<&Chat> {
        self.chats.get(&id)
    }

    pub fn chat_mut_by_id(&mut self, id: i64) -> Option<&mut Chat> {
        self.chats.get_mut(&id)
    }

    pub fn quick_switch_entries(&self, query: &str) -> Vec<(i64, String, String, bool)> {
        let needle = query.trim().to_lowercase();
        type Rank = (u8, Reverse<i64>, Reverse<i64>);
        type Entry = (i64, String, String, bool);
        let mut ranked: Vec<(Rank, Entry)> = Vec::new();
        let mut insert = |key, id, title: &str, subtitle: &str, remote| {
            let index = ranked.partition_point(|(other, ..)| *other < key);
            if index < 32 {
                if ranked.len() == 32 {
                    ranked.pop();
                }
                ranked.insert(
                    index,
                    (key, (id, title.to_owned(), subtitle.to_owned(), remote)),
                );
            }
        };
        let matching = |title: &str, username: &str, preview: &str| {
            let title = title.to_lowercase();
            let user = username.trim_start_matches('@').to_lowercase();
            let username_query = needle.trim_start_matches('@');
            if title == needle || (!username_query.is_empty() && user.starts_with(username_query)) {
                0
            } else if title.starts_with(&needle) {
                1
            } else if title.contains(&needle)
                || (!username_query.is_empty() && user.contains(username_query))
            {
                2
            } else if preview.to_lowercase().contains(&needle) {
                4
            } else {
                5
            }
        };
        for (&id, chat) in &self.chats {
            let remote =
                !needle.is_empty() && self.global_chat_results.iter().any(|item| item.id == id);
            let rank = if needle.is_empty() {
                if chat.pinned {
                    0
                } else if chat.unread > 0 {
                    1
                } else {
                    2
                }
            } else {
                matching(&chat.title, &chat.username, &chat.preview).min(if remote { 3 } else { 5 })
            };
            if rank == 5 {
                continue;
            }
            let order = chat.orders.get(&ChatList::Main).copied().unwrap_or(0);
            let sub = if chat.username.is_empty() {
                chat.preview.clone()
            } else {
                format!("@{} · {}", chat.username, chat.preview)
            };
            insert(
                (rank, Reverse(order), Reverse(id)),
                id,
                &chat.title,
                &sub,
                remote && rank == 3,
            );
        }
        if !needle.is_empty() {
            for remote in &self.global_chat_results {
                if !self.chats.contains_key(&remote.id) {
                    let rank = matching(&remote.title, &remote.username, &remote.subtitle).min(3);
                    insert(
                        (rank, Reverse(0), Reverse(remote.id)),
                        remote.id,
                        &remote.title,
                        &remote.subtitle,
                        true,
                    );
                }
            }
        }
        ranked.into_iter().map(|(_, entry)| entry).collect()
    }

    pub fn has_chat_title(&self, id: i64) -> bool {
        self.chats
            .get(&id)
            .is_some_and(|chat| !chat.title.is_empty())
            || self.sender_names.contains_key(&Sender::Chat(id))
    }

    pub fn notification_chat_title(&self, id: i64) -> Option<&str> {
        self.chats
            .get(&id)
            .map(|chat| chat.title.as_str())
            .filter(|title| !title.is_empty())
            .or_else(|| {
                self.sender_names
                    .get(&Sender::Chat(id))
                    .map(String::as_str)
                    .filter(|title| !title.is_empty())
            })
    }

    pub fn notification_sender(&self, chat_id: i64, message: &Value) -> Option<String> {
        let chat = self.chats.get(&chat_id);
        if chat.is_some_and(|chat| matches!(chat.peer, Some(ChatPeer::Private(_)))) {
            return None;
        }
        let sender = message.get("sender_id").and_then(Sender::parse);
        let name = match sender {
            Some(Sender::User(id)) => self.sender_names.get(&Sender::User(id)).cloned(),
            Some(Sender::Chat(id)) if id != chat_id => {
                self.notification_chat_title(id).map(str::to_owned)
            }
            _ => None,
        }
        .or_else(|| {
            message
                .get("author_signature")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
        })?;
        if self.notification_chat_title(chat_id) == Some(name.as_str()) {
            return None;
        }
        Some(name)
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

    pub fn forward_label(&self, origin: &ForwardOrigin) -> String {
        match origin {
            ForwardOrigin::Sender(sender) => self
                .sender_names
                .get(sender)
                .cloned()
                .unwrap_or_else(|| match sender {
                    Sender::Chat(id) => self.chat_title(*id),
                    Sender::User(id) => format!("用户 {id}"),
                })
                .chars()
                .take(80)
                .collect(),
            ForwardOrigin::Hidden(name) => name.clone(),
        }
    }

    // Only resolve senders in the bounded active message window. Never fetch group members.
    pub fn sender_requests(&mut self) -> Vec<(Sender, Value)> {
        if self.requested_senders.len() >= 1024 {
            self.requested_senders.retain(|sender| {
                self.messages
                    .iter()
                    .chain(&self.search_results)
                    .any(|message| message.sender == Some(*sender)
                        || matches!(message.info.extras().forwarded_from, Some(ForwardOrigin::Sender(origin)) if origin == *sender))
                    || self.active_chat.map(Sender::Chat) == Some(*sender)
                    || self.chat_actions.iter().any(|action| action.sender == *sender)
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
        for sender in active
            .into_iter()
            .chain(self.chat_actions.iter().map(|action| action.sender))
            .chain(
                self.messages
                    .iter()
                    .chain(&self.search_results)
                    .flat_map(|message| {
                        let sender = message.sender.filter(|_| !message.outgoing);
                        let origin = match message.info.extras().forwarded_from {
                            Some(ForwardOrigin::Sender(sender)) => Some(sender),
                            _ => None,
                        };
                        sender.into_iter().chain(origin)
                    }),
            )
        {
            if self.sender_names.contains_key(&sender)
                || matches!(sender, Sender::Chat(id) if self.chats.get(&id).is_some_and(|chat| !chat.title.is_empty()))
            {
                continue;
            }
            if self.requested_senders.len() >= 1024 {
                break;
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
        let mut typing_changed = false;
        if changed {
            let chats: Vec<_> = self
                .chat_actions
                .iter()
                .filter(|action| action.sender == sender)
                .map(|action| action.chat)
                .collect();
            for id in chats {
                typing_changed |= self.refresh_typing(id);
            }
        }
        typing_changed || changed
            && (self
                .messages
                .iter()
                .chain(&self.search_results)
                .any(|message| message.sender == Some(sender)
                    || matches!(message.info.extras().forwarded_from, Some(ForwardOrigin::Sender(origin)) if origin == sender))
                || self.active_chat.map(Sender::Chat) == Some(sender))
    }

    pub fn open(&mut self, id: i64) {
        if self.active_chat != Some(id) {
            if let Some(chat) = self.active_chat.and_then(|id| self.chats.get_mut(&id)) {
                chat.topics = Vec::new();
            }
            self.set_message_scope(None);
            self.active_chat = Some(id);
            self.requested_senders.clear();
        }
    }

    pub fn set_message_scope(&mut self, scope: Option<MessageScope>) {
        if let Some(chat) = self.active_chat.and_then(|id| self.chats.get_mut(&id)) {
            chat.active_topic = None;
        }
        self.message_scope = scope;
        self.messages = Vec::new();
        self.search_results = Vec::new();
        self.history_exhausted = false;
        self.context_window = false;
    }

    pub fn message_in_window(&self, value: &Value) -> bool {
        self.active_chat
            .is_some_and(|id| value["chat_id"].as_i64() == Some(id))
            && self.message_scope.is_none_or(|scope| scope.matches(value))
    }

    pub fn merge_scoped_history(&mut self, values: &[Value]) {
        let oldest = self.oldest_message_id();
        for value in values.iter().take(50) {
            self.add_message(value, true);
        }
        self.history_exhausted = self.messages.is_empty()
            || values.is_empty()
            || oldest.is_some_and(|old| self.oldest_message_id() == Some(old));
    }

    pub fn update_topics(&mut self, chat_id: i64, value: &Value) {
        let Some(chat) = self.chats.get_mut(&chat_id) else {
            return;
        };
        let mut topics = Vec::new();
        for value in value["topics"].as_array().into_iter().flatten() {
            if topics.len() == 64 {
                break;
            }
            if let Some(topic) = parse_forum_topic(value)
                && !topics
                    .iter()
                    .any(|existing: &ForumTopic| existing.thread_id == topic.thread_id)
            {
                topics.push(topic);
            }
        }
        topics.sort_by_key(|topic| {
            (
                Reverse(topic.pinned),
                Reverse(topic.order),
                Reverse(topic.thread_id),
            )
        });
        chat.topics = topics;
    }

    fn apply_chat_action(&mut self, update: &Value, now: Instant) -> bool {
        let (Some(chat), Some(sender)) = (
            update["chat_id"].as_i64(),
            Sender::parse(&update["sender_id"]),
        ) else {
            return false;
        };
        if !self.chats.contains_key(&chat) {
            return false;
        }
        let label = match kind(&update["action"]) {
            "chatActionTyping" => "正在输入…",
            "chatActionUploadingPhoto" => "正在发送图片…",
            "chatActionUploadingDocument" => "正在发送文件…",
            "chatActionUploadingVideo" => "正在发送视频…",
            "chatActionRecordingVoiceNote" => "正在录制语音…",
            "chatActionChoosingSticker" => "正在选贴纸…",
            _ => "",
        };
        self.chat_actions
            .retain(|action| (action.chat, action.sender) != (chat, sender));
        let mut changed = false;
        if !label.is_empty() {
            if self.chat_actions.len() == 128
                && let Some(old) = self.chat_actions.pop_front()
            {
                changed |= self.refresh_typing(old.chat);
            }
            self.chat_actions.push_back(ChatAction {
                chat,
                sender,
                label,
                expires: now + Duration::from_secs(5),
            });
        }
        self.refresh_typing(chat) || changed
    }

    fn refresh_typing(&mut self, id: i64) -> bool {
        let Some(chat) = self.chats.get(&id) else {
            return false;
        };
        let typing = self
            .chat_actions
            .iter()
            .rev()
            .find(|action| action.chat == id)
            .map_or_else(String::new, |action| {
                if matches!(
                    chat.peer,
                    Some(ChatPeer::BasicGroup(_) | ChatPeer::Supergroup(_))
                ) {
                    let name = self
                        .sender_names
                        .get(&action.sender)
                        .cloned()
                        .unwrap_or_else(|| match action.sender {
                            Sender::Chat(id) => self.chat_title(id),
                            Sender::User(id) => format!("用户 {id}"),
                        });
                    format!(
                        "{} {}",
                        name.chars().take(48).collect::<String>(),
                        action.label
                    )
                } else {
                    action.label.to_owned()
                }
            });
        let chat = self.chats.get_mut(&id).expect("chat");
        if chat.typing == typing {
            return false;
        }
        chat.typing = typing;
        true
    }

    pub fn typing_deadline(&self) -> Option<Instant> {
        self.chat_actions.front().map(|action| action.expires)
    }

    pub fn expire_typing(&mut self, now: Instant) -> bool {
        let mut changed = false;
        while self
            .typing_deadline()
            .is_some_and(|deadline| deadline <= now)
        {
            let id = self.chat_actions.pop_front().expect("expired action").chat;
            changed |= self.refresh_typing(id);
        }
        changed
    }

    pub fn remote_chat_summary(&self, id: i64, value: &Value) -> Option<ChatSummary> {
        let (title, subtitle, username) = if let Some(chat) = self.chat(id) {
            (
                chat.title.clone(),
                chat.preview.chars().take(160).collect(),
                chat.username.clone(),
            )
        } else if value["id"].as_i64() == Some(id) && kind(value) == "chat" {
            (
                bounded_text(value.get("title"), 128),
                preview(value.get("last_message"))
                    .chars()
                    .take(160)
                    .collect(),
                bounded_text(
                    value
                        .pointer("/usernames/active_usernames/0")
                        .or_else(|| value.get("username")),
                    64,
                ),
            )
        } else {
            return None;
        };
        Some(ChatSummary {
            id,
            title,
            subtitle,
            username,
        })
    }

    pub fn extend_context(&mut self, values: &[Value], from: i64) -> bool {
        let active = self.active_chat;
        let mut count = 0;
        let mut newest = from;
        // TDLib returns newest first. Insert in chronological order so trimming retains the newer edge.
        for value in values.iter().take(50).rev() {
            if !self.message_in_window(value) {
                continue;
            }
            count += 1;
            if let Some(id) = value["id"].as_i64().filter(|id| *id > from) {
                newest = newest.max(id);
                self.add_message(value, false);
            }
        }
        let latest = active
            .filter(|_| self.message_scope.is_none())
            .and_then(|chat| self.chat(chat))
            .map_or(0, |chat| chat.latest_message_id);
        if (latest > 0 && newest >= latest) || (latest == 0 && count < 50) {
            self.context_window = false;
        }
        newest > from
    }

    pub fn merge_open_chat(&mut self, id: i64, value: &Value, fresh_draft: bool) {
        let Some(chat) = self.chats.get_mut(&id) else {
            self.add_chat(value);
            return;
        };
        if let Some(peer) = parse_peer(value) {
            chat.peer = Some(peer);
        }
        for (field, target) in [
            ("unread_count", &mut chat.unread),
            ("last_read_inbox_message_id", &mut chat.read_inbox),
            ("last_read_outbox_message_id", &mut chat.read_outbox),
        ] {
            if let Some(value) = value[field].as_i64() {
                *target = value;
            }
        }
        if let Some(id) = value.pointer("/last_message/id").and_then(Value::as_i64) {
            chat.latest_message_id = chat.latest_message_id.max(id);
        }
        if let Some(settings) = value.get("notification_settings") {
            chat.notifications = NotificationSettings::parse(settings);
            chat.muted = integer(settings.get("mute_for")) > 0;
        }
        if let Some(pin) = value.get("pinned_message") {
            chat.pinned_message = parse_pin_banner(pin);
        }
        if fresh_draft && value.get("draft_message").is_some() {
            chat.draft_preview = bounded_text(
                value.pointer("/draft_message/input_message_text/text/text"),
                160,
            );
        }
    }

    pub fn replace_context(&mut self, values: &[Value], target: i64) -> bool {
        // TDLib returns newest first. Reserve the target before filling around it.
        let Some(origin) = values.iter().position(|value| {
            self.message_in_window(value) && value["id"].as_i64() == Some(target)
        }) else {
            return false;
        };
        let Some(message) = parse_message(&values[origin]) else {
            return false;
        };
        let budget = 6 * 1024 * 1024;
        let mut bytes = message.retained_bytes();
        if bytes > budget {
            return false;
        }
        self.messages.clear();
        self.messages.push(message);
        for distance in 1..values.len().min(500) {
            for index in [origin.checked_sub(distance), origin.checked_add(distance)]
                .into_iter()
                .flatten()
            {
                if self.messages.len() >= 500 {
                    break;
                }
                if let Some(value) = values.get(index)
                    && self.message_in_window(value)
                    && let Some(message) = parse_message(value)
                    && message.id != target
                {
                    bytes += message.retained_bytes();
                    if bytes > budget {
                        bytes -= message.retained_bytes();
                        break;
                    }
                    self.messages.push(message);
                }
            }
        }
        self.messages.sort_unstable_by_key(|message| message.id);
        self.messages.dedup_by_key(|message| message.id);
        self.context_window = true;
        self.history_exhausted = false;
        true
    }

    pub fn set_pin_banner(&mut self, chat_id: i64, value: Option<&Value>) -> bool {
        let Some(chat) = self.chats.get_mut(&chat_id) else {
            return false;
        };
        let banner = value.and_then(|message| {
            Some(PinnedBanner {
                id: message["id"].as_i64()?,
                excerpt: preview(Some(message)).chars().take(120).collect(),
            })
        });
        if chat.pinned_message == banner {
            return false;
        }
        chat.pinned_message = banner;
        true
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
                | "updateChatNotificationSettings"
                | "updateChatUnreadMentionCount"
                | "updateMessageInteractionInfo"
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
                    let title = bounded_text(update.get("title"), 128);
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
                for chat in self.chats.values_mut() {
                    chat.orders
                        .retain(|list, _| lists.iter().any(|(candidate, _)| candidate == list));
                    chat.pinned_lists
                        .retain(|list| lists.iter().any(|(candidate, _)| candidate == list));
                }
                self.chats.retain(|id, chat| {
                    !chat.orders.is_empty()
                        || Some(*id) == self.active_chat
                        || self.unlisted_chats.contains(id)
                });
                if !lists.iter().any(|(list, _)| *list == self.selected_list) {
                    self.select_list(ChatList::Main);
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
                    chat.title = bounded_text(update.get("title"), 128);
                }
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64) {
                    self.save_sender(Sender::Chat(id), bounded_text(update.get("title"), 128));
                }
            }
            "updateChatReadInbox" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.unread = integer(update.get("unread_count"));
                    let read_id = integer(update.get("last_read_inbox_message_id"));
                    chat.read_inbox = read_id;
                }
            }
            "updateChatReadOutbox" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.read_outbox = integer(update.get("last_read_outbox_message_id"));
                }
            }
            "updateChatNotificationSettings" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.muted = integer(update.pointer("/notification_settings/mute_for")) > 0;
                    chat.notifications =
                        NotificationSettings::parse(&update["notification_settings"]);
                }
            }
            "updateChatUnreadMentionCount" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.mention_count = integer(update.get("unread_mention_count"));
                }
            }
            "updateChatPosition" => {
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(position) = update.get("position")
                    && let Some(list) = position.get("list").and_then(ChatList::from_json)
                {
                    self.set_position(id, list, position);
                    self.track_unlisted_chat(id);
                }
            }
            "updateChatLastMessage" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.preview = preview(update.get("last_message"));
                    chat.latest_message_id = integer(update.pointer("/last_message/id"));
                    chat.last_stamp = update
                        .pointer("/last_message/date")
                        .map(|d| integer(Some(d)))
                        .and_then(crate::calendar::local);
                }
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(positions) = update.get("positions").and_then(Value::as_array)
                {
                    self.replace_positions(id, positions);
                }
            }
            "updateChatDraftMessage" => {
                if let Some(chat) = self.chat_mut(update) {
                    chat.draft_preview = bounded_text(
                        update.pointer("/draft_message/input_message_text/text/text"),
                        160,
                    );
                }
                if let Some(id) = update.get("chat_id").and_then(Value::as_i64)
                    && let Some(positions) = update.get("positions").and_then(Value::as_array)
                {
                    self.replace_positions(id, positions);
                }
            }
            "updateChatAction" => return self.apply_chat_action(update, Instant::now()),
            "updateMessageInteractionInfo" => {
                if update["chat_id"].as_i64() == self.active_chat {
                    let id = integer(update.get("message_id"));
                    let reactions = parse_reactions(update.get("interaction_info"));
                    let reply_count =
                        integer(update.pointer("/interaction_info/reply_info/reply_count")).max(0)
                            as u32;
                    for message in self
                        .messages
                        .iter_mut()
                        .chain(&mut self.search_results)
                        .filter(|m| m.id == id)
                    {
                        if !reactions.is_empty() || message.info.extras.is_some() {
                            message.info.extras_mut().reactions = reactions.clone();
                            message.info.prune_extras();
                        }
                        if reply_count > 0
                            || update.pointer("/interaction_info/reply_info").is_some()
                        {
                            message.info.reply_count = reply_count;
                            if reply_count > 0 {
                                message.info.can_get_thread = true;
                            }
                        }
                    }
                }
            }
            "message" if !self.context_window => self.add_message(update, false),
            "updateNewMessage" | "updateMessageSendSucceeded" => {
                if kind(update) == "updateMessageSendSucceeded"
                    && update["message"]["chat_id"].as_i64() == self.active_chat
                    && let Some(old_id) = update.get("old_message_id")
                {
                    let old_id = integer(Some(old_id));
                    self.messages.retain(|message| message.id != old_id);
                }
                if let Some(message) = update.get("message") {
                    if let Some(chat) = self.chat_mut(message) {
                        chat.latest_message_id =
                            chat.latest_message_id.max(integer(message.get("id")));
                    }
                    if update_kind == "updateNewMessage"
                        && let (Some(chat), Some(sender)) = (
                            message["chat_id"].as_i64(),
                            Sender::parse(&message["sender_id"]),
                        )
                    {
                        self.chat_actions
                            .retain(|action| (action.chat, action.sender) != (chat, sender));
                        self.refresh_typing(chat);
                    }
                    if !self.context_window
                        || self.messages.iter().any(|current| {
                            Some(current.id) == message["id"].as_i64()
                                || Some(current.id) == update["old_message_id"].as_i64()
                        })
                    {
                        self.add_message(message, false);
                    }
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
                let chat_id = integer(update.get("chat_id"));
                let id = integer(update.get("message_id"));
                if Some(chat_id) == self.active_chat {
                    for message in self
                        .messages
                        .iter_mut()
                        .chain(&mut self.search_results)
                        .filter(|message| message.id == id)
                    {
                        if update_kind == "updateMessageEdited" {
                            message.info.edited = integer(update.get("edit_date")) > 0;
                            if let Some(markup) = update.get("reply_markup") {
                                let keyboard = parse_inline_keyboard(Some(markup));
                                if !keyboard.is_empty() || message.info.extras.is_some() {
                                    message.info.extras_mut().inline_keyboard = keyboard;
                                    message.info.prune_extras();
                                }
                            }
                        } else {
                            message.info.pinned = update["is_pinned"] == true;
                        }
                    }
                }
                if update_kind == "updateMessageEdited"
                    && Some(chat_id) == self.active_chat
                    && update.get("reply_markup").is_some()
                {
                    trim_messages(&mut self.messages, 6 * 1024 * 1024, false);
                    trim_messages(&mut self.search_results, 2 * 1024 * 1024, true);
                }
                if update_kind == "updateMessageIsPinned"
                    && let Some(chat) = self.chats.get_mut(&chat_id)
                {
                    if update["is_pinned"] == true
                        && Some(chat_id) == self.active_chat
                        && let Some(message) = self
                            .messages
                            .iter()
                            .chain(&self.search_results)
                            .find(|message| message.id == id)
                    {
                        chat.pinned_message = Some(PinnedBanner {
                            id,
                            excerpt: message.text.chars().take(120).collect(),
                        });
                    } else if update["is_pinned"] == false
                        && chat
                            .pinned_message
                            .as_ref()
                            .is_some_and(|banner| banner.id == id)
                    {
                        chat.pinned_message = None;
                    }
                }
            }
            "updateMessageContent" => {
                let id = integer(update.get("message_id"));
                if let Some(banner) = self
                    .chat_mut(update)
                    .and_then(|chat| chat.pinned_message.as_mut())
                    && banner.id == id
                {
                    banner.excerpt = content_text(update.get("new_content"))
                        .chars()
                        .take(120)
                        .collect();
                }
                if update.get("chat_id").and_then(Value::as_i64) == self.active_chat {
                    for message in self
                        .messages
                        .iter_mut()
                        .chain(&mut self.search_results)
                        .filter(|message| message.id == id)
                    {
                        let new_content = update.get("new_content");
                        message.text = content_text(new_content);
                        message.info.text_message = kind(&update["new_content"]) == "messageText";
                        let entities = parse_entities(new_content, &message.text);
                        let link_preview = new_content.and_then(parse_link_preview);
                        let media_meta = new_content.and_then(parse_media_meta);
                        if !entities.is_empty()
                            || link_preview.is_some()
                            || media_meta.is_some()
                            || message.info.extras.is_some()
                        {
                            let extras = message.info.extras_mut();
                            extras.entities = entities;
                            extras.link_preview = link_preview;
                            extras.media_meta = media_meta;
                            message.info.prune_extras();
                        }
                        message.media = new_content.and_then(media_ref);
                    }
                    trim_messages(&mut self.messages, 6 * 1024 * 1024, false);
                    trim_messages(&mut self.search_results, 2 * 1024 * 1024, true);
                }
            }
            "updateDeleteMessages" => {
                if update["from_cache"] != true
                    && let Some(chat) = self.chat_mut(update)
                    && chat.pinned_message.as_ref().is_some_and(|pin| {
                        update["message_ids"]
                            .as_array()
                            .is_some_and(|ids| ids.iter().any(|id| id.as_i64() == Some(pin.id)))
                    })
                {
                    chat.pinned_message = None;
                }
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
                    .and_then(|s| s.split(':').next()?.parse::<i64>().ok())
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
                    .and_then(|s| s.split(':').next()?.parse::<i64>().ok())
                    && Some(id) == self.active_chat
                {
                    self.search_results = update
                        .get("messages")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|message| self.message_in_window(message))
                        .take(50)
                        .filter_map(parse_message)
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

    pub fn add_chat(&mut self, value: &Value) {
        let Some(id) = value.get("id").and_then(Value::as_i64) else {
            return;
        };
        self.clear_positions(id);
        self.chats.insert(
            id,
            Chat {
                id,
                title: bounded_text(value.get("title"), 128),
                unread: integer(value.get("unread_count")),
                preview: preview(value.get("last_message")),
                read_outbox: integer(value.get("last_read_outbox_message_id")),
                read_inbox: integer(value.get("last_read_inbox_message_id")),
                latest_message_id: integer(value.pointer("/last_message/id")),
                last_stamp: value
                    .pointer("/last_message/date")
                    .map(|d| integer(Some(d)))
                    .and_then(crate::calendar::local),
                pinned: false,
                muted: integer(value.pointer("/notification_settings/mute_for")) > 0,
                mention_count: integer(value.get("unread_mention_count")),
                draft_preview: bounded_text(
                    value.pointer("/draft_message/input_message_text/text/text"),
                    160,
                ),
                typing: bounded_text(value.get("typing"), 64),
                subtitle: bounded_text(value.get("subtitle"), 96),
                username: bounded_text(value.get("username"), 64),
                bio: bounded_text(value.get("bio"), 512),
                pinned_message: value.get("pinned_message").and_then(parse_pin_banner),
                is_forum: value["is_forum"] == true
                    || value.pointer("/view_as_topics").and_then(Value::as_bool) == Some(true),
                topics: value["topics"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(parse_forum_topic)
                    .take(64)
                    .collect(),
                active_topic: None,
                notifications: NotificationSettings::parse(&value["notification_settings"]),
                peer: parse_peer(value),
                orders: HashMap::new(),
                pinned_lists: HashSet::new(),
            },
        );
        self.save_sender(Sender::Chat(id), bounded_text(value.get("title"), 128));
        if let Some(positions) = value.get("positions").and_then(Value::as_array) {
            self.replace_positions(id, positions);
        } else {
            self.track_unlisted_chat(id);
        }
    }

    fn track_unlisted_chat(&mut self, id: i64) {
        self.unlisted_chats.retain(|candidate| *candidate != id);
        if self
            .chats
            .get(&id)
            .is_none_or(|chat| !chat.orders.is_empty())
        {
            return;
        }
        self.unlisted_chats.push_back(id);
        if self.unlisted_chats.len() > 64
            && let Some(index) = self
                .unlisted_chats
                .iter()
                .position(|old| Some(*old) != self.active_chat)
            && let Some(old) = self.unlisted_chats.remove(index)
        {
            self.chats.remove(&old);
        }
    }

    fn clear_positions(&mut self, id: i64) {
        let previous: Vec<_> = self
            .chats
            .get(&id)
            .map(|chat| chat.orders.keys().copied().collect())
            .unwrap_or_default();
        for list in previous {
            self.set_order(id, list, 0);
        }
        if let Some(chat) = self.chats.get_mut(&id) {
            chat.pinned_lists.clear();
            chat.pinned = false;
        }
    }

    fn replace_positions(&mut self, id: i64, positions: &[Value]) {
        self.clear_positions(id);
        for position in positions {
            if let Some(list) = position.get("list").and_then(ChatList::from_json) {
                self.set_position(id, list, position);
            }
        }
        self.track_unlisted_chat(id);
    }

    fn set_position(&mut self, id: i64, list: ChatList, position: &Value) {
        let order = integer(position.get("order"));
        self.set_order(id, list, order);
        self.set_chat_pinned(id, list, order > 0 && position["is_pinned"] == true);
    }

    pub fn chat_pinned(&self, id: i64, list: ChatList) -> bool {
        self.chat(id)
            .is_some_and(|chat| chat.pinned_lists.contains(&list))
    }

    pub fn set_chat_pinned(&mut self, id: i64, list: ChatList, pinned: bool) {
        if let Some(chat) = self.chats.get_mut(&id) {
            if pinned {
                chat.pinned_lists.insert(list);
            } else {
                chat.pinned_lists.remove(&list);
            }
            if list == self.selected_list {
                chat.pinned = pinned;
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
        if !self.message_in_window(value) {
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

    pub fn set_downloading(&mut self, id: i32, downloading: bool) {
        for message in self.messages.iter_mut().chain(&mut self.search_results) {
            if message
                .media
                .as_ref()
                .is_some_and(|media| media.file_id == id)
                && let Some(meta) = message
                    .info
                    .extras
                    .as_deref_mut()
                    .and_then(|extras| extras.media_meta.as_mut())
            {
                meta.downloading = downloading;
            }
        }
    }

    pub fn update_file(&mut self, file: &Value) -> bool {
        let id = integer(file.get("id")) as i32;
        let path = local_path(file);
        let size = file["size"]
            .as_u64()
            .filter(|size| *size > 0)
            .or_else(|| file["expected_size"].as_u64())
            .unwrap_or_default();
        let downloaded = integer(file.pointer("/local/downloaded_size")).max(0) as u64;
        let downloading = file.pointer("/local/is_downloading_active") == Some(&Value::Bool(true));
        let mut changed = false;
        for message in self
            .messages
            .iter_mut()
            .chain(self.search_results.iter_mut())
        {
            if let Some(media) = &mut message.media {
                if media.file_id == id {
                    if media.path != path {
                        media.path = path.clone();
                        changed = true;
                    }
                    if let Some(meta) = message
                        .info
                        .extras
                        .as_deref_mut()
                        .and_then(|extras| extras.media_meta.as_mut())
                        && (meta.downloaded_bytes != downloaded
                            || meta.downloading != downloading
                            || (size > 0 && meta.size_bytes != size))
                    {
                        if size > 0 {
                            meta.size_bytes = size;
                        }
                        meta.downloaded_bytes = downloaded;
                        meta.downloading = downloading;
                        changed = true;
                    }
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

fn parse_peer(value: &Value) -> Option<ChatPeer> {
    let peer = &value["type"];
    Some(match kind(peer) {
        "chatTypePrivate" | "chatTypeSecret" => ChatPeer::Private(peer["user_id"].as_i64()?),
        "chatTypeBasicGroup" => ChatPeer::BasicGroup(peer["basic_group_id"].as_i64()?),
        "chatTypeSupergroup" => ChatPeer::Supergroup(peer["supergroup_id"].as_i64()?),
        _ => return None,
    })
}

fn parse_pin_banner(value: &Value) -> Option<PinnedBanner> {
    Some(PinnedBanner {
        id: value["id"].as_i64()?,
        excerpt: if value.get("excerpt").is_some() {
            bounded_text(value.get("excerpt"), 120)
        } else {
            preview(Some(value)).chars().take(120).collect()
        },
    })
}

fn parse_forward_origin(value: &Value) -> Option<ForwardOrigin> {
    Some(match kind(value) {
        "messageOriginUser" => {
            ForwardOrigin::Sender(Sender::User(value["sender_user_id"].as_i64()?))
        }
        "messageOriginChat" => {
            ForwardOrigin::Sender(Sender::Chat(value["sender_chat_id"].as_i64()?))
        }
        "messageOriginChannel" => ForwardOrigin::Sender(Sender::Chat(value["chat_id"].as_i64()?)),
        "messageOriginHiddenUser" => {
            ForwardOrigin::Hidden(bounded_text(value.get("sender_name"), 80))
        }
        _ => return None,
    })
}

pub fn search_message_preview(value: &Value) -> String {
    preview(Some(value)).chars().take(160).collect()
}

fn parse_message(value: &Value) -> Option<Message> {
    let text = content_text(value.get("content"));
    let content = value.get("content");
    let entities = parse_entities(content, &text);
    let media = content.and_then(media_ref);
    let media_meta = content.and_then(parse_media_meta);
    let reactions = parse_reactions(value.get("interaction_info"));
    let link_preview = content.and_then(parse_link_preview);
    let inline_keyboard = parse_inline_keyboard(value.get("reply_markup"));
    let reply_count =
        integer(value.pointer("/interaction_info/reply_info/reply_count")).max(0) as u32;
    let can_get_thread = value["can_get_message_thread"] == true || reply_count > 0;
    let media_album_id = integer(value.get("media_album_id"));
    let thread_id = message_scope(value).map_or_else(
        || integer(value.get("message_thread_id")),
        |scope| match scope {
            MessageScope::Forum(id) | MessageScope::Discussion(id) => id,
        },
    );
    Some(Message {
        id: value.get("id")?.as_i64()?,
        text,
        outgoing: value
            .get("is_outgoing")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        sender: value.get("sender_id").and_then(Sender::parse),
        author_signature: bounded_text(value.get("author_signature"), 128),
        media,
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
            media_album_id,
            thread_id,
            reply_count,
            can_get_thread,
            extras: MessageExtras {
                forwarded_from: parse_forward_origin(&value["forward_info"]["origin"]),
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
                reactions,
                entities,
                media_meta,
                link_preview,
                inline_keyboard,
            }
            .boxed(),
        },
    })
}

pub fn parse_forum_topic(value: &Value) -> Option<ForumTopic> {
    let info = value.get("info").unwrap_or(value);
    let thread_id = integer(
        info.get("forum_topic_id")
            .or_else(|| info.get("message_thread_id")),
    );
    if thread_id <= 0 {
        return None;
    }
    Some(ForumTopic {
        thread_id,
        title: bounded_text(info.get("name").or_else(|| info.get("title")), 64),
        unread: integer(value.get("unread_count")),
        pinned: value["is_pinned"] == true,
        order: integer(value.get("order")).max(integer(value.pointer("/last_message/date"))),
        closed: info["is_closed"] == true,
        last_preview: value
            .get("last_message")
            .map(|msg| preview(Some(msg)).chars().take(120).collect())
            .unwrap_or_else(|| bounded_text(value.get("last_preview"), 120)),
    })
}

fn parse_link_preview(content: &Value) -> Option<LinkPreview> {
    let lp = content.get("link_preview")?;
    if lp.is_null() {
        return None;
    }
    let site_name = bounded_text(lp.get("site_name"), 48);
    let title = bounded_text(lp.get("title"), 120);
    let description = bounded_text(
        lp.pointer("/description/text")
            .or_else(|| lp.get("description")),
        180,
    );
    let url = link_url(lp.get("url"), crate::text::MAX_WEB_URL_BYTES);
    let instant_view_version = i32::try_from(integer(lp.get("instant_view_version"))).unwrap_or(0);
    if site_name.is_empty() && title.is_empty() && description.is_empty() && url.is_empty() {
        return None;
    }
    Some(LinkPreview {
        site_name,
        title,
        description,
        url,
        instant_view_version,
    })
}

fn link_url(value: Option<&Value>, max_bytes: usize) -> String {
    value
        .and_then(Value::as_str)
        .filter(|url| url.len() <= max_bytes)
        .unwrap_or_default()
        .to_owned()
}

fn parse_inline_keyboard(markup: Option<&Value>) -> Vec<Vec<InlineButton>> {
    let Some(markup) = markup.filter(|m| kind(m) == "replyMarkupInlineKeyboard") else {
        return Vec::new();
    };
    markup["rows"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|row| {
            let buttons: Vec<InlineButton> = row
                .as_array()?
                .iter()
                .take(6)
                .filter_map(|btn| {
                    let text = bounded_text(btn.get("text"), 48);
                    if text.is_empty() {
                        return None;
                    }
                    let btype = &btn["type"];
                    let kind = match kind(btype) {
                        "inlineKeyboardButtonTypeUrl" => {
                            InlineButtonKind::Url(link_url(btype.get("url"), 256))
                        }
                        "inlineKeyboardButtonTypeCallback" => {
                            InlineButtonKind::Callback(bounded_text(btype.get("data"), 256))
                        }
                        _ => InlineButtonKind::Other,
                    };
                    Some(InlineButton { text, kind })
                })
                .collect();
            (!buttons.is_empty()).then_some(buttons)
        })
        .collect()
}

fn parse_reactions(interaction_info: Option<&Value>) -> Vec<Reaction> {
    interaction_info
        .and_then(|info| info.pointer("/reactions/reactions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let count = integer(item.get("total_count")).max(0) as u32;
            if count == 0 {
                return None;
            }
            let chosen = item["is_chosen"] == true;
            match item.pointer("/type/@type").and_then(Value::as_str) {
                Some("reactionTypeCustomEmoji") => {
                    let custom_id = item
                        .pointer("/type/custom_emoji_id")
                        .and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()))?;
                    Some(Reaction {
                        emoji: "✦".into(),
                        custom_emoji_id: Some(custom_id),
                        count,
                        chosen,
                    })
                }
                _ => {
                    let emoji = item
                        .pointer("/type/emoji")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())?;
                    Some(Reaction {
                        emoji: emoji.chars().take(8).collect(),
                        custom_emoji_id: None,
                        count,
                        chosen,
                    })
                }
            }
        })
        .take(8)
        .collect()
}

fn utf16_to_byte(text: &str, utf16_offset: usize) -> Option<usize> {
    let mut units = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        if units >= utf16_offset {
            return (units == utf16_offset).then_some(byte_idx);
        }
        units += ch.len_utf16();
    }
    (units == utf16_offset).then_some(text.len())
}

fn caption_prefix(content_kind: &str) -> Option<&'static str> {
    Some(match content_kind {
        "messagePhoto" => "[图片] ",
        "messageVideo" => "[视频] ",
        "messageDocument" => "[文件] ",
        "messageAudio" => "[音频] ",
        "messageAnimation" => "[动画] ",
        "messageVoiceNote" => "[语音消息] ",
        _ => return None,
    })
}

fn parse_entities(content: Option<&Value>, full_text: &str) -> Vec<TextEntity> {
    let Some(content) = content else {
        return Vec::new();
    };
    let (formatted, prefix_bytes) = if kind(content) == "messageText" {
        (content.get("text"), 0)
    } else if let Some(prefix) = caption_prefix(kind(content)) {
        (content.get("caption"), prefix.len())
    } else {
        return Vec::new();
    };
    let Some(formatted) = formatted else {
        return Vec::new();
    };
    let raw = formatted.get("text").and_then(Value::as_str).unwrap_or("");
    let Some(list) = formatted.get("entities").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    for item in list.iter().take(64) {
        let offset = integer(item.get("offset")).max(0) as usize;
        let length = integer(item.get("length")).max(0) as usize;
        if length == 0 {
            continue;
        }
        let Some((start, end)) =
            utf16_to_byte(raw, offset).zip(utf16_to_byte(raw, offset.saturating_add(length)))
        else {
            continue;
        };
        let (start, end) = (prefix_bytes + start, prefix_bytes + end);
        if start >= end || end > full_text.len() {
            continue;
        }
        let Some(entity_type) = item.get("type") else {
            continue;
        };
        let kind = match kind(entity_type) {
            "textEntityTypeBold" => EntityKind::Bold,
            "textEntityTypeItalic" => EntityKind::Italic,
            "textEntityTypeCode" => EntityKind::Code,
            "textEntityTypeUnderline" => EntityKind::Underline,
            "textEntityTypePre" => EntityKind::Pre(String::new()),
            "textEntityTypePreCode" => {
                EntityKind::Pre(bounded_text(entity_type.get("language"), 32))
            }
            "textEntityTypeBlockQuote" | "textEntityTypeExpandableBlockQuote" => {
                EntityKind::BlockQuote
            }
            "textEntityTypeBotCommand" => EntityKind::BotCommand,
            "textEntityTypeHashtag" => EntityKind::Hashtag,
            "textEntityTypeCashtag" => EntityKind::Cashtag,
            "textEntityTypeEmailAddress" => EntityKind::EmailAddress,
            "textEntityTypePhoneNumber" => EntityKind::PhoneNumber,
            "textEntityTypeStrikethrough" => EntityKind::Strike,
            "textEntityTypeSpoiler" => EntityKind::Spoiler,
            "textEntityTypeUrl" => EntityKind::Url,
            "textEntityTypeTextUrl" => EntityKind::TextUrl(link_url(
                entity_type.get("url"),
                crate::text::MAX_WEB_URL_BYTES,
            )),
            "textEntityTypeMention" | "textEntityTypeMentionName" => EntityKind::Mention,
            _ => continue,
        };
        result.push(TextEntity { start, end, kind });
    }
    result
}

fn parse_media_meta(content: &Value) -> Option<MediaMeta> {
    let (file_name, file, duration_secs) = match kind(content) {
        "messageDocument" => (
            bounded_text(content.pointer("/document/file_name"), 128),
            content.pointer("/document/document")?,
            0u32,
        ),
        "messageVideo" => (
            bounded_text(content.pointer("/video/file_name"), 128),
            content.pointer("/video/video")?,
            integer(content.pointer("/video/duration")).max(0) as u32,
        ),
        "messageVoiceNote" => (
            String::new(),
            content.pointer("/voice_note/voice")?,
            integer(content.pointer("/voice_note/duration")).max(0) as u32,
        ),
        _ => return None,
    };
    let size_bytes = integer(file.get("size").or_else(|| file.get("expected_size"))).max(0) as u64;
    let downloaded_bytes = integer(file.pointer("/local/downloaded_size")).max(0) as u64;
    let downloading = file.pointer("/local/is_downloading_active") == Some(&Value::Bool(true));
    Some(MediaMeta {
        file_name,
        size_bytes,
        downloaded_bytes,
        duration_secs,
        downloading,
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
        "messageVoiceNote" => (MediaKind::Voice, content.pointer("/voice_note/voice")?),
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
        .map(|message| {
            content_text(message.get("content"))
                .chars()
                .take(256)
                .collect()
        })
        .unwrap_or_default()
}

fn content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    let content_kind = kind(content);
    if let Some(prefix) = caption_prefix(content_kind)
        && let Some(caption) = content.pointer("/caption/text").and_then(Value::as_str)
        && !caption.is_empty()
    {
        return format!("{prefix}{caption}");
    }
    match content_kind {
        "messageText" => string(content.pointer("/text/text")),
        "messagePhoto" => "[图片] ".into(),
        "messageSticker" => format!("[贴纸] {}", string(content.pointer("/sticker/emoji"))),
        "messageVideo" => "[视频] ".into(),
        "messageVoiceNote" => "[语音消息]".into(),
        "messageAudio" => "[音频]".into(),
        "messageAnimation" => "[动画]".into(),
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
    fn message_size_budget() {
        let size = std::mem::size_of::<Message>();
        eprintln!(
            "Message={size} MessageInfo={}",
            std::mem::size_of::<MessageInfo>()
        );
        assert!(
            size <= 256,
            "ordinary messages must stay within 256 bytes, got {size}"
        );
        let mut message =
            parse_message(&json!({"id":9,"content":{"@type":"messageText","text":{"text":"hi"}}}))
                .unwrap();
        assert!(message.info.extras.is_none());
        let plain = message.retained_bytes();
        message.info.extras_mut().reply = Some(Reply {
            chat_id: 1,
            message_id: 2,
            excerpt: "quote".into(),
        });
        let excerpt_bytes = message
            .info
            .extras()
            .reply
            .as_ref()
            .unwrap()
            .excerpt
            .capacity();
        assert_eq!(
            message.retained_bytes(),
            plain + std::mem::size_of::<MessageExtras>() + excerpt_bytes
        );
        message.info.extras_mut().reply = None;
        message.info.prune_extras();
        assert_eq!(message.retained_bytes(), plain);
    }

    #[test]
    fn media_captions_use_utf16_offsets_after_the_display_prefix() {
        for content_type in [
            "messagePhoto",
            "messageVideo",
            "messageDocument",
            "messageAudio",
            "messageAnimation",
            "messageVoiceNote",
        ] {
            let value = json!({"id":1,"content":{"@type":content_type,"caption":{
                "text":"中😀 ✏️ 文",
                "entities":[
                    {"offset":4,"length":2,"type":{"@type":"textEntityTypeUnderline"}},
                    {"offset":2,"length":1,"type":{"@type":"textEntityTypeBold"}},
                    {"offset":0,"length":999,"type":{"@type":"textEntityTypeItalic"}}
                ]
            }}});
            let message = parse_message(&value).unwrap();
            assert!(message.text.ends_with("中😀 ✏️ 文"));
            assert_eq!(message.info.extras().entities.len(), 1);
            let entity = &message.info.extras().entities[0];
            assert_eq!(&message.text[entity.start..entity.end], "✏️");
            assert_eq!(entity.kind, EntityKind::Underline);
        }
    }

    #[test]
    fn retains_block_languages_and_semantic_entity_kinds() {
        let types = [
            ("textEntityTypeBlockQuote", EntityKind::BlockQuote),
            ("textEntityTypeExpandableBlockQuote", EntityKind::BlockQuote),
            ("textEntityTypePre", EntityKind::Pre(String::new())),
            ("textEntityTypePreCode", EntityKind::Pre("rust".into())),
            ("textEntityTypeBotCommand", EntityKind::BotCommand),
            ("textEntityTypeHashtag", EntityKind::Hashtag),
            ("textEntityTypeCashtag", EntityKind::Cashtag),
            ("textEntityTypeEmailAddress", EntityKind::EmailAddress),
            ("textEntityTypePhoneNumber", EntityKind::PhoneNumber),
        ];
        let entities: Vec<_> = types
            .iter()
            .enumerate()
            .map(|(index, (kind, _))| {
                json!({
                    "offset":index,"length":1,"type":{"@type":kind,"language":"rust"}
                })
            })
            .collect();
        let message = parse_message(&json!({"id":1,"content":{"@type":"messageText",
            "text":{"text":"123456789","entities":entities}}}))
        .unwrap();
        assert_eq!(
            message
                .info
                .extras()
                .entities
                .iter()
                .map(|entity| entity.kind.clone())
                .collect::<Vec<_>>(),
            types.map(|(_, kind)| kind)
        );
    }

    #[test]
    fn link_previews_keep_full_urls_and_text_only_telegraph_links_offer_both_actions() {
        let long_url = format!("https://telegra.ph/{}", "path".repeat(150));
        let mut message = parse_message(&json!({"id":1,"content":{
            "@type":"messageText",
            "text":{"text":"详情","entities":[{"offset":0,"length":2,"type":{"@type":"textEntityTypeTextUrl","url":long_url}}]}
        }})).unwrap();
        assert_eq!(
            message.links_and_mentions(),
            [
                LinkItem::InstantView(long_url.clone()),
                LinkItem::Url(long_url.clone())
            ]
        );
        let url_only = parse_link_preview(&json!({"link_preview":{"url":long_url}})).unwrap();
        assert_eq!(url_only.url, long_url);
        assert_eq!(url_only.row_count(), 2);
        let preview = parse_link_preview(&json!({
            "link_preview":{"url":"https://example.org/article","instant_view_version":2}
        }))
        .unwrap();
        assert_eq!(preview.instant_view_version, 2);
        assert!(preview.has_instant_view());
        message.info.extras_mut().link_preview = Some(preview);
        assert_eq!(
            message.links_and_mentions()[2],
            LinkItem::InstantView("https://example.org/article".into())
        );
    }

    #[test]
    fn edited_messages_refresh_and_remove_inline_markup() {
        let mut store = Store {
            active_chat: Some(1),
            ..Store::default()
        };
        store.messages.push(Message {
            id: 5,
            ..Message::default()
        });
        store.apply(&json!({"@type":"updateMessageEdited","chat_id":1,"message_id":5,
            "reply_markup":{"@type":"replyMarkupInlineKeyboard","rows":[[
                {"text":"✏️ Other","type":{"@type":"inlineKeyboardButtonTypeCallback","data":"YQ=="}}
            ]]}}));
        assert_eq!(
            store.messages[0].info.extras().inline_keyboard[0][0].text,
            "✏️ Other"
        );
        store.apply(
            &json!({"@type":"updateMessageEdited","chat_id":1,"message_id":5,"reply_markup":null}),
        );
        assert!(store.messages[0].info.extras().inline_keyboard.is_empty());
    }

    #[test]
    fn forwarded_origins_use_bounded_names_or_existing_sender_lookup() {
        let mut store = Store::default();
        store.open(8);
        let origins = [
            json!({"@type":"messageOriginUser","sender_user_id":12}),
            json!({"@type":"messageOriginChat","sender_chat_id":-10}),
            json!({"@type":"messageOriginChannel","chat_id":-20}),
            json!({"@type":"messageOriginHiddenUser","sender_name":"转".repeat(100)}),
        ];
        for (index, origin) in origins.into_iter().enumerate() {
            store.apply(&json!({"@type":"message","chat_id":8,"id":index+1,"is_outgoing":true,
                "forward_info":{"origin":origin},"content":{"@type":"messageText","text":{"text":"hello"}}}));
        }
        let requests = store.sender_requests();
        for sender in [Sender::User(12), Sender::Chat(-10), Sender::Chat(-20)] {
            assert!(requests.iter().any(|(key, _)| *key == sender));
        }
        store.apply(&json!({"@type":"updateUser","user":{"id":12,"first_name":"小林"}}));
        let label = |index: usize| {
            store.forward_label(
                store.messages[index]
                    .info
                    .extras()
                    .forwarded_from
                    .as_ref()
                    .unwrap(),
            )
        };
        assert_eq!(label(0), "小林");
        assert_eq!(label(3).chars().count(), 80);
        let message = &store.messages[3];
        let mut without_origin = message.clone();
        without_origin.info.extras_mut().forwarded_from = None;
        assert!(message.retained_bytes() >= without_origin.retained_bytes() + 80 * 3);
    }

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
        let reply = store
            .messages
            .last()
            .unwrap()
            .info
            .extras()
            .reply
            .as_ref()
            .unwrap();
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
