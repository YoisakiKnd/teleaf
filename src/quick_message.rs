//! Quick sends keep identifiers only; TDLib copies content and reuses remote media.
use std::collections::VecDeque;

use serde_json::{Value, json};

use crate::{App, TdWorker, actions, menu, send_request, store::Sending, ui::Action};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Me,
    Chat,
    Send,
}

#[derive(Clone, Copy)]
struct Pending {
    chat: i64,
    message: i64,
    action: Action,
    generation: u64,
    stage: Stage,
}

#[derive(Default)]
pub struct State {
    saved_chat: Option<i64>,
    generation: u64,
    pending: Option<Pending>,
    // Track at most eight accepted sends, including failures outside the active chat.
    sending: VecDeque<(i64, i64, Action)>,
}

impl State {
    pub fn cancel_preparation(&mut self) {
        if self.pending.is_some_and(|p| p.stage != Stage::Send) {
            self.pending = None;
        }
    }

    pub fn request_failed(&mut self) {
        self.pending = None;
    }
}

pub fn eligible(app: &App) -> bool {
    app.selected_message()
        .is_some_and(|m| m.id != 0 && matches!(m.info.sending, Sending::Sent))
}

fn label(action: Action) -> &'static str {
    if action == Action::SaveMessage {
        "收藏"
    } else {
        "复读"
    }
}

fn extra(pending: Pending) -> Value {
    json!({"kind":"quick-message", "generation":pending.generation,
        "stage":match pending.stage {Stage::Me=>"me",Stage::Chat=>"chat",Stage::Send=>"send"}})
}

fn submission(pending: Pending, target: i64) -> Value {
    let mut request = actions::forward(pending.chat, pending.message, target);
    request["send_copy"] = json!(pending.action == Action::Repeat);
    request["options"] = json!({"@type":"messageSendOptions", "paid_message_star_count":0,
        "allow_paid_broadcast":false});
    request["@extra"] = extra(pending);
    request
}

pub fn start(app: &mut App, worker: &TdWorker, action: Action) {
    if app.quick_message.pending.is_some() {
        app.notice = Some("上一条快捷操作正在处理，请稍候".into());
        return;
    }
    let (Some(chat), Some(message)) = (app.store.active_chat, app.selected_message) else {
        app.notice = Some("请先选择一条消息".into());
        return;
    };
    app.quick_message.generation = app.quick_message.generation.wrapping_add(1);
    let target = if action == Action::Repeat {
        Some(chat)
    } else {
        app.quick_message.saved_chat
    };
    let pending = Pending {
        chat,
        message,
        action,
        generation: app.quick_message.generation,
        stage: if target.is_some() {
            Stage::Send
        } else {
            Stage::Me
        },
    };
    let request = target.map_or_else(
        || json!({"@type":"getMe", "@extra":extra(pending)}),
        |target| submission(pending, target),
    );
    app.quick_message.pending = Some(pending);
    if send_request(app, worker, request) {
        app.notice = Some(format!("正在{}…", label(action)));
    } else {
        app.quick_message.pending = None;
    }
}

/// Tagged errors stay out of the login flow; preparation never follows a changed selection.
pub fn receive(app: &mut App, value: &Value) -> Option<(Option<Value>, bool)> {
    if value["@extra"]["kind"] != "quick-message" {
        return None;
    }
    let Some(mut pending) = app.quick_message.pending else {
        return Some((None, false));
    };
    if value["@extra"] != extra(pending) {
        return Some((None, false));
    }
    let name = label(pending.action);
    if value["@type"] == "error" {
        app.quick_message.pending = None;
        app.notice = Some(format!(
            "{name}失败：{}",
            value["message"].as_str().unwrap_or("Telegram 返回错误")
        ));
        return Some((None, true));
    }
    if pending.stage != Stage::Send
        && (app.store.active_chat != Some(pending.chat)
            || app.selected_message != Some(pending.message)
            || !eligible(app)
            || !menu::permissions(app).is_some_and(|p| p.forward))
    {
        app.quick_message.pending = None;
        app.notice = Some("消息选择或权限已变化，已取消收藏".into());
        return Some((None, true));
    }
    let request = match pending.stage {
        Stage::Me if value["@type"] == "user" => {
            let user = value["id"].as_i64().filter(|id| *id > 0);
            user.map(|user| {
                pending.stage = Stage::Chat;
                json!({"@type":"createPrivateChat", "user_id":user,"force":true,"@extra":extra(pending)})
            })
        }
        Stage::Chat if value["@type"] == "chat" => {
            value["id"].as_i64().filter(|id| *id != 0).map(|chat| {
                app.quick_message.saved_chat = Some(chat);
                pending.stage = Stage::Send;
                submission(pending, chat)
            })
        }
        Stage::Send if value["@type"] == "messages" => {
            app.quick_message.pending = None;
            let message = value["messages"]
                .as_array()
                .and_then(|m| m.first())
                .filter(|m| !m.is_null());
            if let Some(message) = message
                && let (Some(chat), Some(id)) =
                    (message["chat_id"].as_i64(), message["id"].as_i64())
                && message["@type"] == "message"
                && id != 0
                && Some(chat)
                    == if pending.action == Action::Repeat {
                        Some(pending.chat)
                    } else {
                        app.quick_message.saved_chat
                    }
            {
                app.store.apply(message);
                match message
                    .pointer("/sending_state/@type")
                    .and_then(Value::as_str)
                {
                    Some("messageSendingStateFailed") => {
                        app.notice = Some(format!(
                            "{name}失败：{}",
                            message
                                .pointer("/sending_state/error/message")
                                .and_then(Value::as_str)
                                .unwrap_or("发送失败")
                        ));
                    }
                    Some("messageSendingStatePending") => {
                        if app.quick_message.sending.len() == 8 {
                            app.quick_message.sending.pop_front();
                        }
                        app.quick_message
                            .sending
                            .push_back((chat, id, pending.action));
                        app.notice = Some(format!("已提交{name}，等待发送"));
                    }
                    _ => {
                        app.notice = Some(if pending.action == Action::SaveMessage {
                            "已收藏到收藏夹".into()
                        } else {
                            "已复读到当前会话".into()
                        })
                    }
                }
            } else {
                app.notice = Some(format!("{name}失败：这条消息无法发送"));
            }
            return Some((None, true));
        }
        _ => None,
    };
    if request.is_some() {
        app.quick_message.pending = Some(pending);
    } else {
        app.quick_message.pending = None;
        app.notice = Some(format!("{name}失败：Telegram 返回了无效结果"));
    }
    Some((request, true))
}

pub fn observe(app: &mut App, value: &Value) {
    if value["@type"] == "updateAuthorizationState"
        && value["authorization_state"]["@type"] != "authorizationStateReady"
    {
        app.quick_message.saved_chat = None;
        app.quick_message.pending = None;
        app.quick_message.sending.clear();
    }
    if !matches!(
        value["@type"].as_str(),
        Some("updateMessageSendSucceeded" | "updateMessageSendFailed")
    ) {
        return;
    }
    let chat = value["message"]["chat_id"].as_i64();
    let id = value["old_message_id"].as_i64();
    if let Some(index) = app
        .quick_message
        .sending
        .iter()
        .position(|(c, m, _)| Some(*c) == chat && Some(*m) == id)
    {
        let (_, _, action) = app
            .quick_message
            .sending
            .remove(index)
            .expect("matched send");
        app.notice = Some(if value["@type"] == "updateMessageSendFailed" {
            format!(
                "{}失败：{}",
                label(action),
                value["error"]["message"].as_str().unwrap_or("发送失败")
            )
        } else if action == Action::SaveMessage {
            "已收藏到收藏夹".into()
        } else {
            "已复读到当前会话".into()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InputMode, interaction,
        tdlib::{TdCommand, TdEvent},
    };
    use crossterm::event::KeyCode;
    use std::sync::mpsc::Receiver;

    fn request(receiver: &Receiver<TdCommand>) -> Value {
        let TdCommand::Request(value) = receiver.try_recv().expect("request") else {
            panic!("request")
        };
        value
    }

    fn response(app: &mut App, request: &Value, mut value: Value) -> Option<Value> {
        value["@extra"] = request["@extra"].clone();
        app.apply(TdEvent::Update(value)).0
    }

    fn message(chat: i64, id: i64, pending: bool) -> Value {
        json!({"@type":"message","chat_id":chat,"id":id,"is_outgoing":true,
            "sending_state":if pending {json!({"@type":"messageSendingStatePending"})} else {Value::Null},
            "content":{"@type":"messageText","text":{"text":"原消息","entities":[{"offset":0,"length":3,"type":{"@type":"textEntityTypeBold"}}]}}})
    }

    #[test]
    fn save_resolves_own_chat_once_and_preserves_draft_selection_and_history() {
        let mut app = crate::ui::tests::fixture();
        app.input_mode = InputMode::Reply(1);
        app.draft = "未发送草稿".into();
        app.draft_cursor = 6;
        app.timeline_anchor = Some((1, 2));
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        let me = request(&requests);
        assert_eq!(me["@type"], "getMe");
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        assert!(requests.try_recv().is_err(), "duplicate while preparing");
        let chat = response(&mut app, &me, json!({"@type":"user","id":777})).unwrap();
        assert_eq!(chat["user_id"], 777);
        let send = response(&mut app, &chat, json!({"@type":"chat","id":888})).unwrap();
        assert_eq!(send["chat_id"], 888, "use returned chat ID, not user ID");
        assert_eq!(send["from_chat_id"], 1);
        assert_eq!(send["message_ids"], json!([2]));
        assert_eq!(send["send_copy"], false);
        response(
            &mut app,
            &send,
            json!({"@type":"messages","messages":[message(888,10,false)]}),
        );
        assert_eq!(app.notice.as_deref(), Some("已收藏到收藏夹"));
        assert_eq!(
            app.store.messages.len(),
            2,
            "Saved Messages do not enter this chat"
        );
        assert_eq!(app.selected_message, Some(2));
        assert_eq!(app.store.active_chat, Some(1));
        assert_eq!(app.timeline_anchor, Some((1, 2)));
        assert_eq!(app.draft, "未发送草稿");
        assert_eq!(app.draft_cursor, 6);
        assert!(app.input_mode == InputMode::Reply(1));
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        assert_eq!(request(&requests)["@type"], "forwardMessages");
    }

    #[test]
    fn repeat_uses_server_copy_in_source_chat_and_projects_media_without_downloading() {
        for content in [
            message(1, 2, false)["content"].clone(),
            json!({"@type":"messagePhoto","caption":{"text":"保留说明"},"photo":{"sizes":[{"width":100,"height":100,"photo":{"id":42}}]}}),
            json!({"@type":"messageDocument","caption":{"text":"保留说明"},"document":{"file_name":"附件.pdf","document":{"id":43}}}),
            json!({"@type":"messageSticker","sticker":{"emoji":"👋","format":{"@type":"stickerFormatWebp"},"sticker":{"id":44}}}),
        ] {
            let mut app = crate::ui::tests::fixture();
            app.draft = "保留草稿".into();
            let (worker, requests) = TdWorker::test_pair();
            interaction::perform(&mut app, &worker, Action::Repeat);
            let send = request(&requests);
            assert_eq!(send["@type"], "forwardMessages");
            assert_eq!(send["chat_id"], 1);
            assert_eq!(send["from_chat_id"], 1);
            assert_eq!(send["send_copy"], true);
            assert_eq!(send["remove_caption"], false);
            assert_eq!(send["options"]["paid_message_star_count"], 0);
            assert!(send.get("input_message_content").is_none());
            let mut copied = message(1, 10, false);
            copied["content"] = content;
            response(
                &mut app,
                &send,
                json!({"@type":"messages","messages":[copied]}),
            );
            assert_eq!(app.store.messages.len(), 3);
            assert_eq!(app.notice.as_deref(), Some("已复读到当前会话"));
            assert_eq!(app.draft, "保留草稿");
            assert!(requests.try_recv().is_err(), "no downloadFile request");
        }
    }

    #[test]
    fn permissions_distinguish_forward_and_copy_and_never_send_failed_messages() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::Repeat);
        let properties = request(&requests);
        assert_eq!(properties["@type"], "getMessageProperties");
        response(
            &mut app,
            &properties,
            json!({"@type":"messageProperties","can_be_forwarded":false,"can_be_copied":true}),
        );
        menu::complete_pending(&mut app, &worker);
        let send = request(&requests);
        assert_eq!(send["send_copy"], true);
        response(
            &mut app,
            &send,
            json!({"@type":"messages","messages":[message(1,10,false)]}),
        );
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        assert!(requests.try_recv().is_err());
        menu::open_message(&mut app, &worker);
        let properties = request(&requests);
        response(
            &mut app,
            &properties,
            json!({"@type":"messageProperties","can_be_forwarded":true,"can_be_copied":false}),
        );
        assert!(menu::items(&app).any(|item| item.action == Action::SaveMessage));
        assert!(!menu::items(&app).any(|item| item.action == Action::Repeat));
        interaction::perform(&mut app, &worker, Action::Repeat);
        assert!(requests.try_recv().is_err());
        app.store.messages[1].info.sending = Sending::Pending;
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        assert!(requests.try_recv().is_err());
        app.store.messages[1].info.sending = Sending::Failed {
            can_retry: true,
            reason: "test".into(),
        };
        interaction::perform(&mut app, &worker, Action::Repeat);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn escape_selection_changes_and_stale_responses_cannot_finish_save_preparation() {
        for cancel in 0..4 {
            let mut app = crate::ui::tests::fixture();
            let (worker, requests) = TdWorker::test_pair();
            interaction::perform(&mut app, &worker, Action::SaveMessage);
            let me = request(&requests);
            let chat = response(&mut app, &me, json!({"@type":"user","id":777})).unwrap();
            match cancel {
                0 => {
                    crate::handle_ready_key(&mut app, &worker, KeyCode::Esc);
                }
                1 => {
                    app.selected_message = Some(1);
                }
                2 => {
                    app.selected_chat = Some(2);
                    app.open_selected();
                }
                _ => app.close_overlays(),
            }
            assert!(response(&mut app, &chat, json!({"@type":"chat","id":888})).is_none());
            assert!(requests.try_recv().is_err());
            assert!(app.quick_message.pending.is_none());
        }
        let mut app = crate::ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        let old = request(&requests);
        app.close_overlays();
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        let current = request(&requests);
        assert!(response(&mut app, &old, json!({"@type":"user","id":777})).is_none());
        assert!(response(&mut app, &current, json!({"@type":"user","id":777})).is_some());
    }

    #[test]
    fn denied_and_null_results_are_not_reported_as_success_or_login_errors() {
        for result in [
            json!({"@type":"error","code":400,"message":"CHAT_WRITE_FORBIDDEN"}),
            json!({"@type":"messages","messages":[null]}),
            json!({"@type":"messages","messages":[]}),
        ] {
            let mut app = crate::ui::tests::fixture();
            let (worker, requests) = TdWorker::test_pair();
            interaction::perform(&mut app, &worker, Action::Repeat);
            let send = request(&requests);
            response(&mut app, &send, result);
            assert!(app.notice.as_deref().unwrap().starts_with("复读失败"));
            assert!(!app.auth.is_error);
            assert!(app.quick_message.pending.is_none());
            assert_eq!(app.store.messages.len(), 2);
        }
    }

    #[test]
    fn pending_sends_are_bounded_and_failures_in_saved_messages_remain_visible() {
        let mut app = crate::ui::tests::fixture();
        app.quick_message.saved_chat = Some(888);
        let (worker, requests) = TdWorker::test_pair();
        for id in 10..30 {
            interaction::perform(&mut app, &worker, Action::SaveMessage);
            let send = request(&requests);
            response(
                &mut app,
                &send,
                json!({"@type":"messages","messages":[message(888,id,true)]}),
            );
            assert_eq!(app.notice.as_deref(), Some("已提交收藏，等待发送"));
        }
        assert_eq!(app.quick_message.sending.len(), 8);
        assert_eq!(app.store.messages.len(), 2);
        app.apply(TdEvent::Update(
            json!({"@type":"updateMessageSendFailed","old_message_id":29,
            "message":message(888,30,false),"error":{"message":"FLOOD_WAIT_1"}}),
        ));
        assert_eq!(app.notice.as_deref(), Some("收藏失败：FLOOD_WAIT_1"));
        assert_eq!(app.quick_message.sending.len(), 7);
        app.apply(TdEvent::Update(
            json!({"@type":"updateMessageSendSucceeded","old_message_id":28,
            "message":message(888,31,false)}),
        ));
        assert_eq!(app.notice.as_deref(), Some("已收藏到收藏夹"));
        app.apply(TdEvent::Update(json!({"@type":"updateAuthorizationState",
            "authorization_state":{"@type":"authorizationStateClosed"}})));
        assert!(app.quick_message.saved_chat.is_none());
        assert!(app.quick_message.sending.is_empty());
    }

    #[test]
    fn shortcuts_in_composer_are_text_but_menu_actions_preserve_reply_draft() {
        let mut app = crate::ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Reply(1);
        app.composer_focus = true;
        app.draft_cursor = 0;
        crate::handle_ready_key(&mut app, &worker, KeyCode::Char('S'));
        crate::handle_ready_key(&mut app, &worker, KeyCode::Char('D'));
        assert_eq!(app.draft, "SD");
        assert!(requests.try_recv().is_err());
        menu::open_message(&mut app, &worker);
        assert!(menu::execute(&mut app, &worker, Action::Repeat));
        assert_eq!(request(&requests)["send_copy"], true);
        assert_eq!(app.draft, "SD");
        assert!(app.input_mode == InputMode::Reply(1));
        assert!(!menu::execute(&mut app, &worker, Action::SaveMessage));
    }

    #[test]
    fn unavailable_worker_releases_preparation_and_send_state() {
        let mut app = crate::ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        drop(requests);
        interaction::perform(&mut app, &worker, Action::SaveMessage);
        assert!(app.quick_message.pending.is_none());
        interaction::perform(&mut app, &worker, Action::Repeat);
        assert!(app.quick_message.pending.is_none());
    }
}
