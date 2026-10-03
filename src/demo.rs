//! Offline chat fixture for inspecting and testing the TUI without an account.
use crate::tdlib::{TdCommand, TdEvent};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::mpsc::{Receiver, SyncSender};

fn messages(chat_id: i64, path: &str, detail_path: &str) -> Vec<Value> {
    let file = json!({"id":42,"local":{"path":path,"is_downloading_completed":true}});
    let detail = json!({"id":43,"local":{"path":detail_path,"is_downloading_completed":false}});
    let mut messages = vec![
        json!({"id":1,"chat_id":chat_id,"content":{"@type":"messageText","text":{"text":"离线演示：点击分组和会话，滚动消息；这里的操作不会发送到 Telegram。"}}}),
        json!({"id":2,"chat_id":chat_id,"content":{"@type":"messagePhoto","caption":{"text":"图片直接放在这条消息里"},"photo":{"sizes":[{"width":320,"height":160,"photo":file},{"width":1280,"height":640,"photo":detail}]}}}),
        json!({"id":3,"chat_id":chat_id,"content":{"@type":"messageText","text":{"text":"演示回复：点击消息右侧的回复，或右键打开菜单。"}}}),
        json!({"id":4,"chat_id":chat_id,"content":{"@type":"messageSticker","sticker":{"emoji":"👋","format":{"@type":"stickerFormatWebp"},"sticker":file}}}),
        json!({"id":5,"chat_id":chat_id,"is_outgoing":true,"content":{"@type":"messageText","text":{"text":"点击输入框，再点击发送。图片可以单击放大，滚轮缩放。"}}}),
    ];
    for (index, message) in messages.iter_mut().enumerate() {
        message["sender_id"] =
            json!({"@type":"messageSenderUser","user_id":if index % 2 == 0 { 101 } else { 102 }});
    }
    messages
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
        let mut positions = vec![json!({"list":main,"order":(100-id).to_string()})];
        if folder > 0 {
            positions.push(json!({"list":{"@type":"chatListFolder","chat_folder_id":folder},"order":(200-id).to_string()}));
        }
        if !send(
            json!({"@type":"updateNewChat","chat":{"id":id,"title":title,"unread_count":id,"positions":positions,"last_message":{"content":{"@type":"messageText","text":{"text":"点击打开离线演示会话"}}}}}),
        ) {
            return;
        }
    }
    if !send(
        json!({"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateReady"}}),
    ) {
        return;
    }
    let mut next_id = 100;
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
            continue;
        }
        let response = match request["@type"].as_str().unwrap_or("") {
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
            "getChatHistory" => {
                let mut history = if request["from_message_id"].as_i64() == Some(0) && chat != 999 {
                    messages(chat, &path, &detail_path)
                } else {
                    vec![]
                };
                if request["from_message_id"].as_i64() == Some(0) {
                    history.extend(
                        posted
                            .iter()
                            .filter(|m| m["chat_id"].as_i64() == Some(chat))
                            .cloned(),
                    );
                }
                json!({"@type":"messages","@extra":extra,"messages":history})
            }
            "sendMessage" => {
                next_id += 1;
                let message = json!({"@type":"message","id":next_id,"chat_id":chat,"is_outgoing":true,"content":{"@type":"messageText","text":{"text":request.pointer("/input_message_content/text/text").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(||demo_attachment(&request["input_message_content"]))}}});
                if posted.len() == 128 {
                    posted.pop_front();
                }
                posted.push_back(message.clone());
                json!({"@type":"updateNewMessage","message":message})
            }
            "searchChatMessages" => {
                let query = request["query"].as_str().unwrap_or("");
                let found: Vec<_> = messages(chat, &path, &detail_path)
                    .into_iter()
                    .filter(|message| message.to_string().contains(query))
                    .collect();
                json!({"@type":"foundChatMessages","@extra":extra,"messages":found})
            }
            "deleteMessages" => {
                json!({"@type":"updateDeleteMessages","chat_id":chat,"message_ids":request["message_ids"]})
            }
            "editMessageText" => {
                json!({"@type":"updateMessageContent","chat_id":chat,"message_id":request["message_id"],"new_content":{"@type":"messageText","text":request.pointer("/input_message_content/text").cloned().unwrap_or(Value::Null)}})
            }
            "getRecentStickers" | "getFavoriteStickers" | "searchStickers" => {
                json!({"@type":"stickers","@extra":extra,"stickers":demo_stickers(&path,6)})
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
