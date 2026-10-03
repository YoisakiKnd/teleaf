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

fn text_content(text: String) -> Value {
    json!({
        "@type": "inputMessageText",
        "text": {"@type": "formattedText", "text": text, "entities": []},
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
        "input_message_content": text_content(text)
    })
}

pub fn edit_text(chat_id: i64, message_id: i64, text: String) -> Value {
    json!({
        "@type": "editMessageText", "chat_id": chat_id, "message_id": message_id,
        "reply_markup": null, "input_message_content": text_content(text)
    })
}

pub fn send_photo(chat_id: i64, path: &str) -> Value {
    json!({
        "@type": "sendMessage", "chat_id": chat_id, "topic_id": null,
        "reply_to": null, "options": null, "reply_markup": null,
        "input_message_content": {
            "@type": "inputMessagePhoto",
            "photo": {"@type": "inputFileLocal", "path": path},
            "thumbnail": null, "added_sticker_file_ids": [],
            "width": 0, "height": 0, "caption": null,
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

pub fn delete(chat_id: i64, message_id: i64, for_everyone: bool) -> Value {
    json!({
        "@type": "deleteMessages", "chat_id": chat_id,
        "message_ids": [message_id], "revoke": for_everyone
    })
}

pub fn forward(from_chat_id: i64, message_id: i64, target_chat_id: i64) -> Value {
    json!({
        "@type": "forwardMessages", "chat_id": target_chat_id,
        "topic_id": null, "from_chat_id": from_chat_id,
        "message_ids": [message_id], "options": null,
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
        "message_ids": ids, "source": null, "force_read": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
