//! Shared actions and layered mouse routing. Motion alone never requests a redraw.
use std::io::Write;
use std::process::{Command, Stdio};

use crate::{App, InputMode, TdWorker, actions, send_request, submit_composer, text, ui};
use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ui::{Action, Editor, Pane, Target};

pub fn open_attachments(app: &mut App, photos: bool) {
    let reply = if let InputMode::Reply(id) = app.input_mode {
        Some(id)
    } else {
        None
    };
    app.close_overlays();
    app.attachments = Some(crate::attachments::Picker::new(photos, reply));
}

fn begin_input(app: &mut App, mode: InputMode) {
    if app.input_mode != mode
        && !app.draft.is_empty()
        && !matches!(
            (app.input_mode, mode),
            (
                InputMode::Send | InputMode::Reply(_),
                InputMode::Send | InputMode::Reply(_)
            )
        )
    {
        if app.suspended_input.is_none() {
            app.suspended_input = Some((
                app.input_mode,
                std::mem::take(&mut app.draft),
                app.draft_cursor,
            ));
        } else {
            app.draft.clear();
        }
        app.draft_cursor = usize::MAX;
    }
    app.selection = None;
    app.input_mode = mode;
    app.composer_focus = true;
    app.focus_messages = true;
}

pub fn finish_input(app: &mut App) {
    app.selection = None;
    app.draft.clear();
    app.draft_cursor = usize::MAX;
    app.input_mode = InputMode::Off;
    app.composer_focus = false;
    if let Some((mode, draft, cursor)) = app.suspended_input.take() {
        app.input_mode = mode;
        app.draft = draft;
        app.draft_cursor = cursor;
        app.composer_focus = true;
    }
}

fn zoomable(app: &App) -> bool {
    app.preview_message
        .and_then(|id| app.active_messages().iter().find(|m| m.id == id))
        .and_then(|m| m.media.as_ref())
        .is_some_and(|m| {
            matches!(
                m.kind,
                crate::store::MediaKind::Photo | crate::store::MediaKind::Sticker
            )
        })
}

pub fn perform(app: &mut App, worker: &TdWorker, action: Action) -> bool {
    if action == Action::Cancel {
        app.quick_message.cancel_preparation();
    }
    if action == Action::Edit
        && app
            .selected_message()
            .is_some_and(|m| !m.info.text_message || m.media.is_some())
    {
        app.notice = Some("当前仅支持编辑文字消息".into());
        return true;
    }
    if !crate::menu::authorize(app, worker, action) {
        return true;
    }
    if matches!(action, Action::ZoomIn | Action::ZoomOut | Action::ZoomReset) && !zoomable(app) {
        return false;
    }
    match action {
        Action::Submit => {
            if app.sticker_picker {
                crate::stickers::close(app);
            }
            if app.input_mode == InputMode::Off {
                begin_input(app, InputMode::Send);
            }
            return submit_composer(app, worker);
        }
        Action::Confirm => {
            if app.attachments.is_some() {
                if let Some(chat) = app.store.active_chat {
                    match app.attachments.as_ref().unwrap().request(chat) {
                        Ok(request) => {
                            if send_request(app, worker, request) {
                                let consume = app.attachments.as_ref().unwrap().consume_path_draft;
                                app.attachments = None;
                                if consume {
                                    finish_input(app);
                                }
                                app.notice = Some("附件已提交，上传结果见聊天记录".into());
                            }
                        }
                        Err(error) => app.attachments.as_mut().unwrap().error = Some(error),
                    }
                }
            } else if let Some(message_id) = app.confirm_delete {
                if let Some(chat) = app.store.active_chat
                    && send_request(app, worker, actions::delete(chat, message_id, true))
                {
                    app.confirm_delete = None;
                }
            } else if let Some(message_id) = app.forward_message {
                if let (Some(from), Some(to)) = (app.store.active_chat, app.selected_chat)
                    && send_request(app, worker, actions::forward(from, message_id, to))
                {
                    app.forward_message = None;
                    app.selected_chat = app.store.active_chat;
                    app.reveal_chat = true;
                }
            } else if app.sticker_picker {
                if let Some(sticker) = app.sticker_items().get(app.sticker_cursor)
                    && let Some(chat) = app.store.active_chat
                {
                    let request = actions::send_sticker(
                        chat,
                        sticker.file_id,
                        &sticker.emoji,
                        sticker.width,
                        sticker.height,
                    );
                    if send_request(app, worker, request) {
                        app.notice = Some("贴纸已提交".into());
                        send_request(app, worker, actions::recent_stickers());
                    }
                }
            } else if app.auth.setup.is_some() || app.auth.state != "authorizationStateReady" {
                if let Some(request) = app.auth.key(KeyCode::Enter) {
                    send_request(app, worker, request);
                }
            } else {
                return submit_composer(app, worker);
            }
        }
        Action::AttachmentMode => {
            if let Some(picker) = &mut app.attachments {
                picker.photos = !picker.photos;
                picker.error = None;
            }
        }
        Action::AttachmentParent => {
            if let Some(picker) = &mut app.attachments
                && let Some(parent) = picker.directory.parent()
            {
                picker.navigate(parent.to_path_buf());
            }
        }
        Action::StickerPrevious => return crate::stickers::cycle_tab(app, worker, -1),
        Action::StickerNext => return crate::stickers::cycle_tab(app, worker, 1),
        Action::StickerSearch => crate::stickers::search(app, worker),
        Action::StickerFavorite => crate::stickers::favorite(app, worker),
        Action::Cancel => {
            if app.has_overlay() {
                app.close_overlays();
            } else if app.auth.setup.is_some() {
                app.auth.cancel_setup();
            } else if app.input_mode != InputMode::Off {
                finish_input(app);
            } else {
                return false;
            }
        }
        Action::Back if app.auth.setup.is_some() => {
            return app.auth.cancel_setup();
        }
        Action::Back => {
            app.composer_focus = false;
            app.focus_messages = false;
        }
        Action::Bottom => {
            app.quote_back = None;
            app.timeline_anchor = None;
            app.timeline_top = app.timeline_max;
            app.pending_messages = 0;
        }
        Action::Menu => {
            crate::menu::open_message(app, worker);
        }
        Action::ChatMenu => {
            crate::menu::open(app, worker, crate::menu::Scope::Chat);
        }
        Action::Folders => {
            app.close_overlays();
            app.show_folders = true;
            app.folder_offset = app
                .store
                .chat_lists()
                .iter()
                .position(|(list, _)| *list == app.store.selected_list)
                .unwrap_or(0);
        }
        Action::ClearSearch => {
            app.show_search = false;
            app.timeline_heights.clear();
            app.timeline_anchor = None;
            app.selection = None;
        }
        Action::Settings => {
            app.close_overlays();
            app.show_settings = true;
        }
        Action::Help => {
            app.close_overlays();
            app.show_help = true;
        }
        Action::ApiSetup if app.demo => {
            app.notice = Some("离线演示无需 API 配置".into());
        }
        Action::ApiSetup => {
            app.close_overlays();
            app.auth.begin_setup();
        }
        Action::NextField => {
            app.auth.key(crossterm::event::KeyCode::Tab);
        }
        Action::NewLogin => app.auth.choose_new_login(),
        Action::ToggleMouse => {
            app.mouse_enabled = !app.mouse_enabled;
        }
        Action::OpenLink => {
            let url = if app.auth.setup.is_some() || app.show_settings {
                "https://my.telegram.org/apps"
            } else {
                app.auth
                    .confirmation_link
                    .as_deref()
                    .unwrap_or("https://my.telegram.org/apps")
            };
            if let Err(error) = crate::open_url(url) {
                app.notice = Some(error);
            }
        }
        Action::MoreChats => {
            if app.store.exhausted_lists.contains(&app.store.selected_list) {
                app.notice = Some("这个分组的会话已全部加载".into());
            } else {
                send_request(
                    app,
                    worker,
                    actions::load_chat_list(app.store.selected_list),
                );
            }
        }
        Action::ZoomIn => return app.preview_view.step(1),
        Action::ZoomOut => return app.preview_view.step(-1),
        Action::ZoomReset => {
            let changed = app.preview_view != crate::media::View::default();
            app.preview_view = crate::media::View::default();
            return changed;
        }
        Action::OpenExternal => app.open_selected_external(),
        Action::Preview => {
            if let Some(request) = app.preview_selected() {
                let id = request["file_id"].as_i64().unwrap_or(0) as i32;
                if !send_request(app, worker, request) {
                    app.requested_files.remove(&id);
                    app.failed_files.insert(id);
                }
            }
        }
        _ => {
            if app.store.active_chat.is_none() {
                app.notice = Some("请先打开一个会话".into());
                return true;
            }
            match action {
                Action::Write => begin_input(app, InputMode::Send),
                Action::Reply => {
                    if let Some(id) = app.selected_message {
                        begin_input(app, InputMode::Reply(id));
                    }
                }
                Action::Retry => {
                    if let Some(message) = app.selected_message() {
                        if message.retryable() {
                            send_request(
                                app,
                                worker,
                                actions::resend(
                                    app.store.active_chat.expect("active chat"),
                                    message.id,
                                ),
                            );
                        } else if let crate::store::Sending::Failed { reason, .. } =
                            &message.info.sending
                        {
                            app.notice = Some(format!("暂不能重试：{reason}"));
                        } else {
                            app.notice = Some("这条消息没有发送失败".into());
                        }
                    }
                }
                Action::QuoteBack => {
                    if let Some(position) = app.quote_back.take() {
                        app.timeline_anchor = position.anchor;
                        app.selected_message = position.selected;
                        app.reveal_message = false;
                    }
                }
                Action::React => {
                    if let Some(id) = app.selected_message {
                        begin_input(app, InputMode::React(id));
                    }
                }
                Action::Edit => {
                    if let Some((id, value)) = app
                        .selected_message()
                        .filter(|m| m.media.is_none())
                        .map(|m| (m.id, m.text.clone()))
                    {
                        begin_input(app, InputMode::Edit(id));
                        app.draft = value;
                        app.draft_cursor = usize::MAX;
                    } else {
                        app.notice = Some("当前仅支持编辑文字消息".into());
                    }
                }
                Action::Delete => {
                    app.drag = None;
                    app.mouse_press = None;
                    app.confirm_delete = app.selected_message;
                }
                Action::Forward => {
                    app.drag = None;
                    app.mouse_press = None;
                    app.forward_message = app.selected_message;
                    app.reveal_chat = true;
                }
                Action::SaveMessage | Action::Repeat => {
                    crate::quick_message::start(app, worker, action);
                }
                Action::Copy => {
                    let value = match crate::selection::selected_text(app) {
                        Ok(Some(value)) => Some(value),
                        Ok(None) => app.selected_message().map(|m| m.text.clone()),
                        Err(error) => {
                            app.notice = Some(error);
                            return true;
                        }
                    };
                    if let Some(value) = value {
                        app.notice = Some(match copy(&value) {
                            Ok(()) => {
                                if crate::terminal::prefer_terminal_clipboard() {
                                    "已请求终端写入剪贴板".into()
                                } else {
                                    "已复制文字".into()
                                }
                            }
                            Err(e) => format!("复制失败：{e}；可按 F6 使用终端选字"),
                        });
                    }
                }
                Action::Search => begin_input(app, InputMode::Search),
                Action::Photo => open_attachments(app, true),
                Action::File => open_attachments(app, false),
                Action::Stickers => {
                    if app.sticker_picker {
                        crate::stickers::close(app);
                    } else {
                        crate::stickers::open(app, worker);
                    }
                }
                Action::History => {
                    if let Some(request) = app.load_older()
                        && !send_request(app, worker, request)
                    {
                        app.history_loading = false;
                    }
                }
                _ => return false,
            }
        }
    }
    true
}

fn copy(value: &str) -> Result<(), String> {
    if crate::terminal::prefer_terminal_clipboard() {
        return crate::terminal::terminal_clipboard(value);
    }
    let (name, args): (&str, &[&str]) = if cfg!(target_os = "macos") {
        ("pbcopy", &[])
    } else if cfg!(target_os = "windows") || std::env::var_os("WSL_DISTRO_NAME").is_some() {
        ("clip.exe", &[])
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-copy", &[])
    } else {
        ("xclip", &["-selection", "clipboard"])
    };
    let mut child = Command::new(name)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let result = child
        .stdin
        .take()
        .ok_or("无法写入剪贴板".to_owned())
        .and_then(|mut stdin| stdin.write_all(value.as_bytes()).map_err(|e| e.to_string()));
    let status = child.wait().map_err(|e| e.to_string())?;
    result?;
    if status.success() {
        Ok(())
    } else {
        Err("系统剪贴板不可用".into())
    }
}

#[derive(Clone, Copy)]
pub enum Drag {
    Text {
        anchor: crate::selection::Point,
        x: u16,
        y: u16,
        moved: bool,
    },
    Scroll {
        pane: Pane,
        area: ratatui::layout::Rect,
        maximum: usize,
        moved: bool,
    },
    Image {
        area: ratatui::layout::Rect,
        x: u16,
        y: u16,
        view: crate::media::View,
        moved: bool,
    },
    Cancelled,
}

fn point(target: Target) -> Option<crate::selection::Point> {
    match target {
        Target::Text(point) => Some(point),
        Target::Cursor(Editor::Composer, byte) => Some(crate::selection::Point {
            source: crate::selection::Source::Composer,
            byte,
        }),
        _ => None,
    }
}
fn point_at(
    app: &App,
    x: u16,
    y: u16,
    source: crate::selection::Source,
) -> Option<crate::selection::Point> {
    let hit = app.hit_targets.iter().rev().find(|(r, t)| {
        r.contains((x, y).into())
            && point(*t).is_some_and(|p| crate::selection::compatible(source, p.source))
    });
    if let Some((_, target)) = hit {
        return point(*target);
    }
    app.hit_targets
        .iter()
        .rev()
        .filter_map(|(r, t)| {
            point(*t)
                .filter(|p| crate::selection::compatible(source, p.source))
                .map(|p| {
                    let dx = if x < r.x {
                        r.x - x
                    } else {
                        x.saturating_sub(r.right().saturating_sub(1))
                    };
                    let dy = if y < r.y {
                        r.y - y
                    } else {
                        y.saturating_sub(r.bottom().saturating_sub(1))
                    };
                    (usize::from(dy) * 65536 + usize::from(dx), p)
                })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, p)| p)
}
fn drag_text(app: &mut App, anchor: crate::selection::Point, x: u16, y: u16) -> bool {
    let Some(head) = point_at(app, x, y, anchor.source) else {
        return false;
    };
    let selection = crate::selection::Selection { anchor, head };
    let changed = app.selection != Some(selection);
    app.selection = Some(selection);
    match head.source {
        crate::selection::Source::Composer => {
            app.composer_focus = true;
            app.focus_messages = true;
            app.draft_cursor = head.byte;
            if app.input_mode == InputMode::Off {
                app.input_mode = InputMode::Send;
            }
        }
        crate::selection::Source::Message(id) => {
            app.selected_message = Some(id);
            app.composer_focus = false;
            app.focus_messages = true;
        }
    }
    changed
}
pub fn tick(app: &mut App) -> bool {
    let Some(Drag::Text {
        anchor,
        x,
        y,
        moved: true,
    }) = app.drag
    else {
        return false;
    };
    if !matches!(anchor.source, crate::selection::Source::Message(_)) || app.has_overlay() {
        return false;
    }
    let Some((area, _)) = app
        .hit_targets
        .iter()
        .find(|(_, t)| *t == Target::Panel(Pane::Timeline))
    else {
        return false;
    };
    let delta = if y <= area.y {
        -1
    } else if y >= area.bottom().saturating_sub(1) {
        1
    } else {
        0
    };
    if delta != 0 && ui::scroll_timeline(app, delta) {
        drag_text(app, anchor, x, y);
        return true;
    }
    false
}
pub fn pan_key(app: &mut App, key: KeyCode) -> bool {
    if app.preview_view.zoom <= 100 {
        return false;
    }
    let before = app.preview_view;
    match key {
        KeyCode::Left => app.preview_view.x = app.preview_view.x.saturating_sub(100),
        KeyCode::Right => app.preview_view.x = (app.preview_view.x + 100).min(1000),
        KeyCode::Up => app.preview_view.y = app.preview_view.y.saturating_sub(100),
        KeyCode::Down => app.preview_view.y = (app.preview_view.y + 100).min(1000),
        _ => {}
    }
    before != app.preview_view
}
fn move_drag(app: &mut App, mouse: MouseEvent) -> bool {
    let Some(drag) = app.drag else {
        app.mouse_press = None;
        return false;
    };
    match drag {
        Drag::Text { anchor, .. } => {
            let changed = drag_text(app, anchor, mouse.column, mouse.row);
            app.drag = Some(Drag::Text {
                anchor,
                x: mouse.column,
                y: mouse.row,
                moved: true,
            });
            changed
        }
        Drag::Scroll {
            pane,
            area,
            maximum,
            ..
        } => {
            let maximum = if pane == Pane::Timeline {
                app.timeline_max
            } else {
                maximum
            };
            let row = mouse
                .row
                .saturating_sub(area.y)
                .min(area.height.saturating_sub(1));
            let top =
                usize::from(row) * maximum / usize::from(area.height.saturating_sub(1).max(1));
            app.drag = Some(Drag::Scroll {
                pane,
                area,
                maximum,
                moved: true,
            });
            if pane == Pane::Timeline {
                ui::set_timeline_top(app, top)
            } else {
                let changed = app.chat_offset != top;
                app.chat_offset = top;
                changed
            }
        }
        Drag::Image {
            area, x, y, view, ..
        } => {
            app.drag = Some(Drag::Image {
                area,
                x,
                y,
                view,
                moved: true,
            });
            if view.zoom <= 100 {
                return false;
            }
            let before = app.preview_view;
            let shift = |now: u16, origin: u16, size: u16, pan: u16| {
                let delta = (i64::from(now) - i64::from(origin)) * 100000
                    / (i64::from(size.max(1)) * i64::from(view.zoom - 100));
                (i64::from(pan) - delta).clamp(0, 1000) as u16
            };
            app.preview_view.x = shift(mouse.column, x, area.width, view.x);
            app.preview_view.y = shift(mouse.row, y, area.height, view.y);
            before != app.preview_view
        }
        Drag::Cancelled => {
            app.mouse_press = None;
            false
        }
    }
}
fn click_action(app: &mut App, worker: &TdWorker, target: Target, mouse: MouseEvent) -> bool {
    let double = app.last_click.is_some_and(|(previous, x, y, time)| {
        let same = previous == target
            || point(previous)
                .zip(point(target))
                .is_some_and(|(a, b)| a.source == b.source);
        same && x.abs_diff(mouse.column) <= 1
            && y.abs_diff(mouse.row) <= 1
            && time.elapsed() <= std::time::Duration::from_millis(350)
    });
    app.last_click = Some((target, mouse.column, mouse.row, std::time::Instant::now()));
    if double {
        app.last_click = None;
        if let Some(point) = point(target) {
            activate(app, worker, target);
            return crate::selection::word(app, point);
        }
        if let Target::Message(id) = target
            && app
                .active_messages()
                .iter()
                .any(|m| m.id == id && m.media.is_some())
        {
            app.selected_message = Some(id);
            return perform(app, worker, Action::Preview);
        }
    }
    let cleared =
        if point(target).is_some() || matches!(target, Target::Message(_) | Target::Composer) {
            app.selection.take().is_some()
        } else {
            false
        };
    activate(app, worker, target) || cleared
}

pub fn mouse(app: &mut App, worker: &TdWorker, mouse: MouseEvent) -> bool {
    if !app.mouse_enabled {
        return false;
    }
    match mouse.kind {
        MouseEventKind::Moved => return false,
        MouseEventKind::Drag(MouseButton::Left) => return move_drag(app, mouse),
        MouseEventKind::Drag(_) => return false,
        _ => {}
    }
    let target = app
        .hit_targets
        .iter()
        .rev()
        .find(|(area, _)| area.contains((mouse.column, mouse.row).into()))
        .map(|(_, target)| *target);
    match mouse.kind {
        MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) => {
            app.mouse_press = target.map(|target| (button, target));
            app.drag = if button == MouseButton::Left {
                target.map(|target| {
                    if let Some(anchor) = point(target) {
                        Drag::Text {
                            anchor,
                            x: mouse.column,
                            y: mouse.row,
                            moved: false,
                        }
                    } else if let Target::Scroll(pane, _) = target {
                        app.scrollbars
                            .iter()
                            .find(|(p, _, _)| *p == pane)
                            .map(|(_, area, maximum)| Drag::Scroll {
                                pane,
                                area: *area,
                                maximum: *maximum,
                                moved: false,
                            })
                            .unwrap_or(Drag::Cancelled)
                    } else if let Target::Image(area) = target {
                        Drag::Image {
                            area,
                            x: mouse.column,
                            y: mouse.row,
                            view: app.preview_view,
                            moved: false,
                        }
                    } else {
                        Drag::Cancelled
                    }
                })
            } else {
                None
            };
            matches!(
                target,
                Some(
                    Target::Command(_)
                        | Target::Folder(_)
                        | Target::FolderPage(_)
                        | Target::MessageAction(_, _)
                        | Target::Media(_)
                        | Target::Chat(_)
                )
            )
        }
        MouseEventKind::Up(MouseButton::Left) => {
            let pressed = app.mouse_press.take();
            let drag = app.drag.take();
            match drag {
                Some(Drag::Text {
                    anchor,
                    moved: true,
                    ..
                }) => return drag_text(app, anchor, mouse.column, mouse.row),
                Some(Drag::Scroll { moved: true, .. } | Drag::Image { moved: true, .. }) => {
                    return false;
                }
                _ => {}
            }
            let matches = pressed.zip(target).is_some_and(|((button, down), up)| {
                button == MouseButton::Left
                    && (down == up
                        || point(down)
                            .zip(point(up))
                            .is_some_and(|(a, b)| a.source == b.source))
            });
            if !matches {
                return pressed.is_some();
            }
            click_action(app, worker, target.expect("target"), mouse);
            true
        }
        MouseEventKind::Up(MouseButton::Right) => {
            if app.mouse_press.take() != target.map(|t| (MouseButton::Right, t)) {
                return false;
            }
            let id = match target {
                Some(
                    Target::Message(id)
                    | Target::Media(id)
                    | Target::MessageAction(id, _)
                    | Target::Text(crate::selection::Point {
                        source: crate::selection::Source::Message(id),
                        ..
                    }),
                ) => Some(id),
                _ => None,
            };
            if let Some(id) = id {
                if crate::selection::range(app, crate::selection::Source::Message(id)).is_none() {
                    app.selection = None;
                }
                app.selected_message = Some(id);
                app.focus_messages = true;
                app.composer_focus = false;
                crate::menu::open_message(app, worker);
                true
            } else if matches!(
                target,
                Some(Target::Cursor(Editor::Composer, _) | Target::Composer)
            ) && crate::selection::active(app)
            {
                crate::menu::open(app, worker, crate::menu::Scope::Selection);
                true
            } else {
                false
            }
        }
        MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
            app.mouse_press = None;
            app.drag = None;
            let direction = if mouse.kind == MouseEventKind::ScrollUp {
                -1
            } else {
                1
            };
            let delta = direction * crate::terminal::scroll_lines();
            match target {
                Some(Target::AttachmentQueue | Target::AttachmentRemove(_))
                    if app.attachments.is_some() =>
                {
                    let picker = app.attachments.as_mut().unwrap();
                    let before = picker.queue_offset;
                    picker.queue_offset = before
                        .saturating_add_signed(direction)
                        .min(picker.selected.len().saturating_sub(picker.queue_visible));
                    before != picker.queue_offset
                }
                Some(
                    Target::AttachmentEntry(_)
                    | Target::Cursor(Editor::AttachmentPath | Editor::AttachmentCaption, _)
                    | Target::Panel(Pane::Modal)
                    | Target::Modal,
                ) if app.attachments.is_some() => {
                    let picker = app.attachments.as_mut().unwrap();
                    let before = picker.offset;
                    picker.offset = before
                        .saturating_add_signed(delta)
                        .min(picker.entries.len().saturating_sub(picker.visible));
                    picker.cursor = picker.cursor.max(picker.offset).min(
                        (picker.offset + picker.visible - 1)
                            .min(picker.entries.len().saturating_sub(1)),
                    );
                    before != picker.offset
                }
                Some(Target::Panel(Pane::Folders) | Target::Folder(_) | Target::FolderPage(_))
                    if !app.show_folders =>
                {
                    let before = app.folder_offset;
                    app.folder_offset = before
                        .saturating_add_signed(direction)
                        .min(app.store.chat_lists().len().saturating_sub(1));
                    before != app.folder_offset
                }
                Some(Target::Folder(_) | Target::Panel(Pane::Modal) | Target::Modal)
                    if app.show_folders =>
                {
                    let before = app.modal_scroll;
                    app.modal_scroll = before
                        .saturating_add_signed(delta as i16)
                        .min(app.store.chat_lists().len().saturating_sub(1) as u16);
                    before != app.modal_scroll
                }
                Some(Target::Image(_)) if app.preview_message.is_some() => {
                    app.preview_view.step(if delta < 0 { 1 } else { -1 })
                }
                Some(
                    Target::Panel(Pane::Timeline)
                    | Target::Message(_)
                    | Target::Media(_)
                    | Target::MessageAction(_, _)
                    | Target::Text(crate::selection::Point {
                        source: crate::selection::Source::Message(_),
                        ..
                    })
                    | Target::Scroll(Pane::Timeline, _)
                    | Target::Command(Action::Bottom),
                ) => {
                    let changed = ui::scroll_timeline(app, delta);
                    if delta < 0
                        && app.timeline_top <= 3
                        && !app.show_search
                        && !app.store.history_exhausted
                        && !app.history_loading
                    {
                        perform(app, worker, Action::History);
                    }
                    changed
                }
                Some(
                    Target::Panel(Pane::Chats) | Target::Chat(_) | Target::Scroll(Pane::Chats, _),
                ) => {
                    let before = app.chat_offset;
                    app.chat_offset = app.chat_offset.saturating_add_signed(direction).min(
                        app.store
                            .chat_ids()
                            .count()
                            .saturating_sub(app.chat_visible),
                    );
                    before != app.chat_offset
                }
                Some(Target::Action(_) | Target::Panel(Pane::Modal) | Target::Modal)
                    if app.action_menu.is_some() =>
                {
                    let before = app.action_menu;
                    app.action_menu = before.map(|i| {
                        i.saturating_add_signed(direction)
                            .min(crate::menu::items(app).count().saturating_sub(1))
                    });
                    before != app.action_menu
                }
                Some(
                    Target::Sticker(_)
                    | Target::StickerTab(_)
                    | Target::Panel(Pane::Modal)
                    | Target::Modal,
                ) if app.sticker_picker => {
                    let cols = app.sticker_panel.columns.max(1);
                    let rows = app.sticker_panel.rows.max(1);
                    let len = app.sticker_items().len();
                    let before = app.sticker_panel.top;
                    app.sticker_panel.top = before
                        .saturating_add_signed(direction)
                        .min(len.div_ceil(cols).saturating_sub(rows));
                    let start = app.sticker_panel.top * cols;
                    if app.sticker_cursor < start {
                        app.sticker_cursor =
                            (start + app.sticker_cursor % cols).min(len.saturating_sub(1));
                    }
                    if app.sticker_cursor >= start + rows * cols {
                        app.sticker_cursor =
                            (start + (rows - 1) * cols + app.sticker_cursor % cols)
                                .min(len.saturating_sub(1));
                    }
                    before != app.sticker_panel.top
                }
                Some(Target::Panel(Pane::Modal) | Target::Modal)
                    if app.show_help || app.show_settings =>
                {
                    let before = app.modal_scroll;
                    app.modal_scroll = before.saturating_add_signed(delta as i16).min(40);
                    before != app.modal_scroll
                }
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn perform_target(app: &mut App, worker: &TdWorker, target: Target) -> bool {
    activate(app, worker, target)
}

fn activate(app: &mut App, worker: &TdWorker, target: Target) -> bool {
    match target {
        Target::Folder(list) => {
            let changed = app.store.select_list(list);
            let overlay = app.show_folders;
            app.close_overlays();
            if changed {
                app.chat_offset = 0;
                app.selected_chat = app.store.chat_ids().next();
                app.reveal_chat = true;
                let index = app
                    .store
                    .chat_lists()
                    .iter()
                    .position(|(candidate, _)| *candidate == list)
                    .unwrap_or(0);
                app.folder_offset = index;
                if !app.store.exhausted_lists.contains(&list) {
                    send_request(app, worker, actions::load_chat_list(list));
                }
            }
            changed || overlay
        }
        Target::FolderPage(delta) => {
            let before = app.folder_offset;
            app.folder_offset = before
                .saturating_add_signed(delta)
                .min(app.store.chat_lists().len().saturating_sub(1));
            before != app.folder_offset
        }
        Target::Media(id) | Target::MessageAction(id, Action::Preview) => {
            activate(app, worker, Target::Message(id));
            perform(app, worker, Action::Preview);
            true
        }
        Target::MessageAction(id, action) => {
            activate(app, worker, Target::Message(id));
            perform(app, worker, action)
        }
        Target::Text(point) => match point.source {
            crate::selection::Source::Composer => {
                activate(app, worker, Target::Cursor(Editor::Composer, point.byte))
            }
            crate::selection::Source::Message(id) => activate(app, worker, Target::Message(id)),
        },
        Target::Chat(id) => {
            if app.forward_message.is_some() {
                let changed = app.selected_chat != Some(id);
                app.selected_chat = Some(id);
                return changed;
            }
            let changed = app.input_mode != InputMode::Off
                || app.selected_chat != Some(id)
                || app.store.active_chat != Some(id)
                || !app.focus_messages;
            app.selected_chat = Some(id);
            for request in app.open_selected() {
                send_request(app, worker, request);
            }
            changed
        }
        Target::ReferencedMessage(id) => {
            if !app.store.messages.iter().any(|message| message.id == id) {
                return false;
            }
            app.quote_back.get_or_insert(crate::ReadingPosition {
                anchor: app.timeline_anchor,
                selected: app.selected_message,
            });
            activate(app, worker, Target::Message(id));
            app.reveal_message = true;
            app.selection = None;
            true
        }
        Target::Message(id) => {
            let changed =
                app.selected_message != Some(id) || !app.focus_messages || app.composer_focus;
            app.selected_message = Some(id);
            app.focus_messages = true;
            app.composer_focus = false;
            changed
        }
        Target::Composer => {
            if app.sticker_picker {
                crate::stickers::close(app);
            }
            if app.store.active_chat.is_none() {
                return false;
            }
            let changed = !app.composer_focus || app.input_mode == InputMode::Off;
            if app.input_mode == InputMode::Off {
                app.input_mode = InputMode::Send;
            }
            app.focus_messages = true;
            app.composer_focus = true;
            changed
        }
        Target::Cursor(Editor::Composer, position) => {
            let changed = activate(app, worker, Target::Composer);
            let position = text::cursor(&app.draft, position);
            let moved = text::cursor(&app.draft, app.draft_cursor) != position;
            app.draft_cursor = position;
            changed || moved
        }
        Target::Cursor(Editor::Auth(index), position) => {
            if let Some(form) = &mut app.auth.setup {
                let changed = form.focused != index || app.auth.setup_cursors[index] != position;
                form.focused = index;
                app.auth.setup_cursors[index] = position;
                changed
            } else {
                let changed = app.auth.cursor != position;
                app.auth.cursor = position;
                changed
            }
        }
        Target::Action(action) => crate::menu::execute(app, worker, action),
        Target::Sticker(index) => {
            app.sticker_cursor = index;
            perform(app, worker, Action::Confirm)
        }
        Target::StickerTab(tab) => crate::stickers::select_tab(app, worker, tab),
        Target::AttachmentEntry(index) => {
            if let Some(picker) = &mut app.attachments {
                picker.activate(index);
            }
            true
        }
        Target::AttachmentRemove(index) => {
            if let Some(picker) = &mut app.attachments
                && index < picker.selected.len()
            {
                picker.selected.remove(index);
            }
            true
        }
        Target::Cursor(Editor::AttachmentPath, position) => {
            if let Some(picker) = &mut app.attachments {
                picker.focus = crate::attachments::Focus::Path;
                picker.path_cursor = position;
            }
            true
        }
        Target::Cursor(Editor::AttachmentCaption, position) => {
            if let Some(picker) = &mut app.attachments {
                picker.focus = crate::attachments::Focus::Caption;
                picker.caption_cursor = position;
            }
            true
        }
        Target::Cursor(Editor::StickerSearch, position) => {
            app.sticker_panel.search_focus = true;
            app.sticker_panel.query_cursor = position;
            true
        }
        Target::Command(action) => perform(app, worker, action),
        Target::Scroll(Pane::Timeline, top) => ui::set_timeline_top(app, top),
        Target::Scroll(Pane::Chats, top) => {
            let changed = top != app.chat_offset;
            app.chat_offset = top;
            changed
        }
        Target::Backdrop
            if app.action_menu.is_some()
                || app.show_help
                || app.show_settings
                || app.preview_message.is_some()
                || app.show_folders
                || app.sticker_picker
                || app.attachments.is_some() =>
        {
            app.close_overlays();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        handle_ready_key,
        store::Message,
        tdlib::{TdCommand, TdEvent},
    };
    use crossterm::event::KeyModifiers;
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use serde_json::json;

    fn draw(app: &mut App, width: u16, height: u16) {
        Terminal::new(TestBackend::new(width, height))
            .unwrap()
            .draw(|frame| ui::draw(frame, app))
            .unwrap();
    }
    fn event(app: &mut App, worker: &TdWorker, kind: MouseEventKind, area: Rect) -> bool {
        mouse(
            app,
            worker,
            MouseEvent {
                kind,
                column: area.x,
                row: area.y,
                modifiers: KeyModifiers::NONE,
            },
        )
    }
    fn target(app: &App, wanted: Target) -> Rect {
        app.hit_targets
            .iter()
            .find(|(_, t)| *t == wanted)
            .map(|(rect, _)| *rect)
            .expect("visible target")
    }
    fn click(app: &mut App, worker: &TdWorker, wanted: Target) -> bool {
        let rect = target(app, wanted);
        event(app, worker, MouseEventKind::Down(MouseButton::Left), rect);
        event(app, worker, MouseEventKind::Up(MouseButton::Left), rect)
    }
    fn long_chat() -> App {
        let mut app = ui::tests::fixture();
        app.store.messages = (1..=60)
            .map(|id| Message {
                id,
                text: format!("消息 {id}\n第二行中文 👋"),
                outgoing: false,
                sender: None,
                author_signature: String::new(),
                media: None,
                ..Message::default()
            })
            .collect();
        app.selected_message = Some(60);
        app
    }

    #[test]
    fn quote_navigation_restores_position_and_retry_uses_existing_failed_message() {
        let mut app = long_chat();
        let (worker, requests) = TdWorker::test_pair();
        app.timeline_anchor = Some((58, 1));
        app.selected_message = Some(60);
        activate(&mut app, &worker, Target::ReferencedMessage(1));
        assert_eq!(app.selected_message, Some(1));
        assert!(app.reveal_message);
        perform(&mut app, &worker, Action::QuoteBack);
        assert_eq!(app.timeline_anchor, Some((58, 1)));
        assert_eq!(app.selected_message, Some(60));
        app.store.messages.last_mut().unwrap().info.sending = crate::store::Sending::Failed {
            can_retry: true,
            reason: "temporary".into(),
        };
        perform(&mut app, &worker, Action::Retry);
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("request");
        };
        assert_eq!(request["@type"], "resendMessages");
        assert_eq!(request["message_ids"], serde_json::json!([60]));
        assert_eq!(request["paid_message_star_count"], 0);
    }

    #[test]
    fn wheel_follows_panel_and_preserves_selection_and_composer() {
        let mut app = long_chat();
        let (worker, _) = TdWorker::test_pair();
        perform(&mut app, &worker, Action::Write);
        app.draft = "未发送的消息".into();
        draw(&mut app, 80, 24);
        let panel = target(&app, Target::Panel(Pane::Timeline));
        let before = app.timeline_top;
        assert!(event(&mut app, &worker, MouseEventKind::ScrollUp, panel));
        assert_eq!(app.timeline_top, before - 3);
        assert_eq!(app.selected_message, Some(60));
        assert_eq!(app.draft, "未发送的消息");
        assert!(app.composer_focus);
        assert!(!event(&mut app, &worker, MouseEventKind::Moved, panel));
        assert!(!event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            panel
        ));
        for id in 4..=30 {
            app.store.apply(&json!({"@type":"updateNewChat","chat":{"id":id,"title":"更多会话","positions":[{"list":{"@type":"chatListMain"},"order":100-id}]}}));
        }
        draw(&mut app, 80, 24);
        let chats = target(&app, Target::Panel(Pane::Chats));
        let top = app.timeline_top;
        assert!(event(&mut app, &worker, MouseEventKind::ScrollDown, chats));
        assert_eq!(app.chat_offset, 1);
        assert_eq!(app.timeline_top, top);
        assert_eq!(app.selected_chat, Some(1));
    }

    #[test]
    fn incoming_messages_and_older_history_keep_the_reading_anchor() {
        let mut app = long_chat();
        draw(&mut app, 80, 24);
        ui::scroll_timeline(&mut app, -12);
        draw(&mut app, 80, 24);
        let anchor = app.timeline_anchor;
        let top = app.timeline_top;
        app.apply(TdEvent::Update(json!({"@type":"updateNewMessage","message":{"id":61,"chat_id":1,"content":{"@type":"messageText","text":{"text":"最新消息"}}}})));
        draw(&mut app, 80, 24);
        assert_eq!(app.timeline_anchor, anchor);
        assert_eq!(app.timeline_top, top);
        assert_eq!(app.pending_messages, 1);
        app.apply(TdEvent::Update(json!({"@type":"messages","@extra":"history:1","messages":[{"id":0,"chat_id":1,"content":{"@type":"messageText","text":{"text":"更早消息"}}}]})));
        draw(&mut app, 80, 24);
        assert_eq!(app.timeline_anchor, anchor);
        assert!(app.timeline_top > top);
        let (worker, _) = TdWorker::test_pair();
        assert!(click(&mut app, &worker, Target::Command(Action::Bottom)));
        draw(&mut app, 80, 24);
        assert!(app.timeline_anchor.is_none());
        assert_eq!(app.pending_messages, 0);
        app.apply(TdEvent::Update(json!({"@type":"updateNewMessage","message":{"id":62,"chat_id":1,"content":{"@type":"messageText","text":{"text":"跟随新消息"}}}})));
        draw(&mut app, 80, 24);
        assert_eq!(app.timeline_top, app.timeline_max);
    }

    #[test]
    fn switching_chats_preserves_drafts_and_attachment_cancel() {
        let mut app = ui::tests::fixture();
        let (worker, _) = TdWorker::test_pair();
        perform(&mut app, &worker, Action::Write);
        app.draft = "保留草稿".into();
        app.draft_cursor = 3;
        perform(&mut app, &worker, Action::File);
        draw(&mut app, 80, 24);
        assert!(app.attachments.is_some());
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| matches!(t, Target::Chat(_)))
        );
        perform(&mut app, &worker, Action::Cancel);
        assert!(app.attachments.is_none());
        assert_eq!(app.draft, "保留草稿");
        assert_eq!(app.draft_cursor, 3);
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Chat(2));
        assert!(app.draft.is_empty());
        perform(&mut app, &worker, Action::Write);
        app.draft = "另一个聊天".into();
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Chat(1));
        assert_eq!(app.draft, "保留草稿");
        assert_eq!(app.draft_cursor, 3);
        assert!(app.input_mode == InputMode::Send);
    }

    #[test]
    fn context_menu_and_delete_buttons_require_matching_release_and_isolate_background() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 80, 24);
        let message = target(&app, Target::Message(2));
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Right),
            message,
        );
        assert!(event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Right),
            message
        ));
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Action(Action::Delete));
        assert_eq!(app.confirm_delete, Some(2));
        draw(&mut app, 80, 24);
        let confirm = target(&app, Target::Command(Action::Confirm));
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            confirm,
        );
        event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            Rect::new(0, 0, 1, 1),
        );
        assert!(requests.try_recv().is_err());
        assert_eq!(app.confirm_delete, Some(2));
        assert!(click(&mut app, &worker, Target::Command(Action::Confirm)));
        let TdCommand::Request(value) = requests.try_recv().unwrap() else {
            panic!("request");
        };
        assert_eq!(value["@type"], "deleteMessages");
        assert_eq!(value["message_ids"], json!([2]));
    }

    #[test]
    fn quick_message_menu_buttons_send_once_and_preserve_composer_draft() {
        for action in [Action::SaveMessage, Action::Repeat] {
            let mut app = crate::ui::tests::fixture();
            let (worker, requests) = TdWorker::test_pair();
            app.input_mode = InputMode::Reply(1);
            app.draft = "草稿不要丢".into();
            app.draft_cursor = 3;
            draw(&mut app, 80, 24);
            let message = target(&app, Target::Message(2));
            event(
                &mut app,
                &worker,
                MouseEventKind::Down(MouseButton::Right),
                message,
            );
            event(
                &mut app,
                &worker,
                MouseEventKind::Up(MouseButton::Right),
                message,
            );
            draw(&mut app, 80, 24);
            let button = target(&app, Target::Action(action));
            event(
                &mut app,
                &worker,
                MouseEventKind::Down(MouseButton::Left),
                button,
            );
            assert!(requests.try_recv().is_err());
            event(
                &mut app,
                &worker,
                MouseEventKind::Up(MouseButton::Left),
                button,
            );
            let TdCommand::Request(request) = requests.try_recv().expect("quick action") else {
                panic!("request")
            };
            assert_eq!(
                request["@type"],
                if action == Action::SaveMessage {
                    "getMe"
                } else {
                    "forwardMessages"
                }
            );
            event(
                &mut app,
                &worker,
                MouseEventKind::Up(MouseButton::Left),
                button,
            );
            assert!(
                requests.try_recv().is_err(),
                "stale release must not send twice"
            );
            assert!(app.action_menu.is_none());
            assert_eq!(app.draft, "草稿不要丢");
            assert_eq!(app.draft_cursor, 3);
            assert!(app.input_mode == InputMode::Reply(1));
        }
    }

    #[test]
    fn cursor_clicks_support_multiline_unicode_and_masked_authentication() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Send;
        app.draft = "中👨‍👩‍👧‍👦e\u{301}\n第二行".into();
        for (width, height) in [(80, 24), (45, 18), (30, 12)] {
            draw(&mut app, width, height);
            let index = "中👨‍👩‍👧‍👦e\u{301}\n".len();
            click(&mut app, &worker, Target::Cursor(Editor::Composer, index));
            assert_eq!(app.draft_cursor, index);
        }
        handle_ready_key(&mut app, &worker, KeyCode::Char('好'));
        assert!(app.draft.ends_with("\n好第二行"));
        app.auth.on_update(&json!({"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateWaitPassword"}}));
        app.auth.paste("中👨‍👩‍👧‍👦文");
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Cursor(Editor::Auth(0), 3));
        app.auth.key(KeyCode::Char('好'));
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Command(Action::Confirm));
        let TdCommand::Request(value) = requests.try_recv().unwrap() else {
            panic!("request");
        };
        assert_eq!(value["@type"], "checkAuthenticationPassword");
        assert_eq!(value["password"], "中好👨‍👩‍👧‍👦文");
    }

    #[test]
    fn api_fields_narrow_navigation_and_disabled_mouse_work() {
        let mut app = ui::tests::fixture();
        let (worker, _) = TdWorker::test_pair();
        app.auth.begin_setup();
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Cursor(Editor::Auth(1), 0));
        assert_eq!(app.auth.setup.as_ref().unwrap().focused, 1);
        draw(&mut app, 30, 12);
        click(&mut app, &worker, Target::Command(Action::NextField));
        assert_eq!(app.auth.setup.as_ref().unwrap().focused, 0);
        app.mouse_enabled = false;
        let area = target(&app, Target::Command(Action::NextField));
        assert!(!event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            area
        ));
        assert_eq!(app.auth.setup.as_ref().unwrap().focused, 0);
    }

    #[test]
    fn recovery_mouse_buttons_and_masked_key_work_in_small_terminals() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.auth.begin_setup();
        app.auth.on_update(&json!({"@type":"error","code":400,"message":"Wrong database encryption key","@extra":"set-tdlib-parameters"}));
        app.auth.paste("secret-key");
        for (width, height) in [(110, 32), (80, 24), (45, 18), (30, 12)] {
            draw(&mut app, width, height);
            click(&mut app, &worker, Target::Cursor(Editor::Auth(2), 3));
            assert_eq!(app.auth.setup_cursors[2], 3);
            click(&mut app, &worker, Target::Command(Action::NewLogin));
            assert!(app.auth.is_new_login());
            assert!(!app.auth.take_restart());
            draw(&mut app, width, height);
            assert!(
                !app.hit_targets
                    .iter()
                    .any(|(_, target)| matches!(target, Target::Cursor(Editor::Auth(_), _)))
            );
            click(&mut app, &worker, Target::Command(Action::Back));
            assert!(app.auth.is_recovering() && !app.auth.is_new_login());
        }
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn draft_cache_is_bounded_and_scrollbar_click_reaches_edges() {
        let mut app = long_chat();
        let (worker, _) = TdWorker::test_pair();
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Scroll(Pane::Timeline, 0));
        assert_eq!(app.timeline_top, 0);
        for id in 2..=25 {
            app.input_mode = InputMode::Send;
            app.draft = "x".repeat(65536);
            app.selected_chat = Some(id);
            app.open_selected();
        }
        assert!(app.drafts.len() <= 16);
        assert!(
            app.drafts
                .iter()
                .map(|(_, saved)| saved.text.len())
                .sum::<usize>()
                <= 524288
        );
    }
    #[test]
    fn mouse_folder_switch_loads_matching_list_and_modal_isolates_chat_controls() {
        let mut app = ui::tests::fixture();
        app.store.apply(&json!({"@type":"updateChatFolders","chat_folders":[{"id":7,"name":{"text":{"text":"工作"}}}]}));
        app.store.apply(&json!({"@type":"updateChatPosition","chat_id":2,"position":{"list":{"@type":"chatListFolder","chat_folder_id":7},"order":"100"}}));
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 110, 32);
        click(
            &mut app,
            &worker,
            Target::Folder(crate::store::ChatList::Folder(7)),
        );
        assert_eq!(app.store.chat_ids().collect::<Vec<_>>(), vec![2]);
        assert_eq!(app.selected_chat, Some(2));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("load request");
        };
        assert_eq!(request["chat_list"]["chat_folder_id"], 7);
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Command(Action::Folders));
        for (width, height) in [(110, 32), (45, 18), (30, 12)] {
            draw(&mut app, width, height);
            assert!(app.hit_targets.iter().all(|(_, target)| !matches!(
                target,
                Target::Chat(_) | Target::Message(_) | Target::Composer
            )));
            assert!(
                app.hit_targets
                    .iter()
                    .any(|(_, target)| *target == Target::Folder(crate::store::ChatList::Main))
            );
        }
        click(
            &mut app,
            &worker,
            Target::Folder(crate::store::ChatList::Main),
        );
        assert!(!app.show_folders);
        assert_eq!(app.store.selected_list, crate::store::ChatList::Main);
    }

    #[test]
    fn photo_preview_downloads_detail_on_demand_and_preserves_thumbnail() {
        let mut app = ui::tests::fixture();
        app.store.messages[1].media = Some(crate::store::MediaRef {
            file_id: 42,
            path: Some("/thumbnail.png".into()),
            kind: crate::store::MediaKind::Photo,
            detail: Some(crate::store::MediaFile {
                file_id: 43,
                path: None,
            }),
        });
        app.selected_message = Some(2);
        let (worker, requests) = TdWorker::test_pair();
        perform(&mut app, &worker, Action::Preview);
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("download");
        };
        assert_eq!(request["@type"], "downloadFile");
        assert_eq!(request["file_id"], 43);
        assert!(app.requested_files.contains(&43));
        assert_eq!(
            app.store.messages[1]
                .media
                .as_ref()
                .unwrap()
                .path
                .as_deref(),
            Some("/thumbnail.png")
        );
        perform(&mut app, &worker, Action::Preview);
        assert!(requests.try_recv().is_err(), "duplicate download");
        app.apply(TdEvent::Update(json!({"@type":"updateFile","file":{"id":43,"local":{"is_downloading_completed":true,"path":"/detail.png"}}})));
        assert_eq!(
            app.store.messages[1].media.as_ref().unwrap().detail_file(),
            (43, Some("/detail.png"))
        );
        assert!(!app.requested_files.contains(&43));
    }

    #[test]
    fn inline_images_remain_with_messages_and_only_visible_files_download() {
        let mut app = ui::tests::fixture();
        for (index, message) in app.store.messages.iter_mut().enumerate() {
            message.text = format!("caption {}", message.id);
            message.media = Some(crate::store::MediaRef {
                file_id: 40 + index as i32,
                path: None,
                kind: crate::store::MediaKind::Photo,
                detail: None,
            });
        }
        draw(&mut app, 110, 40);
        assert_eq!(app.visible_media, vec![40, 41]);
        let first = target(&app, Target::Media(1));
        let second = target(&app, Target::Media(2));
        assert!(first.bottom() < second.y);
        let request = app.visible_media_download().unwrap();
        assert_eq!(request["file_id"], 40);
        assert_eq!(app.visible_media_download().unwrap()["file_id"], 41);
        assert!(app.visible_media_download().is_none());
        app.selected_message = Some(1);
        draw(&mut app, 110, 40);
        assert_eq!(target(&app, Target::Media(2)), second);
        app.store.messages.insert(
            0,
            Message {
                id: 0,
                text: "off-screen".repeat(40),
                outgoing: false,
                sender: None,
                author_signature: String::new(),
                media: Some(crate::store::MediaRef {
                    file_id: 99,
                    path: None,
                    kind: crate::store::MediaKind::Photo,
                    detail: None,
                }),
                ..Message::default()
            },
        );
        app.timeline_heights.clear();
        draw(&mut app, 110, 24);
        assert!(!app.visible_media.contains(&99));
        app.apply(TdEvent::Update(
            json!({"@type":"error","code":400,"@extra":"download:41","message":"download failed"}),
        ));
        assert!(app.failed_files.contains(&41));
        assert!(app.visible_media_download().is_none());
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 110, 24);
        click(&mut app, &worker, Target::Media(2));
        assert_eq!(app.preview_message, Some(2));
        assert!(!app.failed_files.contains(&41));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("retry");
        };
        assert_eq!(request["file_id"], 41);
    }

    #[test]
    fn chat_header_buttons_receive_clicks_above_the_timeline_panel() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Command(Action::Search));
        assert!(app.input_mode == InputMode::Search);
        assert!(requests.try_recv().is_err());
        perform(&mut app, &worker, Action::Cancel);
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Command(Action::ChatMenu));
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Action(Action::History));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("history");
        };
        assert_eq!(request["@type"], "getChatHistory");
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Command(Action::ChatMenu));
        assert!(app.action_menu.is_some());
    }

    #[test]
    fn mouse_reply_button_targets_its_message_and_wheel_loads_history_once() {
        let mut app = long_chat();
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::MessageAction(60, Action::Reply));
        assert!(app.input_mode == InputMode::Reply(60));
        assert!(requests.try_recv().is_err());
        ui::set_timeline_top(&mut app, 0);
        draw(&mut app, 110, 32);
        let timeline = target(&app, Target::Panel(Pane::Timeline));
        for _ in 0..4 {
            event(&mut app, &worker, MouseEventKind::ScrollUp, timeline);
        }
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("history");
        };
        assert_eq!(request["@type"], "getChatHistory");
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn inline_media_selection_keeps_geometry_and_click_downloads_preview() {
        let mut app = long_chat();
        app.store.messages[59].media = Some(crate::store::MediaRef {
            file_id: 42,
            path: None,
            kind: crate::store::MediaKind::Photo,
            detail: None,
        });
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 110, 32);
        let area = target(&app, Target::Panel(Pane::Timeline));
        let maximum = app.timeline_max;
        app.selected_message = Some(59);
        draw(&mut app, 110, 32);
        assert_eq!(target(&app, Target::Panel(Pane::Timeline)), area);
        assert_eq!(app.timeline_max, maximum);
        app.selected_message = Some(60);
        draw(&mut app, 110, 32);
        click(&mut app, &worker, Target::Media(60));
        assert_eq!(app.preview_message, Some(60));
        let TdCommand::Request(value) = requests.try_recv().unwrap() else {
            panic!("request");
        };
        assert_eq!(value["@type"], "downloadFile");
        assert_eq!(value["file_id"], 42);
        draw(&mut app, 110, 32);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| matches!(t, Target::Message(_) | Target::Chat(_)))
        );
    }

    #[test]
    fn deleting_reading_anchor_keeps_nearby_history_and_layout_cache_survives_chat_updates() {
        let mut app = long_chat();
        draw(&mut app, 80, 24);
        ui::scroll_timeline(&mut app, -24);
        draw(&mut app, 80, 24);
        let (id, _) = app.timeline_anchor.unwrap();
        let cached = app.timeline_heights.clone();
        app.apply(TdEvent::Update(
            json!({"@type":"updateChatTitle","chat_id":1,"title":"新标题"}),
        ));
        draw(&mut app, 80, 24);
        assert_eq!(app.timeline_heights, cached);
        app.apply(TdEvent::Update(
            json!({"@type":"updateDeleteMessages","chat_id":1,"message_ids":[id]}),
        ));
        draw(&mut app, 80, 24);
        assert_eq!(app.timeline_anchor.map(|(anchor, _)| anchor), Some(id + 1));
        assert!(app.timeline_top < app.timeline_max);
    }
    #[test]
    fn drag_across_wrapped_messages_copies_original_text_and_reverse_selection() {
        let mut app = ui::tests::fixture();
        app.store.messages[0].text = "hello 中文 👨‍👩‍👧‍👦\nnext".into();
        app.store.messages[1].text = "second e\u{301} message".into();
        let (worker, requests) = TdWorker::test_pair();
        draw(&mut app, 45, 18);
        let a = Target::Text(crate::selection::Point {
            source: crate::selection::Source::Message(1),
            byte: 6,
        });
        let b = Target::Text(crate::selection::Point {
            source: crate::selection::Source::Message(2),
            byte: 6,
        });
        let from = target(&app, a);
        let to = target(&app, b);
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            from,
        );
        assert!(event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            to
        ));
        event(&mut app, &worker, MouseEventKind::Up(MouseButton::Left), to);
        assert_eq!(
            crate::selection::selected_text(&app).unwrap(),
            Some("中文 👨‍👩‍👧‍👦\nnext\nsecond".into())
        );
        assert!(app.drag.is_none());
        assert!(requests.try_recv().is_err());
        draw(&mut app, 45, 18);
        // Reverse dragging selects the exact same logical range.
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            to,
        );
        event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            from,
        );
        event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            from,
        );
        assert_eq!(
            crate::selection::selected_text(&app).unwrap(),
            Some("中文 👨‍👩‍👧‍👦\nnext\nsecond".into())
        );
    }

    #[test]
    fn double_click_selects_word_then_typing_replaces_and_scrollbar_drags_clamp() {
        let mut app = long_chat();
        let (worker, _) = TdWorker::test_pair();
        app.input_mode = InputMode::Send;
        app.draft = "hello 中文\nsecond".into();
        draw(&mut app, 80, 24);
        let word = Target::Cursor(Editor::Composer, 1);
        click(&mut app, &worker, word);
        draw(&mut app, 80, 24);
        click(&mut app, &worker, word);
        assert_eq!(
            crate::selection::selected_text(&app).unwrap(),
            Some("hello".into())
        );
        handle_ready_key(&mut app, &worker, KeyCode::Char('好'));
        assert_eq!(app.draft, "好 中文\nsecond");
        draw(&mut app, 80, 24);
        let bar = target(&app, Target::Scroll(Pane::Timeline, 0));
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            bar,
        );
        event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            Rect::new(bar.x, 100, 1, 1),
        );
        assert_eq!(app.timeline_top, app.timeline_max);
        event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            Rect::new(bar.x, 0, 1, 1),
        );
        assert_eq!(app.timeline_top, 0);
        event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            bar,
        );
        assert!(app.drag.is_none());
    }

    #[test]
    fn double_click_media_zoom_pan_reset_and_fallback_render() {
        let mut app = long_chat();
        // A nonexistent fixture path queues work without opening external applications.
        app.store.messages[59].media = Some(crate::store::MediaRef {
            file_id: 42,
            path: Some("/nonexistent/tg-mouse-test.png".into()),
            kind: crate::store::MediaKind::Photo,
            detail: None,
        });
        let (worker, _) = TdWorker::test_pair();
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Message(60));
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Message(60));
        assert_eq!(app.preview_message, Some(60));
        draw(&mut app, 80, 24);
        let area = app
            .hit_targets
            .iter()
            .find_map(|(_, t)| {
                if let Target::Image(r) = t {
                    Some(*r)
                } else {
                    None
                }
            })
            .unwrap();
        for _ in 0..10 {
            event(&mut app, &worker, MouseEventKind::ScrollUp, area);
        }
        assert_eq!(app.preview_view.zoom, 400);
        let origin = Rect::new(area.x + area.width / 2, area.y + area.height / 2, 1, 1);
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            origin,
        );
        assert!(event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            Rect::new(origin.x + 5, origin.y + 2, 1, 1)
        ));
        assert!(app.preview_view.x < 500 && app.preview_view.y < 500);
        event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            origin,
        );
        assert!(app.drag.is_none());
        perform(&mut app, &worker, Action::ZoomReset);
        assert_eq!(app.preview_view, crate::media::View::default());
        assert!(!event(&mut app, &worker, MouseEventKind::ScrollDown, area));
        for (width, height) in [(110, 32), (45, 18), (30, 12)] {
            draw(&mut app, width, height);
        }
    }

    #[test]
    fn drag_selection_auto_scrolls_history_without_activating_buttons() {
        let mut app = long_chat();
        let (worker, _) = TdWorker::test_pair();
        draw(&mut app, 80, 24);
        ui::scroll_timeline(&mut app, -20);
        draw(&mut app, 80, 24);
        let text = app
            .hit_targets
            .iter()
            .find(|(_, t)| matches!(t, Target::Text(_)))
            .unwrap()
            .0;
        let area = target(&app, Target::Panel(Pane::Timeline));
        event(
            &mut app,
            &worker,
            MouseEventKind::Down(MouseButton::Left),
            text,
        );
        event(
            &mut app,
            &worker,
            MouseEventKind::Drag(MouseButton::Left),
            Rect::new(text.x, area.bottom(), 1, 1),
        );
        let before = app.timeline_top;
        assert!(tick(&mut app));
        assert_eq!(app.timeline_top, before + 1);
        draw(&mut app, 80, 24);
        assert!(crate::selection::active(&app));
        event(
            &mut app,
            &worker,
            MouseEventKind::Up(MouseButton::Left),
            Rect::new(text.x, area.bottom(), 1, 1),
        );
        assert!(app.drag.is_none());
    }
    #[test]
    fn attachment_mouse_selection_caption_confirmation_and_draft_survive() {
        let folder = std::env::temp_dir().join(format!("tg-attach-click-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("选择我.txt");
        std::fs::write(&path, b"hello").unwrap();
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Reply(2);
        app.draft = "原来的草稿".into();
        app.draft_cursor = 3;
        perform(&mut app, &worker, Action::File);
        app.attachments.as_mut().unwrap().navigate(folder.clone());
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::AttachmentEntry(1));
        assert_eq!(app.attachments.as_ref().unwrap().selected.len(), 1);
        assert!(
            requests.try_recv().is_err(),
            "selecting a file must not send it"
        );
        app.attachments.as_mut().unwrap().caption = "文件说明".into();
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Command(Action::Confirm));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(
            request["input_message_content"]["@type"],
            "inputMessageDocument"
        );
        assert_eq!(
            request["input_message_content"]["caption"]["text"],
            "文件说明"
        );
        assert_eq!(request["reply_to"]["message_id"], 2);
        assert!(app.attachments.is_none());
        assert_eq!(app.draft, "原来的草稿");
        assert_eq!(app.draft_cursor, 3);
        std::fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn sticker_grid_click_sends_tile_and_keeps_tray_open() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.sticker_picker = true;
        app.store.apply(&json!({"@type":"stickers","@extra":"recent-stickers","stickers":(1..=12).map(|id|json!({"sticker":{"id":id},"emoji":"😊","width":128,"height":128,"format":{"@type":"stickerFormatWebp"}})).collect::<Vec<_>>()}));
        draw(&mut app, 80, 24);
        let tile = target(&app, Target::Sticker(1));
        assert!(
            tile.width > 5 && tile.height >= 3,
            "stickers must be a thumbnail grid"
        );
        click(&mut app, &worker, Target::Sticker(1));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(request["input_message_content"]["sticker"]["id"], 2);
        assert!(app.sticker_picker, "tray should support consecutive sends");
        while requests.try_recv().is_ok() {}
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Command(Action::StickerFavorite));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(request["@type"], "addFavoriteSticker");
        assert_eq!(request["sticker"]["id"], 2);
    }

    #[test]
    fn composer_send_while_tray_is_open_sends_text_and_closing_preserves_draft() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.input_mode = InputMode::Send;
        app.draft = "草稿不能变成贴纸".into();
        app.sticker_picker = true;
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Composer);
        assert!(!app.sticker_picker);
        assert_eq!(app.draft, "草稿不能变成贴纸");
        app.sticker_picker = true;
        draw(&mut app, 80, 24);
        click(&mut app, &worker, Target::Command(Action::Submit));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!("text");
        };
        assert_eq!(
            request["input_message_content"]["@type"],
            "inputMessageText"
        );
        assert_eq!(
            request["input_message_content"]["text"]["text"],
            "草稿不能变成贴纸"
        );
        assert!(!app.sticker_picker);
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn sticker_wheel_scrolls_grid_immediately_and_click_uses_new_visible_tile() {
        let mut app = ui::tests::fixture();
        let (worker, requests) = TdWorker::test_pair();
        app.sticker_picker = true;
        app.store.apply(&json!({"@type":"stickers","@extra":"recent-stickers","stickers":(1..=60).map(|id|json!({"sticker":{"id":id},"emoji":"😊","width":128,"height":128,"format":{"@type":"stickerFormatWebp"}})).collect::<Vec<_>>()}));
        draw(&mut app, 80, 24);
        let first = target(&app, Target::Sticker(0));
        mouse(
            &mut app,
            &worker,
            MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: first.x + 1,
                row: first.y + 1,
                modifiers: KeyModifiers::NONE,
            },
        );
        draw(&mut app, 80, 24);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Sticker(0)),
            "first wheel event should scroll the grid, not only change selection"
        );
        let index = app.sticker_panel.columns;
        assert_eq!(target(&app, Target::Sticker(index)), first);
        click(&mut app, &worker, Target::Sticker(index));
        let TdCommand::Request(request) = requests.try_recv().unwrap() else {
            panic!()
        };
        assert_eq!(request["input_message_content"]["sticker"]["id"], index + 1);
    }
}
