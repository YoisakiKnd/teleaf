//! Bounded TDLib state for shared media, batch actions, reactions and storage.
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::{App, InputMode, QUICK_REACTIONS, TdWorker, actions, send_request};

const GLOBAL_LIMIT: usize = 32;
const BATCH_IN_FLIGHT: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaFilter {
    #[default]
    All,
    Photo,
    Document,
    Link,
    Voice,
}

impl MediaFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::All => "全部",
            Self::Photo => "图片",
            Self::Document => "文件",
            Self::Link => "链接",
            Self::Voice => "语音",
        }
    }

    pub fn tdlib_type(self) -> &'static str {
        match self {
            Self::All => "searchMessagesFilterEmpty",
            Self::Photo => "searchMessagesFilterPhoto",
            Self::Document => "searchMessagesFilterDocument",
            Self::Link => "searchMessagesFilterUrl",
            Self::Voice => "searchMessagesFilterVoiceNote",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Photo,
            Self::Photo => Self::Document,
            Self::Document => Self::Link,
            Self::Link => Self::Voice,
            Self::Voice => Self::All,
        }
    }
}

struct GlobalHit {
    chat: i64,
    message: i64,
    title: String,
    preview: String,
}

#[derive(Default)]
struct GlobalSearch {
    query: String,
    generation: u64,
    deadline: Option<Instant>,
    results: Vec<GlobalHit>,
    pending_chats: Vec<i64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Permission {
    Unsent,
    Pending,
    Denied,
    Allowed,
    OnlySelf,
}

struct Batch {
    chat: i64,
    generation: u64,
    forward: bool,
    items: Vec<(i64, Permission)>,
}

impl Batch {
    fn checking(&self) -> bool {
        self.items
            .iter()
            .any(|(_, p)| matches!(p, Permission::Unsent | Permission::Pending))
    }
}

struct BatchResult {
    generation: u64,
    pending: u8,
    rejected: usize,
    skipped: usize,
}

#[derive(Clone, PartialEq, Eq)]
pub enum ReactionChoice {
    Emoji(String),
    Custom(i64),
}

impl ReactionChoice {
    pub fn label(&self) -> &str {
        match self {
            Self::Emoji(emoji) => emoji,
            Self::Custom(_) => "自定义",
        }
    }

    fn request_type(&self) -> Value {
        match self {
            Self::Emoji(emoji) => json!({"@type":"reactionTypeEmoji","emoji":emoji}),
            Self::Custom(id) => {
                json!({"@type":"reactionTypeCustomEmoji","custom_emoji_id":id.to_string()})
            }
        }
    }
}

struct AvailableReactions {
    chat: i64,
    message: i64,
    generation: u64,
    choices: Option<Vec<ReactionChoice>>,
}

#[derive(Default)]
pub struct Parity {
    pub filter: MediaFilter,
    global: GlobalSearch,
    batch: Option<Batch>,
    batch_result: Option<BatchResult>,
    reactions: Option<AvailableReactions>,
    reaction_submission: Option<(i64, u64)>,
    pub reaction_page: usize,
    pub storage_size: Option<u64>,
    pub storage_confirmation: bool,
    storage_requested: bool,
    storage_generation: Option<u64>,
    cleanup_generation: Option<u64>,
}

impl App {
    pub fn global_search_deadline(&self) -> Option<Instant> {
        self.parity.global.deadline
    }

    pub fn global_message_mode(&self) -> bool {
        self.quick_switcher
            .as_ref()
            .is_some_and(|sw| sw.global_messages)
    }

    pub fn quick_entries(&self) -> Vec<(i64, String, String, bool)> {
        if self.global_message_mode() {
            self.parity
                .global
                .results
                .iter()
                .enumerate()
                .map(|(index, hit)| (index as i64, hit.title.clone(), hit.preview.clone(), false))
                .collect()
        } else {
            self.store
                .quick_switch_entries(self.quick_switcher.as_ref().map_or("", |sw| &sw.query))
        }
    }

    pub fn clear_global_search(&mut self) {
        self.parity.global = GlobalSearch::default();
    }

    pub fn service_global_search(&mut self, worker: &TdWorker, now: Instant) -> bool {
        if !self.global_message_mode() {
            self.clear_global_search();
            return false;
        }
        let query = self.quick_switcher.as_ref().expect("switcher").query.trim();
        let mut changed = false;
        if query != self.parity.global.query {
            let has_query = !query.is_empty();
            changed = !self.parity.global.results.is_empty();
            self.parity.global.results.clear();
            self.parity.global.pending_chats.clear();
            self.parity.global.query = query.chars().take(128).collect();
            self.parity.global.generation = self.next_sync_generation();
            self.parity.global.deadline = has_query.then_some(now + Duration::from_millis(250));
        }
        if self.parity.global.deadline.is_some_and(|at| at <= now) {
            self.parity.global.deadline = None;
            let request = json!({"@type":"searchMessages","query":self.parity.global.query,
                "offset":"","limit":GLOBAL_LIMIT,"filter":{"@type":"searchMessagesFilterEmpty"},
                "@extra":format!("global-messages:{}", self.parity.global.generation)});
            changed |= !send_request(self, worker, request);
        }
        while let Some(chat) = self.parity.global.pending_chats.pop() {
            let request = json!({"@type":"getChat","chat_id":chat,
                "@extra":format!("global-chat:{}:{chat}", self.parity.global.generation)});
            if !send_request(self, worker, request) {
                self.parity.global.pending_chats.clear();
                changed = true;
                break;
            }
        }
        changed
    }

    pub fn global_hit_key(&self, index: usize) -> Option<(i64, i64)> {
        self.parity
            .global
            .results
            .get(index)
            .map(|hit| (hit.chat, hit.message))
    }

    pub fn open_global_hit(&mut self, worker: &TdWorker, chat: i64, target: i64) -> bool {
        if !self
            .quick_switcher
            .as_ref()
            .is_some_and(|sw| sw.global_messages && sw.query.trim() == self.parity.global.query)
            || !self
                .parity
                .global
                .results
                .iter()
                .any(|hit| (hit.chat, hit.message) == (chat, target))
        {
            return false;
        }
        self.close_overlays();
        self.selected_chat = Some(chat);
        for request in self.open_selected() {
            if request["@type"] != "getChatHistory" {
                send_request(self, worker, request);
            }
        }
        self.jump_to(worker, target)
    }

    pub fn start_batch(&mut self, worker: &TdWorker, forward: bool) {
        let Some(chat) = self.store.active_chat else {
            return;
        };
        self.parity.batch = Some(Batch {
            chat,
            generation: self.next_sync_generation(),
            forward,
            items: self
                .multi_select
                .iter()
                .take(100)
                .map(|&id| (id, Permission::Unsent))
                .collect(),
        });
        self.service_batch(worker);
    }

    pub fn cancel_batch(&mut self) {
        self.parity.batch = None;
    }

    pub fn service_batch(&mut self, worker: &TdWorker) -> bool {
        let Some(batch) = &mut self.parity.batch else {
            return false;
        };
        let remaining = BATCH_IN_FLIGHT.saturating_sub(
            batch
                .items
                .iter()
                .filter(|(_, state)| *state == Permission::Pending)
                .count(),
        );
        let mut changed = false;
        for (message, state) in batch
            .items
            .iter_mut()
            .filter(|(_, state)| *state == Permission::Unsent)
            .take(remaining)
        {
            let sent = worker.request(
                json!({"@type":"getMessageProperties","chat_id":batch.chat,"message_id":message,
                "@extra":format!("batch-permission:{}:{}:{message}",batch.chat,batch.generation)}),
            );
            *state = if sent.is_ok() {
                Permission::Pending
            } else {
                Permission::Denied
            };
            changed |= sent.is_err();
        }
        changed
    }

    pub fn batch_prompt(&self) -> Option<String> {
        let batch = self.parity.batch.as_ref()?;
        if batch.checking() {
            return Some("正在检查所选消息的权限…".into());
        }
        let denied = batch
            .items
            .iter()
            .filter(|(_, p)| *p == Permission::Denied)
            .count();
        let local = batch
            .items
            .iter()
            .filter(|(_, p)| *p == Permission::OnlySelf)
            .count();
        Some(if batch.forward {
            if denied == 0 {
                "全部可转发 · 确认转发".into()
            } else {
                format!("{denied} 条无法转发，将跳过 · 确认转发")
            }
        } else {
            format!("{local} 条仅为自己删除 · {denied} 条无法删除将跳过")
        })
    }

    pub fn confirm_batch(&mut self, worker: &TdWorker) {
        let Some(batch) = self.parity.batch.as_ref() else {
            return;
        };
        if batch.checking() || Some(batch.chat) != self.store.active_chat {
            return;
        }
        let mut primary: Vec<_> = batch
            .items
            .iter()
            .filter_map(|&(id, p)| (p == Permission::Allowed).then_some(id))
            .collect();
        let mut local: Vec<_> = batch
            .items
            .iter()
            .filter_map(|&(id, p)| (p == Permission::OnlySelf).then_some(id))
            .collect();
        primary.sort_unstable();
        local.sort_unstable();
        let skipped = batch.items.len() - primary.len() - local.len();
        let mut requests = Vec::new();
        if batch.forward {
            let Some(to) = self.selected_chat else { return };
            if !primary.is_empty() {
                requests.push(actions::forward_many(batch.chat, primary, to));
            }
        } else {
            if !primary.is_empty() {
                requests.push(actions::delete_many(batch.chat, primary, true));
            }
            if !local.is_empty() {
                requests.push(actions::delete_many(batch.chat, local, false));
            }
        }
        if self.parity.batch_result.is_some() {
            self.notice = Some("上一项批量操作正在提交".into());
            return;
        }
        let generation = batch.generation;
        let count = requests.len();
        self.close_overlays();
        self.multi_select.clear();
        let mut pending = 0;
        let mut rejected = 0;
        for mut request in requests {
            request["@extra"] = json!(format!("batch-result:{generation}"));
            if send_request(self, worker, request) {
                pending += 1;
            } else {
                rejected += 1;
            }
        }
        if pending > 0 {
            self.parity.batch_result = Some(BatchResult {
                generation,
                pending,
                rejected,
                skipped,
            });
        } else {
            self.notice_error = true;
            self.notice = Some(if count == 0 {
                format!("{skipped} 条消息均无法处理")
            } else {
                "批量请求提交失败".into()
            });
        }
    }

    pub fn request_reactions(&mut self, worker: &TdWorker, message: i64) {
        let Some(chat) = self.store.active_chat else {
            return;
        };
        let generation = self.next_sync_generation();
        self.parity.reaction_page = 0;
        self.parity.reactions = Some(AvailableReactions {
            chat,
            message,
            generation,
            choices: None,
        });
        send_request(
            self,
            worker,
            json!({"@type":"getMessageAvailableReactions","chat_id":chat,
            "message_id":message,"row_size":8,"@extra":format!("reactions:{chat}:{message}:{generation}")}),
        );
    }

    pub fn clear_reactions(&mut self) {
        self.parity.reactions = None;
        self.parity.reaction_page = 0;
    }

    pub fn reaction_choices(&self) -> Vec<ReactionChoice> {
        self.parity
            .reactions
            .as_ref()
            .and_then(|state| state.choices.clone())
            .unwrap_or_else(|| {
                QUICK_REACTIONS
                    .iter()
                    .map(|emoji| ReactionChoice::Emoji((*emoji).into()))
                    .collect()
            })
    }

    pub fn pick_reaction(&mut self, worker: &TdWorker, index: usize) -> bool {
        let (Some(chat), InputMode::React(message)) = (self.store.active_chat, self.input_mode)
        else {
            return false;
        };
        let Some(choice) = self
            .reaction_choices()
            .into_iter()
            .nth(self.parity.reaction_page * 8 + index)
        else {
            return false;
        };
        let mut request = actions::react(chat, message, String::new());
        request["reaction_type"] = choice.request_type();
        let generation = self.tag_reaction_submission(chat, &mut request);
        if send_request(self, worker, request) {
            self.parity.reaction_submission = Some((chat, generation));
            crate::interaction::finish_input(self);
            true
        } else {
            false
        }
    }

    fn tag_reaction_submission(&mut self, chat: i64, request: &mut Value) -> u64 {
        let generation = self.next_sync_generation();
        request["@extra"] = json!(format!("apply-reaction:{chat}:{generation}"));
        generation
    }

    pub fn reset_storage_view(&mut self) {
        self.parity.storage_requested = false;
        self.parity.storage_generation = None;
        self.parity.storage_confirmation = false;
    }

    pub fn service_storage(&mut self, worker: &TdWorker) -> bool {
        if !self.show_settings {
            self.reset_storage_view();
            return false;
        }
        if self.parity.storage_requested
            || self.parity.cleanup_generation.is_some()
            || self.auth.state != "authorizationStateReady"
        {
            return false;
        }
        self.parity.storage_requested = true;
        let generation = self.next_sync_generation();
        self.parity.storage_generation = Some(generation);
        !send_request(
            self,
            worker,
            json!({"@type":"getStorageStatisticsFast",
            "@extra":format!("storage-stat:{generation}")}),
        )
    }

    pub fn confirm_storage_cleanup(&mut self, worker: &TdWorker) {
        if !std::mem::take(&mut self.parity.storage_confirmation)
            || self.parity.cleanup_generation.is_some()
        {
            return;
        }
        let generation = self.next_sync_generation();
        if send_request(
            self,
            worker,
            json!({"@type":"optimizeStorage","size":0,"ttl":0,"count":0,
            "immunity_delay":60,"file_types":[],"chat_ids":[],"exclude_chat_ids":[],
            "return_deleted_file_statistics":false,"chat_limit":0,"@extra":format!("storage-clean:{generation}")}),
        ) {
            self.parity.storage_generation = None;
            self.parity.cleanup_generation = Some(generation);
            self.notice = Some("正在清理 TDLib 缓存…".into());
        }
    }

    pub fn receive_parity(&mut self, value: &Value) -> Option<bool> {
        let mut parts = value["@extra"].as_str()?.split(':');
        let route = parts.next()?;
        let kind = value["@type"].as_str().unwrap_or("");
        match route {
            "global-messages" => {
                let current = parts.next().and_then(|part| part.parse().ok())
                    == Some(self.parity.global.generation)
                    && self.quick_switcher.as_ref().is_some_and(|sw| {
                        sw.global_messages && sw.query.trim() == self.parity.global.query
                    });
                if !current {
                    return Some(false);
                }
                if kind == "error" {
                    self.sync_error(value, "全局消息搜索失败");
                } else if kind == "foundMessages" {
                    let mut results = Vec::new();
                    let mut missing_chats = Vec::new();
                    for msg in value["messages"].as_array().into_iter().flatten() {
                        if results.len() == GLOBAL_LIMIT {
                            break;
                        }
                        let (Some(chat), Some(message)) =
                            (msg["chat_id"].as_i64(), msg["id"].as_i64())
                        else {
                            continue;
                        };
                        if results
                            .iter()
                            .any(|hit: &GlobalHit| (hit.chat, hit.message) == (chat, message))
                        {
                            continue;
                        }
                        if !self.store.has_chat_title(chat)
                            && missing_chats.len() < GLOBAL_LIMIT
                            && !missing_chats.contains(&chat)
                        {
                            missing_chats.push(chat);
                        }
                        results.push(GlobalHit {
                            chat,
                            message,
                            title: self.store.chat_title(chat),
                            preview: crate::store::search_message_preview(msg),
                        });
                    }
                    missing_chats.reverse();
                    self.parity.global.pending_chats = missing_chats;
                    self.parity.global.results = results;
                }
                Some(true)
            }
            "global-chat" => {
                let current = parts.next().and_then(|part| part.parse().ok())
                    == Some(self.parity.global.generation)
                    && self.quick_switcher.as_ref().is_some_and(|sw| {
                        sw.global_messages && sw.query.trim() == self.parity.global.query
                    });
                let chat = parts.next().and_then(|part| part.parse::<i64>().ok());
                if !current || kind != "chat" || chat.is_none() || value["id"].as_i64() != chat {
                    return Some(false);
                }
                let title: String = value["title"]
                    .as_str()
                    .unwrap_or_default()
                    .trim()
                    .chars()
                    .take(128)
                    .collect();
                if title.is_empty() {
                    return Some(false);
                }
                let chat = chat.expect("chat");
                let mut updated = false;
                for hit in &mut self.parity.global.results {
                    if hit.chat == chat && hit.title != title {
                        hit.title = title.clone();
                        updated = true;
                    }
                }
                Some(updated)
            }
            "batch-permission" => {
                let chat = parts.next().and_then(|part| part.parse().ok());
                let generation = parts.next().and_then(|part| part.parse().ok());
                let message = parts.next().and_then(|part| part.parse().ok());
                let Some(batch) = self.parity.batch.as_mut().filter(|batch| {
                    Some(batch.chat) == chat
                        && Some(batch.generation) == generation
                        && chat == self.store.active_chat
                }) else {
                    return Some(false);
                };
                let Some((_, state)) = batch
                    .items
                    .iter_mut()
                    .find(|(id, state)| Some(*id) == message && *state == Permission::Pending)
                else {
                    return Some(false);
                };
                *state = if kind != "messageProperties" {
                    Permission::Denied
                } else if batch.forward {
                    if value["can_be_forwarded"] == true {
                        Permission::Allowed
                    } else {
                        Permission::Denied
                    }
                } else if value["can_be_deleted_for_all_users"] == true {
                    Permission::Allowed
                } else if value["can_be_deleted_only_for_self"] == true {
                    Permission::OnlySelf
                } else {
                    Permission::Denied
                };
                Some(true)
            }
            "batch-result" => {
                let generation = parts.next().and_then(|part| part.parse().ok());
                let Some(result) = self
                    .parity
                    .batch_result
                    .as_mut()
                    .filter(|result| Some(result.generation) == generation)
                else {
                    return Some(false);
                };
                result.pending -= 1;
                result.rejected += usize::from(
                    kind == "error"
                        || (kind == "messages"
                            && value["messages"].as_array().is_none_or(|messages| {
                                messages.is_empty() || messages.iter().any(Value::is_null)
                            })),
                );
                if result.pending == 0 {
                    let result = self.parity.batch_result.take().expect("batch result");
                    self.notice_error = result.rejected > 0;
                    self.notice = Some(if result.skipped == 0 && result.rejected == 0 {
                        "批量操作已完成".into()
                    } else if result.rejected == 0 {
                        format!("批量操作完成 · 跳过 {} 条受限消息", result.skipped)
                    } else {
                        format!(
                            "批量操作结束 · 跳过 {} 条 · {} 项请求失败或部分失败",
                            result.skipped, result.rejected
                        )
                    });
                }
                if kind == "messages"
                    && let Some(messages) = value["messages"].as_array()
                {
                    for message in messages.iter().take(100).filter(|msg| msg.is_object()) {
                        self.store.apply(message);
                    }
                }
                Some(true)
            }
            "apply-reaction" => {
                let chat = parts.next().and_then(|part| part.parse().ok());
                let generation = parts.next().and_then(|part| part.parse().ok());
                if let Some((chat_id, request)) = self.parity.reaction_submission
                    && Some(chat_id) == chat
                    && Some(request) == generation
                {
                    self.parity.reaction_submission = None;
                    if self.store.active_chat == chat && kind == "error" {
                        self.sync_error(value, "回应失败");
                        return Some(true);
                    }
                }
                Some(false)
            }
            "reactions" => {
                let chat = parts.next().and_then(|part| part.parse().ok());
                let message = parts.next().and_then(|part| part.parse().ok());
                let generation = parts.next().and_then(|part| part.parse().ok());
                let Some(state) = self.parity.reactions.as_mut().filter(|state| {
                    Some(state.chat) == chat
                        && Some(state.message) == message
                        && Some(state.generation) == generation
                        && self.store.active_chat == chat
                        && self.input_mode == InputMode::React(state.message)
                }) else {
                    return Some(false);
                };
                if kind == "availableReactions" {
                    let mut choices = Vec::new();
                    for entry in ["top_reactions", "recent_reactions", "popular_reactions"]
                        .into_iter()
                        .flat_map(|key| value[key].as_array().into_iter().flatten())
                    {
                        if choices.len() == 24 {
                            break;
                        }
                        if entry["needs_premium"] == true {
                            continue;
                        }
                        let reaction = &entry["type"];
                        let choice = match reaction["@type"].as_str() {
                            Some("reactionTypeEmoji") => reaction["emoji"]
                                .as_str()
                                .filter(|s| !s.is_empty())
                                .map(|s| ReactionChoice::Emoji(s.chars().take(16).collect())),
                            Some("reactionTypeCustomEmoji") => reaction["custom_emoji_id"]
                                .as_i64()
                                .or_else(|| reaction["custom_emoji_id"].as_str()?.parse().ok())
                                .map(ReactionChoice::Custom),
                            _ => None,
                        };
                        if let Some(choice) = choice
                            && !choices.contains(&choice)
                        {
                            choices.push(choice);
                        }
                    }
                    state.choices = Some(choices);
                }
                Some(true)
            }
            "storage-stat" => {
                let generation = parts.next().and_then(|part| part.parse().ok());
                if generation.is_none()
                    || generation != self.parity.storage_generation
                    || !self.show_settings
                {
                    return Some(false);
                }
                self.parity.storage_generation = None;
                if kind == "error" {
                    self.sync_error(value, "无法读取缓存占用");
                } else {
                    self.parity.storage_size = value["files_size"].as_u64();
                }
                Some(true)
            }
            "storage-clean" => {
                let generation = parts.next().and_then(|part| part.parse().ok());
                if generation.is_none() || generation != self.parity.cleanup_generation {
                    return Some(false);
                }
                self.parity.cleanup_generation = None;
                if kind == "error" {
                    self.sync_error(value, "清理缓存失败");
                } else {
                    self.parity.storage_size = value["size"].as_u64();
                    self.notice = Some("TDLib 缓存清理完成".into());
                    self.notice_error = false;
                }
                Some(true)
            }
            _ => None,
        }
    }
}
