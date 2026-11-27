//! TDLib requests used by the interactive client.

use serde_json::{Value, json};

pub fn load_chats() -> Value {
    load_chat_list(crate::store::ChatList::Main)
}

pub fn load_chat_list(list: crate::store::ChatList) -> Value {
    let chat_list = list.json();
    json!({"@type":"loadChats","chat_list":chat_list,"limit":100,"@extra":format!("load-chats:{chat_list}")})
}

pub fn history(chat_id: i64, from_message_id: i64) -> Value {
    json!({
        "@type": "getChatHistory", "chat_id": chat_id,
        "from_message_id": from_message_id, "offset": 0, "limit": 50,
        "only_local": false, "@extra": format!("history:{chat_id}")
    })
}

fn text_content(text: Value) -> Value {
    json!({
        "@type": "inputMessageText",
        "text": text,
        "link_preview_options": null,
        "clear_draft": true
    })
}

pub fn send_text(chat_id: i64, text: String, reply_to: Option<i64>) -> Value {
    let reply_to = reply_to.map(|message_id| {
        json!({"@type": "inputMessageReplyToMessage", "message_id": message_id,
            "quote": null, "checklist_task_id": 0})
    });
    json!({
        "@type": "sendMessage", "chat_id": chat_id, "topic_id": null,
        "reply_to": reply_to, "options": null, "reply_markup": null,
        "input_message_content": text_content(crate::markdown::formatted_text(&text))
    })
}

pub fn edit_text(chat_id: i64, message_id: i64, text: String) -> Value {
    json!({
        "@type": "editMessageText", "chat_id": chat_id, "message_id": message_id,
        "reply_markup": null, "input_message_content": text_content(crate::markdown::formatted_text(&text))
    })
}

pub fn send_photo(chat_id: i64, path: &str, width: u32, height: u32) -> Value {
    json!({
        "@type": "sendMessage", "chat_id": chat_id, "topic_id": null,
        "reply_to": null, "options": null, "reply_markup": null,
        "input_message_content": {
            "@type": "inputMessagePhoto",
            "photo": {"@type": "inputFileLocal", "path": path},
            "thumbnail": null, "added_sticker_file_ids": [],
            "width": width, "height": height, "caption": null,
            "show_caption_above_media": false, "self_destruct_type": null,
            "has_spoiler": false
        }
    })
}

pub fn send_file(chat_id: i64, path: &str) -> Value {
    json!({
        "@type": "sendMessage", "chat_id": chat_id, "topic_id": null,
        "reply_to": null, "options": null, "reply_markup": null,
        "input_message_content": {
            "@type": "inputMessageDocument",
            "document": {"@type": "inputFileLocal", "path": path},
            "thumbnail": null, "disable_content_type_detection": true,
            "caption": null
        }
    })
}

pub fn delete_many(chat_id: i64, message_ids: Vec<i64>, for_everyone: bool) -> Value {
    json!({
        "@type": "deleteMessages", "chat_id": chat_id,
        "message_ids": message_ids, "revoke": for_everyone
    })
}

pub fn forward(from_chat_id: i64, message_id: i64, target_chat_id: i64) -> Value {
    forward_many(from_chat_id, vec![message_id], target_chat_id)
}

pub fn forward_many(from_chat_id: i64, message_ids: Vec<i64>, target_chat_id: i64) -> Value {
    json!({
        "@type": "forwardMessages", "chat_id": target_chat_id,
        "topic_id": null, "from_chat_id": from_chat_id,
        "message_ids": message_ids, "options": null,
        "send_copy": false, "remove_caption": false
    })
}

pub fn react(chat_id: i64, message_id: i64, emoji: String) -> Value {
    json!({
        "@type": "addMessageReaction", "chat_id": chat_id,
        "message_id": message_id,
        "reaction_type": {"@type": "reactionTypeEmoji", "emoji": emoji},
        "is_big": false, "update_recent_reactions": true
    })
}

pub fn remove_reaction(chat_id: i64, message_id: i64, emoji: String) -> Value {
    json!({
        "@type": "removeMessageReaction", "chat_id": chat_id,
        "message_id": message_id,
        "reaction_type": {"@type": "reactionTypeEmoji", "emoji": emoji}
    })
}

pub fn toggle_chat_mute(chat_id: i64, mute: bool) -> Value {
    json!({
        "@type": "setChatNotificationSettings",
        "chat_id": chat_id,
        "notification_settings": {
            "@type": "chatNotificationSettings",
            "use_default_mute_for": false,
            "mute_for": if mute { i32::MAX } else { 0 },
            "use_default_sound": true,
            "use_default_show_preview": true,
            "use_default_mute_stories": true,
            "use_default_story_sound": true,
            "use_default_show_story_sender": true,
            "use_default_disable_pinned_message_notifications": true,
            "use_default_disable_mention_notifications": true
        },
        "@extra": format!("mute:{chat_id}")
    })
}

pub fn toggle_chat_pin(list: crate::store::ChatList, chat_id: i64, pinned: bool) -> Value {
    json!({
        "@type": "toggleChatIsPinned",
        "chat_list": list.json(),
        "chat_id": chat_id,
        "is_pinned": pinned,
        "@extra": format!("pin:{chat_id}")
    })
}

pub fn jump_to_message(chat_id: i64, message_id: i64) -> Value {
    json!({
        "@type": "getChatHistory",
        "chat_id": chat_id,
        "from_message_id": message_id,
        "offset": -20,
        "limit": 50,
        "only_local": false,
        "@extra": format!("jump:{chat_id}:{message_id}")
    })
}

pub fn search_chats(query: String) -> Value {
    json!({
        "@type": "searchChats",
        "query": query,
        "limit": 32,
        "@extra": "quick-switch"
    })
}

pub fn search(chat_id: i64, query: String) -> Value {
    json!({
        "@type": "searchChatMessages", "chat_id": chat_id,
        "topic_id": null, "query": query, "sender_id": null,
        "from_message_id": 0, "offset": 0, "limit": 50, "filter": null,
        "@extra": format!("search:{chat_id}")
    })
}

pub fn download(file_id: i32) -> Value {
    json!({
        "@type": "downloadFile", "file_id": file_id,
        "priority": 16, "offset": 0, "limit": 0, "synchronous": false,
        "@extra": format!("download:{file_id}")
    })
}

pub fn resend(chat_id: i64, message_id: i64) -> Value {
    json!({"@type":"resendMessages", "chat_id":chat_id,
        "message_ids":[message_id], "paid_message_star_count":0,
        "@extra":format!("resend:{chat_id}")})
}

pub fn recent_stickers() -> Value {
    json!({"@type": "getRecentStickers", "is_attached": false,
        "@extra": "recent-stickers"})
}

pub fn send_sticker(chat_id: i64, file_id: i32, emoji: &str, width: i32, height: i32) -> Value {
    json!({
        "@type": "sendMessage", "chat_id": chat_id, "topic_id": null,
        "reply_to": null, "options": null, "reply_markup": null,
        "input_message_content": {
            "@type": "inputMessageSticker",
            "sticker": {"@type": "inputFileId", "id": file_id},
            "thumbnail": null, "width": width, "height": height, "emoji": emoji
        }
    })
}

pub fn view_messages(chat_id: i64, ids: Vec<i64>) -> Value {
    json!({
        "@type": "viewMessages", "chat_id": chat_id,
        "message_ids": ids, "source": {"@type":"messageSourceChatHistory"}, "force_read": false
    })
}

pub fn set_draft(chat_id: i64, text: &str, reply_to: Option<i64>) -> Value {
    let draft = (!text.is_empty() || reply_to.is_some()).then(|| {
        let mut content = text_content(json!({"@type":"formattedText","text":text,"entities":[]}));
        content["clear_draft"] = json!(false);
        json!({
            "@type":"draftMessage",
            "reply_to":reply_to.map(|message_id| json!({
                "@type":"inputMessageReplyToMessage", "message_id":message_id
            })),
            "input_message_text":content
        })
    });
    json!({"@type":"setChatDraftMessage","chat_id":chat_id,"topic_id":null,
        "draft_message":draft,"@extra":format!("draft:{chat_id}")})
}

pub fn get_instant_view(url: &str) -> Value {
    json!({"@type":"getWebPageInstantView","url":url,"only_local":false,"@extra":"instant-view"})
}

pub fn get_proxies() -> Value {
    json!({"@type": "getProxies", "@extra": "proxy-list"})
}

pub fn save_proxy(parsed: &crate::proxy::ParsedProxy, existing: Option<i32>) -> Value {
    let mut request = json!({
        "@type": if existing.is_some() { "editProxy" } else { "addProxy" },
        "proxy": {
            "@type": "proxy",
            "server": parsed.server,
            "port": parsed.port,
            "type": crate::proxy::proxy_type_json(parsed)
        },
        "enable": true
    });
    if let Some(id) = existing {
        request["proxy_id"] = json!(id);
    }
    request
}

pub fn enable_proxy(proxy_id: i32) -> Value {
    json!({"@type": "enableProxy", "proxy_id": proxy_id, "@extra": format!("proxy-toggle:{proxy_id}:1")})
}

pub fn disable_proxy() -> Value {
    json!({"@type": "disableProxy", "@extra": "proxy-toggle:0:0"})
}

pub fn remove_proxy(proxy_id: i32) -> Value {
    json!({"@type": "removeProxy", "proxy_id": proxy_id, "@extra": format!("proxy-remove:{proxy_id}")})
}

pub fn ping_proxy(proxy_id: i32) -> Value {
    json!({"@type": "pingProxy", "proxy_id": proxy_id, "@extra": format!("proxy-ping:{proxy_id}")})
}

pub fn search_public_chat(username: &str) -> Value {
    json!({
        "@type": "searchPublicChat",
        "username": username.trim_start_matches('@'),
        "@extra": format!("mention:{}", username.trim_start_matches('@'))
    })
}

pub fn callback_query(chat_id: i64, message_id: i64, data_base64: &str) -> Value {
    json!({
        "@type": "getCallbackQueryAnswer",
        "chat_id": chat_id,
        "message_id": message_id,
        "payload": {
            "@type": "callbackQueryPayloadData",
            "data": data_base64
        },
        "@extra": format!("callback:{chat_id}:{message_id}")
    })
}

pub fn get_forum_topics(chat_id: i64) -> Value {
    json!({
        "@type": "getForumTopics",
        "chat_id": chat_id,
        "query": "",
        "offset_date": 0,
        "offset_message_id": 0,
        "offset_forum_topic_id": 0,
        "limit": 64,
        "@extra": format!("topics:{chat_id}")
    })
}

pub fn get_message_thread(chat_id: i64, message_id: i64) -> Value {
    json!({
        "@type": "getMessageThread",
        "chat_id": chat_id,
        "message_id": message_id,
        "@extra": format!("thread-info:{chat_id}:{message_id}")
    })
}

pub fn get_message_thread_history(chat_id: i64, message_id: i64, from_message_id: i64) -> Value {
    json!({
        "@type": "getMessageThreadHistory",
        "chat_id": chat_id,
        "message_id": message_id,
        "from_message_id": from_message_id,
        "offset": 0,
        "limit": 50,
        "@extra": format!("thread-history:{chat_id}:{message_id}")
    })
}

pub fn scoped_history(chat: i64, scope: crate::store::MessageScope, from: i64) -> Value {
    match scope {
        crate::store::MessageScope::Forum(id) => json!({
            "@type":"getForumTopicHistory","chat_id":chat,"forum_topic_id":id,
            "from_message_id":from,"offset":0,"limit":50,
            "@extra":format!("thread-history:{chat}:{id}")
        }),
        crate::store::MessageScope::Discussion(id) => get_message_thread_history(chat, id, from),
    }
}

pub fn read_all_chat_mentions(chat_id: i64) -> Value {
    json!({"@type":"readAllChatMentions", "chat_id":chat_id})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sent_text_is_formatted_but_synced_drafts_keep_the_original_markup() {
        let input = "中文😀 **测试**";
        let sent = send_text(7, input.into(), Some(11));
        let text = &sent["input_message_content"]["text"];
        assert_eq!(sent["reply_to"]["message_id"], 11);
        assert_eq!(text["text"], "中文😀 测试");
        assert_eq!(text["entities"][0]["offset"], 5);
        assert_eq!(text["entities"][0]["length"], 2);
        let draft = set_draft(7, input, None);
        let text = &draft["draft_message"]["input_message_text"]["text"];
        assert_eq!(text["text"], input);
        assert_eq!(text["entities"], json!([]));
    }

    #[test]
    fn reply_and_forward_keep_source_and_target_ids() {
        let reply = send_text(10, "你好".into(), Some(20));
        assert_eq!(reply["chat_id"], 10);
        assert_eq!(reply["reply_to"]["message_id"], 20);
        assert_eq!(reply["input_message_content"]["text"]["text"], "你好");
        let forward = forward(10, 20, 30);
        assert_eq!(forward["from_chat_id"], 10);
        assert_eq!(forward["chat_id"], 30);
        assert_eq!(forward["message_ids"], json!([20]));
    }

    #[test]
    fn search_and_history_tag_responses_with_chat_id() {
        assert_eq!(history(42, 100)["@extra"], "history:42");
        assert_eq!(search(42, "test".into())["@extra"], "search:42");
    }
}
