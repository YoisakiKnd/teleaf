//! Small context menus; only the current message's permission response is kept.
use serde_json::{Value, json};

use crate::{App, TdWorker, interaction, send_request, ui::Action};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Scope {
    #[default]
    Chat,
    Message {
        chat: i64,
        message: i64,
    },
    Selection,
}

#[derive(Clone, Copy, Default)]
pub struct Permissions {
    pub edit: bool,
    pub delete: bool,
    pub forward: bool,
    pub copy: bool,
}

pub struct Check {
    chat: i64,
    message: i64,
    generation: u64,
    value: Option<Permissions>,
    pending: Option<Action>,
}

impl Check {
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }
}

#[derive(Clone, Copy)]
pub struct Item {
    pub label: &'static str,
    pub key: char,
    pub action: Action,
}

const CHAT: &[Item] = &[
    Item {
        label: "写消息",
        key: 'i',
        action: Action::Write,
    },
    Item {
        label: "搜索当前会话",
        key: '/',
        action: Action::Search,
    },
    Item {
        label: "加载更早消息",
        key: 'g',
        action: Action::History,
    },
    Item {
        label: "回到最新消息",
        key: 'G',
        action: Action::Bottom,
    },
    Item {
        label: "返回引用前位置",
        key: 'b',
        action: Action::QuoteBack,
    },
    Item {
        label: "返回聊天记录",
        key: '\0',
        action: Action::ClearSearch,
    },
];
const MESSAGE: &[Item] = &[
    Item {
        label: "回复消息",
        key: 'r',
        action: Action::Reply,
    },
    Item {
        label: "快速收藏到收藏夹",
        key: 'S',
        action: Action::SaveMessage,
    },
    Item {
        label: "复读到当前会话",
        key: 'D',
        action: Action::Repeat,
    },
    Item {
        label: "复制文字",
        key: 'c',
        action: Action::Copy,
    },
    Item {
        label: "表情回应",
        key: 'x',
        action: Action::React,
    },
    Item {
        label: "预览 / 下载媒体",
        key: 'v',
        action: Action::Preview,
    },
    Item {
        label: "用系统程序打开",
        key: 'o',
        action: Action::OpenExternal,
    },
    Item {
        label: "重试失败消息",
        key: 'R',
        action: Action::Retry,
    },
    Item {
        label: "编辑文字",
        key: 'e',
        action: Action::Edit,
    },
    Item {
        label: "转发消息",
        key: 'f',
        action: Action::Forward,
    },
    Item {
        label: "为双方删除",
        key: 'd',
        action: Action::Delete,
    },
];
const SELECTION: &[Item] = &[Item {
    label: "复制选中文字",
    key: 'c',
    action: Action::Copy,
}];

pub fn permissions(app: &App) -> Option<Permissions> {
    let message = app.selected_message()?;
    if app.demo {
        return Some(Permissions {
            edit: message.outgoing,
            delete: true,
            forward: true,
            copy: true,
        });
    }
    app.message_check
        .as_ref()
        .filter(|check| Some(check.chat) == app.store.active_chat && check.message == message.id)
        .and_then(|check| check.value)
}

pub fn items(app: &App) -> impl Iterator<Item = Item> + '_ {
    let entries = match app.menu_scope {
        Scope::Chat => CHAT,
        Scope::Message { .. } => MESSAGE,
        Scope::Selection => SELECTION,
    };
    entries.iter().copied().filter(|item| {
        if let Scope::Message { chat, message } = app.menu_scope
            && (Some(chat) != app.store.active_chat || Some(message) != app.selected_message)
        {
            return false;
        }
        let message = app.selected_message();
        match item.action {
            Action::Copy => {
                crate::selection::active(app) || message.is_some_and(|m| !m.text.is_empty())
            }
            Action::Preview | Action::OpenExternal => message.is_some_and(|m| m.media.is_some()),
            Action::Retry => message.is_some_and(|m| m.retryable()),
            Action::Edit => {
                message.is_some_and(|m| m.info.text_message && m.media.is_none())
                    && permissions(app).is_some_and(|p| p.edit)
            }
            Action::Delete => permissions(app).is_some_and(|p| p.delete),
            Action::Forward => permissions(app).is_some_and(|p| p.forward),
            Action::SaveMessage => {
                crate::quick_message::eligible(app) && permissions(app).is_some_and(|p| p.forward)
            }
            Action::Repeat => {
                crate::quick_message::eligible(app) && permissions(app).is_some_and(|p| p.copy)
            }
            Action::Reply | Action::React => message.is_some(),
            Action::QuoteBack => app.quote_back.is_some(),
            Action::ClearSearch => app.show_search,
            _ => app.store.active_chat.is_some(),
        }
    })
}

fn query(app: &mut App, worker: &TdWorker, pending: Option<Action>) {
    let (Some(chat), Some(message)) = (app.store.active_chat, app.selected_message) else {
        return;
    };
    if let Some(check) = &mut app.message_check
        && check.chat == chat
        && check.message == message
        && check.value.is_none()
    {
        if pending.is_some() {
            check.pending = pending;
        }
        return;
    }
    app.menu_generation = app.menu_generation.wrapping_add(1);
    let generation = app.menu_generation;
    app.message_check = Some(Check {
        chat,
        message,
        generation,
        value: None,
        pending,
    });
    if !send_request(
        app,
        worker,
        json!({
            "@type": "getMessageProperties", "chat_id": chat, "message_id": message,
            "@extra": {"kind": "message-properties", "chat": chat, "message": message, "generation": generation}
        }),
    ) {
        app.message_check = None;
    }
}

pub fn open(app: &mut App, worker: &TdWorker, scope: Scope) {
    app.close_overlays();
    app.menu_scope = scope;
    app.action_menu = Some(0);
    if matches!(scope, Scope::Message { .. }) && !app.demo {
        query(app, worker, None);
    }
}

pub fn open_message(app: &mut App, worker: &TdWorker) {
    if let (Some(chat), Some(message)) = (app.store.active_chat, app.selected_message) {
        open(app, worker, Scope::Message { chat, message });
    } else {
        app.notice = Some("请先选择一条消息".into());
    }
}

pub fn receive(app: &mut App, value: &Value) -> Option<bool> {
    let extra = &value["@extra"];
    if extra["kind"].as_str() != Some("message-properties") {
        return None;
    }
    let Some(check) = app.message_check.as_mut() else {
        return Some(false);
    };
    if extra["chat"].as_i64() != Some(check.chat)
        || extra["message"].as_i64() != Some(check.message)
        || extra["generation"].as_u64() != Some(check.generation)
        || app.store.active_chat != Some(check.chat)
        || app.selected_message != Some(check.message)
    {
        return Some(false);
    }
    if value["@type"] == "messageProperties" {
        check.value = Some(Permissions {
            edit: value["can_be_edited"].as_bool().unwrap_or(false),
            delete: value["can_be_deleted_for_all_users"]
                .as_bool()
                .unwrap_or(false),
            forward: value["can_be_forwarded"].as_bool().unwrap_or(false),
            copy: value["can_be_copied"].as_bool().unwrap_or(false),
        });
    } else {
        check.value = Some(Permissions::default());
        check.pending = None;
        app.notice = Some("无法读取消息权限，请重新打开菜单重试".into());
    }
    Some(true)
}

pub fn invalidate(app: &mut App, value: &Value) -> bool {
    let Some(check) = &app.message_check else {
        return false;
    };
    let same_chat = value["chat_id"].as_i64() == Some(check.chat);
    let affected = match value["@type"].as_str().unwrap_or("") {
        "updateMessageContent" | "updateMessageEdited" => {
            same_chat && value["message_id"].as_i64() == Some(check.message)
        }
        "updateDeleteMessages" => {
            same_chat
                && value["message_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id.as_i64() == Some(check.message)))
        }
        "updateChatPermissions" | "updateChatIsProtectedContent" => same_chat,
        "updateMessageSendSucceeded" | "updateMessageSendFailed" => {
            value["message"]["chat_id"].as_i64() == Some(check.chat)
                && value["old_message_id"].as_i64() == Some(check.message)
        }
        "updateSupergroup" | "updateSupergroupFullInfo" => true,
        _ => false,
    };
    if !affected {
        return false;
    }
    let pending = check.pending.is_some();
    app.message_check = None;
    let visible = app.action_menu.is_some() && matches!(app.menu_scope, Scope::Message { .. });
    if visible || pending {
        app.action_menu = None;
        app.notice = Some("消息状态已变化，请重新打开菜单".into());
    }
    visible || pending
}

pub fn complete_pending(app: &mut App, worker: &TdWorker) -> bool {
    let pending = app.message_check.as_mut().and_then(|check| {
        (check.value.is_some()
            && Some(check.chat) == app.store.active_chat
            && Some(check.message) == app.selected_message)
            .then(|| check.pending.take())
            .flatten()
    });
    pending.is_some_and(|action| interaction::perform(app, worker, action))
}

pub fn authorize(app: &mut App, worker: &TdWorker, action: Action) -> bool {
    if !matches!(
        action,
        Action::Edit | Action::Delete | Action::Forward | Action::SaveMessage | Action::Repeat
    ) {
        return true;
    }
    if matches!(action, Action::SaveMessage | Action::Repeat)
        && !crate::quick_message::eligible(app)
    {
        app.notice = Some("请先选择一条已发送的消息".into());
        return false;
    }
    if let Some(permissions) = permissions(app) {
        if match action {
            Action::Edit => permissions.edit,
            Action::Delete => permissions.delete,
            Action::Forward | Action::SaveMessage => permissions.forward,
            Action::Repeat => permissions.copy,
            _ => false,
        } {
            return true;
        }
        app.notice = Some("这条消息当前不允许此操作".into());
    } else if app.selected_message().is_some() {
        query(app, worker, Some(action));
        app.notice = Some("正在读取消息权限…".into());
    }
    false
}

pub fn execute(app: &mut App, worker: &TdWorker, action: Action) -> bool {
    if app.action_menu.is_none() || !items(app).any(|item| item.action == action) {
        return false;
    }
    app.action_menu = None;
    app.hit_targets.clear();
    app.mouse_press = None;
    app.drag = None;
    if action == Action::Copy && matches!(app.menu_scope, Scope::Selection) {
        return interaction::perform(app, worker, action);
    }
    app.focus_messages = true;
    interaction::perform(app, worker, action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        InputMode,
        tdlib::{TdCommand, TdEvent},
    };

    fn request(receiver: &std::sync::mpsc::Receiver<TdCommand>) -> Value {
        let TdCommand::Request(value) = receiver.try_recv().unwrap() else {
            panic!("request");
        };
        value
    }

    fn response(request: &Value, edit: bool, delete: bool, forward: bool) -> Value {
        json!({"@type":"messageProperties", "@extra":request["@extra"],
            "can_be_edited":edit, "can_be_deleted_for_all_users":delete, "can_be_forwarded":forward})
    }

    #[test]
    fn scopes_filter_actions_and_bind_message_target() {
        let mut app = crate::ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        open(&mut app, &worker, Scope::Chat);
        assert!(items(&app).any(|i| i.action == Action::History));
        assert!(
            !items(&app).any(|i| matches!(i.action, Action::Delete | Action::Edit | Action::Reply))
        );
        open_message(&mut app, &worker);
        assert!(items(&app).any(|i| i.action == Action::Delete));
        assert!(
            !items(&app)
                .any(|i| matches!(i.action, Action::History | Action::Photo | Action::File))
        );
        app.selected_message = Some(1);
        assert!(!execute(&mut app, &worker, Action::Delete));
        assert!(app.confirm_delete.is_none());
        app.selected_message = Some(2);
        assert!(execute(&mut app, &worker, Action::Reply));
        assert!(
            !execute(&mut app, &worker, Action::Delete),
            "stale menu targets must stop working immediately"
        );
        assert!(app.confirm_delete.is_none());
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn edit_uses_tdlib_permission_even_for_non_outgoing_text() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        app.store.messages[1].outgoing = false;
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::Edit);
        let check = request(&requests);
        assert_eq!(check["@type"], "getMessageProperties");
        assert_eq!(check["message_id"], 2);
        assert!(app.input_mode == InputMode::Off);
        app.apply(TdEvent::Update(response(&check, true, false, false)));
        assert!(complete_pending(&mut app, &worker));
        assert!(app.input_mode == InputMode::Edit(2));
        assert_eq!(app.draft, app.store.messages[1].text);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn editable_polls_are_not_mistaken_for_editable_text() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        app.store.apply(
            &json!({"@type":"updateMessageContent","chat_id":1,"message_id":2,
            "new_content":{"@type":"messagePoll","poll":{"question":{"text":"投票"}}}}),
        );
        let (worker, requests) = TdWorker::test_pair();
        open_message(&mut app, &worker);
        let check = request(&requests);
        receive(&mut app, &response(&check, true, true, true));
        assert!(!items(&app).any(|item| item.action == Action::Edit));
        interaction::perform(&mut app, &worker, Action::Edit);
        assert!(app.input_mode == InputMode::Off);
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn late_or_denied_permissions_do_not_enable_delete_and_requests_coalesce() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::Delete);
        let old = request(&requests);
        interaction::perform(&mut app, &worker, Action::Delete);
        assert!(requests.try_recv().is_err());
        app.selected_message = Some(1);
        open_message(&mut app, &worker);
        let current = request(&requests);
        assert_eq!(
            receive(&mut app, &response(&old, true, true, true)),
            Some(false)
        );
        assert!(permissions(&app).is_none());
        receive(&mut app, &response(&current, false, false, false));
        assert!(
            !items(&app)
                .any(|i| matches!(i.action, Action::Delete | Action::Edit | Action::Forward))
        );
        interaction::perform(&mut app, &worker, Action::Delete);
        assert!(app.confirm_delete.is_none());
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn escape_cancels_pending_action_and_permission_error_stays_out_of_login() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        let (worker, requests) = TdWorker::test_pair();
        interaction::perform(&mut app, &worker, Action::Delete);
        let check = request(&requests);
        crate::handle_ready_key(&mut app, &worker, crossterm::event::KeyCode::Esc);
        app.apply(TdEvent::Update(response(&check, false, true, true)));
        assert!(!complete_pending(&mut app, &worker));
        assert!(app.confirm_delete.is_none());
        open_message(&mut app, &worker);
        let check = request(&requests);
        app.apply(TdEvent::Update(
            json!({"@type":"error","code":400,"message":"test", "@extra":check["@extra"]}),
        ));
        assert!(!app.auth.is_error);
        assert!(app.notice.as_deref().unwrap().contains("权限"));
        assert!(!permissions(&app).unwrap().delete);
    }

    #[test]
    fn permission_changes_discard_cached_capabilities_and_cancel_open_menu() {
        let mut app = crate::ui::tests::fixture();
        app.demo = false;
        let (worker, requests) = TdWorker::test_pair();
        open_message(&mut app, &worker);
        let check = request(&requests);
        receive(&mut app, &response(&check, true, true, true));
        assert!(!invalidate(
            &mut app,
            &json!({"@type":"updateChatPermissions","chat_id":999})
        ));
        assert!(permissions(&app).unwrap().delete);
        app.apply(TdEvent::Update(
            json!({"@type":"updateChatPermissions","chat_id":1}),
        ));
        assert!(app.action_menu.is_none() && app.message_check.is_none());
        interaction::perform(&mut app, &worker, Action::Delete);
        let current = request(&requests);
        assert_eq!(
            receive(&mut app, &response(&check, true, true, true)),
            Some(false)
        );
        assert!(app.confirm_delete.is_none());
        receive(&mut app, &response(&current, false, false, false));
        complete_pending(&mut app, &worker);
        assert!(app.confirm_delete.is_none());
    }
}
