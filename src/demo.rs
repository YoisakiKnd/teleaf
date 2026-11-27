//! Offline chat fixture for inspecting and testing the TUI without an account.
use crate::tdlib::{TdCommand, TdEvent};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, SyncSender};

fn demo_search_matches(message: &Value, query: &str, filter: Option<&str>) -> bool {
    let content = &message["content"];
    let text = content
        .pointer("/text/text")
        .or_else(|| content.pointer("/caption/text"))
        .or_else(|| content.pointer("/document/file_name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let matches_filter = match filter {
        Some("searchMessagesFilterPhoto") => content["@type"] == "messagePhoto",
        Some("searchMessagesFilterDocument") => content["@type"] == "messageDocument",
        Some("searchMessagesFilterVoiceNote") => content["@type"] == "messageVoiceNote",
        Some("searchMessagesFilterUrl") => text.contains("https://") || text.contains("http://"),
        Some("searchMessagesFilterPinned") => message["is_pinned"] == true,
        _ => true,
    };
    matches_filter && text.to_lowercase().contains(&query.to_lowercase())
}

fn messages(chat_id: i64, path: &str, detail_path: &str) -> Vec<Value> {
    let file = json!({"id":42,"local":{"path":path,"is_downloading_completed":true}});
    let detail = json!({"id":43,"local":{"path":detail_path,"is_downloading_completed":false}});
    if chat_id == 80 {
        return vec![
            json!({"id":50,"chat_id":80,"content":{"@type":"messageText","text":{"text":"讨论群中的频道原帖"}}}),
            json!({"id":51,"chat_id":80,"topic_id":{"@type":"messageTopicThread","message_thread_id":50},
                "content":{"@type":"messageText","text":{"text":"这是关联讨论群中的评论"}}}),
            json!({"id":52,"chat_id":80,"content":{"@type":"messageText","text":{"text":"讨论群的其他消息"}}}),
        ];
    }
    if chat_id == 2 {
        let introduction = "欢迎查看桌面端对齐演示：支持粗体、行内代码 cargo test、链接 https://telegram.org 以及隐藏剧透内容。";
        let mention = "@teleaf_product";
        return vec![
            json!({
                "id": 1, "chat_id": chat_id, "date": 1700000000,
                "sender_id": {"@type": "messageSenderUser", "user_id": 101},
                "forward_info":{"origin":{"@type":"messageOriginHiddenUser","sender_name":"项目公告"}},
                "content": {
                    "@type": "messageText",
                    "text": {
                        "text": format!("{introduction} {mention}"),
                        "entities": [
                            {"offset": 14, "length": 2, "type": {"@type": "textEntityTypeBold"}},
                            {"offset": 22, "length": 10, "type": {"@type": "textEntityTypeCode"}},
                            {"offset": 36, "length": 20, "type": {"@type": "textEntityTypeUrl"}},
                            {"offset": 61, "length": 4, "type": {"@type": "textEntityTypeSpoiler"}},
                            {"offset": introduction.encode_utf16().count() + 1, "length": mention.len(),
                                "type": {"@type": "textEntityTypeMention"}}
                        ]
                    }
                },
                "link_preview": {
                    "site_name": "GitHub",
                    "title": "YoisakiKnd/teleaf · Lightweight Telegram TUI",
                    "description": "支持内联图片、代理管理、桌面通知与完整键鼠交互的轻量终端客户端。",
                    "url": "https://github.com/YoisakiKnd/teleaf"
                },
                "reply_markup": {
                    "@type": "replyMarkupInlineKeyboard",
                    "rows": [[
                        {"text": "查看仓库", "type": {"@type": "inlineKeyboardButtonTypeUrl", "url": "https://github.com/YoisakiKnd/teleaf"}},
                        {"text": "确认已读", "type": {"@type": "inlineKeyboardButtonTypeCallback", "data": "YWNr"}}
                    ]]
                },
                "can_get_message_thread": true,
                "interaction_info": {
                    "reply_info": {"reply_count": 5},
                    "reactions": {
                        "reactions": [
                            {"type": {"@type": "reactionTypeEmoji", "emoji": "👍"}, "total_count": 3, "is_chosen": true},
                            {"type": {"@type": "reactionTypeEmoji", "emoji": "🔥"}, "total_count": 2, "is_chosen": false}
                        ]
                    }
                }
            }),
            json!({
                "id": 2, "chat_id": chat_id, "date": 1700000060, "is_outgoing": true,
                "sender_id": {"@type": "messageSenderUser", "user_id": 999},
                "content": {
                    "@type": "messageDocument",
                    "document": {
                        "file_name": "teleaf-desktop-spec.pdf",
                        "document": {
                            "id": 44, "size": 2516582,
                            "local": {"path": "", "is_downloading_active": true, "is_downloading_completed": false, "downloaded_size": 1635778}
                        }
                    }
                }
            }),
            json!({
                "id": 3, "chat_id": chat_id, "date": 1700000120,
                "sender_id": {"@type": "messageSenderUser", "user_id": 101},
                "content": {
                    "@type": "messageVoiceNote",
                    "voice_note": {
                        "duration": 18,
                        "voice": {"id": 45, "size": 98304, "local": {"path": "", "is_downloading_completed": false, "downloaded_size": 0}}
                    }
                }
            }),
        ];
    }
    let mut messages = vec![
        json!({"id":1,"chat_id":chat_id,"content":{"@type":"messageText","text":{"text":"离线演示：点击分组和会话，滚动消息；这里的操作不会发送到 Telegram。"}}}),
        json!({"id":2,"chat_id":chat_id,"content":{"@type":"messagePhoto","caption":{"text":"图片直接放在这条消息里"},"photo":{"sizes":[{"width":320,"height":160,"photo":file},{"width":1280,"height":640,"photo":detail}]}}}),
        json!({"id":3,"chat_id":chat_id,"content":{"@type":"messageText","text":{"text":"演示回复：点击消息右侧的回复，或右键打开菜单。"}}}),
        json!({"id":4,"chat_id":chat_id,"content":{"@type":"messageSticker","sticker":{"emoji":"👋","format":{"@type":"stickerFormatWebp"},"sticker":file}}}),
        json!({"id":5,"chat_id":chat_id,"is_outgoing":true,"content":{"@type":"messageText","text":{"text":"点击输入框，再点击发送。图片可以单击放大，滚轮缩放。"}}}),
    ];
    for (index, message) in messages.iter_mut().enumerate() {
        if chat_id == 3 {
            message["topic_id"] =
                json!({"@type":"messageTopicForum","forum_topic_id":if index < 3 {11} else {12}});
        }
        message["sender_id"] =
            json!({"@type":"messageSenderUser","user_id":if index % 2 == 0 { 101 } else { 102 }});
    }
    messages
}

struct DemoProxy {
    id: i64,
    server: String,
    port: u16,
    kind: &'static str,
    enabled: bool,
}

impl DemoProxy {
    fn json(&self) -> Value {
        json!({"@type":"addedProxy","id":self.id,"is_enabled":self.enabled,
            "proxy":{"@type":"proxy","server":self.server,"port":self.port,"type":{"@type":self.kind}}})
    }
}

pub(crate) fn serve(events: SyncSender<TdEvent>, commands: Receiver<TdCommand>) {
    let directory = std::env::temp_dir().join(format!("tg-tui-demo-{}", std::process::id()));
    let path = directory.join("preview.png");
    let detail_path = directory.join("detail.png");
    if let Err(error) = std::fs::create_dir_all(&directory).and_then(|_| {
        let detail = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(1280, 640, |x, y| {
            image::Rgb([(x * 255 / 1279) as u8, (y * 255 / 639) as u8, 120])
        }));
        detail
            .save(&detail_path)
            .and_then(|_| detail.thumbnail(320, 160).save(&path))
            .map_err(std::io::Error::other)
    }) {
        let _ = events.send(TdEvent::Error(format!("无法创建离线演示：{error}")));
        return;
    }
    let send = |value| events.send(TdEvent::Update(value)).is_ok();
    let path = path.to_string_lossy().into_owned();
    let detail_path = detail_path.to_string_lossy().into_owned();
    for (id, first_name) in [(101, "小林"), (102, "陈同学")] {
        if !send(
            json!({"@type":"updateUser","user":{"id":id,"first_name":first_name,"last_name":""}}),
        ) {
            return;
        }
    }
    let _ = events.send(TdEvent::Connected {
        version: "离线演示".into(),
        path: "/offline-demo".into(),
    });
    if !send(
        json!({"@type":"updateChatFolders","main_chat_list_position":0,"chat_folders":[{"id":7,"name":{"text":{"text":"工作"}}},{"id":8,"name":{"text":{"text":"朋友"}}}]}),
    ) {
        return;
    }
    for (id, title, folder) in [
        (1, "产品讨论", 7),
        (2, "小林", 8),
        (3, "设计讨论", 7),
        (4, "归档示例", 0),
    ] {
        let main = if id == 4 {
            json!({"@type":"chatListArchive"})
        } else {
            json!({"@type":"chatListMain"})
        };
        let mut positions =
            vec![json!({"list":main,"order":(100-id).to_string(),"is_pinned":id == 2})];
        if folder > 0 {
            positions.push(json!({"list":{"@type":"chatListFolder","chat_folder_id":folder},"order":(200-id).to_string(),"is_pinned":false}));
        }
        let mut chat_json = json!({
            "id": id,
            "title": title,
            "unread_count": id,
            "positions": positions,
            "last_message": {"date": 1700000000 + id * 60, "content": {"@type": "messageText", "text": {"text": "点击打开离线演示会话"}}}
        });
        if id == 1 {
            chat_json["subtitle"] = json!("12 位成员 · 3 人在线");
            chat_json["username"] = json!("teleaf_product");
            chat_json["bio"] =
                json!("Teleaf 终端客户端产品讨论组。按 I 查看详情，Ctrl+K 快速切会话。");
        } else if id == 2 {
            chat_json["subtitle"] = json!("在线");
            chat_json["username"] = json!("xiaolin_demo");
            chat_json["bio"] = json!("演示富文本、回应、置顶横幅、未读分隔线与下载进度条。");
            chat_json["unread_mention_count"] = json!(1);
            chat_json["last_read_inbox_message_id"] = json!(2);
            chat_json["pinned_message"] = json!({"id": 1, "excerpt": "欢迎查看桌面端对齐演示：支持粗体、行内代码、链接与回应"});
        } else if id == 3 {
            chat_json["is_forum"] = json!(true);
            chat_json["notification_settings"] = json!({"mute_for": 2147483647});
            chat_json["draft_message"] =
                json!({"input_message_text": {"text": {"text": "评审纪要草稿待发"}}});
        }
        if !send(json!({"@type": "updateNewChat", "chat": chat_json})) {
            return;
        }
    }
    if !send(
        json!({"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateReady"}}),
    ) {
        return;
    }
    let mut next_id = 100;
    let mut proxies = vec![DemoProxy {
        id: 1,
        server: "127.0.0.1".into(),
        port: 7890,
        kind: "proxyTypeSocks5",
        enabled: false,
    }];
    let mut next_proxy = 1;
    let mut posted: VecDeque<Value> = VecDeque::new();
    while let Ok(TdCommand::Request(request)) = commands.recv() {
        let chat = request["chat_id"].as_i64().unwrap_or(1);
        let extra = request.get("@extra").cloned().unwrap_or(Value::Null);
        if request["@type"] == "sendMessageAlbum" {
            for content in request["input_message_contents"]
                .as_array()
                .into_iter()
                .flatten()
            {
                next_id += 1;
                if !send(
                    json!({"@type":"updateNewMessage","message":{"id":next_id,"chat_id":chat,"is_outgoing":true,
                    "content":{"@type":"messageText","text":{"text":demo_attachment(content)}}}}),
                ) {
                    break;
                }
            }
            if extra
                .as_str()
                .is_some_and(|tag| tag.starts_with("attachment:"))
            {
                send(json!({"@type":"ok","@extra":extra}));
            }
            continue;
        }
        let response = match request["@type"].as_str().unwrap_or("") {
            "getProxies" => {
                let entries: Vec<_> = proxies.iter().map(DemoProxy::json).collect();
                json!({"@type":"addedProxies","@extra":extra,"proxies":entries})
            }
            "addProxy" | "editProxy" => {
                let id = request["proxy_id"].as_i64().unwrap_or_else(|| {
                    next_proxy += 1;
                    next_proxy
                });
                let enabled = request["enable"] == true;
                if enabled {
                    for proxy in &mut proxies {
                        proxy.enabled = false;
                    }
                }
                let kind = match request["proxy"]["type"]["@type"].as_str() {
                    Some("proxyTypeHttp") => "proxyTypeHttp",
                    Some("proxyTypeMtproto") => "proxyTypeMtproto",
                    _ => "proxyTypeSocks5",
                };
                let proxy = DemoProxy {
                    id,
                    server: request["proxy"]["server"]
                        .as_str()
                        .unwrap_or_default()
                        .chars()
                        .take(128)
                        .collect(),
                    port: request["proxy"]["port"]
                        .as_u64()
                        .and_then(|port| u16::try_from(port).ok())
                        .unwrap_or_default(),
                    kind,
                    enabled,
                };
                let mut response = proxy.json();
                if let Some(existing) = proxies.iter_mut().find(|entry| entry.id == id) {
                    *existing = proxy;
                } else if proxies.len() < 32 {
                    proxies.push(proxy);
                }
                response["@extra"] = extra;
                response
            }
            "enableProxy" | "disableProxy" => {
                let id = request["proxy_id"].as_i64();
                for proxy in &mut proxies {
                    proxy.enabled = request["@type"] == "enableProxy" && Some(proxy.id) == id;
                }
                json!({"@type":"ok","@extra":extra})
            }
            "removeProxy" => {
                proxies.retain(|proxy| Some(proxy.id) != request["proxy_id"].as_i64());
                json!({"@type":"ok","@extra":extra})
            }
            "searchPublicChat" => match request["username"].as_str() {
                Some("teleaf" | "teleaf_product") => {
                    json!({"@type":"chat","id":1,"title":"产品讨论","@extra":extra})
                }
                Some("xiaolin_demo") => {
                    json!({"@type":"chat","id":2,"title":"小林","@extra":extra})
                }
                _ => {
                    json!({"@type":"error","code":404,"message":"未找到离线演示账号","@extra":extra})
                }
            },
            "getForumTopics" if chat == 3 => json!({"@type":"forumTopics","@extra":extra,"topics":[
                {"info":{"forum_topic_id":12,"name":"媒体与草稿"},"order":"40"},
                {"info":{"forum_topic_id":11,"name":"设计评审"},"order":"20","is_pinned":true,"unread_count":3}
            ]}),
            "getMessageThread" if chat == 2 && request["message_id"] == 1 => {
                if !send(json!({"@type":"updateNewChat","chat":{"id":80,"title":"关联讨论群"}}))
                {
                    break;
                }
                json!({"@type":"messageThreadInfo","chat_id":80,"message_thread_id":50,"@extra":extra})
            }
            "getMessageThread" => {
                json!({"@type":"error","code":404,"message":"没有评论串","@extra":extra})
            }
            "pingProxy" => json!({"@type": "seconds", "seconds": 0.042, "@extra": extra}),
            "getCallbackQueryAnswer" => {
                json!({"@type": "callbackQueryAnswer", "text": "离线演示：已触发回调", "show_alert": false, "@extra": extra})
            }
            "getMessageProperties" => json!({"@type":"messageProperties","@extra":extra,
                "can_be_forwarded":true,"can_be_deleted_for_all_users":true,"can_be_deleted_only_for_self":true}),
            "getMessageAvailableReactions" => json!({"@type":"availableReactions","@extra":extra,
                "top_reactions":[{"type":{"@type":"reactionTypeEmoji","emoji":"👍"}},{"type":{"@type":"reactionTypeEmoji","emoji":"❤️"}}]}),
            "getStorageStatisticsFast" => {
                json!({"@type":"storageStatisticsFast","files_size":0,"@extra":extra})
            }
            "optimizeStorage" => json!({"@type":"storageStatistics","size":0,"@extra":extra}),
            "getMe" => json!({"@type":"user","id":999,"first_name":"我","@extra":extra}),
            "createPrivateChat" => {
                let saved = json!({"@type":"chat","id":999,"title":"收藏夹",
                    "positions":[{"list":{"@type":"chatListMain"},"order":"90"}]});
                if !send(json!({"@type":"updateNewChat","chat":saved})) {
                    break;
                }
                let mut saved = saved;
                saved["@extra"] = extra;
                saved
            }
            "forwardMessages" => {
                let source = request["from_chat_id"].as_i64().unwrap_or(1);
                let id = request["message_ids"][0].as_i64();
                let original = posted
                    .iter()
                    .rev()
                    .find(|m| m["chat_id"].as_i64() == Some(source) && m["id"].as_i64() == id)
                    .cloned()
                    .or_else(|| {
                        messages(source, &path, &detail_path)
                            .into_iter()
                            .find(|m| m["id"].as_i64() == id)
                    });
                let forwarded = original.map(|mut message| {
                    next_id += 1;
                    message["@type"] = json!("message");
                    message["id"] = json!(next_id);
                    message["chat_id"] = json!(chat);
                    message["is_outgoing"] = json!(true);
                    message["sender_id"] = json!({"@type":"messageSenderUser","user_id":999});
                    message
                });
                if let Some(message) = &forwarded {
                    if posted.len() == 128 {
                        posted.pop_front();
                    }
                    posted.push_back(message.clone());
                    if !send(json!({"@type":"updateNewMessage","message":message})) {
                        break;
                    }
                }
                json!({"@type":"messages","@extra":extra,"messages":[forwarded]})
            }
            "loadChats" => {
                json!({"@type":"error","code":404,"message":"All chats loaded","@extra":extra})
            }
            "getChatHistory" | "getForumTopicHistory" | "getMessageThreadHistory" => {
                let mut history = if chat == 999 {
                    Vec::new()
                } else {
                    messages(chat, &path, &detail_path)
                };
                history.extend(
                    posted
                        .iter()
                        .filter(|m| m["chat_id"].as_i64() == Some(chat))
                        .cloned(),
                );
                if request["@type"] == "getForumTopicHistory" {
                    history
                        .retain(|m| m["topic_id"]["forum_topic_id"] == request["forum_topic_id"]);
                } else if request["@type"] == "getMessageThreadHistory" {
                    history.retain(|m| {
                        m["id"] == request["message_id"]
                            || m["topic_id"]["message_thread_id"] == request["message_id"]
                    });
                }
                history.sort_by_key(|m| std::cmp::Reverse(m["id"].as_i64()));
                let from = request["from_message_id"].as_i64().unwrap_or_default();
                let offset = request["offset"].as_i64().unwrap_or_default();
                let limit = request["limit"].as_u64().unwrap_or(50) as usize;
                let start = if from == 0 {
                    0
                } else {
                    history
                        .partition_point(|m| m["id"].as_i64().is_some_and(|id| id > from))
                        .saturating_add_signed(offset as isize)
                };
                let page: Vec<_> = history.into_iter().skip(start).take(limit).collect();
                json!({"@type":"messages","@extra":extra,"messages":page})
            }
            "sendMessage" => {
                next_id += 1;
                let content = &request["input_message_content"];
                let text = content
                    .get("text")
                    .cloned()
                    .unwrap_or_else(|| json!({"text":demo_attachment(content)}));
                let message = json!({"@type":"message","id":next_id,"chat_id":chat,"topic_id":request["topic_id"],"is_outgoing":true,"content":{"@type":"messageText","text":text}});
                if posted.len() == 128 {
                    posted.pop_front();
                }
                posted.push_back(message.clone());
                if extra
                    .as_str()
                    .is_some_and(|tag| tag.starts_with("attachment:"))
                {
                    send(json!({"@type":"ok","@extra":extra}));
                }
                json!({"@type":"updateNewMessage","message":message})
            }
            "searchChatMessages" => {
                let query = request["query"].as_str().unwrap_or("");
                let found: Vec<_> = messages(chat, &path, &detail_path)
                    .into_iter()
                    .filter(|message| {
                        demo_search_matches(message, query, request["filter"]["@type"].as_str())
                    })
                    .take(50)
                    .collect();
                json!({"@type":"foundChatMessages","@extra":extra,"messages":found})
            }
            "searchMessages" => {
                let query = request["query"].as_str().unwrap_or("");
                let found: Vec<_> = (1..=3)
                    .flat_map(|chat| messages(chat, &path, &detail_path))
                    .filter(|message| demo_search_matches(message, query, None))
                    .take(32)
                    .collect();
                json!({"@type":"foundMessages","@extra":extra,"messages":found})
            }
            "deleteMessages" => {
                if !send(
                    json!({"@type":"updateDeleteMessages","chat_id":chat,"message_ids":request["message_ids"]}),
                ) {
                    break;
                }
                json!({"@type":"ok","@extra":extra})
            }
            "editMessageText" => {
                json!({"@type":"updateMessageContent","chat_id":chat,"message_id":request["message_id"],"new_content":{"@type":"messageText","text":request.pointer("/input_message_content/text").cloned().unwrap_or(Value::Null)}})
            }
            "getRecentStickers" | "getFavoriteStickers" | "searchStickers" => {
                json!({"@type":"stickers","@extra":extra,"stickers":demo_stickers(&path,6)})
            }
            "getSavedAnimations" => {
                json!({"@type":"animations","@extra":extra,"animations":[]})
            }
            "getInstalledStickerSets" => {
                json!({"@type":"stickerSets","@extra":extra,"sets":[{"id":"1001","title":"演示贴纸包"},{"id":"1002","title":"问候"}]})
            }
            "getStickerSet" => {
                json!({"@type":"stickerSet","@extra":extra,"id":request["set_id"],"stickers":demo_stickers(&path,30)})
            }
            "downloadFile" => {
                let id = request["file_id"].as_i64().unwrap_or(42);
                json!({"@type":"file","id":id,"@extra":extra,"local":{"path":if id == 43 { &detail_path } else { &path },"is_downloading_completed":true}})
            }
            _ => json!({"@type":"ok","@extra":extra}),
        };
        if !send(response) {
            break;
        }
    }
    let _ = std::fs::remove_dir_all(directory);
}

fn demo_stickers(path: &str, count: usize) -> Vec<Value> {
    (0..count).map(|i|json!({"sticker":{"id":100+i,"local":{"path":path,"is_downloading_completed":true}},
        "emoji":(["👋","😊","❤️","👍","🎉","🐱"][i%6]),"width":320,"height":160,"format":{"@type":"stickerFormatWebp"}})).collect()
}
fn demo_attachment(content: &Value) -> String {
    if content["@type"] == "inputMessageSticker" {
        return format!("[演示贴纸] {}", content["emoji"].as_str().unwrap_or(""));
    }
    if content["@type"] == "inputMessageAnimation" {
        return "[演示 GIF]".into();
    }
    let path = content
        .pointer("/photo/path")
        .or_else(|| content.pointer("/document/path"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let name = std::path::Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let caption = content
        .pointer("/caption/text")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("[演示附件] {name} {caption}")
}
