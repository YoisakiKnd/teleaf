//! Shared page layout. Views render only visible messages and reuse media caches.
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui_image::Image;
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::selection::{self, Point, Source};
use crate::store::{MediaKind, MediaRef};
use crate::theme::palette;
use crate::{App, InputMode};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Pane {
    Chats,
    Timeline,
    Modal,
    Folders,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Editor {
    Composer,
    Auth(usize),
    AttachmentPath,
    AttachmentCaption,
    StickerSearch,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Action {
    Write,
    Reply,
    Retry,
    QuoteBack,
    Edit,
    Forward,
    SaveMessage,
    Repeat,
    React,
    Delete,
    Photo,
    File,
    Stickers,
    Search,
    History,
    MoreChats,
    Copy,
    Menu,
    ChatMenu,
    Submit,
    Confirm,
    Cancel,
    Bottom,
    OpenExternal,
    Preview,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    OpenLink,
    ApiSetup,
    Settings,
    Help,
    ToggleMouse,
    NextField,
    NewLogin,
    Folders,
    ClearSearch,
    Back,
    AttachmentMode,
    AttachmentParent,
    PasteClipboard,
    StickerPrevious,
    StickerNext,
    StickerSearch,
    StickerFavorite,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Target {
    Chat(i64),
    Folder(crate::store::ChatList),
    FolderPage(isize),
    Media(i64),
    MessageAction(i64, Action),
    Message(i64),
    ReferencedMessage(i64),
    Text(Point),
    Image(Rect),
    Composer,
    Action(Action),
    Sticker(usize),
    StickerTab(crate::stickers::Tab),
    AttachmentEntry(usize),
    AttachmentRemove(usize),
    AttachmentQueue,
    Command(Action),
    Panel(Pane),
    Cursor(Editor, usize),
    Scroll(Pane, usize),
    Backdrop,
    Modal,
}

pub fn action_for_key(key: char) -> Option<Action> {
    Some(match key {
        'i' => Action::Write,
        'r' => Action::Reply,
        'R' => Action::Retry,
        'b' => Action::QuoteBack,
        'e' => Action::Edit,
        'f' => Action::Forward,
        'S' => Action::SaveMessage,
        'D' => Action::Repeat,
        'x' => Action::React,
        'd' => Action::Delete,
        'c' => Action::Copy,
        'p' => Action::Photo,
        'a' => Action::File,
        't' => Action::Stickers,
        '/' => Action::Search,
        'g' => Action::History,
        'G' => Action::Bottom,
        'm' => Action::MoreChats,
        'o' => Action::OpenExternal,
        'v' => Action::Preview,
        _ => return None,
    })
}

fn button(frame: &mut Frame, app: &mut App, area: Rect, label: &str, action: Action) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let pressed = (app.show_settings && app.settings_focus == Some(action))
        || app
            .mouse_press
            .is_some_and(|(_, target)| target == Target::Command(action));
    let primary = matches!(action, Action::Submit | Action::Confirm);
    let destructive =
        action == Action::Delete || (action == Action::Confirm && app.confirm_delete.is_some());
    let style = if primary && !destructive {
        palette().primary()
    } else if pressed {
        palette()
            .selection()
            .fg(if destructive {
                palette().error
            } else {
                palette().text
            })
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(if destructive {
            palette().error
        } else {
            palette().muted
        })
    };
    // A compact filled label keeps the border and whitespace quiet. The whole
    // original button rectangle remains clickable, including its surrounding space.
    let label_width =
        (UnicodeWidthStr::width(label) as u16 + if primary { 2 } else { 0 }).min(area.width);
    let visual = Rect::new(
        area.x + (area.width - label_width) / 2,
        area.y,
        label_width,
        area.height,
    );
    frame.render_widget(
        Paragraph::new(label)
            .style(style)
            .alignment(Alignment::Center),
        visual,
    );
    app.hit_targets.push((area, Target::Command(action)));
}

fn target_button(frame: &mut Frame, app: &mut App, area: Rect, label: &str, target: Target) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let selected = matches!(target, Target::Folder(list) if list == app.store.selected_list);
    let pressed = app
        .mouse_press
        .is_some_and(|(_, pressed)| pressed == target);
    let style = if selected {
        palette()
            .selection()
            .fg(palette().accent)
            .add_modifier(Modifier::BOLD)
    } else if pressed {
        palette().selection()
    } else {
        Style::default().fg(palette().muted)
    };
    frame.render_widget(
        Paragraph::new(label)
            .style(style)
            .alignment(Alignment::Center),
        area,
    );
    app.hit_targets.push((area, target));
}

fn folder_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    app.hit_targets.push((area, Target::Panel(Pane::Folders)));
    let lists = app.store.chat_lists();
    let slots = usize::from(area.width.saturating_sub(6) / 8).max(1);
    app.folder_offset = app.folder_offset.min(lists.len().saturating_sub(slots));
    target_button(
        frame,
        app,
        Rect::new(area.x, area.y, 2, 1),
        "‹",
        Target::FolderPage(-1),
    );
    for (index, (list, name)) in lists.iter().skip(app.folder_offset).take(slots).enumerate() {
        let rect = Rect::new(
            area.x + 2 + index as u16 * 8,
            area.y,
            8.min(area.width.saturating_sub(6)),
            1,
        );
        let label = crate::text::ellipsize(
            &format!(
                "{}{}",
                if *list == app.store.selected_list {
                    "●"
                } else {
                    " "
                },
                name
            ),
            usize::from(rect.width),
        );
        target_button(frame, app, rect, &label, Target::Folder(*list));
    }
    target_button(
        frame,
        app,
        Rect::new(area.right().saturating_sub(4), area.y, 2, 1),
        "›",
        Target::FolderPage(1),
    );
    button(
        frame,
        app,
        Rect::new(area.right().saturating_sub(2), area.y, 2, 1),
        "▾",
        Action::Folders,
    );
}

fn modal(frame: &mut Frame, app: &mut App, area: Rect, title: &str) -> Rect {
    let title = wrap_text(title, usize::from(area.width.saturating_sub(8)).max(1))
        .into_iter()
        .next()
        .unwrap_or_default();
    let inner = dialog(frame, area, &title);
    app.hit_targets.push((area, Target::Modal));
    app.hit_targets.push((inner, Target::Panel(Pane::Modal)));
    let close = Rect::new(area.right().saturating_sub(5), area.y, 3.min(area.width), 1);
    button(frame, app, close, "×", Action::Cancel);
    inner
}

fn scrollbar(frame: &mut Frame, app: &mut App, area: Rect, pane: Pane, top: usize, maximum: usize) {
    if maximum == 0 || area.height < 2 || area.width == 0 {
        return;
    }
    app.scrollbars.push((pane, area, maximum));
    let thumb = top * usize::from(area.height - 1) / maximum;
    for row in 0..area.height {
        let cell = Rect::new(area.right() - 1, area.y + row, 1, 1);
        frame.render_widget(
            Paragraph::new(if usize::from(row) == thumb {
                "┃"
            } else {
                "│"
            })
            .style(Style::default().fg(if usize::from(row) == thumb {
                palette().accent
            } else {
                palette().border
            })),
            cell,
        );
        app.hit_targets.push((
            cell,
            Target::Scroll(
                pane,
                usize::from(row) * maximum / usize::from(area.height - 1),
            ),
        ));
    }
}

pub fn scroll_timeline(app: &mut App, delta: isize) -> bool {
    set_timeline_top(
        app,
        app.timeline_top
            .saturating_add_signed(delta)
            .min(app.timeline_max),
    )
}

pub fn set_timeline_top(app: &mut App, top: usize) -> bool {
    let top = top.min(app.timeline_max);
    if top == app.timeline_top {
        return false;
    }
    app.timeline_top = top;
    anchor_at(app, top);
    true
}

fn anchor_at(app: &mut App, top: usize) {
    if top >= app.timeline_max {
        app.timeline_anchor = None;
        app.pending_messages = 0;
        return;
    }
    let mut offset = top;
    for &(id, height) in &app.timeline_rows {
        if offset < height {
            app.timeline_anchor = Some((id, offset));
            return;
        }
        offset -= height;
    }
    app.timeline_anchor = None;
}

fn panel(title: impl Into<Line<'static>>, focused: bool) -> Block<'static> {
    let title = title.into().style(
        Style::default()
            .fg(if focused {
                palette().accent
            } else {
                palette().text
            })
            .add_modifier(Modifier::BOLD),
    );
    Block::default()
        .style(palette().base())
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(if focused {
                    palette().accent
                } else {
                    palette().border
                })
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
}

fn muted(text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), Style::default().fg(palette().muted))
}

// Flatten and truncate only the visible prefix. A long reply must not copy its
// full body into a one-line composer title on every keystroke.
fn excerpt(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0;
    for grapheme in value.graphemes(true) {
        let grapheme = if grapheme.chars().any(char::is_control) {
            " "
        } else {
            grapheme
        };
        let size = UnicodeWidthStr::width(grapheme);
        if used + size > width
            || output.len().saturating_add(grapheme.len()) > width.saturating_mul(16).max(32)
        {
            while used >= width {
                let (index, last) = output
                    .grapheme_indices(true)
                    .next_back()
                    .expect("nonempty prefix");
                used = used.saturating_sub(UnicodeWidthStr::width(last));
                output.truncate(index);
            }
            output.push('…');
            break;
        }
        output.push_str(grapheme);
        used += size;
    }
    output
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    )
}

fn dialog(frame: &mut Frame, area: Rect, title: &str) -> Rect {
    frame.render_widget(Clear, area);
    let block = panel(title.to_owned(), true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    inner
}

fn empty(frame: &mut Frame, area: Rect, title: &str, hint: &str) {
    let inner = centered(area, area.width.saturating_sub(4), 4);
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                title.to_owned(),
                Style::default()
                    .fg(palette().text)
                    .add_modifier(Modifier::BOLD),
            ),
            Line::from(""),
            muted(hint),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: false }),
        inner,
    );
}

fn auth_field(frame: &mut Frame, app: &mut App, area: Rect, index: usize, label: &str) {
    let (value, position, masked, focused) = if let Some(form) = &app.auth.setup {
        (
            form.field(index).to_owned(),
            app.auth.setup_cursors[index],
            index != 0,
            form.focused == index,
        )
    } else {
        (
            app.auth.input_text().to_owned(),
            app.auth.cursor,
            app.auth.is_password(),
            true,
        )
    };
    let block = panel(label.to_owned(), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hit_targets.push((
        area,
        Target::Cursor(Editor::Auth(index), crate::text::cursor(&value, position)),
    ));
    editor(
        frame,
        app,
        inner,
        Editor::Auth(index),
        &value,
        position,
        masked,
    );
}

fn selected_line(
    value: &str,
    row: crate::text::Row,
    range: Option<std::ops::Range<usize>>,
    prefix: &str,
) -> Line<'static> {
    let mut spans = vec![Span::raw(prefix.to_owned())];
    let start = range
        .as_ref()
        .map_or(row.end, |r| r.start.clamp(row.start, row.end));
    let end = range
        .as_ref()
        .map_or(row.end, |r| r.end.clamp(row.start, row.end));
    spans.push(Span::raw(value[row.start..start].to_owned()));
    if start < end {
        spans.push(Span::styled(
            value[start..end].to_owned(),
            palette().primary(),
        ));
    }
    spans.push(Span::raw(value[end..row.end].to_owned()));
    Line::from(spans)
}

fn point_targets(
    value: &str,
    row: crate::text::Row,
    source: Source,
    area: Rect,
) -> Vec<(Rect, Target)> {
    let mut targets = vec![(
        area,
        Target::Text(Point {
            source,
            byte: row.end,
        }),
    )];
    let mut x = area.x;
    for (i, g) in value[row.start..row.end].grapheme_indices(true) {
        let width = UnicodeWidthStr::width(g) as u16;
        if x + width <= area.right() {
            targets.push((
                Rect::new(x, area.y, width, 1),
                Target::Text(Point {
                    source,
                    byte: row.start + i,
                }),
            ));
        }
        x += width;
    }
    targets
}

fn editor(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    kind: Editor,
    value: &str,
    position: usize,
    masked: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let position = crate::text::cursor(value, position);
    let width = usize::from(area.width.saturating_sub(1)).max(1);
    let rows = if masked {
        // Secrets are mapped by grapheme, never by their encoded byte length.
        let indices: Vec<_> = value
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(value.len()))
            .collect();
        indices[..indices.len() - 1]
            .chunks(width)
            .enumerate()
            .map(|(row, chunk)| crate::text::Row {
                start: chunk[0],
                end: indices[row * width + chunk.len()],
            })
            .collect::<Vec<_>>()
    } else {
        crate::text::rows(value, width)
    };
    let rows = if rows.is_empty() {
        vec![crate::text::Row { start: 0, end: 0 }]
    } else {
        rows
    };
    let cursor_row = rows
        .iter()
        .rposition(|row| row.start <= position)
        .unwrap_or(0);
    let start = cursor_row.saturating_sub(usize::from(area.height) - 1);
    for (y, row) in rows
        .iter()
        .enumerate()
        .skip(start)
        .take(usize::from(area.height))
    {
        let y = area.y + (y - start) as u16;
        let line_area = Rect::new(area.x, y, area.width, 1);
        let shown = if masked {
            "•".repeat(value[row.start..row.end].graphemes(true).count())
        } else {
            value[row.start..row.end].to_owned()
        };
        if kind == Editor::Composer {
            frame.render_widget(
                Paragraph::new(selected_line(
                    value,
                    *row,
                    selection::range(app, Source::Composer),
                    "",
                )),
                line_area,
            );
        } else {
            frame.render_widget(Paragraph::new(shown), line_area);
        }
        app.hit_targets
            .push((line_area, Target::Cursor(kind, row.end)));
        let mut x = area.x;
        for (i, g) in value[row.start..row.end].grapheme_indices(true) {
            let w = if masked {
                1
            } else {
                UnicodeWidthStr::width(g) as u16
            };
            if x + w <= area.right() {
                app.hit_targets
                    .push((Rect::new(x, y, w, 1), Target::Cursor(kind, row.start + i)));
            }
            x += w;
        }
    }
    let overlay_editor = matches!(
        kind,
        Editor::AttachmentPath | Editor::AttachmentCaption | Editor::StickerSearch
    );
    let focused = match kind {
        Editor::AttachmentPath => app
            .attachments
            .as_ref()
            .is_some_and(|p| p.focus == crate::attachments::Focus::Path),
        Editor::AttachmentCaption => app
            .attachments
            .as_ref()
            .is_some_and(|p| p.focus == crate::attachments::Focus::Caption),
        Editor::StickerSearch => app.sticker_panel.search_focus,
        Editor::Composer => app.composer_focus && app.input_mode != InputMode::Off,
        Editor::Auth(index) => app
            .auth
            .setup
            .as_ref()
            .is_none_or(|form| form.focused == index),
    };
    if focused && (!app.has_overlay() || overlay_editor) {
        let row = rows[cursor_row];
        let prefix = &value[row.start..position.min(row.end)];
        let x = if masked {
            prefix.graphemes(true).count()
        } else {
            UnicodeWidthStr::width(prefix)
        };
        frame.set_cursor_position((
            area.x + (x as u16).min(area.width - 1),
            area.y + (cursor_row - start) as u16,
        ));
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.media.begin_frame();
    app.visible_media.clear();
    app.hit_targets.clear();
    app.scrollbars.clear();
    let area = frame.area();
    frame.buffer_mut().set_style(area, palette().base());
    if area.width < 30 || area.height < 12 {
        empty(
            frame,
            area,
            "终端窗口太小",
            "请调到至少 30 列 × 12 行 · Ctrl+Q 退出",
        );
        app.media.end_frame();
        return;
    }
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    frame
        .buffer_mut()
        .set_style(layout[0], Style::default().bg(palette().surface));
    frame
        .buffer_mut()
        .set_style(layout[2], Style::default().bg(palette().surface));
    let connection = if app.demo {
        "离线演示"
    } else if app.auth.is_recovering() {
        "恢复登录"
    } else if app.auth.setup.is_some() {
        "首次设置"
    } else if app.auth.state == "authorizationStateReady" {
        &app.connection_label
    } else if app.tdlib_connected {
        "登录 Telegram"
    } else if app.status.starts_with("正在") {
        "连接中"
    } else {
        "连接失败"
    };
    let ready = app.auth.state == "authorizationStateReady" && app.auth.setup.is_none();
    let back = !ready || (area.width < 78 && app.focus_messages && app.store.active_chat.is_some());
    let tools_width = if !ready {
        if area.width < 60 { 18 } else { 23 }
    } else if area.width < 60 {
        12
    } else {
        16
    };
    let brand_x = if ready && back { area.x + 6 } else { area.x };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " Teleaf ",
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if area.width < 60 {
                    String::new()
                } else {
                    format!("· {connection}")
                },
                Style::default().fg(palette().muted),
            ),
        ])),
        Rect::new(
            brand_x,
            layout[0].y,
            area.width.saturating_sub(tools_width + brand_x - area.x),
            1,
        ),
    );
    if ready && back {
        button(
            frame,
            app,
            Rect::new(area.x, layout[0].y, 6, 1),
            "[返回]",
            Action::Back,
        );
    }
    let tools = Rect::new(
        area.right().saturating_sub(tools_width),
        layout[0].y,
        tools_width,
        1,
    );
    let cols = Layout::horizontal([
        Constraint::Length(if ready {
            0
        } else if area.width < 60 {
            6
        } else {
            7
        }),
        Constraint::Length(if area.width < 60 { 6 } else { 8 }),
        Constraint::Min(1),
    ])
    .split(tools);
    if !ready {
        button(frame, app, cols[0], "[返回]", Action::Back);
    }
    button(frame, app, cols[1], "[设置]", Action::Settings);
    button(frame, app, cols[2], "[帮助]", Action::Help);
    if app.auth.setup.is_some() || app.auth.state != "authorizationStateReady" {
        auth_page(frame, app, layout[1]);
    } else {
        chat_page(frame, app, layout[1]);
    }
    let hint = if app.show_settings {
        "Tab 选择按钮 · Enter 操作 · ↑↓ 滚动 · F6 鼠标 · Esc 返回"
    } else if app.show_help {
        "↑↓ 滚动 · Esc 返回"
    } else if app.auth.is_new_login() {
        "Enter 确认重新登录 · Esc 返回 · Ctrl+C 退出"
    } else if app.auth.is_recovering() {
        "填入旧本地密钥 · Enter 恢复 · F5 重新登录 · Esc 修改 API"
    } else if app.auth.setup.is_some() {
        "Tab 切换字段 · Enter 保存 · F2 官网 · F1 注册帮助 · Ctrl+C 退出"
    } else if app.auth.state != "authorizationStateReady" {
        "Enter 继续 · F3 修改 API · F4 设置 · Ctrl+C 退出"
    } else if app.attachments.is_some() {
        "Space 选择 · Tab 切换 · F8 / Ctrl+Enter 发送 · Esc 返回"
    } else if app.sticker_picker {
        "方向键选择 · 点击/Enter 发送 · [ ] 切换贴纸包 · / 搜索"
    } else if app.action_menu.is_some() || app.forward_message.is_some() {
        "↑ ↓ 选择 · Enter 确认 · Esc 返回"
    } else if app.show_help || app.show_settings {
        "↑ ↓ 滚动 · Esc 返回"
    } else if app.confirm_delete.is_some() {
        "Enter / y 删除 · Esc / n 取消"
    } else if app.preview_message.is_some() {
        "滚轮缩放 · 拖动平移 · 0 复位 · Esc 返回"
    } else if app.input_mode != InputMode::Off {
        "Enter 提交 · Alt+Enter 换行 · Esc 取消"
    } else if app.focus_messages {
        "i 写消息 · Enter 操作/预览 · Space 更多 · Tab 会话 · ? 帮助"
    } else {
        "↑ ↓ 选择 · Enter 打开 · Tab 消息 · s 设置 · ? 帮助"
    };
    let notice = app.notice.as_deref().or(app.media.last_error.as_deref());
    let hint = if area.width < 60 {
        if app.show_settings {
            "Tab 选择 · Enter 操作 · Esc 返回"
        } else if app.show_help {
            "↑↓ 滚动 · Esc 返回"
        } else if app.attachments.is_some() {
            "Space 选择 · Tab 切换 · F8 / Ctrl+Enter 发送 · Esc 返回"
        } else if app.sticker_picker {
            "方向键选择 · 点击/Enter 发送 · [ ] 切换贴纸包 · / 搜索"
        } else if app.action_menu.is_some() || app.forward_message.is_some() {
            "↑↓选择 Enter 确认 Esc 返回"
        } else if app.confirm_delete.is_some() {
            "Enter/y 删除 · Esc/n 取消"
        } else if app.preview_message.is_some() {
            "滚轮缩放 · 拖动平移 · Esc 返回"
        } else if app.auth.is_new_login() {
            "Enter 确认 · Esc 返回"
        } else if app.auth.is_recovering() {
            "Enter 恢复 · F5 重新登录"
        } else if app.auth.setup.is_some() {
            "Tab 切换 · Enter 继续 · F2"
        } else if app.auth.state != "authorizationStateReady" {
            "Enter 继续 · F4 设置 · ^C 退出"
        } else if app.input_mode != InputMode::Off {
            "Enter 提交 · Esc 取消"
        } else if app.focus_messages {
            "i 输入 · Enter 操作 · Tab 会话"
        } else {
            "↑↓ 选择 · Enter 打开 · ? 帮助"
        }
    } else {
        hint
    };
    let footer = if app.auth.is_error && app.auth.state == "authorizationStateReady" {
        app.auth.message.as_str()
    } else {
        notice.unwrap_or(hint)
    };
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(
            if app.auth.is_error || (app.notice.is_none() && app.media.last_error.is_some()) {
                palette().error
            } else if notice.is_some() {
                palette().text
            } else {
                palette().muted
            },
        )),
        layout[2],
    );

    if app.show_help
        || app.show_settings
        || app.show_folders
        || app.action_menu.is_some()
        || app.forward_message.is_some()
        || app.sticker_picker
        || app.attachments.is_some()
        || app.confirm_delete.is_some()
        || app.preview_message.is_some()
    {
        app.hit_targets.clear();
        app.scrollbars.clear();
        app.hit_targets.push((area, Target::Backdrop));
        frame.buffer_mut().set_style(
            layout[1],
            Style::default()
                .fg(palette().muted)
                .add_modifier(Modifier::DIM),
        );
    }
    if let Some(id) = app.preview_message {
        let inner = modal(
            frame,
            app,
            centered(
                area,
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            ),
            &format!(
                "媒体预览 · {:.1}×",
                f32::from(app.preview_view.zoom) / 100.0
            ),
        );
        let media = app
            .active_messages()
            .iter()
            .find(|message| message.id == id)
            .and_then(|message| message.media.clone());
        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        let zoomable = media
            .as_ref()
            .is_some_and(|m| matches!(m.kind, MediaKind::Photo | MediaKind::Sticker));
        media_view(frame, app, rows[0], media);
        if zoomable {
            let tools = Layout::horizontal([
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ])
            .split(rows[1]);
            for (rect, label, action) in [
                (tools[0], "[−]", Action::ZoomOut),
                (tools[1], "[+]", Action::ZoomIn),
                (tools[2], "[复位]", Action::ZoomReset),
                (tools[3], "[打开]", Action::OpenExternal),
            ] {
                button(frame, app, rect, label, action);
            }
        } else {
            button(frame, app, rows[1], "[系统打开]", Action::OpenExternal);
        }
    }
    if app.show_folders {
        let inner = modal(frame, app, centered(layout[1], 44, 20), "Telegram 分组");
        app.modal_scroll = app.modal_scroll.min(
            app.store
                .chat_lists()
                .len()
                .saturating_sub(usize::from(inner.height)) as u16,
        );
        for (row, (list, name)) in app
            .store
            .chat_lists()
            .into_iter()
            .skip(usize::from(app.modal_scroll))
            .take(usize::from(inner.height))
            .enumerate()
        {
            let label = format!(
                "{} {name}",
                if list == app.store.selected_list {
                    "●"
                } else {
                    "○"
                }
            );
            target_button(
                frame,
                app,
                Rect::new(inner.x, inner.y + row as u16, inner.width, 1),
                &label,
                Target::Folder(list),
            );
        }
    }
    if app.show_help {
        help_dialog(frame, app, layout[1]);
    }
    if app.show_settings {
        settings_dialog(frame, app, layout[1]);
    }
    if let Some(index) = app.action_menu {
        action_dialog(frame, app, layout[1], index);
    }
    if app.forward_message.is_some() {
        let area = centered(layout[1], 60, 22);
        let inner = modal(frame, app, area, "转发到…");
        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        chat_list(frame, app, rows[0], true, false);
        button(frame, app, rows[1], "[确认转发]", Action::Confirm);
    }
    if app.attachments.is_some() {
        attachment_dialog(frame, app, layout[1]);
    }
    if app.sticker_picker {
        sticker_dialog(frame, app, layout[1]);
        let column = chat_column(layout[1]);
        let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(column);
        frame.render_widget(Clear, rows[1]);
        composer(frame, app, rows[1]);
    }
    if app.confirm_delete.is_some() {
        let inner = modal(frame, app, centered(layout[1], 56, 9), "删除消息");
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from("确认删除这条消息？"),
                muted("将请求为双方删除，删除后无法撤销。"),
                Line::from(""),
                Line::from(""),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
            inner,
        );
        let row = Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 1);
        let cols =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(row);
        button(frame, app, cols[0], "[删除]", Action::Confirm);
        button(frame, app, cols[1], "[取消]", Action::Cancel);
    }
    app.media.end_frame();
}

fn recovery_page(frame: &mut Frame, app: &mut App, area: Rect) {
    let fresh = app.auth.is_new_login();
    let compact = area.width < 50 || area.height < 18;
    let inner = dialog(frame, centered(area, 72, 18), app.auth.title());
    let rows = Layout::vertical([
        Constraint::Length(if compact { 2 } else { 5 }),
        Constraint::Length(if fresh { 0 } else { 3 }),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(inner);
    let detail = match (fresh, compact) {
        (true, true) => "旧数据保留。重新验证手机号后同步云端聊天。",
        (true, false) => {
            "不知道旧密钥也可以继续。
将创建独立的登录目录，旧数据库和文件保留在原处。
API ID 和 Hash 沿用刚才的填写，无需重新领取。
接下来用手机号、验证码登录，云端聊天会重新同步。"
        }
        (false, true) => {
            "旧本地密钥：TG_DB_KEY
不知道密钥可重新登录。"
        }
        (false, false) => {
            "本机有旧登录数据，但没有保存对应的本地密钥。
这里填写旧版本 TG_DB_KEY 的值。
它不是 API Hash，也不是 Telegram 两步验证密码。
不知道密钥？点击下方“重新登录”即可保留旧数据继续。"
        }
    };
    frame.render_widget(Paragraph::new(detail).wrap(Wrap { trim: true }), rows[0]);
    if !fresh {
        auth_field(frame, app, rows[1], 2, "旧本地密钥（TG_DB_KEY）");
    }
    frame.render_widget(
        Paragraph::new(app.auth.message.clone())
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette().error)),
        rows[2],
    );
    let buttons = if compact {
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(rows[3])
    } else {
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(rows[3])
    };
    button(
        frame,
        app,
        buttons[0],
        if fresh {
            "[返回]"
        } else {
            "[不知道密钥，重新登录]"
        },
        if fresh {
            Action::Back
        } else {
            Action::NewLogin
        },
    );
    button(
        frame,
        app,
        buttons[1],
        if fresh {
            "[确认重新登录]"
        } else {
            "[恢复登录]"
        },
        Action::Confirm,
    );
}

fn auth_page(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.auth.is_recovering() {
        recovery_page(frame, app, area);
        return;
    }
    let compact = area.height < 22 || area.width < 50;
    let setup = app.auth.setup.is_some();
    let inner = dialog(
        frame,
        centered(
            area,
            66,
            if compact {
                16
            } else if setup {
                22
            } else {
                17
            },
        ),
        app.auth.title(),
    );
    let rows = Layout::vertical([
        Constraint::Length(if compact {
            2
        } else if setup {
            5
        } else {
            3
        }),
        Constraint::Length(3),
        Constraint::Length(if setup && !compact { 3 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let detail = if setup {
        if compact {
            "填写 API ID / Hash；Tab 切换
官网无法注册：F1 查看帮助"
        } else {
            "此安装包尚未提供项目 API 凭据，请填写一次。
1  打开官网，登录后进入 API development tools
2  创建应用，Platform 选择 Desktop
3  粘贴 API ID 和 API Hash，只需填写一次
官网无法注册：按 F1 查看帮助。"
        }
    } else {
        &app.auth.detail
    };
    frame.render_widget(
        Paragraph::new(detail.to_owned()).wrap(Wrap { trim: true }),
        rows[0],
    );
    if setup {
        let focused = app.auth.setup.as_ref().expect("form").focused;
        auth_field(
            frame,
            app,
            rows[1],
            if compact { focused } else { 0 },
            if compact && focused == 1 {
                "API Hash · 2/2"
            } else {
                "API ID · 1/2"
            },
        );
        if !compact {
            auth_field(frame, app, rows[2], 1, "API Hash · 2/2");
        }
    } else if let Some(label) = app.auth.input_label() {
        auth_field(frame, app, rows[1], 0, label);
    } else if let Some(link) = &app.auth.confirmation_link {
        frame.render_widget(
            Paragraph::new(link.to_owned())
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(palette().accent)),
            rows[1],
        );
        app.hit_targets
            .push((rows[1], Target::Command(Action::OpenLink)));
    }
    let message = if !app.tdlib_connected && !app.status.starts_with("正在") && !setup {
        &app.status
    } else {
        &app.auth.message
    };
    frame.render_widget(
        Paragraph::new(message.to_owned())
            .style(Style::default().fg(if app.auth.is_error {
                palette().error
            } else {
                palette().accent
            }))
            .wrap(Wrap { trim: true }),
        rows[3],
    );
    let cols = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(34),
        Constraint::Percentage(33),
    ])
    .split(rows[4]);
    button(frame, app, cols[0], "[官网]", Action::OpenLink);
    button(
        frame,
        app,
        cols[1],
        if setup { "[切换]" } else { "[API]" },
        if setup {
            Action::NextField
        } else {
            Action::ApiSetup
        },
    );
    if setup || app.auth.has_input() {
        button(frame, app, cols[2], "[继续]", Action::Confirm);
    }
}

fn chat_column(area: Rect) -> Rect {
    if area.width < 78 {
        return area;
    }
    Layout::horizontal([
        Constraint::Length((area.width / 3).clamp(26, 36)),
        Constraint::Min(1),
    ])
    .split(area)[1]
}

fn chat_page(frame: &mut Frame, app: &mut App, area: Rect) {
    let wide = area.width >= 78;
    let show_chat = wide || !app.focus_messages || app.store.active_chat.is_none();
    let columns = if wide {
        Layout::horizontal([
            Constraint::Length((area.width / 3).clamp(26, 36)),
            Constraint::Min(1),
        ])
        .split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(100), Constraint::Length(0)]).split(area)
    };
    if show_chat {
        chat_list(frame, app, columns[0], !app.focus_messages, true);
    }
    if wide || !show_chat {
        let message_area = if wide { columns[1] } else { columns[0] };
        let rows =
            Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(message_area);
        let title = app
            .store
            .active_chat
            .map(|id| app.store.chat_title(id))
            .unwrap_or_else(|| "消息".into());
        let title = if app.show_search {
            format!(
                "搜索「{}」· {} 条",
                app.search_query,
                app.store.search_results.len()
            )
        } else {
            title
        };
        let control_width = if rows[0].width >= 26 { 12 } else { 6 };
        let block = panel(
            crate::text::ellipsize(
                &title,
                usize::from(rows[0].width.saturating_sub(control_width + 3)),
            ),
            app.focus_messages && app.input_mode == InputMode::Off,
        );
        let inner = block.inner(rows[0]);
        frame.render_widget(block, rows[0]);
        app.hit_targets
            .push((rows[0], Target::Panel(Pane::Timeline)));

        let controls = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(if control_width >= 12 { 6 } else { 0 }),
            Constraint::Length(6),
        ])
        .split(Rect::new(inner.x, rows[0].y, inner.width, 1));
        for (rect, label, action) in [
            (
                controls[1],
                if app.show_search {
                    "[返回]"
                } else {
                    "[搜索]"
                },
                if app.show_search {
                    Action::ClearSearch
                } else {
                    Action::Search
                },
            ),
            (controls[2], "[更多]", Action::ChatMenu),
        ] {
            button(frame, app, rect, label, action);
        }
        if app.store.active_chat.is_none() {
            empty(frame, inner, "选择一个会话", "在左侧选择后按 Enter 打开");
        } else {
            message_list(frame, app, inner);
        }
        composer(frame, app, rows[1]);
    }
}

fn chat_list(frame: &mut Frame, app: &mut App, area: Rect, focused: bool, bordered: bool) {
    let block = panel(
        format!(
            "{} · {}",
            app.store.list_name(),
            app.store.chat_ids().count()
        ),
        focused,
    );
    let inner = if bordered { block.inner(area) } else { area };
    if bordered {
        frame.render_widget(block, area);
    }
    app.hit_targets.push((area, Target::Panel(Pane::Chats)));
    let inner = if bordered {
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        folder_tabs(frame, app, parts[0]);
        button(frame, app, parts[2], "[更多会话]", Action::MoreChats);
        parts[1]
    } else {
        inner
    };
    let ids: Vec<_> = app.store.chat_ids().collect();
    if ids.is_empty() {
        empty(
            frame,
            inner,
            if app.store.exhausted_lists.contains(&app.store.selected_list) {
                "此分组暂无会话"
            } else {
                "正在加载会话…"
            },
            "点击更多会话加载",
        );
        return;
    }
    let selected = ids
        .iter()
        .position(|id| Some(*id) == app.selected_chat)
        .unwrap_or(0);
    let visible = usize::from(inner.height / 2).max(1);
    app.chat_visible = visible;
    if app.reveal_chat {
        if selected < app.chat_offset {
            app.chat_offset = selected;
        } else if selected >= app.chat_offset + visible {
            app.chat_offset = selected + 1 - visible;
        }
        app.reveal_chat = false;
    }
    app.chat_offset = app.chat_offset.min(ids.len().saturating_sub(visible));
    let start = app.chat_offset;
    let items: Vec<_> = ids
        .iter()
        .skip(start)
        .take(visible)
        .filter_map(|id| app.store.chat(*id))
        .map(|chat| {
            // Reserve two columns for the selection marker and one for scrolling.
            let width = usize::from(inner.width.saturating_sub(3));
            let unread = if chat.unread > 0 {
                if chat.unread > 999 {
                    " 999+ ".to_owned()
                } else {
                    format!(" {} ", chat.unread)
                }
            } else {
                String::new()
            };
            let badge_width = UnicodeWidthStr::width(unread.as_str()).min(width);
            let title = crate::text::ellipsize(
                &format!(
                    "{}{}",
                    if Some(chat.id) == app.store.active_chat {
                        "· "
                    } else {
                        ""
                    },
                    chat.title
                ),
                width.saturating_sub(badge_width + usize::from(badge_width > 0)),
            );
            let padding =
                width.saturating_sub(UnicodeWidthStr::width(title.as_str()) + badge_width);
            let preview = if chat.preview.is_empty() {
                "暂无消息".to_owned()
            } else {
                excerpt(&chat.preview, width)
            };
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(title, Style::default().add_modifier(Modifier::BOLD)),
                    Span::raw(" ".repeat(padding)),
                    Span::styled(unread, palette().primary()),
                ]),
                muted(preview),
            ])
            .style(if ids.get(selected) == Some(&chat.id) {
                palette().selection()
            } else {
                Style::default()
            })
        })
        .collect();
    let mut state = ListState::default().with_selected(
        (selected >= start && selected < start + visible).then_some(selected.saturating_sub(start)),
    );
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            // Keep the gutter when the selected chat is outside the viewport.
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::default()),
        inner,
        &mut state,
    );
    for (row, id) in ids
        .iter()
        .skip(start + state.offset())
        .take(visible)
        .enumerate()
    {
        let y = inner.y + (row * 2) as u16;
        app.hit_targets.push((
            Rect::new(
                inner.x,
                y,
                inner.width,
                2.min(inner.bottom().saturating_sub(y)),
            ),
            Target::Chat(*id),
        ));
    }
    scrollbar(
        frame,
        app,
        inner,
        Pane::Chats,
        start,
        ids.len().saturating_sub(visible),
    );
}

fn message_body(message: &crate::store::Message) -> &str {
    match message.media.as_ref().map(|media| media.kind) {
        Some(MediaKind::Photo) => message
            .text
            .strip_prefix("[图片] ")
            .unwrap_or(&message.text),
        Some(MediaKind::Sticker) if message.text.starts_with("[贴纸]") => "",
        _ => &message.text,
    }
}

fn message_list(frame: &mut Frame, app: &mut App, area: Rect) {
    app.visible_media.clear();
    if app.active_messages().is_empty() {
        empty(
            frame,
            area,
            if app.show_search {
                "没有找到消息"
            } else {
                "这里还没有消息"
            },
            "i 写消息 · g 加载历史",
        );
        app.timeline_rows.clear();
        app.timeline_top = 0;
        app.timeline_max = 0;
        return;
    }
    let width = area.width.saturating_sub(3).max(1);
    if app.timeline_width != width {
        app.timeline_width = width;
        app.timeline_heights.clear();
    }
    if app.timeline_heights.len() > 1024 {
        app.timeline_heights.clear();
    }
    let media_height = (width / 5).clamp(3, 8) as usize;
    let source = if app.show_search {
        &app.store.search_results
    } else {
        &app.store.messages
    };
    app.timeline_rows.clear();
    let mut date_headers = std::collections::HashSet::new();
    let mut previous_date: Option<crate::calendar::Stamp> = None;
    for message in source {
        if let Some(stamp) = message.info.stamp {
            if previous_date.is_none_or(|previous| !previous.same_date(stamp)) {
                date_headers.insert(message.id);
            }
            previous_date = Some(stamp);
        }
        let height = *app.timeline_heights.entry(message.id).or_insert_with(|| {
            let body = message_body(message);
            (if body.is_empty() {
                0
            } else {
                crate::text::row_count(body, usize::from(width))
            }) + 2
                + usize::from(message.info.reply.is_some())
                + if message.media.as_ref().is_some_and(|media| {
                    matches!(media.kind, MediaKind::Photo | MediaKind::Sticker)
                }) {
                    media_height
                } else {
                    0
                }
        });
        app.timeline_rows.push((
            message.id,
            height + usize::from(date_headers.contains(&message.id)),
        ));
    }
    let total: usize = app.timeline_rows.iter().map(|(_, height)| height).sum();
    app.timeline_max = total.saturating_sub(usize::from(area.height));
    let mut top = app.timeline_max;
    if let Some((anchor, offset)) = app.timeline_anchor {
        top = 0;
        let mut before = 0;
        for &(id, height) in &app.timeline_rows {
            if id == anchor {
                top = before + offset.min(height - 1);
                break;
            } else if !app.show_search && id > anchor {
                top = before;
                break;
            }
            before += height;
        }
    }
    if app.reveal_message {
        let mut before = 0;
        for &(id, height) in &app.timeline_rows {
            if Some(id) == app.selected_message {
                if before < top {
                    top = before;
                } else if before >= top + usize::from(area.height) {
                    top = (before + height.min(usize::from(area.height)))
                        .saturating_sub(usize::from(area.height));
                }
                break;
            }
            before += height;
        }
        app.reveal_message = false;
    }
    top = top.min(app.timeline_max);
    app.timeline_top = top;
    anchor_at(app, top);
    let source = app.active_messages();
    let mut before = 0;
    let mut visible = Vec::new();
    let mut text_targets = Vec::new();
    for (message, &(_, height)) in source.iter().zip(&app.timeline_rows) {
        if before + height > top && before < top + usize::from(area.height) {
            let label = app.store.sender_label(message);
            let tag = message
                .media
                .as_ref()
                .map(|media| match media.kind {
                    MediaKind::Photo => " · 图片",
                    MediaKind::Sticker => " · 贴纸",
                    MediaKind::Document => " · 文件",
                    MediaKind::Video => " · 视频",
                })
                .unwrap_or("");
            let divider = usize::from(date_headers.contains(&message.id));
            let status = message.status(
                app.store
                    .active_chat
                    .and_then(|id| app.store.chat(id))
                    .map_or(0, |chat| chat.read_outbox),
            );
            let status = crate::text::ellipsize(&status, usize::from(width.saturating_sub(16)));
            let status_width =
                UnicodeWidthStr::width(status.as_str()) + usize::from(!status.is_empty()) * 3;
            let controls_width = if Some(message.id) == app.selected_message {
                if width >= 32 || message.retryable() {
                    12
                } else {
                    6
                }
            } else if message.retryable() {
                6
            } else {
                0
            };
            let mut lines = Vec::new();
            if divider > 0 {
                lines.push(
                    Line::styled(
                        message.info.stamp.expect("date header").date(),
                        Style::default().fg(palette().muted),
                    )
                    .alignment(Alignment::Center),
                );
            }
            lines.push(Line::from(vec![
                Span::styled(
                    format!(
                        "{}{}",
                        if Some(message.id) == app.selected_message {
                            "› "
                        } else {
                            "  "
                        },
                        crate::text::ellipsize(
                            &format!("{label}{tag}"),
                            usize::from(width.saturating_sub(controls_width))
                                .saturating_sub(status_width)
                        )
                    ),
                    Style::default()
                        .fg(if message.outgoing {
                            palette().accent
                        } else {
                            palette().incoming
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if status.is_empty() {
                        String::new()
                    } else {
                        format!(" · {status}")
                    },
                    Style::default().fg(if message.retryable() {
                        palette().error
                    } else {
                        palette().muted
                    }),
                ),
            ]));
            if let Some(reply) = &message.info.reply {
                let referenced = (Some(reply.chat_id) == app.store.active_chat)
                    .then(|| app.store.messages.iter().find(|m| m.id == reply.message_id))
                    .flatten();
                let excerpt = if !reply.excerpt.is_empty() {
                    reply.excerpt.as_str()
                } else {
                    referenced.map_or("原消息未加载", |m| m.text.as_str())
                };
                let label = referenced
                    .map(|m| app.store.sender_label(m))
                    .unwrap_or_else(|| "引用".into());
                lines.push(Line::styled(
                    format!(
                        "  ↪ {}",
                        crate::text::ellipsize(
                            &format!("{label}：{}", excerpt.replace(['\n', '\r'], " ")),
                            usize::from(width)
                        )
                    ),
                    Style::default().fg(palette().muted),
                ));
                let quote_row = before + divider + 1;
                if referenced.is_some()
                    && !app.show_search
                    && quote_row >= top
                    && quote_row < top + usize::from(area.height)
                {
                    text_targets.push((
                        Rect::new(area.x + 2, area.y + (quote_row - top) as u16, width, 1),
                        Target::ReferencedMessage(reply.message_id),
                    ));
                }
            }
            let prefix = divider + 1 + usize::from(message.info.reply.is_some());
            let body = message_body(message);
            let body_offset = message.text.len() - body.len();
            let body_rows = if body.is_empty() {
                vec![]
            } else {
                crate::text::rows(body, usize::from(width))
                    .into_iter()
                    .map(|row| crate::text::Row {
                        start: row.start + body_offset,
                        end: row.end + body_offset,
                    })
                    .collect()
            };
            let source = Source::Message(message.id);
            let selected_range = selection::range(app, source);
            lines.extend(
                body_rows
                    .iter()
                    .map(|row| selected_line(&message.text, *row, selected_range.clone(), "  ")),
            );
            let inline = message
                .media
                .as_ref()
                .filter(|media| matches!(media.kind, MediaKind::Photo | MediaKind::Sticker));
            let media_start = before + prefix + body_rows.len();
            if inline.is_some() {
                lines.extend((0..media_height).map(|_| Line::from("")));
            }
            lines.push(Line::from(""));
            let skip = top.saturating_sub(before);
            let y = area.y + before.saturating_sub(top) as u16;
            let h = (height - skip).min(usize::from(area.bottom() - y)) as u16;
            for (index, row) in body_rows.iter().enumerate() {
                let absolute_row = before + index + prefix;
                if absolute_row >= top && absolute_row < top + usize::from(area.height) {
                    let row_area = Rect::new(
                        area.x + 2,
                        area.y + (absolute_row - top) as u16,
                        area.width.saturating_sub(3),
                        1,
                    );
                    text_targets.extend(point_targets(&message.text, *row, source, row_area));
                }
            }
            visible.push((
                message.id,
                Rect::new(area.x, y, area.width.saturating_sub(1), h),
                lines
                    .into_iter()
                    .skip(skip)
                    .take(usize::from(h))
                    .collect::<Vec<_>>(),
                inline.and_then(|media| {
                    let start = media_start.max(top);
                    let end = (media_start + media_height).min(top + usize::from(area.height));
                    (end > start).then(|| {
                        (
                            media.clone(),
                            Rect::new(
                                area.x + 2,
                                area.y + (start - top) as u16,
                                width.min(50),
                                (end - start) as u16,
                            ),
                            (start - media_start) as u16,
                        )
                    })
                }),
                (before + divider >= top && before + divider < top + usize::from(area.height))
                    .then(|| area.y + (before + divider - top) as u16),
                message.retryable(),
            ));
        }
        before += height;
    }
    for (id, rect, lines, inline, header, retryable) in visible {
        let style = if Some(id) == app.selected_message {
            palette().selection()
        } else {
            Style::default()
        };
        frame.render_widget(Paragraph::new(lines).style(style), rect);
        app.hit_targets.push((rect, Target::Message(id)));
        if let Some((media, area, skip)) = inline {
            if media.path.is_none() && !app.visible_media.contains(&media.file_id) {
                app.visible_media.push(media.file_id);
            }
            inline_media_view(frame, app, area, &media, media_height as u16, skip);
            app.hit_targets.push((area, Target::Media(id)));
        }
        if let Some(y) = header
            && (Some(id) == app.selected_message || retryable)
        {
            let menu = Rect::new(rect.right().saturating_sub(5), y, 5.min(rect.width), 1);
            if Some(id) == app.selected_message {
                target_button(
                    frame,
                    app,
                    menu,
                    "[⋯]",
                    Target::MessageAction(id, Action::Menu),
                );
            }
            if rect.width >= 32 || retryable {
                target_button(
                    frame,
                    app,
                    Rect::new(
                        if Some(id) == app.selected_message {
                            menu.x.saturating_sub(6)
                        } else {
                            rect.right().saturating_sub(6)
                        },
                        y,
                        6,
                        1,
                    ),
                    if retryable { "[重试]" } else { "[回复]" },
                    Target::MessageAction(
                        id,
                        if retryable {
                            Action::Retry
                        } else {
                            Action::Reply
                        },
                    ),
                );
            }
        }
    }
    app.hit_targets.extend(text_targets);
    scrollbar(frame, app, area, Pane::Timeline, top, app.timeline_max);
    if app.quote_back.is_some() && area.height > 0 {
        button(
            frame,
            app,
            Rect::new(area.x, area.bottom() - 1, area.width.saturating_sub(1), 1),
            "[返回引用前位置 · b]",
            Action::QuoteBack,
        );
    } else if top < app.timeline_max && area.height > 0 {
        let button_area = Rect::new(area.x, area.bottom() - 1, area.width.saturating_sub(1), 1);
        button(
            frame,
            app,
            button_area,
            &if app.pending_messages > 0 {
                format!("[↓ {} 条新消息 · 回到底部]", app.pending_messages)
            } else {
                "[↓ 回到底部]".into()
            },
            Action::Bottom,
        );
    }
}

fn composer(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.input_mode {
        InputMode::Off | InputMode::Send => "写消息",
        InputMode::Reply(_) => "回复消息",
        InputMode::Edit(_) => "编辑消息",
        InputMode::Search => "搜索",
        InputMode::React(_) => "表情回应",
    };
    let title = if let InputMode::Reply(id) | InputMode::Edit(id) = app.input_mode {
        app.store
            .messages
            .iter()
            .find(|message| message.id == id)
            .map(|message| {
                crate::text::ellipsize(
                    &format!(
                        "{title} · {}：{}",
                        app.store.sender_label(message),
                        excerpt(&message.text, usize::from(area.width.saturating_sub(8)))
                    ),
                    usize::from(area.width.saturating_sub(8)),
                )
            })
            .unwrap_or_else(|| title.to_owned())
    } else {
        title.to_owned()
    };
    let block = panel(
        title,
        app.composer_focus && app.input_mode != InputMode::Off,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hit_targets.push((area, Target::Composer));
    if app.input_mode != InputMode::Off {
        button(
            frame,
            app,
            Rect::new(area.right().saturating_sub(5), area.y, 3, 1),
            "×",
            Action::Cancel,
        );
    }
    let tools = Rect::new(
        area.x + 1,
        area.bottom() - 1,
        area.width.saturating_sub(2),
        1,
    );
    let cols = Layout::horizontal([
        Constraint::Percentage(33),
        Constraint::Percentage(33),
        Constraint::Percentage(34),
    ])
    .split(tools);
    for (rect, label, action) in [
        (cols[0], "[附件]", Action::File),
        (cols[1], "[贴纸]", Action::Stickers),
        (
            cols[2],
            if matches!(app.input_mode, InputMode::Search) {
                "[查找]"
            } else if matches!(app.input_mode, InputMode::Edit(_)) {
                "[保存]"
            } else {
                "[发送]"
            },
            Action::Submit,
        ),
    ] {
        button(frame, app, rect, label, action);
    }
    let value = app.draft.clone();
    editor(
        frame,
        app,
        inner,
        Editor::Composer,
        &value,
        app.draft_cursor,
        false,
    );
    if value.is_empty() && app.input_mode == InputMode::Off {
        frame.render_widget(
            Paragraph::new("点击写消息").style(Style::default().fg(palette().muted)),
            inner,
        );
    }
}

fn inline_media_view(
    frame: &mut Frame,
    app: &mut App,
    area: Rect,
    media: &MediaRef,
    full_height: u16,
    skip: u16,
) {
    // The fullscreen preview covers the timeline. Do not prepare hidden images.
    if app.preview_message.is_some() || app.sticker_picker || app.attachments.is_some() {
        return;
    }
    let Some(path) = &media.path else {
        let label = if media.kind == MediaKind::Sticker {
            "[贴纸]"
        } else {
            "[图片]"
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{label} {}",
                if app.failed_files.contains(&media.file_id) {
                    "下载失败 · 点击重试"
                } else if app.requested_files.contains(&media.file_id) {
                    "↓ 正在下载 · 点击查看"
                } else {
                    "↓ 加载图片 · 点击查看"
                }
            ))
            .style(Style::default().fg(palette().muted))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    };
    let size = Size::new(area.width, full_height);
    app.media.request_inline(media.file_id, path, size);
    if let Some(image) = app.media.get_inline(media.file_id, size) {
        frame.render_widget(
            SlicedImage::new(image, SignedPosition::from((0, -(skip as i16)))),
            area,
        );
        finish_sixel(frame, area, &app.media.protocol_name);
    } else {
        let label = if media.kind == MediaKind::Sticker {
            "[贴纸]"
        } else {
            "[图片]"
        };
        frame.render_widget(
            Paragraph::new(format!(
                "{label} {}",
                if app.media.inline_failed(media.file_id, size) {
                    "无法预览 · 按 o 系统打开"
                } else {
                    "准备中 · 点击查看"
                }
            ))
            .style(Style::default().fg(palette().muted)),
            area,
        );
    }
}

fn finish_sixel(frame: &mut Frame, area: Rect, protocol: &str) {
    if protocol != "Sixel" || area.is_empty() {
        return;
    }
    let next_column = area.x.saturating_add(2).min(frame.area().right());
    if let Some(cell) = frame.buffer_mut().cell_mut((area.x, area.y))
        && cell.symbol().contains("\x1bP")
    {
        // Sixel can leave the physical cursor on another row. Ratatui treats
        // this payload as one cell; adjacent text must start at that next cell.
        let mut payload = cell.symbol().to_owned();
        use std::fmt::Write;
        let _ = write!(payload, "\x1b[{};{next_column}H", area.y + 1);
        cell.set_symbol(&payload);
    }
}

fn media_view(frame: &mut Frame, app: &mut App, area: Rect, media: Option<MediaRef>) {
    let Some(mut media) = media else {
        empty(frame, area, "没有可预览的媒体", "Esc 返回");
        return;
    };
    let thumbnail_id = media.file_id;
    let thumbnail_path = media.path.clone();
    let preview = app.preview_message.is_some();
    if preview
        && let Some(detail) = &media.detail
        && detail.path.is_some()
    {
        media.file_id = detail.file_id;
        media.path = detail.path.clone();
    }
    let Some(path) = media.path else {
        empty(frame, area, "正在下载…", "下载完成后自动显示");
        return;
    };
    if !matches!(media.kind, MediaKind::Photo | MediaKind::Sticker) {
        empty(frame, area, "文件已下载", "按 o 用系统程序打开");
        return;
    }
    let viewport = centered(area, 160, 60);
    let size = Size::new(viewport.width, viewport.height);
    let view = if preview {
        app.preview_view
    } else {
        crate::media::View::default()
    };
    if preview {
        app.media.request_view(media.file_id, &path, size, view);
        // Keep the thumbnail on screen until the detailed version is encoded.
        if media.file_id != thumbnail_id
            && app.media.get_view(media.file_id, size, view).is_none()
            && let Some(path) = &thumbnail_path
        {
            app.media.request_view(thumbnail_id, path, size, view);
        }
    } else {
        app.media.request(media.file_id, &path, size);
    }
    let image = if preview {
        let ready = app.media.get_view(media.file_id, size, view).is_some();
        app.media
            .get_view(if ready { media.file_id } else { thumbnail_id }, size, view)
    } else {
        app.media.get(media.file_id, size)
    };
    if let Some(image) = image {
        let image_area = centered(viewport, image.size().width, image.size().height);
        frame.render_widget(Image::new(image).allow_clipping(true), image_area);
        finish_sixel(frame, image_area, &app.media.protocol_name);
        if preview {
            app.hit_targets
                .push((image_area, Target::Image(image_area)));
        }
    } else if let Some(error) = &app.media.last_error {
        if preview {
            app.hit_targets.push((viewport, Target::Image(viewport)));
        }
        empty(frame, area, "无法预览图片", error);
    } else {
        if preview {
            app.hit_targets.push((viewport, Target::Image(viewport)));
        }
        empty(
            frame,
            area,
            "正在准备图片…",
            "不支持图片的终端会自动使用字符预览",
        );
    }
}

fn action_dialog(frame: &mut Frame, app: &mut App, area: Rect, selected: usize) {
    let title = match app.menu_scope {
        crate::menu::Scope::Chat => "会话操作",
        crate::menu::Scope::Message { .. } => "消息操作",
        crate::menu::Scope::Selection => "选中文字",
    };
    let entries: Vec<_> = crate::menu::items(app).collect();
    let selected = selected.min(entries.len().saturating_sub(1));
    app.action_menu = Some(selected);
    let height = (entries.len() as u16 + 3).max(5);
    let inner = modal(frame, app, centered(area, 40, height), title);
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let items: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(index, item)| {
            ListItem::new(Line::from(vec![
                Span::raw(item.label),
                Span::styled(
                    if item.key == '\0' {
                        String::new()
                    } else {
                        format!("  {}", item.key)
                    },
                    Style::default().fg(palette().muted),
                ),
            ]))
            .style(if index == selected {
                palette().selection()
            } else {
                Style::default()
            })
        })
        .collect();
    let mut state = ListState::default().with_selected((!entries.is_empty()).then_some(selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("› ")
            .highlight_style(Style::default()),
        rows[0],
        &mut state,
    );
    for (row, item) in entries
        .iter()
        .skip(state.offset())
        .take(usize::from(rows[0].height))
        .enumerate()
    {
        app.hit_targets.push((
            Rect::new(rows[0].x, rows[0].y + row as u16, rows[0].width, 1),
            Target::Action(item.action),
        ));
    }
    let checking = matches!(app.menu_scope, crate::menu::Scope::Message { .. })
        && crate::menu::permissions(app).is_none();
    frame.render_widget(
        Paragraph::new(if checking {
            "正在读取消息权限…"
        } else {
            "Enter 确认 · Esc 关闭"
        })
        .style(Style::default().fg(palette().muted)),
        rows[1],
    );
}

fn sticker_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    // Telegram-like tray anchored above the composer, with tabs and a thumbnail grid.
    let column = chat_column(area);
    let composer_y = column.bottom().saturating_sub(4);
    let space = composer_y.saturating_sub(column.y);
    let context = if space >= 10 { 3 } else { 0 };
    let height = space.saturating_sub(context).min(14);
    let width = column.width.min(68);
    let tray = Rect::new(
        column.right().saturating_sub(width),
        composer_y.saturating_sub(height),
        width,
        height,
    );
    let inner = modal(frame, app, tray, "贴纸 · 点击发送");
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(if inner.height >= 9 { 1 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(if inner.height >= 6 { 1 } else { 0 }),
    ])
    .split(inner);
    let tabs = app.sticker_tabs();
    let selected = tabs
        .iter()
        .position(|(tab, _)| *tab == app.sticker_panel.tab)
        .unwrap_or(0);
    let available = parts[0].width.saturating_sub(6);
    let tab_width = 10u16.min(available.max(1));
    let capacity = usize::from(available / tab_width).max(1);
    let start = selected.saturating_sub(capacity - 1);
    button(
        frame,
        app,
        Rect::new(parts[0].x, parts[0].y, 3, 1),
        "‹",
        Action::StickerPrevious,
    );
    for (i, (tab, label)) in tabs.into_iter().skip(start).take(capacity).enumerate() {
        let rect = Rect::new(
            parts[0].x + 3 + i as u16 * tab_width,
            parts[0].y,
            tab_width,
            1,
        )
        .intersection(parts[0]);
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                &label,
                usize::from(tab_width.saturating_sub(1)),
            ))
            .style(if tab == app.sticker_panel.tab {
                palette().primary()
            } else {
                Style::default().fg(palette().muted)
            }),
            rect,
        );
        app.hit_targets.push((rect, Target::StickerTab(tab)));
    }
    button(
        frame,
        app,
        Rect::new(parts[0].right().saturating_sub(3), parts[0].y, 3, 1),
        "›",
        Action::StickerNext,
    );
    let title = match app.sticker_panel.tab {
        crate::stickers::Tab::Recent => "最近使用".into(),
        crate::stickers::Tab::Favorites => "收藏的贴纸".into(),
        crate::stickers::Tab::Set(id) => app
            .store
            .installed_sticker_sets
            .iter()
            .find(|(key, _)| *key == id)
            .map(|(_, name)| name.clone())
            .unwrap_or("贴纸包".into()),
        crate::stickers::Tab::Search => format!("搜索：{}", app.sticker_panel.submitted),
    };
    frame.render_widget(
        Paragraph::new(title).style(Style::default().fg(palette().muted)),
        parts[1],
    );
    let grid = parts[2];
    let columns = usize::from((grid.width / 12).max(1)).min(6);
    let tile_height = if grid.height >= 8 {
        (grid.height / 2).clamp(3, 5)
    } else {
        grid.height.clamp(1, 5)
    };
    app.sticker_panel.preview_visible = tile_height >= 3;
    let rows = usize::from((grid.height / tile_height).max(1));
    app.sticker_panel.columns = columns;
    app.sticker_panel.rows = rows;
    let total = app.sticker_items().len();
    app.sticker_cursor = app.sticker_cursor.min(total.saturating_sub(1));
    let selected_row = app.sticker_cursor / columns;
    if selected_row < app.sticker_panel.top {
        app.sticker_panel.top = selected_row;
    }
    if selected_row >= app.sticker_panel.top + rows {
        app.sticker_panel.top = selected_row + 1 - rows;
    }
    if total == 0 {
        let loading = app.sticker_panel.loading
            || matches!(app.sticker_panel.tab,crate::stickers::Tab::Set(id) if app.sticker_panel.requested_sets.contains(&id));
        empty(
            frame,
            grid,
            if loading {
                "正在加载贴纸…"
            } else {
                "这里还没有贴纸"
            },
            "可切换贴纸包，或在下面搜索",
        );
    }
    let start = app.sticker_panel.top * columns;
    let stickers: Vec<_> = app
        .sticker_items()
        .iter()
        .skip(start)
        .take(columns * rows)
        .cloned()
        .collect();
    for (i, sticker) in stickers.into_iter().enumerate() {
        let index = start + i;
        let tile = Rect::new(
            grid.x + (i % columns) as u16 * (grid.width / columns as u16),
            grid.y + (i / columns) as u16 * tile_height,
            grid.width / columns as u16,
            tile_height,
        )
        .intersection(grid);
        if tile.height < 3 {
            frame.render_widget(
                Paragraph::new(sticker.emoji).alignment(Alignment::Center),
                tile,
            );
            app.hit_targets.push((tile, Target::Sticker(index)));
            continue;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if index == app.sticker_cursor {
                palette().accent
            } else {
                palette().muted
            }));
        let image_area = block.inner(tile);
        frame.render_widget(block, tile);
        if let Some(path) = &sticker.preview.path {
            app.media.request(
                sticker.preview.file_id,
                path,
                Size::new(image_area.width, image_area.height),
            );
            if let Some(image) = app.media.get(
                sticker.preview.file_id,
                Size::new(image_area.width, image_area.height),
            ) {
                let image_area = centered(image_area, image.size().width, image.size().height);
                frame.render_widget(Image::new(image).allow_clipping(true), image_area);
                finish_sixel(frame, image_area, &app.media.protocol_name);
            } else {
                frame.render_widget(
                    Paragraph::new(sticker.emoji).alignment(Alignment::Center),
                    image_area,
                );
            }
        } else {
            frame.render_widget(
                Paragraph::new(sticker.emoji).alignment(Alignment::Center),
                image_area,
            );
        }
        app.hit_targets.push((tile, Target::Sticker(index)));
    }
    let search = Layout::horizontal([Constraint::Min(1), Constraint::Length(8)]).split(parts[3]);
    let value = app.sticker_panel.query.clone();
    editor(
        frame,
        app,
        search[0],
        Editor::StickerSearch,
        &value,
        app.sticker_panel.query_cursor,
        false,
    );
    if value.is_empty() {
        frame.render_widget(
            Paragraph::new("搜索贴纸 / 表情…").style(Style::default().fg(palette().muted)),
            search[0],
        );
    }
    button(frame, app, search[1], "[搜索]", Action::StickerSearch);
    let footer = Layout::horizontal([Constraint::Min(1), Constraint::Length(12)]).split(parts[4]);
    frame.render_widget(
        Paragraph::new(format!("{total} 张 · Enter 发送 · Tab 搜索"))
            .style(Style::default().fg(palette().muted)),
        footer[0],
    );
    let favorite = app
        .sticker_items()
        .get(app.sticker_cursor)
        .is_some_and(|s| {
            app.store
                .favorite_stickers
                .iter()
                .any(|f| f.file_id == s.file_id)
        });
    button(
        frame,
        app,
        footer[1],
        if favorite {
            "[★取消收藏]"
        } else {
            "[☆收藏]"
        },
        Action::StickerFavorite,
    );
}

fn attachment_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 86, 25), "发送附件");
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let tools = Layout::horizontal([
        Constraint::Length(12),
        Constraint::Min(1),
        Constraint::Length(8),
    ])
    .split(parts[0]);
    let picker = app.attachments.as_mut().unwrap();
    let mode = if picker.photos {
        "[图片 F5]"
    } else {
        "[原文件 F5]"
    };
    let directory = crate::text::ellipsize(
        &picker.directory.to_string_lossy(),
        usize::from(parts[1].width),
    );
    let path = picker.path.clone();
    let path_cursor = picker.path_cursor;
    let caption = picker.caption.clone();
    let caption_cursor = picker.caption_cursor;
    let error = picker.error.clone();
    picker.visible = usize::from(parts[3].height).max(1);
    picker.offset = picker
        .offset
        .min(picker.entries.len().saturating_sub(picker.visible));
    button(frame, app, tools[0], mode, Action::AttachmentMode);
    button(frame, app, tools[1], "[粘贴 F7]", Action::PasteClipboard);
    button(frame, app, tools[2], "[上一级]", Action::AttachmentParent);
    frame.render_widget(
        Paragraph::new(directory).style(Style::default().fg(palette().muted)),
        parts[1],
    );
    editor(
        frame,
        app,
        parts[2],
        Editor::AttachmentPath,
        &path,
        path_cursor,
        false,
    );
    if path.is_empty() {
        frame.render_widget(
            Paragraph::new("粘贴/拖入路径，或点击下方文件（Tab 编辑路径）")
                .style(Style::default().fg(palette().muted)),
            parts[2],
        );
    }
    let columns = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(parts[3]);
    let picker = app.attachments.as_ref().unwrap();
    let entries: Vec<_> = picker
        .entries
        .iter()
        .enumerate()
        .skip(picker.offset)
        .take(picker.visible)
        .map(|(i, e)| {
            let parent = i == 0
                && e.directory
                && e.path == picker.directory.parent().unwrap_or(&picker.directory);
            let name = if parent {
                "..".into()
            } else {
                e.path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            };
            let mark = if e.directory {
                "/"
            } else if picker.selected.contains(&e.path) {
                "✓"
            } else {
                " "
            };
            (i, format!("{mark} {name}"), i == picker.cursor)
        })
        .collect();
    for (row, (index, label, selected)) in entries.into_iter().enumerate() {
        let rect = Rect::new(columns[0].x, columns[0].y + row as u16, columns[0].width, 1);
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(&label, usize::from(rect.width))).style(
                if selected {
                    palette().selection().fg(palette().accent)
                } else {
                    Style::default()
                },
            ),
            rect,
        );
        app.hit_targets.push((rect, Target::AttachmentEntry(index)));
    }
    let picker = app.attachments.as_mut().unwrap();
    picker.queue_visible = picker
        .selected
        .len()
        .min(6)
        .min(usize::from(columns[1].height.saturating_sub(1)))
        .max(1);
    picker.queue_offset = picker
        .queue_offset
        .min(picker.selected.len().saturating_sub(picker.queue_visible));
    let queue_offset = picker.queue_offset;
    let queue_visible = picker.queue_visible;
    let preview_path = picker
        .selected
        .get(queue_offset)
        .filter(|p| crate::attachments::photo_paths(&[(*p).clone()]))
        .cloned();
    let selected: Vec<_> = picker
        .selected
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i,
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            )
        })
        .collect();
    frame.render_widget(
        Paragraph::new(format!("已选 {}/10 · 滚动/移除", selected.len()))
            .style(Style::default().fg(palette().accent)),
        Rect::new(columns[1].x, columns[1].y, columns[1].width, 1),
    );
    app.hit_targets.push((columns[1], Target::AttachmentQueue));
    let queue_rows = selected.len().min(queue_visible);
    for (row, (index, name)) in selected
        .into_iter()
        .skip(queue_offset)
        .take(queue_rows)
        .enumerate()
    {
        let rect = Rect::new(
            columns[1].x,
            columns[1].y + 1 + row as u16,
            columns[1].width,
            1,
        );
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                &format!("× {name}"),
                usize::from(rect.width),
            )),
            rect,
        );
        app.hit_targets
            .push((rect, Target::AttachmentRemove(index)));
    }
    if let Some(path) = preview_path {
        let preview = Rect::new(
            columns[1].x,
            columns[1].y + 1 + queue_rows as u16,
            columns[1].width,
            columns[1].height.saturating_sub(1 + queue_rows as u16),
        );
        let size = Size::new(preview.width, preview.height);
        let id = crate::attachments::preview_id(&path);
        app.media.request(id, &path.to_string_lossy(), size);
        if let Some(image) = app.media.get(id, size) {
            let image_area = centered(preview, image.size().width, image.size().height);
            frame.render_widget(Image::new(image).allow_clipping(true), image_area);
            finish_sixel(frame, image_area, &app.media.protocol_name);
        }
    }
    editor(
        frame,
        app,
        parts[4],
        Editor::AttachmentCaption,
        &caption,
        caption_cursor,
        false,
    );
    if caption.is_empty() {
        frame.render_widget(
            Paragraph::new("添加说明（可选，Tab 或点击输入）")
                .style(Style::default().fg(palette().muted)),
            parts[4],
        );
    }
    frame.render_widget(
        Paragraph::new(error.unwrap_or("Enter 选择 · Space 多选 · F5 切换模式 · F8 发送".into()))
            .style(
                Style::default().fg(if app.attachments.as_ref().unwrap().error.is_some() {
                    palette().error
                } else {
                    palette().muted
                }),
            ),
        parts[5],
    );
    let footer = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(parts[6]);
    button(frame, app, footer[0], "[发送附件]", Action::Confirm);
    button(frame, app, footer[1], "[取消]", Action::Cancel);
}

fn help_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 64, 24), "使用帮助");
    let lines = vec![
        Line::styled("浏览会话", Style::default().fg(palette().accent)),
        muted("单击选择 · 右键操作 · 滚轮滚动所在面板"),
        Line::from("↑ ↓ / j k 选择    Enter 打开    Tab 切换会话与消息"),
        Line::from("g 加载更早消息    m 更多会话    PgUp/PgDn 滚动消息"),
        Line::from(""),
        Line::styled("聊天与附件", Style::default().fg(palette().accent)),
        Line::from("i 写消息    Enter 消息操作/媒体预览    Space 更多操作"),
        Line::from("r 回复    e 编辑    f 转发    x 回应    d 删除"),
        Line::from("S 快速收藏到收藏夹    D 复读到当前会话"),
        Line::from("p 图片 / a 文件 / Ctrl+O 附件 · t 贴纸 · / 搜索"),
        Line::from("F7 / Ctrl+V 读取剪贴板图片、文件或文字；F8 确认发送"),
        Line::from("附件：点击多选 / 拖入路径 · Tab 说明 · F8 / Ctrl+Enter 发送"),
        Line::from("贴纸：缩略图网格 · [ ] 切换包 · / 搜索 · 点击发送"),
        Line::from("v 预览媒体    o 用系统程序打开"),
        Line::from(""),
        Line::styled("输入与导航", Style::default().fg(palette().accent)),
        Line::from("支持直接粘贴    Alt+Enter 换行    Ctrl+U 清空输入"),
        Line::from("Enter 提交    Esc 取消当前操作    s 设置    q 退出"),
        Line::from("c 复制消息 · F6 鼠标开关 · Ctrl+Q 随时退出"),
        Line::from("拖选文字 · 双击选词 / 打开媒体 · 拖动滚动条"),
        Line::from("图片：滚轮或 +/- 缩放 · 拖动或方向键平移 · 0 复位"),
        Line::from("选中文字后 Ctrl+C 复制 · 输入区 Ctrl+A 全选"),
        muted("终端自带的选字修饰键因终端而异，可用 F6 暂停鼠标。"),
        Line::from(""),
        Line::styled("API 凭据与官网注册", Style::default().fg(palette().accent)),
        Line::from("已有应用：登录 my.telegram.org/apps 复制原 API ID / Hash。"),
        Line::from("每个号码只能创建一个 API ID，已有应用无需重新注册。"),
        Line::from("官网 ERROR 是 Telegram 的注册错误，客户端无法代为修复。"),
        Line::from("联系发布者提供含 Teleaf 项目凭据的安装包，可免手动注册。"),
        muted("二维码登录也需要应用凭据；官方示例 ID 不适合发行。"),
    ];
    scrolling_text(frame, lines, inner, &mut app.modal_scroll);
}

fn settings_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 70, 21), "设置与连接");
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(2)]).split(inner);
    let tools = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(rows[1].x, rows[1].y, rows[1].width, 1));
    button(frame, app, tools[0], "[修改 API]", Action::ApiSetup);
    button(frame, app, tools[1], "[官网]", Action::OpenLink);
    button(
        frame,
        app,
        Rect::new(rows[1].x, rows[1].y + 1, rows[1].width, 1),
        if app.mouse_enabled {
            "[鼠标：开 · F6 切换]"
        } else {
            "[鼠标：关 · F6 切换]"
        },
        Action::ToggleMouse,
    );
    let (api_id, directory) = app.auth.config_summary();
    let lines = vec![
        Line::styled("账号连接", Style::default().fg(palette().accent)),
        Line::from(format!("API ID   {api_id}")),
        muted("API Hash 已隐藏 · F3 修改 API 配置并重新连接"),
        muted("F2 打开 Telegram API 页面"),
        Line::from(""),
        Line::styled("本机数据", Style::default().fg(palette().accent)),
        Line::from(directory),
        muted("凭据和自动生成的密钥保存在 config.json，权限仅当前用户。"),
        Line::from(""),
        Line::styled("终端与诊断", Style::default().fg(palette().accent)),
        Line::from(format!("图片显示   {}", app.media.protocol_name)),
        Line::from(format!("界面配色   {} · TG_THEME 可切换", palette().name)),
        Line::from(crate::terminal::summary()),
        Line::from(app.status.clone()),
        muted(app.tdlib_path.as_deref().unwrap_or("TDLib 尚未加载")),
    ];
    scrolling_text(frame, lines, rows[0], &mut app.modal_scroll);
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    crate::text::rows(text, width)
        .into_iter()
        .map(|row| text[row.start..row.end].to_owned())
        .collect()
}

fn scrolling_text(frame: &mut Frame, lines: Vec<Line<'static>>, area: Rect, scroll: &mut u16) {
    let total: usize = lines
        .iter()
        .map(|line| wrap_text(&line.to_string(), usize::from(area.width).max(1)).len())
        .sum();
    *scroll = (*scroll).min(total.saturating_sub(usize::from(area.height)) as u16);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((*scroll, 0)),
        area,
    );
}

#[cfg(test)]
pub(crate) mod tests {
    #[test]
    fn sixel_restores_cursor_before_adjacent_text_and_keeps_idle_frames_unchanged() {
        use ratatui_image::protocol::{Protocol, sixel::Sixel};
        let image = Protocol::Sixel(Sixel {
            data: "\x1bP0;1;0q#0;2;100;0;0#0~-\x1b\\".into(),
            size: Size::new(1, 1),
            is_tmux: false,
        });
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).unwrap();
        let mut previous = None;
        for _ in 0..2 {
            terminal
                .draw(|frame| {
                    let area = Rect::new(2, 1, 1, 1);
                    frame.render_widget(Image::new(&image), area);
                    finish_sixel(frame, area, "Sixel");
                    frame.render_widget(Paragraph::new("ok"), Rect::new(3, 1, 2, 1));
                })
                .unwrap();
            let buffer = terminal.backend().buffer();
            assert!(buffer[(2, 1)].symbol().ends_with("\x1b[2;4H"));
            assert_eq!(buffer[(3, 1)].symbol(), "o");
            if let Some(previous) = &previous {
                assert_eq!(buffer, previous);
            }
            previous = Some(buffer.clone());
        }
    }
    use super::*;
    use crate::auth::AuthFlow;
    use crate::media::MediaManager;
    use crate::store::{Message, Sticker};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;
    use serde_json::json;

    pub(crate) fn fixture() -> App {
        let mut app = App::with_parts(
            AuthFlow::empty(),
            MediaManager::from_picker(Picker::halfblocks()),
        );
        app.demo = true;
        app.auth.state = "authorizationStateReady".into();
        app.tdlib_connected = true;
        app.status = "TDLib 1.8.61 已连接".into();
        app.connection_label = "在线".into();
        app.tdlib_path = Some("/example/target/tdlib/libtdjson.dylib".into());
        for (id, title, preview, unread) in [
            (1, "产品讨论", "新界面已完成，来看看吧", 3),
            (2, "小林", "明天下午见", 0),
            (3, "Telegram", "欢迎使用 Telegram", 1),
        ] {
            app.store.apply(&json!({"@type":"updateNewChat","chat": {
                "id":id,"title":title,"unread_count":unread,
                "positions":[{"list":{"@type":"chatListMain"},"order":100-id}],
                "last_message":{"content":{"@type":"messageText","text":{"text":preview}}}
            }}));
        }
        app.store.open(1);
        app.store.messages = vec![
            Message { id:1, text:"我们来看看新的终端界面。长消息现在会自动换行，中文、English 和 emoji 👋 都能正常阅读。".into(), outgoing:false, sender:None, author_signature:String::new(), media:None, ..Message::default() },
            Message { id:2, text:"好，输入、回复、图片和贴纸都在更多操作菜单里。\n按 Enter 试试吧。".into(), outgoing:true, sender:None, author_signature:String::new(), media:None, ..Message::default() },
        ];
        app.selected_chat = Some(1);
        for message in &mut app.store.messages {
            message.info.text_message = true;
        }
        app.selected_message = Some(2);
        app.focus_messages = true;
        app
    }

    #[test]
    fn timestamps_quotes_and_retry_buttons_render_at_correct_rows() {
        let mut app = fixture();
        app.store.messages[0].info.stamp = crate::calendar::local(1700000000);
        app.store.messages[1].info.stamp = crate::calendar::local(1700000060);
        app.store.messages[1].info.reply = Some(crate::store::Reply {
            chat_id: 1,
            message_id: 1,
            excerpt: "引用内容".into(),
        });
        app.store.messages[1].info.sending = crate::store::Sending::Failed {
            can_retry: true,
            reason: "断网".into(),
        };
        let screen = snapshot(&mut app, 110, 40);
        assert!(screen.contains("引用内容") && screen.contains("发送失败"));
        assert!(screen.contains(&app.store.messages[0].info.stamp.unwrap().date()));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, target)| *target == Target::ReferencedMessage(1))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, target)| *target == Target::MessageAction(2, Action::Retry))
        );
        app.timeline_anchor = Some((2, 1));
        snapshot(&mut app, 45, 18);
        assert!(
            app.hit_targets
                .iter()
                .all(|(rect, _)| rect.right() <= 45 && rect.bottom() <= 18)
        );
    }

    #[test]
    fn buttons_follow_selection_but_failed_messages_keep_retry_visible() {
        let mut app = fixture();
        app.store.messages[0].info.sending = crate::store::Sending::Failed {
            can_retry: true,
            reason: "断网".into(),
        };
        snapshot(&mut app, 110, 32);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(1, Action::Menu))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(1, Action::Retry))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(2, Action::Menu))
        );
        app.selected_message = Some(1);
        snapshot(&mut app, 110, 32);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(2, Action::Menu))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(1, Action::Menu))
        );
        app.reveal_message = true;
        snapshot(&mut app, 30, 12);
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::MessageAction(1, Action::Retry))
        );
    }

    #[test]
    fn sticker_tray_is_inside_chat_column_above_an_interactive_composer() {
        for (width, height) in [(110, 32), (80, 24), (45, 18), (30, 12)] {
            let mut app = fixture();
            app.sticker_picker = true;
            app.store.recent_stickers.push(Sticker {
                file_id: 10,
                emoji: "👋".into(),
                width: 100,
                height: 100,
                preview: MediaRef {
                    file_id: 10,
                    path: None,
                    kind: MediaKind::Sticker,
                    detail: None,
                },
            });
            let output = snapshot(&mut app, width, height);
            let column = chat_column(Rect::new(0, 1, width, height - 2));
            let tray = app
                .hit_targets
                .iter()
                .find(|(_, t)| *t == Target::Modal)
                .unwrap()
                .0;
            assert!(tray.x >= column.x && tray.right() <= column.right());
            assert!(tray.bottom() <= height - 5);
            assert!(app.hit_targets.iter().any(|(_, t)| *t == Target::Composer));
            assert!(
                app.hit_targets
                    .iter()
                    .any(|(_, t)| *t == Target::Command(Action::Submit))
            );
            assert!(
                app.hit_targets
                    .iter()
                    .any(|(_, t)| *t == Target::Sticker(0))
            );
            assert!(
                app.hit_targets
                    .iter()
                    .all(|(r, _)| r.right() <= width && r.bottom() <= height)
            );
            assert!(
                output.contains("[附件]") && output.contains("[贴纸]") && output.contains("[发送]")
            );
            if height == 12 {
                assert!(!app.sticker_panel.preview_visible);
            }
        }
    }

    #[test]
    fn main_navigation_shows_back_only_for_narrow_chat_and_preserves_auth_back() {
        let mut app = fixture();
        snapshot(&mut app, 110, 32);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Command(Action::Back))
        );
        snapshot(&mut app, 45, 18);
        assert!(
            app.hit_targets
                .iter()
                .any(|(r, t)| r.y == 0 && *t == Target::Command(Action::Back))
        );
        app.focus_messages = false;
        snapshot(&mut app, 45, 18);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Command(Action::Back))
        );
        app.auth.begin_setup();
        snapshot(&mut app, 45, 18);
        assert!(
            app.hit_targets
                .iter()
                .any(|(r, t)| r.y == 0 && *t == Target::Command(Action::Back))
        );
    }

    #[test]
    fn inline_media_hides_duplicate_labels_preserves_caption_offsets_and_stable_height() {
        let mut app = fixture();
        let image = crate::clipboard::test_image();
        app.store.messages[0].text = "[图片] 真实说明 👩‍💻".into();
        app.store.messages[0].media = Some(MediaRef {
            file_id: 71,
            path: Some(image.path.to_string_lossy().into()),
            kind: MediaKind::Photo,
            detail: None,
        });
        app.store.messages[1].text = "[贴纸] 🙂".into();
        app.store.messages[1].media = Some(MediaRef {
            file_id: 72,
            path: Some(image.path.to_string_lossy().into()),
            kind: MediaKind::Sticker,
            detail: None,
        });
        let mut terminal = Terminal::new(TestBackend::new(50, 32)).unwrap();
        terminal
            .draw(|frame| message_list(frame, &mut app, frame.area()))
            .unwrap();
        let output = |terminal: &Terminal<TestBackend>| {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .replace(' ', "")
        };
        assert!(
            output(&terminal).contains("[图片]"),
            "{}",
            output(&terminal)
        );
        assert!(output(&terminal).contains("[贴纸]"));
        let rows = app.timeline_rows.clone();
        let size = Size::new(47, 8);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while app.media.get_inline(71, size).is_none() || app.media.get_inline(72, size).is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "inline image preparation timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.media.poll();
        }
        app.hit_targets.clear();
        terminal
            .draw(|frame| message_list(frame, &mut app, frame.area()))
            .unwrap();
        let ready = output(&terminal);
        assert!(!ready.contains("[图片]") && !ready.contains("[贴纸]"));
        assert!(ready.contains("真实说明"));
        assert_eq!(
            app.timeline_rows, rows,
            "ready images must not shift the timeline"
        );
        let start = "[图片] ".len();
        assert!(app.hit_targets.iter().any(|(_, target)| *target
            == Target::Text(Point {
                source: Source::Message(1),
                byte: start
            })));
        app.selection = Some(selection::Selection {
            anchor: Point {
                source: Source::Message(1),
                byte: start,
            },
            head: Point {
                source: Source::Message(1),
                byte: app.store.messages[0].text.len(),
            },
        });
        assert_eq!(
            selection::selected_text(&app).unwrap().as_deref(),
            Some("真实说明 👩‍💻")
        );
        let message = crate::store::Message {
            text: "[图片] 用户自己输入的文字".into(),
            ..Default::default()
        };
        assert_eq!(message_body(&message), message.text);
    }

    #[test]
    fn chat_list_keeps_marker_columns_when_selection_scrolls_out_of_view() {
        for (width, height, focused) in [(26, 12, true), (36, 13, false), (45, 18, true)] {
            let mut app = fixture();
            for id in 4..=30 {
                app.store.apply(&json!({"@type":"updateNewChat","chat": {
                    "id":id,"title":format!("会话 {id}"),
                    "positions":[{"list":{"@type":"chatListMain"},"order":100-id}],
                    "last_message":{"content":{"@type":"messageText","text":{"text":"预览消息"}}}
                }}));
            }
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for offset in [0, 1, 7, 30, 0] {
                app.chat_offset = offset;
                terminal
                    .draw(|frame| {
                        app.hit_targets.clear();
                        app.scrollbars.clear();
                        chat_list(frame, &mut app, frame.area(), focused, true);
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                for (rect, target) in &app.hit_targets {
                    let Target::Chat(id) = target else { continue };
                    let selected = Some(*id) == app.selected_chat;
                    assert_eq!(
                        buffer[(rect.x, rect.y)].symbol(),
                        if selected { "›" } else { " " },
                        "marker at offset {offset}, chat {id}"
                    );
                    assert_eq!(buffer[(rect.x + 1, rect.y)].symbol(), " ");
                    assert_eq!(buffer[(rect.x, rect.y + 1)].symbol(), " ");
                    assert_eq!(buffer[(rect.x + 1, rect.y + 1)].symbol(), " ");
                    assert_eq!(
                        buffer[(rect.x + 2, rect.y)].symbol(),
                        if Some(*id) == app.store.active_chat {
                            "·"
                        } else if *id <= 3 {
                            if *id == 2 { "小" } else { "T" }
                        } else {
                            "会"
                        }
                    );
                    if *id >= 4 {
                        assert_eq!(buffer[(rect.x + 2, rect.y + 1)].symbol(), "预");
                    }
                }
                assert_eq!(app.selected_chat, Some(1));
                assert_eq!(app.store.active_chat, Some(1));
            }
        }
    }

    #[test]
    fn long_chat_titles_keep_badges_and_selected_prefix_styles() {
        for (width, height) in [(110, 32), (80, 24), (45, 18), (30, 12)] {
            let mut app = fixture();
            app.focus_messages = false;
            app.store.apply(&json!({"@type":"updateChatTitle","chat_id":1,"title":"开发讨论组 👩‍💻 很长的中文与 English 会话标题"}));
            app.store
                .apply(&json!({"@type":"updateChatReadInbox","chat_id":1,"unread_count":2345}));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let rect = app
                .hit_targets
                .iter()
                .find(|(_, target)| *target == Target::Chat(1))
                .unwrap()
                .0;
            let buffer = terminal.backend().buffer();
            let row: String = (rect.x..rect.right())
                .map(|x| buffer[(x, rect.y)].symbol())
                .collect();
            assert!(row.contains("999+"), "badge clipped: {row}");
            assert!(row.contains('…'), "title should truncate: {row}");
            let prefix = &buffer[(rect.x, rect.y)];
            let body = &buffer[(rect.x + 2, rect.y)];
            assert_eq!(prefix.bg, body.bg);
            assert_eq!(
                prefix.modifier.contains(Modifier::REVERSED),
                body.modifier.contains(Modifier::REVERSED)
            );
            let badge = &buffer[(rect.right() - 2, rect.y)];
            assert_eq!(badge.bg, palette().primary().bg.unwrap());
            assert_eq!(badge.fg, palette().primary().fg.unwrap());
        }
    }

    #[test]
    fn one_line_excerpts_preserve_graphemes_and_bound_long_replies() {
        assert_eq!(excerpt("ab\r\ncd\te", 20), "ab cd e");
        assert_eq!(excerpt("a\x1b[31mb\0", 20), "a [31mb ");
        assert_eq!(excerpt("abcde", 5), "abcde");
        assert_eq!(excerpt("abcdef", 5), "abcd…");
        assert_eq!(excerpt("中文测试", 5), "中文…");
        assert_eq!(excerpt("👩‍💻hello", 2), "…");
        assert_eq!(excerpt("👩‍💻", 2), "👩‍💻");
        assert!(excerpt(&"长消息\n".repeat(100_000), 22).len() < 100);
        assert!(excerpt(&"\u{200b}".repeat(100_000), 10).len() <= 163);
        let mut app = fixture();
        app.input_mode = InputMode::Reply(1);
        for (width, height) in [(110, 32), (30, 12)] {
            let output = snapshot(&mut app, width, height);
            assert!(output.contains("回复消息"));
            assert!(
                app.hit_targets
                    .iter()
                    .any(|(_, t)| *t == Target::Command(Action::Cancel))
            );
        }
    }

    fn snapshot(app: &mut App, width: u16, height: u16) -> String {
        snapshot_named(app, width, height, None)
    }

    fn snapshot_named(
        app: &mut App,
        width: u16,
        height: u16,
        path: Option<&std::path::Path>,
    ) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).expect("terminal");
        terminal.draw(|frame| draw(frame, app)).expect("draw");
        let buffer = terminal.backend().buffer();
        // Synthetic fixture only. Export actual cell styles for visual review;
        // this code is excluded from the application binary.
        if std::env::var_os("TG_UI_PREVIEW").is_some()
            && let Some(path) = path
        {
            let cells: Vec<_> = (0..height)
                .flat_map(|y| {
                    (0..width).map(move |x| {
                        let cell = &buffer[(x, y)];
                    json!({"x":x,"y":y,"text":cell.symbol(),"width":UnicodeWidthStr::width(cell.symbol()).max(1),"fg":format!("{:?}",cell.fg),
                    "bg":format!("{:?}",cell.bg),"modifiers":cell.modifier.bits()})
                    })
                })
                .collect();
            std::fs::write(
                path,
                serde_json::to_vec(&json!({"width":width,"height":height,"cells":cells})).unwrap(),
            )
            .expect("write styled preview");
        }
        let mut output = String::new();
        for y in 0..height {
            let mut x = 0;
            while x < width {
                let symbol = buffer[(x, y)].symbol();
                output.push_str(symbol);
                x += (UnicodeWidthStr::width(symbol) as u16).max(1);
            }
            output.push('\n');
        }
        let cursor = terminal.get_cursor_position().expect("cursor");
        assert!(
            cursor.x < width && cursor.y < height,
            "cursor outside terminal"
        );
        output
    }

    #[test]
    fn group_title_and_sender_survive_long_names_and_narrow_controls() {
        let mut app = fixture();
        app.store.apply(
            &json!({"@type":"updateChatTitle","chat_id":1,"title":"群名称很长的项目开发讨论群"}),
        );
        app.store
            .apply(&json!({"@type":"updateUser","user":{"id":7,"first_name":"小林"}}));
        app.store.messages[0].sender = Some(crate::store::Sender::User(7));
        app.store.messages[1].sender = Some(crate::store::Sender::Chat(1));
        app.store.messages[1].outgoing = false;
        app.store.messages[1].author_signature = "管理员".into();
        for width in [180, 110, 80, 45, 30] {
            let output = snapshot(&mut app, width, 32);
            assert!(
                output.contains("群名称"),
                "group title missing at {width}: {output}"
            );
            assert!(
                output.contains("小林"),
                "sender missing at {width}: {output}"
            );
            assert!(output.contains("[⋯]") || output.contains("[更多]"));
        }
    }

    #[test]
    fn offline_demo_drives_real_worker_commands_and_renders_inline_images() {
        let worker = crate::tdlib::TdWorker::spawn_demo();
        let mut app = App::with_parts(
            AuthFlow::empty(),
            MediaManager::from_picker(Picker::halfblocks()),
        );
        app.demo = true;
        // The demo creates real PNG fixtures before emitting authorization.
        // Debug builds on shared Intel CI can take longer than one second.
        // Use one bounded deadline rather than failing on a short idle interval.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while app.auth.state != "authorizationStateReady" {
            let event = worker
                .events
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("demo worker did not initialize before the deadline");
            if let (Some(request), _) = app.apply(event) {
                worker.request(request).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
        }
        app.selected_chat = Some(1);
        for request in app.open_selected() {
            worker.request(request).unwrap();
        }
        while app.store.messages.is_empty() {
            let event = worker
                .events
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("demo worker did not load messages before the deadline");
            if let (Some(request), _) = app.apply(event) {
                worker.request(request).unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
        }
        let output = loop {
            app.media.poll();
            let output = snapshot(&mut app, 110, 40);
            if output.contains('▄') || output.contains('▀') {
                break output;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(output.contains("离线演示") && output.contains("工作"));
        assert!(output.contains("产品讨论") && output.contains("陈同学"));
        assert!(!output.contains("图片 / 贴纸预览"));
        std::fs::create_dir_all("target/ui-previews").unwrap();
        std::fs::write("target/ui-previews/demo-110x40.txt", output).unwrap();
        worker.shutdown();
    }

    #[test]
    fn all_pages_render_on_wide_and_small_terminals() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/ui-previews");
        std::fs::create_dir_all(&directory).expect("preview directory");
        for (width, height) in [(110, 32), (80, 24), (45, 18), (30, 12)] {
            for page in [
                "setup",
                "setup-hash",
                "recovery",
                "new-login",
                "connecting",
                "error",
                "phone",
                "code",
                "password",
                "email",
                "email-address",
                "registration",
                "confirmation",
                "chats",
                "compose",
                "reply",
                "edit",
                "search",
                "photo",
                "file",
                "reaction",
                "media",
                "stickers",
                "actions",
                "forward",
                "delete",
                "help",
                "settings",
                "folders",
                "inline-images",
                "selection",
                "zoom",
            ] {
                let mut app = fixture();
                match page {
                    "selection" => {
                        app.selection = Some(crate::selection::Selection {
                            anchor: Point {
                                source: Source::Message(1),
                                byte: 3,
                            },
                            head: Point {
                                source: Source::Message(2),
                                byte: 9,
                            },
                        });
                    }
                    "zoom" => {
                        app.store.messages[1].media = Some(MediaRef {
                            file_id: 10,
                            path: None,
                            kind: MediaKind::Photo,
                            detail: None,
                        });
                        app.preview_message = Some(2);
                        app.preview_view.zoom = 300;
                    }
                    "recovery" | "new-login" => {
                        app.auth.begin_setup();
                        app.auth.on_update(&json!({"@type":"error","code":400,"message":"Wrong database encryption key","@extra":"set-tdlib-parameters"}));
                        app.auth.paste("private-database-key");
                        if page == "new-login" {
                            app.auth.choose_new_login();
                        }
                    }
                    "setup" | "setup-hash" => {
                        app.auth.begin_setup();
                        app.auth.setup.as_mut().expect("form").api_hash = "private-api-hash".into();
                        if page == "setup-hash" {
                            app.auth.setup.as_mut().expect("form").focused = 1;
                        }
                    }
                    "connecting" | "error" => {
                        app.auth.state.clear();
                        app.tdlib_connected = false;
                        app.status = if page == "error" {
                            "无法连接，请在设置中查看 TDLib 状态"
                        } else {
                            "正在连接 TDLib…"
                        }
                        .into();
                    }
                    "phone" | "code" | "password" | "email" | "email-address" | "registration"
                    | "confirmation" => {
                        let state = match page {
                            "phone" => "authorizationStateWaitPhoneNumber",
                            "code" => "authorizationStateWaitCode",
                            "password" => "authorizationStateWaitPassword",
                            "email" => "authorizationStateWaitEmailCode",
                            "email-address" => "authorizationStateWaitEmailAddress",
                            "registration" => "authorizationStateWaitRegistration",
                            _ => "authorizationStateWaitOtherDeviceConfirmation",
                        };
                        app.auth.on_update(&json!({"@type":"updateAuthorizationState","authorization_state":{"@type":state,"password_hint":"你的密码提示","link":"tg://login?token=example","email_address_pattern":"a***@example.com"}}));
                        if page == "password" {
                            app.auth.paste("private-password");
                        }
                    }
                    "compose" | "reply" | "edit" => {
                        app.input_mode = match page {
                            "compose" => InputMode::Send,
                            "reply" => InputMode::Reply(1),
                            _ => InputMode::Edit(2),
                        };
                        app.draft = "你好，粘贴的文字也可以直接发送。\n第二行内容".into();
                    }
                    "search" => {
                        app.show_search = true;
                        app.search_query = "计划".into();
                        app.input_mode = InputMode::Search;
                        app.draft = "计划".into();
                    }
                    "photo" => crate::interaction::open_attachments(&mut app, true),
                    "file" => crate::interaction::open_attachments(&mut app, false),
                    "reaction" => app.input_mode = InputMode::React(1),
                    "media" => {
                        app.store.messages[1].media = Some(MediaRef {
                            file_id: 10,
                            path: None,
                            kind: MediaKind::Photo,
                            detail: None,
                        });
                        app.preview_message = Some(2);
                    }
                    "stickers" => {
                        app.sticker_picker = true;
                        app.store.recent_stickers.push(Sticker {
                            file_id: 10,
                            emoji: "👋".into(),
                            width: 100,
                            height: 100,
                            preview: MediaRef {
                                file_id: 10,
                                path: None,
                                kind: MediaKind::Sticker,
                                detail: None,
                            },
                        });
                    }
                    "actions" => {
                        app.menu_scope = crate::menu::Scope::Message {
                            chat: 1,
                            message: 2,
                        };
                        app.action_menu = Some(1);
                    }
                    "forward" => app.forward_message = Some(2),
                    "delete" => app.confirm_delete = Some(2),
                    "help" => app.show_help = true,
                    "folders" => {
                        app.store.apply(&json!({"@type":"updateChatFolders","chat_folders":[{"id":7,"name":{"text":{"text":"工作"}}}]}));
                        app.show_folders = true;
                    }
                    "inline-images" => {
                        for (index, message) in app.store.messages.iter_mut().enumerate() {
                            message.media = Some(MediaRef {
                                file_id: 100 + index as i32,
                                path: None,
                                kind: MediaKind::Photo,
                                detail: None,
                            });
                        }
                    }
                    "settings" => app.show_settings = true,
                    _ => {}
                }
                let text = snapshot_named(
                    &mut app,
                    width,
                    height,
                    Some(&directory.join(format!("{page}-{width}x{height}.json"))),
                );
                assert!(
                    !text.contains("private-api-hash")
                        && !text.contains("private-password")
                        && !text.contains("private-database-key")
                );
                assert!(!text.contains("authorizationState"));
                if page == "setup" {
                    assert!(text.contains("API ID"));
                }
                if page == "recovery" {
                    assert!(text.contains("旧本地密钥"));
                    assert!(text.contains("重新登录") && text.contains("恢复登录"));
                }
                if page == "new-login" {
                    assert!(text.contains("确认重新登录"));
                }
                if page == "chats" {
                    assert!(text.contains("产品讨论"));
                }
                std::fs::write(directory.join(format!("{page}-{width}x{height}.txt")), text)
                    .expect("write preview");
            }
        }
    }

    #[test]
    fn wrapping_keeps_unicode_and_long_message_content() {
        let text = "中文👨‍👩‍👧‍👦e\u{301} test\n第二行";
        let lines = wrap_text(text, 8);
        assert_eq!(lines.concat(), text.replace('\n', ""));
        assert!(
            lines
                .iter()
                .all(|line| UnicodeWidthStr::width(line.as_str()) <= 8)
        );
        let mut app = fixture();
        app.store.messages[1].text = "这是长消息。".repeat(50);
        app.timeline_anchor = Some((2, 5));
        let text = snapshot(&mut app, 45, 18);
        assert!(text.contains("长消息"));
        assert!(text.contains("回到底部"));
    }

    #[test]
    fn modal_mouse_targets_do_not_activate_background_controls() {
        let mut app = fixture();
        snapshot(&mut app, 80, 24);
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, target)| matches!(target, Target::Chat(1)))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, target)| matches!(target, Target::Message(2)))
        );
        app.action_menu = Some(1);
        app.menu_scope = crate::menu::Scope::Message {
            chat: 1,
            message: 2,
        };
        snapshot(&mut app, 80, 24);
        assert!(!app.hit_targets.is_empty());
        assert!(app.hit_targets.iter().all(|(_, target)| !matches!(
            target,
            Target::Chat(_) | Target::Message(_) | Target::Composer
        )));
        app.close_overlays();
        app.show_help = true;
        snapshot(&mut app, 80, 24);
        assert!(
            app.hit_targets
                .iter()
                .all(|(_, target)| !matches!(target, Target::Chat(_) | Target::Message(_)))
        );
    }
}
