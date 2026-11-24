//! Shared page layout. Views render only visible messages and reuse media caches.
use ratatui::Frame;
use ratatui::buffer::CellDiffOption;
use ratatui::layout::{Alignment, Constraint, Layout, Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph, Wrap,
};
use ratatui_image::Image;
use ratatui_image::sliced::{SignedPosition, SlicedImage};
use unicode_segmentation::UnicodeSegmentation;

use crate::selection::{self, Point, Source};
use crate::store::{EntityKind, MediaKind, MediaRef, Message, TextEntity};
use crate::text::{self, Row, grapheme_width, width as text_width};
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
    Proxy(usize),
    AttachmentPath,
    AttachmentCaption,
    StickerSearch,
    QuickSwitch,
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
    ToggleNotifications,
    CycleTheme,
    QuickSwitch,
    ChatInfo,
    ToggleMute,
    TogglePin,
    DismissPinBanner,
    ToggleMultiSelect,
    MultiCopy,
    MultiForward,
    MultiDelete,
    MultiClear,
    OpenMessageLink,
    InlineButtons,
    OpenInstantViewBrowser,
    CloseInstantView,
    JumpToMessage,
    QuickReact(usize),
    CycleMediaFilter,
    CleanStorage,
    ConfirmStorage,
    CancelStorage,
    ProxySettings,
    ProxyAdd,
    ProxyImportEnv,
    ProxyToggle,
    ProxyPing,
    ProxyDelete,
    ProxyDisableAll,
    ProxySubmit,
    ProxyCancelForm,
    ProxyKindPrev,
    ProxyKindNext,
    SaveFileAs,
    PreviewPrev,
    PreviewNext,
    Topics,
    ExitTopic,
    ExitThread,
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
    Quote(i64, i64),
    PinnedBanner(i64),
    Reaction(i64, usize),
    QuickSwitchChat(i64),
    GlobalSearchHit(i64, i64),
    ProxyEntry(usize),
    ProxyField(usize),
    LinkEntry(usize),
    TopicEntry(usize),
    InlineButton(i64, usize, usize),
    AlbumItem(i64),
    PreviewLink(i64),
    InstantViewLink(i64),
    CommentThread(i64),
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
        'c' | 'y' => Action::Copy,
        'p' => Action::Photo,
        'a' => Action::File,
        't' => Action::Stickers,
        '/' => Action::Search,
        'g' => Action::History,
        'G' => Action::Bottom,
        'm' => Action::MoreChats,
        'o' => Action::OpenExternal,
        'v' => Action::Preview,
        'K' => Action::QuickSwitch,
        'I' => Action::ChatInfo,
        'V' => Action::ToggleMultiSelect,
        'L' => Action::OpenMessageLink,
        'B' => Action::InlineButtons,
        'E' => Action::SaveFileAs,
        'T' => Action::Topics,
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
    let label_width = (text_width(label) as u16 + if primary { 2 } else { 0 }).min(area.width);
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
    let available = area.width.saturating_sub(6);
    let tab_widths: Vec<u16> = lists
        .iter()
        .map(|(_, name)| {
            ((text_width(name.as_str()) as u16) + 3)
                .clamp(7, 11)
                .min(available.max(1))
        })
        .collect();
    // Clamp folder_offset so the rightmost visible group still fills the bar.
    let mut max_offset = lists.len().saturating_sub(1);
    let mut tail_used = 0u16;
    for (idx, &w) in tab_widths.iter().enumerate().rev() {
        if tail_used + w <= available {
            tail_used += w;
            max_offset = idx;
        } else {
            break;
        }
    }
    app.folder_offset = app.folder_offset.min(max_offset);
    let can_prev = app.folder_offset > 0;
    let prev_rect = Rect::new(area.x, area.y, 2.min(area.width), 1);
    frame.render_widget(
        Paragraph::new("‹")
            .style(Style::default().fg(if can_prev {
                palette().muted
            } else {
                palette().border
            }))
            .alignment(Alignment::Center),
        prev_rect,
    );
    app.hit_targets.push((prev_rect, Target::FolderPage(-1)));

    let mut x = area.x.saturating_add(2);
    let limit = area.right().saturating_sub(4);
    let mut shown_end = app.folder_offset;
    for (index, ((list, name), &width)) in lists
        .iter()
        .zip(&tab_widths)
        .enumerate()
        .skip(app.folder_offset)
    {
        let remaining = limit.saturating_sub(x);
        if remaining == 0 {
            break;
        }
        let rect_w = if shown_end == app.folder_offset {
            width.min(remaining)
        } else if width <= remaining {
            width
        } else {
            break;
        };
        let rect = Rect::new(x, area.y, rect_w, 1);
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
        x = x.saturating_add(rect_w);
        shown_end = index + 1;
    }

    let can_next = shown_end < lists.len();
    let next_rect = Rect::new(area.right().saturating_sub(4), area.y, 2.min(area.width), 1);
    frame.render_widget(
        Paragraph::new("›")
            .style(Style::default().fg(if can_next {
                palette().muted
            } else {
                palette().border
            }))
            .alignment(Alignment::Center),
        next_rect,
    );
    app.hit_targets.push((next_rect, Target::FolderPage(1)));
    button(
        frame,
        app,
        Rect::new(area.right().saturating_sub(2), area.y, 2, 1),
        "▾",
        Action::Folders,
    );
}

fn modal(frame: &mut Frame, app: &mut App, area: Rect, title: &str) -> Rect {
    let title = crate::text::ellipsize(title, usize::from(area.width.saturating_sub(10)).max(1));
    let inner = dialog(frame, area, &title);
    app.hit_targets.push((area, Target::Modal));
    app.hit_targets.push((inner, Target::Panel(Pane::Modal)));
    let close = Rect::new(area.right().saturating_sub(5), area.y, 3.min(area.width), 1);
    button(frame, app, close, "[×]", Action::Cancel);
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

fn panel_with_style(title: String, focused: bool, base: Style) -> Block<'static> {
    let label = if focused {
        format!("▸ {title}")
    } else {
        title
    };
    let title = Line::from(label).style(
        Style::default()
            .fg(if focused {
                palette().accent
            } else {
                palette().text
            })
            .add_modifier(Modifier::BOLD),
    );
    Block::default()
        .style(base)
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

fn panel(title: impl Into<String>, focused: bool) -> Block<'static> {
    panel_with_style(title.into(), focused, palette().base())
}

fn muted(text: impl Into<String>) -> Line<'static> {
    Line::styled(text.into(), Style::default().fg(palette().muted))
}

fn is_error_notice(text: &str) -> bool {
    text.contains("失败")
        || text.starts_with("无法")
        || text.starts_with("暂不能")
        || text.starts_with("这条消息当前不允许")
        || text.starts_with("选中文字超过")
        || text.starts_with("本次运行的剪贴板")
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
        let size = grapheme_width(grapheme);
        if used + size > width
            || output.len().saturating_add(grapheme.len()) > width.saturating_mul(16).max(32)
        {
            while used >= width {
                let (index, last) = output
                    .grapheme_indices(true)
                    .next_back()
                    .expect("nonempty prefix");
                used = used.saturating_sub(grapheme_width(last));
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
    let block = panel_with_style(title.to_owned(), true, palette().surface_base());
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

fn draw_line(frame: &mut Frame, area: Rect, line: &Line<'_>, base: Style) {
    let style = base.patch(line.style);
    let buffer = frame.buffer_mut();
    buffer.set_style(area, style);
    let mut x = area.x;
    if let Some(alignment @ (Alignment::Center | Alignment::Right)) = line.alignment {
        let line_width = line
            .spans
            .iter()
            .map(|span| text_width(&span.content))
            .sum();
        let padding = usize::from(area.width).saturating_sub(line_width) as u16;
        x += if alignment == Alignment::Center {
            padding / 2
        } else {
            padding
        };
    }
    for span in &line.spans {
        let span_style = style.patch(span.style);
        for grapheme in span.content.graphemes(true) {
            let width = grapheme_width(grapheme) as u16;
            if width == 0 {
                continue;
            }
            if width > area.right().saturating_sub(x) {
                return;
            }
            let cell = &mut buffer[(x, area.y)];
            cell.set_symbol(grapheme).set_style(span_style);
            // Skip continuation cells even when replacing old text. A write
            // into the second cell can erase the entire VS16 emoji.
            if grapheme.contains('\u{fe0f}') {
                cell.set_diff_option(CellDiffOption::ForcedWidth(
                    std::num::NonZeroU16::new(width).unwrap(),
                ));
            }
            for trailing in x + 1..x + width {
                let cell = &mut buffer[(trailing, area.y)];
                cell.reset();
                cell.set_style(span_style);
            }
            x += width;
        }
    }
}

fn styled_message_line(
    value: &str,
    row: Row,
    selection: Option<std::ops::Range<usize>>,
    prefix: &str,
    entities: &[TextEntity],
    reveal_spoiler: bool,
    search_query: Option<&str>,
) -> Line<'static> {
    if entities.is_empty() && search_query.is_none() {
        return selected_line(value, row, selection, prefix);
    }
    let slice = &value[row.start..row.end];
    let mut search_ranges = Vec::new();
    if let Some(query) = search_query.filter(|q| !q.is_empty()) {
        let lower_slice = slice.to_lowercase();
        let lower_query = query.to_lowercase();
        if lower_slice.len() == slice.len() {
            search_ranges.extend(
                lower_slice
                    .match_indices(&lower_query)
                    .map(|(i, text)| row.start + i..row.start + i + text.len()),
            );
        }
    }
    let mut spans: Vec<Span<'static>> = Vec::new();
    let overlaps = |range: &std::ops::Range<usize>, a, b| range.start < b && range.end > a;
    for (offset, grapheme) in slice.grapheme_indices(true) {
        let a = row.start + offset;
        let b = a + grapheme.len();
        let in_selection = selection
            .as_ref()
            .is_some_and(|range| overlaps(range, a, b));
        let in_search = search_ranges.iter().any(|range| overlaps(range, a, b));
        let mut style = Style::default();
        let mut hidden_spoiler = false;
        for entity in entities
            .iter()
            .filter(|entity| entity.start < b && entity.end > a)
        {
            style = match entity.kind {
                EntityKind::Bold => style.add_modifier(Modifier::BOLD),
                EntityKind::Italic => style.add_modifier(Modifier::ITALIC),
                EntityKind::Underline => style.add_modifier(Modifier::UNDERLINED),
                EntityKind::Code | EntityKind::Pre(_) => {
                    style.fg(palette().incoming).bg(palette().surface)
                }
                EntityKind::BlockQuote => style.fg(palette().muted),
                EntityKind::Strike => style.add_modifier(Modifier::CROSSED_OUT),
                EntityKind::Spoiler if reveal_spoiler => style.bg(palette().surface),
                EntityKind::Spoiler => {
                    hidden_spoiler = true;
                    style.fg(palette().muted)
                }
                EntityKind::Url | EntityKind::TextUrl(_) => style
                    .fg(palette().incoming)
                    .add_modifier(Modifier::UNDERLINED),
                EntityKind::Mention => style.fg(palette().accent),
                EntityKind::BotCommand => style.fg(palette().accent).add_modifier(Modifier::BOLD),
                EntityKind::Hashtag => style.fg(palette().incoming).add_modifier(Modifier::BOLD),
                EntityKind::Cashtag => style.fg(palette().accent).add_modifier(Modifier::ITALIC),
                EntityKind::EmailAddress => style
                    .fg(palette().accent)
                    .add_modifier(Modifier::UNDERLINED),
                EntityKind::PhoneNumber => {
                    style.fg(palette().incoming).add_modifier(Modifier::ITALIC)
                }
            };
        }
        if in_search {
            style = style
                .fg(palette().accent)
                .bg(palette().selected)
                .add_modifier(Modifier::BOLD);
        }
        if in_selection {
            style = palette().primary();
        }
        let span: &mut Span<'_> =
            if let Some(last) = spans.last_mut().filter(|span| span.style == style) {
                last
            } else {
                spans.push(Span::styled(String::new(), style));
                spans.last_mut().unwrap()
            };
        let text = span.content.to_mut();
        if hidden_spoiler && !in_selection {
            text.extend(std::iter::repeat_n('░', grapheme_width(grapheme)));
        } else {
            text.push_str(grapheme);
        }
    }
    spans.insert(0, Span::raw(prefix.to_owned()));
    Line::from(spans)
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn format_duration(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
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
        let width = grapheme_width(g) as u16;
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
        let line = if kind == Editor::Composer {
            selected_line(value, *row, selection::range(app, Source::Composer), "")
        } else {
            Line::raw(shown)
        };
        draw_line(frame, line_area, &line, Style::default());
        app.hit_targets
            .push((line_area, Target::Cursor(kind, row.end)));
        let mut x = area.x;
        for (i, g) in value[row.start..row.end].grapheme_indices(true) {
            let w = if masked { 1 } else { grapheme_width(g) as u16 };
            if x + w <= area.right() {
                app.hit_targets
                    .push((Rect::new(x, y, w, 1), Target::Cursor(kind, row.start + i)));
            }
            x += w;
        }
    }
    let overlay_editor = matches!(
        kind,
        Editor::AttachmentPath
            | Editor::AttachmentCaption
            | Editor::StickerSearch
            | Editor::QuickSwitch
            | Editor::Proxy(_)
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
        Editor::QuickSwitch => app.quick_switcher.is_some(),
        Editor::Proxy(index) => {
            app.proxy.open
                && app
                    .proxy
                    .form
                    .as_ref()
                    .is_some_and(|form| form.field == index)
        }
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
            text_width(prefix)
        };
        frame.set_cursor_position((
            area.x + (x as u16).min(area.width - 1),
            area.y + (cursor_row - start) as u16,
        ));
    }
}

fn shortcut_hint(app: &App, width: u16) -> &'static str {
    let (wide, narrow) = if app.proxy.open {
        if app.proxy.form.is_some() {
            (
                "Tab 切换字段 · ←→ 选协议 · Enter 保存并启用 · Esc 取消",
                "Tab 切换 · ←→ 协议 · Enter 保存 · Esc",
            )
        } else {
            (
                "↑↓ 选择 · Enter 开关 · a 新增 · e 导入 · t 测速 · d 删除 · Esc 关闭",
                "↑↓ 选择 · Enter 开关 · a 新增 · Esc",
            )
        }
    } else if app.instant_view.is_some() {
        (
            "j/k 滚动 · PgUp/PgDn 翻页 · Home/End 首尾 · o 浏览器 · Esc/q 关闭",
            "j/k 滚动 · o 浏览器 · Esc/q 关闭",
        )
    } else if app.inline_picker.is_some() {
        (
            "↑↓/jk 选择按钮 · Enter 触发 · Esc 关闭",
            "↑↓ 选择 · Enter 触发 · Esc",
        )
    } else if app.link_picker.is_some() {
        (
            "↑↓/jk 选择链接或 @提及 · Enter 打开 · Esc 关闭",
            "↑↓ 选链接 · Enter 打开 · Esc",
        )
    } else if app.show_topics {
        (
            "↑↓/jk 选择话题 · Enter 进入话题 · Esc 关闭",
            "↑↓ 选话题 · Enter 进入 · Esc",
        )
    } else if app.quick_switcher.is_some() {
        (
            "输入关键词筛选 · ↑↓ 选择 · Enter 打开会话 · Esc 关闭",
            "输入筛选 · Enter 打开 · Esc 关闭",
        )
    } else if app.show_settings {
        (
            "Tab 选择 · Enter 操作 · p 代理 · t 配色 · c 缓存 · F6 鼠标 · Esc 返回",
            "Tab 选择 · Enter 操作 · p 代理 · Esc",
        )
    } else if app.show_help {
        (
            "↑↓/jk 滚动 · PgUp/PgDn 翻页 · Esc 返回",
            "↑↓/jk 滚动 · Esc 返回",
        )
    } else if app.show_folders {
        (
            "↑↓/jk 选择 · Enter 切换分组 · Esc 返回",
            "↑↓ 选择 · Enter 切换 · Esc",
        )
    } else if app.show_chat_info && app.chat_info_modal {
        ("↑↓ 滚动 · 点击操作 · Esc 返回", "↑↓ 滚动 · Esc 返回")
    } else if app.auth.is_new_login() {
        (
            "Enter 确认重新登录 · F9 代理 · Esc 返回 · Ctrl+C 退出",
            "Enter 确认 · Esc 返回",
        )
    } else if app.auth.is_recovering() {
        (
            "填入旧本地密钥 · Enter 恢复 · F5 重新登录 · F9 代理 · Esc 修改 API",
            "Enter 恢复 · F5 重新登录",
        )
    } else if app.auth.setup.is_some() {
        (
            "Tab 切换字段 · Enter 保存 · F9 代理 · F2 官网 · F1 帮助 · Ctrl+C 退出",
            "Tab 切换 · Enter 保存 · F9 代理",
        )
    } else if app.auth.state != "authorizationStateReady" {
        (
            "Enter 继续 · F9 代理 · F3 修改 API · F4 设置 · Ctrl+C 退出",
            "Enter 继续 · F9 代理 · Ctrl+C 退出",
        )
    } else if app.attachments.is_some() {
        (
            "Space 选择 · Tab 切换 · F8 / Ctrl+Enter 发送 · Esc 返回",
            "Space 选择 · Tab 切换 · F8 发送 · Esc",
        )
    } else if app.sticker_picker {
        (
            "方向键选择 · 点击/Enter 发送 · [ ] 切换贴纸包 · / 搜索",
            "↑↓ 选择 · Enter 发送 · [ ] 切换",
        )
    } else if app.action_menu.is_some() || app.forward_message.is_some() {
        (
            "↑↓ 选择 · Enter 确认 · Esc 返回",
            "↑↓ 选择 · Enter 确认 · Esc",
        )
    } else if app.confirm_delete.is_some() {
        ("Enter / y 删除 · Esc / n 取消", "Enter/y 删除 · Esc/n 取消")
    } else if app.preview_message.is_some() {
        (
            "滚轮缩放 · 拖动平移 · 0 复位 · Esc 返回",
            "滚轮缩放 · 拖动平移 · Esc 返回",
        )
    } else if !app.multi_select.is_empty() {
        (
            "Space / 点击勾选消息 · 底部按钮批量操作 · Esc 退出多选",
            "Space 勾选 · Esc 退出多选",
        )
    } else if matches!(app.input_mode, InputMode::React(_)) && app.composer_focus {
        (
            "1–8 快捷回应 · Enter 提交表情 · Esc 取消",
            "1–8 回应 · Enter 提交 · Esc",
        )
    } else if app.input_mode != InputMode::Off && app.composer_focus {
        (
            "Enter 提交 · Alt+Enter 换行 · ↑↓ 移光标 · Ctrl+W 删词 · Esc 取消",
            "Enter 提交 · Alt+Enter 换行 · Esc",
        )
    } else if app.focus_messages {
        (
            "i 写消息 · r 回复 · Enter 操作 · / 搜索 · Ctrl+K 找会话 · Space 更多 · ? 帮助",
            "i 写消息 · r 回复 · / 搜索 · ? 帮助",
        )
    } else {
        (
            "↑↓ 选择 · Enter 打开 · Ctrl+K 找会话 · Tab 消息 · s 设置 · ? 帮助",
            "↑↓ 选择 · Enter 打开 · Ctrl+K",
        )
    };
    if width >= 68 && text_width(wide) <= usize::from(width) {
        wide
    } else if app.focus_messages && wide.starts_with("i 写消息") && width >= 50 {
        "i 写消息 · r 回复 · Enter 操作 · / 搜索 · ? 帮助"
    } else {
        narrow
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.media.begin_frame();
    app.visible_media.clear();
    app.visible_read_ids.clear();
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
    let mut header_spans = vec![
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
    ];
    if app.proxy.active_summary().is_some() && area.width >= 60 {
        header_spans.push(Span::styled(
            " · [代理]",
            Style::default()
                .fg(palette().accent)
                .add_modifier(Modifier::BOLD),
        ));
    }
    if !app.mouse_enabled {
        header_spans.push(Span::styled(
            if area.width < 60 {
                " [鼠标关]"
            } else {
                " · [鼠标关 · F6]"
            },
            Style::default()
                .fg(palette().error)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(header_spans)),
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
    let hint = shortcut_hint(app, area.width);
    let notice = app.notice.as_deref().or(app.media.last_error.as_deref());
    let auth_footer_error = app.auth.is_error && app.auth.state == "authorizationStateReady";
    let media_footer_error = app.notice.is_none() && app.media.last_error.is_some();
    let notice_is_error = app
        .notice
        .as_deref()
        .is_some_and(|text| app.notice_error || is_error_notice(text));
    let footer = if auth_footer_error {
        app.auth.message.as_str()
    } else {
        notice.unwrap_or(hint)
    };
    let footer_style = Style::default()
        .fg(
            if auth_footer_error || media_footer_error || notice_is_error {
                palette().error
            } else if notice.is_some() {
                palette().text
            } else {
                palette().muted
            },
        )
        .bg(palette().surface);
    let footer_line = Line::raw(text::ellipsize(footer, usize::from(layout[2].width)));
    draw_line(frame, layout[2], &footer_line, footer_style);

    if app.has_overlay() {
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
            if rows[1].width >= 48 {
                let tools = Layout::horizontal([
                    Constraint::Length(5),
                    Constraint::Length(5),
                    Constraint::Length(5),
                    Constraint::Min(8),
                    Constraint::Min(10),
                    Constraint::Min(8),
                    Constraint::Length(5),
                ])
                .split(rows[1]);
                for (rect, label, action) in [
                    (tools[0], "[‹]", Action::PreviewPrev),
                    (tools[1], "[−]", Action::ZoomOut),
                    (tools[2], "[+]", Action::ZoomIn),
                    (tools[3], "[复位]", Action::ZoomReset),
                    (tools[4], "[另存为]", Action::SaveFileAs),
                    (tools[5], "[打开]", Action::OpenExternal),
                    (tools[6], "[›]", Action::PreviewNext),
                ] {
                    button(frame, app, rect, label, action);
                }
            } else {
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
                    (tools[2], "[另存为]", Action::SaveFileAs),
                    (tools[3], "[打开]", Action::OpenExternal),
                ] {
                    button(frame, app, rect, label, action);
                }
            }
        } else {
            let tools =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(rows[1]);
            button(frame, app, tools[0], "[另存为 · E]", Action::SaveFileAs);
            button(frame, app, tools[1], "[系统打开 · o]", Action::OpenExternal);
        }
    }
    if app.show_folders {
        let inner = modal(frame, app, centered(layout[1], 44, 20), "Telegram 分组");
        let lists = app.store.chat_lists();
        let visible = usize::from(inner.height).max(1);
        app.modal_visible = visible;
        app.folder_cursor = app.folder_cursor.min(lists.len().saturating_sub(1));
        let max_scroll = lists.len().saturating_sub(visible);
        let mut scroll = usize::from(app.modal_scroll).min(max_scroll);
        if app.folder_cursor < scroll {
            scroll = app.folder_cursor;
        } else if app.folder_cursor >= scroll + visible {
            scroll = app.folder_cursor + 1 - visible;
        }
        app.modal_scroll = scroll as u16;
        let has_scroll = max_scroll > 0 && inner.width >= 4;
        let list_width = if has_scroll {
            inner.width.saturating_sub(1)
        } else {
            inner.width
        };
        for (row, (abs_index, (list, name))) in lists
            .iter()
            .enumerate()
            .skip(scroll)
            .take(visible)
            .enumerate()
        {
            let focused = abs_index == app.folder_cursor;
            let active = *list == app.store.selected_list;
            let pressed = app
                .mouse_press
                .is_some_and(|(_, target)| target == Target::Folder(*list));
            let row_rect = Rect::new(inner.x, inner.y + row as u16, list_width, 1);
            let row_style = if focused || pressed {
                palette().selection()
            } else {
                palette().surface_base()
            };
            let prefix = if focused { "› " } else { "  " };
            let dot = if active { "● " } else { "○ " };
            let name_text = crate::text::ellipsize(name, usize::from(list_width.saturating_sub(4)));
            let line = Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if focused {
                        palette().accent
                    } else {
                        palette().muted
                    }),
                ),
                Span::styled(
                    dot,
                    Style::default()
                        .fg(if active {
                            palette().accent
                        } else {
                            palette().muted
                        })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    name_text,
                    if active {
                        Style::default()
                            .fg(palette().accent)
                            .add_modifier(Modifier::BOLD)
                    } else if focused {
                        Style::default()
                            .fg(palette().text)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(palette().text)
                    },
                ),
            ]);
            frame.render_widget(Paragraph::new(line).style(row_style), row_rect);
            app.hit_targets.push((row_rect, Target::Folder(*list)));
        }
        if has_scroll {
            scrollbar(frame, app, inner, Pane::Modal, scroll, max_scroll);
        }
    }
    if app.quick_switcher.is_some() {
        quick_switcher_dialog(frame, app, layout[1]);
    }
    if app.show_chat_info && app.chat_info_modal {
        let inner = modal(frame, app, centered(layout[1], 56, 20), "会话详情");
        chat_info_body(frame, app, inner);
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
        let title = if app.forward_many && !app.multi_select.is_empty() {
            format!("转发 {} 条消息到…", app.multi_select.len())
        } else {
            "转发到…".into()
        };
        let inner = modal(frame, app, area, &title);
        let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
        chat_list(frame, app, rows[0], true, false);
        let prompt = app.batch_prompt().unwrap_or_else(|| "[确认转发]".into());
        button(frame, app, rows[1], &prompt, Action::Confirm);
    }
    if app.attachments.is_some() {
        attachment_dialog(frame, app, layout[1]);
    }
    if app.sticker_picker {
        sticker_dialog(frame, app, layout[1]);
        let column = chat_column(app, layout[1]);
        let ch = composer_height(app, column.width, column.height);
        let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(ch)]).split(column);
        frame.render_widget(Clear, rows[1]);
        composer(frame, app, rows[1]);
    }
    if app.confirm_delete.is_some() {
        let inner = modal(frame, app, centered(layout[1], 56, 9), "删除消息");
        let prompt = if app.confirm_delete_many && !app.multi_select.is_empty() {
            format!("确认删除选中的 {} 条消息？", app.multi_select.len())
        } else {
            "确认删除这条消息？".into()
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(prompt),
                muted(
                    app.batch_prompt()
                        .unwrap_or_else(|| "将请求为双方删除，删除后无法撤销。".into()),
                ),
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
    if app.show_topics {
        topics_dialog(frame, app, layout[1]);
    }
    if app.link_picker.is_some() {
        link_picker_dialog(frame, app, layout[1]);
    }
    if app.inline_picker.is_some() {
        inline_picker_dialog(frame, app, layout[1]);
    }
    if app.instant_view.is_some() {
        instant_view_dialog(frame, app, layout[1]);
    }
    if app.proxy.open {
        proxy_dialog(frame, app, area);
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
    let connection_error = !app.tdlib_connected && !app.status.starts_with("正在") && !setup;
    let message = if connection_error {
        &app.status
    } else {
        &app.auth.message
    };
    frame.render_widget(
        Paragraph::new(message.to_owned())
            .style(
                Style::default().fg(if app.auth.is_error || connection_error {
                    palette().error
                } else {
                    palette().accent
                }),
            )
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

fn chat_column(app: &App, area: Rect) -> Rect {
    if area.width < 78 {
        return area;
    }
    let chat_w = (area.width / 3).clamp(26, 36);
    if app.show_chat_info && area.width >= 120 {
        Layout::horizontal([
            Constraint::Length(chat_w),
            Constraint::Min(1),
            Constraint::Length(32),
        ])
        .split(area)[1]
    } else {
        Layout::horizontal([Constraint::Length(chat_w), Constraint::Min(1)]).split(area)[1]
    }
}

fn composer_height(app: &App, column_width: u16, column_height: u16) -> u16 {
    if !app.multi_select.is_empty() {
        return 3;
    }
    let reaction_rows =
        u16::from(matches!(app.input_mode, InputMode::React(_)) && column_height >= 10);
    let edit_width = usize::from(column_width.saturating_sub(3)).max(1);
    let min_rows = if column_height >= 16 { 2 } else { 1 };
    let rows = text::row_count(&app.draft, edit_width).max(min_rows);
    let max_h = column_height.saturating_sub(4).clamp(4, 8);
    rows.saturating_add(3 + usize::from(reaction_rows))
        .min(usize::from(max_h)) as u16
}

fn chat_page(frame: &mut Frame, app: &mut App, area: Rect) {
    let wide = area.width >= 78;
    let inline_info = app.show_chat_info && area.width >= 120 && app.store.active_chat.is_some();
    app.chat_info_modal = !inline_info;
    let show_chat = wide || !app.focus_messages || app.store.active_chat.is_none();
    let columns = if inline_info {
        Layout::horizontal([
            Constraint::Length((area.width / 3).clamp(26, 36)),
            Constraint::Min(1),
            Constraint::Length(32),
        ])
        .split(area)
    } else if wide {
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
        let ch = composer_height(app, message_area.width, message_area.height);
        let rows =
            Layout::vertical([Constraint::Min(3), Constraint::Length(ch)]).split(message_area);
        let base_title = app
            .store
            .active_chat
            .map(|id| app.store.chat_title(id))
            .unwrap_or_else(|| "消息".into());
        let active_chat_ref = app.store.active_chat.and_then(|id| app.store.chat(id));
        let is_forum = active_chat_ref.is_some_and(|c| c.is_forum);
        let active_topic_name = active_chat_ref
            .and_then(|c| c.active_topic.as_ref())
            .map(|(_, name)| name.clone());
        let subtitle = active_chat_ref.map(|c| c.subtitle.as_str()).unwrap_or("");
        let title = if app.show_search {
            format!(
                "搜索 · {}「{}」· {} 条",
                app.parity.filter.label(),
                app.search_query,
                app.store.search_results.len()
            )
        } else if let Some(thread) = &app.active_thread {
            format!("评论串 · {}", thread.title)
        } else if let Some(topic) = &active_topic_name {
            format!("{base_title} · #{topic}")
        } else if app.history_loading && rows[0].width >= 32 {
            format!("{base_title} · 加载中…")
        } else if !subtitle.is_empty() && rows[0].width >= 52 {
            format!("{base_title} · {subtitle}")
        } else {
            base_title
        };
        let show_topic_btn = rows[0].width >= 44 && is_forum && app.active_thread.is_none();
        let show_info_btn = rows[0].width >= 38 && app.store.active_chat.is_some();
        let show_search_btn = rows[0].width >= 26;
        let control_width = 6
            + u16::from(show_search_btn) * 6
            + u16::from(show_info_btn) * 6
            + u16::from(show_topic_btn) * 6;
        let focused_timeline = app.focus_messages && app.input_mode == InputMode::Off;
        let block = panel(
            crate::text::ellipsize(
                &title,
                usize::from(
                    rows[0]
                        .width
                        .saturating_sub(control_width + 3 + u16::from(focused_timeline) * 2),
                ),
            ),
            focused_timeline,
        );
        let inner = block.inner(rows[0]);
        frame.render_widget(block, rows[0]);
        app.hit_targets
            .push((rows[0], Target::Panel(Pane::Timeline)));

        let controls = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(if show_topic_btn { 6 } else { 0 }),
            Constraint::Length(if show_search_btn { 6 } else { 0 }),
            Constraint::Length(if show_info_btn { 6 } else { 0 }),
            Constraint::Length(6),
        ])
        .split(Rect::new(inner.x, rows[0].y, inner.width, 1));
        if show_topic_btn {
            button(
                frame,
                app,
                controls[1],
                if active_topic_name.is_some() {
                    "[全部]"
                } else {
                    "[话题]"
                },
                if active_topic_name.is_some() {
                    Action::ExitTopic
                } else {
                    Action::Topics
                },
            );
        }
        if show_search_btn {
            let (label, action) = if app.show_search {
                ("[返回]", Action::ClearSearch)
            } else if app.active_thread.is_some() {
                ("[返回]", Action::ExitThread)
            } else {
                ("[搜索]", Action::Search)
            };
            button(frame, app, controls[2], label, action);
        }
        if show_info_btn {
            button(frame, app, controls[3], "[详情]", Action::ChatInfo);
        }
        button(frame, app, controls[4], "[更多]", Action::ChatMenu);
        if app.store.active_chat.is_none() {
            empty(frame, inner, "选择一个会话", "在左侧选择后按 Enter 打开");
        } else {
            message_list(frame, app, inner);
        }
        if !app.multi_select.is_empty() {
            multi_select_bar(frame, app, rows[1]);
        } else {
            composer(frame, app, rows[1]);
        }
    }
    if inline_info {
        let block = panel("会话详情", false);
        let inner = block.inner(columns[2]);
        frame.render_widget(block, columns[2]);
        button(
            frame,
            app,
            Rect::new(columns[2].right().saturating_sub(5), columns[2].y, 3, 1),
            "[×]",
            Action::ChatInfo,
        );
        chat_info_body(frame, app, inner);
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
    let ids: Vec<_> = app.store.chat_ids().collect();
    let inner = if bordered {
        let parts = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        folder_tabs(frame, app, parts[0]);
        if !ids.is_empty() && app.store.exhausted_lists.contains(&app.store.selected_list) {
            frame.render_widget(
                Paragraph::new(format!("已加载全部 · {} 个会话", ids.len()))
                    .style(Style::default().fg(palette().muted))
                    .alignment(Alignment::Center),
                parts[2],
            );
        } else {
            button(frame, app, parts[2], "[更多会话]", Action::MoreChats);
        }
        parts[1]
    } else {
        inner
    };
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
            let is_selected = ids.get(selected) == Some(&chat.id);
            let is_active = Some(chat.id) == app.store.active_chat;
            let unread = if chat.unread > 0 {
                if chat.unread > 999 {
                    " 999+ ".to_owned()
                } else {
                    format!(" {} ", chat.unread)
                }
            } else {
                String::new()
            };
            let badge_width = text_width(unread.as_str()).min(width);
            let mention = if chat.mention_count > 0 && width >= badge_width + 10 {
                "@ "
            } else {
                ""
            };
            let pin_tag = if chat.pinned && width >= badge_width + 12 {
                "★ "
            } else {
                ""
            };
            let time_tag = if let Some(stamp) = chat.last_stamp
                && width >= badge_width + 18
            {
                format!("{} ", stamp.time())
            } else {
                String::new()
            };
            let meta_width =
                text_width(time_tag.as_str()) + text_width(pin_tag) + text_width(mention);
            let right_width = meta_width + badge_width;
            let active_prefix = if is_active { "· " } else { "" };
            let title_text = crate::text::ellipsize(
                &chat.title,
                width.saturating_sub(
                    right_width + usize::from(right_width > 0) + text_width(active_prefix),
                ),
            );
            let title_width = text_width(active_prefix) + text_width(title_text.as_str());
            let padding = width.saturating_sub(title_width + right_width);
            let title_style = if is_active {
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            };
            let meta_style = if is_selected {
                palette().muted_on_selection()
            } else {
                Style::default().fg(palette().muted)
            };
            let badge_style = if chat.muted {
                Style::default().fg(palette().muted).bg(palette().surface)
            } else {
                palette().primary()
            };
            let mut title_spans = Vec::with_capacity(7);
            if !active_prefix.is_empty() {
                title_spans.push(Span::styled(active_prefix, title_style));
            }
            title_spans.push(Span::styled(title_text, title_style));
            title_spans.push(Span::raw(" ".repeat(padding)));
            if !time_tag.is_empty() {
                title_spans.push(Span::styled(time_tag, meta_style));
            }
            if !pin_tag.is_empty() {
                title_spans.push(Span::styled(pin_tag, Style::default().fg(palette().accent)));
            }
            if !mention.is_empty() {
                title_spans.push(Span::styled(
                    mention,
                    Style::default()
                        .fg(palette().accent)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            title_spans.push(Span::styled(unread, badge_style));
            let preview_style = if is_selected {
                palette().muted_on_selection()
            } else {
                Style::default().fg(palette().muted)
            };
            let local_draft = app.local_draft_preview(chat.id);
            let draft_text = local_draft.or_else(|| {
                (!chat.draft_preview.is_empty()).then_some(chat.draft_preview.as_str())
            });
            let preview_line = if !chat.typing.is_empty() {
                Line::styled(
                    excerpt(&chat.typing, width),
                    Style::default().fg(palette().accent),
                )
            } else if let Some(draft) = draft_text {
                let tag = "[草稿] ";
                let tag_w = text_width(tag);
                Line::from(vec![
                    Span::styled(tag, Style::default().fg(palette().error)),
                    Span::styled(excerpt(draft, width.saturating_sub(tag_w)), preview_style),
                ])
            } else {
                let preview = if chat.preview.is_empty() {
                    "暂无消息".to_owned()
                } else {
                    excerpt(&chat.preview, width)
                };
                Line::styled(preview, preview_style)
            };
            ListItem::new(vec![Line::from(title_spans), preview_line]).style(if is_selected {
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

fn message_body(message: &Message) -> &str {
    match message.media.as_ref().map(|media| media.kind) {
        Some(MediaKind::Photo) => message
            .text
            .strip_prefix("[图片] ")
            .unwrap_or(&message.text),
        Some(MediaKind::Sticker) if message.text.starts_with("[贴纸]") => "",
        _ => &message.text,
    }
}

fn album_captions(group: &[Message]) -> impl Iterator<Item = (&Message, &str)> {
    let mut seen = std::collections::HashSet::new();
    group.iter().filter_map(move |message| {
        let text = match message.media.as_ref().map(|media| media.kind) {
            Some(MediaKind::Video) => message
                .text
                .strip_prefix("[视频] ")
                .unwrap_or(&message.text),
            Some(MediaKind::Document)
                if message.info.media_meta.as_ref().is_some_and(|meta| {
                    message.text.strip_prefix("[文件] ") == Some(meta.file_name.as_str())
                        && message.info.entities.is_empty()
                }) =>
            {
                ""
            }
            Some(MediaKind::Document) => message
                .text
                .strip_prefix("[文件] ")
                .unwrap_or(&message.text),
            _ => message_body(message),
        };
        (!text.is_empty() && seen.insert(text)).then_some((message, text))
    })
}

fn inline_media(message: &Message) -> Option<&MediaRef> {
    message
        .media
        .as_ref()
        .filter(|media| matches!(media.kind, MediaKind::Photo | MediaKind::Sticker))
}

fn has_media_card(message: &Message) -> bool {
    message.info.media_meta.is_some()
        && message.media.as_ref().is_some_and(|media| {
            matches!(
                media.kind,
                MediaKind::Document | MediaKind::Video | MediaKind::Voice
            )
        })
}

fn message_body_rows<'a>(
    message: &'a Message,
    body: &str,
    width: usize,
    mut visit: impl FnMut(Row, Option<&'a EntityKind>, bool),
) {
    if body.is_empty() {
        return;
    }
    let body_offset = message.text.len() - body.len();
    let blocks = message
        .info
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, EntityKind::Pre(_) | EntityKind::BlockQuote));
    let mut start = body_offset;
    for logical_line in body
        .split_inclusive('\n')
        .chain(body.ends_with('\n').then_some(""))
    {
        let content = logical_line.strip_suffix('\n').unwrap_or(logical_line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        let end = start + content.len();
        let block = blocks.clone().find(|entity| {
            entity.start <= end && entity.end > start && (entity.start < end || content.is_empty())
        });
        if let Some(entity) = block
            && matches!(&entity.kind, EntityKind::Pre(language) if !language.is_empty())
            && entity.start >= start
        {
            visit(Row { start, end: start }, Some(&entity.kind), true);
        }
        let line_width = width.saturating_sub(2 * usize::from(block.is_some()));
        text::scan_rows(content, line_width, |row| {
            visit(
                Row {
                    start: start + row.start,
                    end: start + row.end,
                },
                block.map(|entity| &entity.kind),
                false,
            );
        });
        start += logical_line.len();
    }
}

fn message_body_height(message: &Message, body: &str, width: usize) -> usize {
    if body.is_empty() {
        0
    } else if !message
        .info
        .entities
        .iter()
        .any(|entity| matches!(entity.kind, EntityKind::Pre(_) | EntityKind::BlockQuote))
    {
        text::row_count(body, width)
    } else {
        let mut height = 0;
        message_body_rows(message, body, width, |_, _, _| height += 1);
        height
    }
}

struct TimelineLines {
    visible: Vec<Line<'static>>,
    next: usize,
    viewport: std::ops::Range<usize>,
}

impl TimelineLines {
    fn push(&mut self, line: Line<'static>) {
        if self.viewport.contains(&self.next) {
            self.visible.push(line);
        }
        self.next += 1;
    }

    fn blank(&mut self, height: usize) {
        let begin = self.next.max(self.viewport.start);
        let end = (self.next + height).min(self.viewport.end);
        self.visible.extend((begin..end).map(|_| Line::default()));
        self.next += height;
    }
}

fn append_message_body(
    lines: &mut TimelineLines,
    message: &Message,
    body: &str,
    app: &App,
    area: Rect,
    targets: &mut Vec<(Rect, Target)>,
) {
    let width = area.width.saturating_sub(3).max(1);
    let source = Source::Message(message.id);
    let selection = selection::range(app, source);
    let search =
        (app.show_search && !app.search_query.is_empty()).then_some(app.search_query.as_str());
    message_body_rows(
        message,
        body,
        usize::from(width),
        |row, block, language_header| {
            if !lines.viewport.contains(&lines.next) {
                lines.next += 1;
                return;
            }
            let y = area.y + (lines.next - lines.viewport.start) as u16;
            let quote_style = Style::default().fg(palette().muted);
            let code_style = Style::default()
                .fg(palette().incoming)
                .bg(palette().surface);
            let mut line =
                if let Some(EntityKind::Pre(language)) = block.filter(|_| language_header) {
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled("┃ ", code_style),
                        Span::styled(
                            text::ellipsize(language, usize::from(width.saturating_sub(2))),
                            code_style.add_modifier(Modifier::BOLD),
                        ),
                    ])
                } else {
                    let mut line = styled_message_line(
                        &message.text,
                        row,
                        selection.clone(),
                        "  ",
                        &message.info.entities,
                        app.selected_message == Some(message.id),
                        search,
                    );
                    if block.is_some() {
                        let style = if matches!(block, Some(EntityKind::Pre(_))) {
                            code_style
                        } else {
                            quote_style
                        };
                        line.spans.insert(1, Span::styled("┃ ", style));
                    }
                    let inset = 2 * u16::from(block.is_some());
                    let row_area = Rect::new(area.x + 2 + inset, y, width.saturating_sub(inset), 1);
                    targets.extend(point_targets(&message.text, row, source, row_area));
                    line
                };
            if matches!(block, Some(EntityKind::Pre(_))) {
                let used = line
                    .spans
                    .iter()
                    .map(|span| text_width(&span.content))
                    .sum();
                line.spans.push(Span::styled(
                    " ".repeat((usize::from(width) + 2).saturating_sub(used)),
                    code_style,
                ));
            }
            lines.push(line);
        },
    );
}

fn inline_label(button: &crate::store::InlineButton, width: u16) -> String {
    let label = button.text.replace(['\r', '\n', '\t'], " ");
    let width = usize::from(width);
    if matches!(button.kind, crate::store::InlineButtonKind::Url(_)) {
        let suffix = if width >= 3 { " ↗" } else { "↗" };
        return format!(
            "{}{suffix}",
            text::ellipsize(&label, width.saturating_sub(text_width(suffix)))
        );
    }
    text::ellipsize(&label, width)
}

fn album_strip(
    group: &[Message],
    current: usize,
    checked: &[i64],
    width: u16,
    row: Option<Rect>,
    targets: &mut Vec<(Rect, Target)>,
) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    let item_width = group.len().ilog10() as u16 + 3;
    let hint = if width >= 45 {
        "[ / ] 切图 · o 大图 · E 保存"
    } else if width >= 26 {
        "[ / ] 切图"
    } else {
        "[ / ]"
    };
    let max_items =
        usize::from(width.saturating_sub(text_width(hint) as u16 + 1) / (item_width + 1)).max(1);
    let start = current.saturating_add(1).saturating_sub(max_items);
    let end = (start + max_items).min(group.len());
    let mut x = 0;
    for (index, message) in group.iter().enumerate().take(end).skip(start) {
        let label = format!("{:^w$}", index + 1, w = usize::from(item_width));
        let mut style = if index == current {
            palette().primary()
        } else {
            palette().surface_base().fg(palette().incoming)
        };
        if checked.contains(&message.id) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        spans.push(Span::styled(label, style));
        if let Some(row) = row {
            targets.push((
                Rect::new(row.x + x, row.y, item_width, 1),
                Target::AlbumItem(message.id),
            ));
        }
        spans.push(Span::raw(" "));
        x += item_width + 1;
    }
    let label = if start > 0 || end < group.len() {
        format!("… {hint}")
    } else {
        hint.to_owned()
    };
    spans.push(Span::styled(
        text::ellipsize(&label, usize::from(width.saturating_sub(x))),
        Style::default().fg(palette().muted),
    ));
    Line::from(spans)
}

fn message_list(frame: &mut Frame, app: &mut App, area: Rect) {
    app.visible_media.clear();
    let pinned_banner =
        (!app.show_search && app.collapsed_pin_chat != app.store.active_chat && area.height >= 7)
            .then(|| {
                app.store
                    .active_chat
                    .and_then(|id| app.store.chat(id))
                    .and_then(|chat| chat.pinned_message.clone())
            })
            .flatten();
    let area = if let Some(pin) = pinned_banner {
        let banner_rect = Rect::new(area.x, area.y, area.width, 1);
        frame
            .buffer_mut()
            .set_style(banner_rect, palette().surface_base());
        let dismiss_w = 3u16.min(banner_rect.width);
        let text_w = banner_rect.width.saturating_sub(dismiss_w + 1);
        let prefix = "│ 置顶：";
        let prefix_w = text_width(prefix);
        let body = crate::text::ellipsize(
            &excerpt(&pin.excerpt, usize::from(text_w).saturating_sub(prefix_w)),
            usize::from(text_w).saturating_sub(prefix_w),
        );
        let click_rect = Rect::new(banner_rect.x, banner_rect.y, text_w, 1);
        let banner_line = Line::from(vec![
            Span::styled(
                "│ ",
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "置顶：",
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(body, Style::default().fg(palette().text)),
        ]);
        draw_line(frame, click_rect, &banner_line, palette().surface_base());
        app.hit_targets
            .push((click_rect, Target::PinnedBanner(pin.id)));
        button(
            frame,
            app,
            Rect::new(
                banner_rect.right().saturating_sub(dismiss_w),
                banner_rect.y,
                dismiss_w,
                1,
            ),
            "[×]",
            Action::DismissPinBanner,
        );
        Rect::new(area.x, area.y + 1, area.width, area.height - 1)
    } else {
        area
    };
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
    let unread_count = app
        .store
        .active_chat
        .and_then(|id| app.store.chat(id))
        .map_or(0, |chat| chat.unread);
    let read_inbox = app
        .store
        .active_chat
        .and_then(|id| app.store.chat(id))
        .map_or(0, |chat| chat.read_inbox);
    let source = if app.show_search {
        &app.store.search_results
    } else {
        &app.store.messages
    };
    let unread_divider_id = if app.store.message_scope.is_none()
        && !app.show_search
        && unread_count > 0
        && (read_inbox > 0 || !app.demo)
    {
        source
            .iter()
            .find(|message| !message.outgoing && message.id > read_inbox)
            .map(|message| message.id)
    } else {
        None
    };
    app.timeline_rows.clear();
    let mut date_headers = std::collections::HashSet::new();
    let mut previous_date: Option<crate::calendar::Stamp> = None;
    let mut selected_row = None;
    let mut unread_row = None;
    let mut anchor_row = None;
    for group in source.chunk_by(Message::same_album) {
        let first = &group[0];
        let selected = group
            .iter()
            .find(|message| Some(message.id) == app.selected_message);
        if selected.is_some() {
            selected_row = Some(first.id);
        }
        if group
            .iter()
            .any(|message| Some(message.id) == unread_divider_id)
        {
            unread_row = Some(first.id);
        }
        if let Some((anchor, _)) = app.timeline_anchor
            && group.iter().any(|message| message.id == anchor)
        {
            anchor_row = Some(first.id);
        }
        if let Some(stamp) = first.info.stamp {
            if previous_date.is_none_or(|previous| !previous.same_date(stamp)) {
                date_headers.insert(first.id);
            }
            previous_date = Some(stamp);
        }
        let message = selected.unwrap_or(first);
        let base_height = |body_height| {
            body_height
                + 2
                + usize::from(message.info.reply.is_some())
                + usize::from(message.info.forwarded_from.is_some())
                + inline_media(message).map_or(0, |_| media_height)
        };
        let height = if group.len() > 1 {
            let captions = album_captions(group)
                .map(|(caption, body)| message_body_height(caption, body, usize::from(width)))
                .sum();
            base_height(captions) + 1
        } else {
            *app.timeline_heights.entry(message.id).or_insert_with(|| {
                base_height(message_body_height(
                    message,
                    message_body(message),
                    usize::from(width),
                ))
            })
        };
        let details = usize::from(has_media_card(message))
            + message
                .info
                .link_preview
                .as_ref()
                .map_or(0, |lp| lp.row_count())
            + message.info.inline_keyboard.len()
            + usize::from(message.info.can_get_thread && app.active_thread.is_none())
            + usize::from(!message.info.reactions.is_empty());
        let dividers = usize::from(date_headers.contains(&first.id))
            + usize::from(unread_row == Some(first.id));
        app.timeline_rows
            .push((first.id, height + details + dividers));
    }
    let total: usize = app.timeline_rows.iter().map(|(_, height)| height).sum();
    app.timeline_max = total.saturating_sub(usize::from(area.height));
    let mut top = app.timeline_max;
    if let Some((anchor, offset)) = app.timeline_anchor {
        let anchor = anchor_row.unwrap_or(anchor);
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
            if Some(id) == selected_row {
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
    let mut read_ids = Vec::new();
    let measure_reads = !app.demo && !app.show_search && !app.has_overlay();
    let navigation_bar =
        app.quote_back.is_some() || top < app.timeline_max || app.store.context_window;
    let readable_end = top + usize::from(area.height).saturating_sub(usize::from(navigation_bar));
    for (group, &(_, height)) in source.chunk_by(Message::same_album).zip(&app.timeline_rows) {
        if before + height > top && before < top + usize::from(area.height) {
            let first = &group[0];
            let active = group
                .iter()
                .position(|message| Some(message.id) == app.selected_message)
                .unwrap_or(0);
            let message = &group[active];
            let is_album = group.len() > 1;
            let label = app.store.sender_label(first);
            let tag = message
                .media
                .as_ref()
                .map(|media| match media.kind {
                    MediaKind::Photo => " · 图片",
                    MediaKind::Sticker => " · 贴纸",
                    MediaKind::Document => " · 文件",
                    MediaKind::Video => " · 视频",
                    MediaKind::Voice => " · 语音",
                })
                .unwrap_or("");
            let date_div = usize::from(date_headers.contains(&first.id));
            let unread_div = usize::from(unread_row == Some(first.id));
            let divider = date_div + unread_div;
            if measure_reads && before + divider < readable_end && before + height - 1 > top {
                read_ids.extend(
                    group
                        .iter()
                        .filter(|item| !item.outgoing && item.id > read_inbox)
                        .map(|item| item.id),
                );
            }
            let is_selected = Some(message.id) == app.selected_message;
            let multi_active = !app.multi_select.is_empty();
            let multi_checked = multi_active && app.multi_select.contains(&message.id);
            let failed = matches!(message.info.sending, crate::store::Sending::Failed { .. });
            let controls_width = if is_selected {
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
            let compact_status = width < 36;
            let raw_status = message.status_formatted(
                app.store
                    .active_chat
                    .and_then(|id| app.store.chat(id))
                    .map_or(0, |chat| chat.read_outbox),
                compact_status,
            );
            let max_status = usize::from(width.saturating_sub(controls_width + 10));
            let status = crate::text::ellipsize(&raw_status, max_status);
            let status_width = text_width(status.as_str()) + usize::from(!status.is_empty()) * 3;
            let mut lines = TimelineLines {
                visible: Vec::new(),
                next: before,
                viewport: top..top + usize::from(area.height),
            };
            if date_div > 0 {
                let date = first.info.stamp.expect("date header").date();
                let date_line = if width >= 18 {
                    Line::from(vec![
                        Span::styled("── ", Style::default().fg(palette().border)),
                        Span::styled(date, Style::default().fg(palette().muted)),
                        Span::styled(" ──", Style::default().fg(palette().border)),
                    ])
                } else {
                    Line::styled(date, Style::default().fg(palette().muted))
                };
                lines.push(date_line.alignment(Alignment::Center));
            }
            if unread_div > 0 {
                let unread_label = format!("── {unread_count} 条未读消息 ──");
                lines.push(
                    Line::styled(
                        crate::text::ellipsize(&unread_label, usize::from(width)),
                        Style::default()
                            .fg(palette().accent)
                            .add_modifier(Modifier::BOLD),
                    )
                    .alignment(Alignment::Center),
                );
            }
            let status_style = if failed {
                Style::default().fg(palette().error)
            } else if is_selected {
                palette().muted_on_selection()
            } else {
                Style::default().fg(palette().muted)
            };
            let check_width = usize::from(multi_active) * 4;
            let mut header_spans = Vec::with_capacity(4);
            if multi_active {
                header_spans.push(Span::styled(
                    if multi_checked { "[✓] " } else { "[ ] " },
                    Style::default()
                        .fg(if multi_checked {
                            palette().accent
                        } else {
                            palette().muted
                        })
                        .add_modifier(if multi_checked {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ));
            }
            let album_tag = if is_album {
                format!("[相册 {}/{}] ", active + 1, group.len())
            } else {
                String::new()
            };
            let tag = if is_album { "" } else { tag };
            header_spans.push(Span::styled(
                format!(
                    "{}{}",
                    if is_selected { "› " } else { "  " },
                    crate::text::ellipsize(
                        &format!("{album_tag}{label}{tag}"),
                        usize::from(width.saturating_sub(controls_width))
                            .saturating_sub(status_width + check_width)
                    )
                ),
                Style::default()
                    .fg(if message.outgoing {
                        palette().accent
                    } else {
                        palette().incoming
                    })
                    .add_modifier(Modifier::BOLD),
            ));
            header_spans.push(Span::styled(
                if status.is_empty() {
                    String::new()
                } else {
                    format!(" · {status}")
                },
                status_style,
            ));
            lines.push(Line::from(header_spans));
            let forward_rows = usize::from(message.info.forwarded_from.is_some());
            if let Some(origin) = &message.info.forwarded_from {
                let name = app.store.forward_label(origin);
                let label = format!("  ↳ 转发自 {}", name.replace(['\r', '\n'], " "));
                lines.push(Line::styled(
                    crate::text::ellipsize(&label, usize::from(width)),
                    if is_selected {
                        palette().muted_on_selection()
                    } else {
                        Style::default().fg(palette().muted)
                    },
                ));
            }
            if let Some(reply) = &message.info.reply {
                let referenced = (Some(reply.chat_id) == app.store.active_chat)
                    .then(|| app.store.messages.iter().find(|m| m.id == reply.message_id))
                    .flatten();
                let excerpt_text = if !reply.excerpt.is_empty() {
                    reply.excerpt.as_str()
                } else {
                    referenced.map_or("原消息未加载", |m| m.text.as_str())
                };
                let reply_label = referenced
                    .map(|m| app.store.sender_label(m))
                    .unwrap_or_else(|| "引用".into());
                // "  │ " uses 4 columns while `width` only subtracts 2 left + 1 scrollbar.
                let quote_width = usize::from(width).saturating_sub(2);
                let label_prefix = format!("{reply_label}：");
                let label_w = text_width(label_prefix.as_str());
                let quote_muted = if is_selected {
                    palette().muted_on_selection()
                } else {
                    Style::default().fg(palette().muted)
                };
                let bar_color = if referenced.is_some() && !app.show_search {
                    palette().accent
                } else {
                    palette().border
                };
                let quote_line = if label_w + 2 <= quote_width {
                    Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(bar_color)),
                        Span::styled(
                            label_prefix,
                            Style::default()
                                .fg(palette().text)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            crate::text::ellipsize(
                                &excerpt_text.replace(['\n', '\r'], " "),
                                quote_width.saturating_sub(label_w),
                            ),
                            quote_muted,
                        ),
                    ])
                } else {
                    Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(bar_color)),
                        Span::styled(
                            crate::text::ellipsize(
                                &format!(
                                    "{label_prefix}{}",
                                    excerpt_text.replace(['\n', '\r'], " ")
                                ),
                                quote_width,
                            ),
                            quote_muted,
                        ),
                    ])
                };
                lines.push(quote_line);
                let quote_row = before + divider + 1 + forward_rows;
                if Some(reply.chat_id) == app.store.active_chat
                    && reply.message_id != 0
                    && !app.show_search
                    && quote_row >= top
                    && quote_row < top + usize::from(area.height)
                {
                    text_targets.push((
                        Rect::new(area.x + 2, area.y + (quote_row - top) as u16, width, 1),
                        if app.multi_select.is_empty() {
                            Target::ReferencedMessage(reply.message_id)
                        } else {
                            Target::Quote(message.id, reply.message_id)
                        },
                    ));
                }
            }
            if !is_album {
                append_message_body(
                    &mut lines,
                    message,
                    message_body(message),
                    app,
                    area,
                    &mut text_targets,
                );
            }
            let inline = inline_media(message);
            let media_start = lines.next;
            if inline.is_some() {
                lines.blank(media_height);
            }
            if let (Some(media), Some(meta)) = (&message.media, &message.info.media_meta)
                && matches!(
                    media.kind,
                    MediaKind::Document | MediaKind::Video | MediaKind::Voice
                )
            {
                let icon = match media.kind {
                    MediaKind::Voice => "▸ 语音",
                    MediaKind::Video => "▸ 视频",
                    _ => "▸ 文件",
                };
                let mut detail_parts = Vec::new();
                if !meta.file_name.is_empty() {
                    detail_parts.push(meta.file_name.clone());
                }
                if meta.duration_secs > 0 {
                    detail_parts.push(format_duration(meta.duration_secs));
                }
                if meta.size_bytes > 0 {
                    detail_parts.push(format_bytes(meta.size_bytes));
                }
                let progress = if media.path.is_some() {
                    "已下载 · o 打开".into()
                } else if app.cancelling_large_file(media.file_id) {
                    "正在取消下载…".into()
                } else if app.failed_files.contains(&media.file_id) {
                    "下载失败 · 按 v 重试".into()
                } else if meta.downloading && meta.size_bytes > 0 {
                    let pct =
                        ((meta.downloaded_bytes.saturating_mul(100)) / meta.size_bytes).min(100);
                    let filled = (pct as usize * 8 / 100).min(8);
                    format!(
                        "{}{} {pct}% · v 取消",
                        "█".repeat(filled),
                        "░".repeat(8 - filled)
                    )
                } else if meta.downloading {
                    "正在下载… · v 取消".into()
                } else {
                    "按 v 下载".into()
                };
                detail_parts.push(progress);
                let card = format!("  {icon} · {}", detail_parts.join(" · "));
                let row = lines.next;
                if row >= top && row < readable_end {
                    text_targets.push((
                        Rect::new(area.x, area.y + (row - top) as u16, width, 1),
                        Target::Media(message.id),
                    ));
                }
                lines.push(Line::styled(
                    crate::text::ellipsize(&card, usize::from(width) + 2),
                    Style::default()
                        .fg(palette().incoming)
                        .bg(palette().surface),
                ));
            }
            if is_album {
                let rect = lines
                    .viewport
                    .contains(&lines.next)
                    .then(|| Rect::new(area.x + 2, area.y + (lines.next - top) as u16, width, 1));
                lines.push(album_strip(
                    group,
                    active,
                    &app.multi_select,
                    width,
                    rect,
                    &mut text_targets,
                ));
                for (caption, body) in album_captions(group) {
                    append_message_body(&mut lines, caption, body, app, area, &mut text_targets);
                }
            }
            if let Some(lp) = &message.info.link_preview {
                let lp_width = usize::from(width.saturating_sub(2));
                let head = match (lp.site_name.is_empty(), lp.title.is_empty()) {
                    (false, false) => format!("{} · {}", lp.site_name, lp.title),
                    (false, true) => lp.site_name.clone(),
                    (true, false) => lp.title.clone(),
                    (true, true) => String::new(),
                };
                let accent = Style::default().fg(palette().accent);
                let link = Style::default()
                    .fg(palette().incoming)
                    .add_modifier(Modifier::UNDERLINED);
                let card_line = |label: &str, style| {
                    Line::from(vec![
                        Span::styled("  ┃ ", accent),
                        Span::styled(
                            text::ellipsize(&label.replace(['\r', '\n'], " "), lp_width),
                            style,
                        ),
                    ])
                };
                if !head.is_empty() {
                    lines.push(card_line(
                        &head,
                        Style::default()
                            .fg(palette().text)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                if !lp.description.is_empty() {
                    lines.push(card_line(
                        &lp.description,
                        if is_selected {
                            palette().muted_on_selection()
                        } else {
                            Style::default().fg(palette().muted)
                        },
                    ));
                }
                for (show, label, style, target) in [
                    (
                        lp.has_instant_view(),
                        "[阅读全文]".into(),
                        accent.bg(palette().surface),
                        Target::InstantViewLink(message.id),
                    ),
                    (
                        !lp.url.is_empty(),
                        format!("🔗 {}", lp.url),
                        link,
                        Target::PreviewLink(message.id),
                    ),
                ] {
                    if show {
                        if lines.viewport.contains(&lines.next) {
                            let label_width = text_width(&text::ellipsize(&label, lp_width)) as u16;
                            text_targets.push((
                                Rect::new(
                                    area.x + 4,
                                    area.y + (lines.next - top) as u16,
                                    label_width,
                                    1,
                                ),
                                target,
                            ));
                        }
                        lines.push(card_line(&label, style));
                    }
                }
            }
            let kb_width = message
                .info
                .inline_keyboard
                .iter()
                .map(|row| {
                    let max_cell = row
                        .iter()
                        .map(|button| {
                            let extra =
                                if matches!(button.kind, crate::store::InlineButtonKind::Url(_)) {
                                    2
                                } else {
                                    0
                                };
                            text_width(&button.text.replace(['\r', '\n', '\t'], " ")) + extra + 4
                        })
                        .max()
                        .unwrap_or(0);
                    let n = row.len().max(1);
                    (max_cell * n + n - 1).min(usize::from(width)) as u16
                })
                .max()
                .unwrap_or(0)
                .max(28.min(width))
                .min(width);
            for (r_idx, row) in message.info.inline_keyboard.iter().enumerate() {
                let mut spans = vec![Span::raw("  ")];
                let mut x = area.x + 2;
                let n = row.len().max(1) as u16;
                let available = kb_width.saturating_sub(n - 1);
                for (c_idx, button) in row.iter().enumerate() {
                    let w = available / n + u16::from((c_idx as u16) < available % n);
                    if w == 0 {
                        break;
                    }
                    let target = Target::InlineButton(message.id, r_idx, c_idx);
                    let pressed = app
                        .mouse_press
                        .is_some_and(|(_, pressed)| pressed == target);
                    let style = if pressed {
                        palette().primary()
                    } else {
                        palette()
                            .surface_base()
                            .fg(palette().accent)
                            .add_modifier(Modifier::BOLD)
                    };
                    if w >= 6 {
                        let inner_w = w - 2;
                        let label = inline_label(button, inner_w.saturating_sub(2));
                        let spare = usize::from(inner_w).saturating_sub(text_width(&label));
                        let left_pad = (spare / 2).max(1);
                        let right_pad = spare.saturating_sub(left_pad);
                        let inner =
                            format!("{}{}{}", " ".repeat(left_pad), label, " ".repeat(right_pad));
                        let edge_style = if pressed {
                            style
                        } else {
                            palette().surface_base().fg(palette().border)
                        };
                        spans.push(Span::styled("[", edge_style));
                        spans.push(Span::styled(inner, style));
                        spans.push(Span::styled("]", edge_style));
                    } else {
                        let label = inline_label(button, w);
                        let spare = usize::from(w).saturating_sub(text_width(&label));
                        let label = format!(
                            "{}{}{}",
                            " ".repeat(spare / 2),
                            label,
                            " ".repeat(spare - spare / 2)
                        );
                        spans.push(Span::styled(label, style));
                    }
                    if lines.viewport.contains(&lines.next) {
                        text_targets.push((
                            Rect::new(x, area.y + (lines.next - top) as u16, w, 1),
                            target,
                        ));
                    }
                    x += w + 1;
                    spans.push(Span::raw(" "));
                }
                lines.push(Line::from(spans));
            }
            if message.info.can_get_thread && app.active_thread.is_none() {
                let c_row = lines.next;
                let pill = if message.info.reply_count > 0 {
                    format!(" 💬 {} 条评论 › ", message.info.reply_count)
                } else {
                    " 💬 评论 › ".to_owned()
                };
                let cw = text_width(pill.as_str()) as u16;
                if c_row >= top && c_row < top + usize::from(area.height) {
                    text_targets.push((
                        Rect::new(area.x + 2, area.y + (c_row - top) as u16, cw.min(width), 1),
                        Target::CommentThread(message.id),
                    ));
                }
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        pill,
                        Style::default()
                            .fg(palette().incoming)
                            .bg(palette().surface),
                    ),
                ]));
            }
            if !message.info.reactions.is_empty() {
                let react_row = lines.next;
                let mut r_spans = vec![Span::raw("  ")];
                let mut rx = area.x + 2;
                let max_x = area.x + 2 + width;
                for (idx, r) in message.info.reactions.iter().enumerate() {
                    let pill = format!(" {} {} ", r.emoji, r.count);
                    let pw = text_width(pill.as_str()) as u16;
                    if rx + pw > max_x {
                        break;
                    }
                    let style = if r.chosen {
                        palette()
                            .selection()
                            .fg(palette().accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(palette().muted).bg(palette().surface)
                    };
                    r_spans.push(Span::styled(pill, style));
                    if react_row >= top && react_row < top + usize::from(area.height) {
                        text_targets.push((
                            Rect::new(rx, area.y + (react_row - top) as u16, pw, 1),
                            Target::Reaction(message.id, idx),
                        ));
                    }
                    rx = rx.saturating_add(pw + 1);
                    r_spans.push(Span::raw(" "));
                }
                lines.push(Line::from(r_spans));
            }
            lines.push(Line::from(""));
            let skip = top.saturating_sub(before);
            let y = area.y + before.saturating_sub(top) as u16;
            let h = (height - skip).min(usize::from(area.bottom() - y)) as u16;
            debug_assert_eq!(lines.next, before + height);
            let has_trailing_blank = skip + usize::from(h) == height;
            visible.push((
                message.id,
                Rect::new(area.x, y, area.width.saturating_sub(1), h),
                lines.visible,
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
                has_trailing_blank,
            ));
        }
        before += height;
    }
    for (id, rect, mut lines, inline, header, retryable, has_trailing_blank) in visible {
        let selected = Some(id) == app.selected_message;
        if selected && has_trailing_blank && rect.height > 1 {
            lines.pop();
        }
        for (row, line) in lines.iter().enumerate() {
            let row = Rect::new(rect.x, rect.y + row as u16, rect.width, 1);
            draw_line(
                frame,
                row,
                line,
                if selected {
                    palette().selection()
                } else {
                    Style::default()
                },
            );
        }
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
    app.visible_read_ids = read_ids;
    app.hit_targets.extend(text_targets);
    scrollbar(frame, app, area, Pane::Timeline, top, app.timeline_max);
    if app.quote_back.is_some() && area.height > 0 {
        let label = " 返回引用前位置 · b ";
        let avail = area.width.saturating_sub(1);
        let pill_w = (text_width(label) as u16).min(avail);
        let bar = Rect::new(
            area.x + avail.saturating_sub(pill_w),
            area.bottom() - 1,
            pill_w,
            1,
        );
        let style = Style::default()
            .fg(palette().accent)
            .bg(palette().surface)
            .add_modifier(Modifier::BOLD)
            .add_modifier(if palette().is_monochrome() {
                Modifier::REVERSED
            } else {
                Modifier::empty()
            });
        draw_line(
            frame,
            bar,
            &Line::raw(text::ellipsize(label, usize::from(pill_w))),
            style,
        );
        app.hit_targets
            .push((bar, Target::Command(Action::QuoteBack)));
    } else if (top < app.timeline_max || app.store.context_window) && area.height > 0 {
        let label = if app.pending_messages > 0 {
            format!(" ↓ {} 条新消息 · 回到底部 ", app.pending_messages)
        } else {
            " ↓ 回到底部 ".into()
        };
        let avail = area.width.saturating_sub(1);
        let pill_w = (text_width(&label) as u16).min(avail);
        let button_area = Rect::new(
            area.x + avail.saturating_sub(pill_w),
            area.bottom() - 1,
            pill_w,
            1,
        );
        let style = if app.pending_messages > 0 {
            palette().primary()
        } else {
            Style::default()
                .fg(palette().accent)
                .bg(palette().surface)
                .add_modifier(Modifier::BOLD)
                .add_modifier(if palette().is_monochrome() {
                    Modifier::REVERSED
                } else {
                    Modifier::empty()
                })
        };
        draw_line(
            frame,
            button_area,
            &Line::raw(text::ellipsize(&label, usize::from(pill_w))),
            style,
        );
        app.hit_targets
            .push((button_area, Target::Command(Action::Bottom)));
    }
}

fn multi_select_bar(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = format!("多选模式 · 已选 {} 条消息", app.multi_select.len());
    let block = panel(title, true);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }
    let cols = Layout::horizontal([
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
        Constraint::Percentage(25),
    ])
    .split(Rect::new(inner.x, inner.y, inner.width, 1));
    button(frame, app, cols[0], "[转发]", Action::MultiForward);
    button(frame, app, cols[1], "[复制]", Action::MultiCopy);
    button(frame, app, cols[2], "[删除]", Action::MultiDelete);
    button(frame, app, cols[3], "[取消]", Action::MultiClear);
}

fn composer(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = match app.input_mode {
        InputMode::Off | InputMode::Send => "写消息",
        InputMode::Reply(_) => "回复消息",
        InputMode::Edit(_) => "编辑消息",
        InputMode::Search => "搜索",
        InputMode::React(_) => "表情回应",
    };
    let focused = app.composer_focus && app.input_mode != InputMode::Off;
    let inner_width = area.width.saturating_sub(2);
    let reaction_row = matches!(app.input_mode, InputMode::React(_)) && area.height >= 5;
    let inner_height = area.height.saturating_sub(3 + u16::from(reaction_row));
    app.composer_width = usize::from(inner_width.saturating_sub(1)).max(1);
    let row_hint = if !app.draft.is_empty() && inner_width > 1 && inner_height > 0 {
        let edit_width = app.composer_width;
        let rows = crate::text::rows(&app.draft, edit_width);
        if rows.len() > usize::from(inner_height) {
            let pos = crate::text::cursor(&app.draft, app.draft_cursor);
            let current = rows.iter().rposition(|row| row.start <= pos).unwrap_or(0) + 1;
            format!(" · 第 {current}/{} 行", rows.len())
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    let max_title = usize::from(area.width.saturating_sub(8 + u16::from(focused) * 2));
    let title = if let InputMode::Reply(id) | InputMode::Edit(id) = app.input_mode {
        app.store
            .messages
            .iter()
            .find(|message| message.id == id)
            .map(|message| {
                crate::text::ellipsize(
                    &format!(
                        "{title} · {}：{}{row_hint}",
                        app.store.sender_label(message),
                        excerpt(&message.text, max_title)
                    ),
                    max_title,
                )
            })
            .unwrap_or_else(|| crate::text::ellipsize(&format!("{title}{row_hint}"), max_title))
    } else if !row_hint.is_empty() {
        crate::text::ellipsize(&format!("{title}{row_hint}"), max_title)
    } else {
        title.to_owned()
    };
    let block = panel(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    app.hit_targets.push((area, Target::Composer));
    if app.input_mode != InputMode::Off {
        button(
            frame,
            app,
            Rect::new(area.right().saturating_sub(5), area.y, 3, 1),
            "[×]",
            Action::Cancel,
        );
    }
    let tools = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let (submit_label, tool) = match app.input_mode {
        InputMode::Search => ("[查找]", Some(format!("[{} ↻]", app.parity.filter.label()))),
        InputMode::Edit(_) => ("[保存]", None),
        InputMode::React(_) => ("[回应]", None),
        _ => ("[发送]", None),
    };
    frame
        .buffer_mut()
        .set_style(tools, palette().surface_base());
    let submit_width = if tools.width >= 36 { 14 } else { 8 }.min(tools.width);
    let submit = Rect::new(tools.right() - submit_width, tools.y, submit_width, 1);
    if let Some(label) = tool {
        let filter = Rect::new(tools.x, tools.y, (tools.width - submit_width).min(18), 1);
        button(frame, app, filter, &label, Action::CycleMediaFilter);
    } else {
        let left_width = (tools.width - submit_width).min(20);
        let first = Rect::new(tools.x, tools.y, left_width / 2, 1);
        let second = Rect::new(first.right(), tools.y, left_width - first.width, 1);
        button(frame, app, first, "[附件]", Action::File);
        button(frame, app, second, "[贴纸]", Action::Stickers);
    }
    button(frame, app, submit, submit_label, Action::Submit);

    let inner = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let editor_area = if reaction_row && inner.height >= 2 {
        let strip = Rect::new(inner.x, inner.y, inner.width, 1);
        let mut x = strip.x;
        for (idx, reaction) in app
            .reaction_choices()
            .iter()
            .skip(app.parity.reaction_page * 8)
            .take(8)
            .enumerate()
        {
            let label = format!("{}{}", idx + 1, reaction.label());
            let w = (text_width(label.as_str()) as u16) + 2;
            if x + w > strip.right() {
                break;
            }
            let cell = Rect::new(x, strip.y, w, 1);
            draw_line(
                frame,
                cell,
                &Line::raw(label).alignment(Alignment::Center),
                Style::default().fg(palette().text).bg(palette().surface),
            );
            app.hit_targets
                .push((cell, Target::Command(Action::QuickReact(idx))));
            x = x.saturating_add(w + 1);
        }
        Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 1)
    } else {
        inner
    };
    let value = app.draft.clone();
    editor(
        frame,
        app,
        editor_area,
        Editor::Composer,
        &value,
        app.draft_cursor,
        false,
    );
    if value.is_empty() {
        let media_hint = format!(
            "{} · Ctrl+F 切换类型 · 可留空查找",
            app.parity.filter.label()
        );
        let reaction_count = if matches!(app.input_mode, InputMode::React(_)) {
            app.reaction_choices().len()
        } else {
            0
        };
        let reaction_hint = if reaction_count > 8 {
            "1-8 回应 · PgUp/PgDn 翻页"
        } else if reaction_count == 0 {
            "此消息没有可用回应"
        } else {
            "按 1-8 快捷回应，或输入表情"
        };
        let placeholder = match app.input_mode {
            InputMode::Off => Some(if editor_area.width >= 22 {
                "点击写消息（或按 i）"
            } else {
                "点击写消息"
            }),
            InputMode::React(_) => Some(reaction_hint),
            InputMode::Search => Some(media_hint.as_str()),
            _ => None,
        };
        if let Some(text) = placeholder {
            frame.render_widget(
                Paragraph::new(crate::text::ellipsize(text, usize::from(editor_area.width)))
                    .style(Style::default().fg(palette().muted)),
                editor_area,
            );
        }
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
    // Pause hidden inline images behind media overlays, and skip native terminal
    // graphics protocols behind any modal so placements cannot bleed over borders.
    if app.preview_message.is_some()
        || app.sticker_picker
        || app.attachments.is_some()
        || (app.has_overlay() && app.media.protocol_name != "Halfblocks")
    {
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
    let has_scroll = entries.len() > usize::from(rows[0].height) && rows[0].width >= 4;
    let list_area = if has_scroll {
        Rect::new(
            rows[0].x,
            rows[0].y,
            rows[0].width.saturating_sub(1),
            rows[0].height,
        )
    } else {
        rows[0]
    };
    let items: Vec<_> = entries
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let label_style = if item.action == Action::Delete {
                Style::default().fg(palette().error)
            } else {
                Style::default()
            };
            let key_style = if index == selected {
                palette().muted_on_selection()
            } else {
                Style::default().fg(palette().muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(item.label, label_style),
                Span::styled(
                    if item.key == '\0' {
                        String::new()
                    } else {
                        format!("  {}", item.key)
                    },
                    key_style,
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
        list_area,
        &mut state,
    );
    for (row, item) in entries
        .iter()
        .skip(state.offset())
        .take(usize::from(list_area.height))
        .enumerate()
    {
        app.hit_targets.push((
            Rect::new(list_area.x, list_area.y + row as u16, list_area.width, 1),
            Target::Action(item.action),
        ));
    }
    if has_scroll {
        scrollbar(
            frame,
            app,
            rows[0],
            Pane::Modal,
            selected,
            entries.len().saturating_sub(1),
        );
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
    let column = chat_column(app, area);
    let ch = composer_height(app, column.width, column.height);
    let composer_y = column.bottom().saturating_sub(ch);
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
    let total_rows = total.div_ceil(columns).max(1);
    if parts[1].height > 0 {
        let progress = if total_rows > rows {
            format!(" · 第 {}/{} 行", selected_row + 1, total_rows)
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                &format!("{title}{progress}"),
                usize::from(parts[1].width),
            ))
            .style(Style::default().fg(palette().muted)),
            parts[1],
        );
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
        let tile_selected = index == app.sticker_cursor && !app.sticker_panel.search_focus;
        if tile.height < 3 {
            frame.render_widget(
                Paragraph::new(sticker.emoji)
                    .style(if tile_selected {
                        palette().selection().add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    })
                    .alignment(Alignment::Center),
                tile,
            );
            app.hit_targets.push((tile, Target::Sticker(index)));
            continue;
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if tile_selected {
                palette().accent
            } else if index == app.sticker_cursor {
                palette().border
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
    let search = Layout::horizontal([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(8),
    ])
    .split(parts[3]);
    let search_focused = app.sticker_panel.search_focus;
    if search_focused {
        frame.buffer_mut().set_style(
            Rect::new(parts[3].x, parts[3].y, search[0].width + search[1].width, 1),
            palette().base(),
        );
    }
    frame.render_widget(
        Paragraph::new("/ ").style(
            Style::default()
                .fg(if search_focused {
                    palette().accent
                } else {
                    palette().muted
                })
                .add_modifier(if search_focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        search[0],
    );
    app.hit_targets.push((
        search[0],
        Target::Cursor(Editor::StickerSearch, app.sticker_panel.query_cursor),
    ));
    let value = app.sticker_panel.query.clone();
    editor(
        frame,
        app,
        search[1],
        Editor::StickerSearch,
        &value,
        app.sticker_panel.query_cursor,
        false,
    );
    if value.is_empty() {
        frame.render_widget(
            Paragraph::new("搜索贴纸 / 表情…").style(Style::default().fg(palette().muted)),
            search[1],
        );
    }
    button(frame, app, search[2], "[搜索]", Action::StickerSearch);
    let footer = Layout::horizontal([Constraint::Min(1), Constraint::Length(12)]).split(parts[4]);
    let summary = if total_rows > rows && parts[1].height == 0 {
        format!("{total} 张 · {}/{}行", selected_row + 1, total_rows)
    } else {
        format!("{total} 张 · Enter 发送 · Tab 搜索")
    };
    frame.render_widget(
        Paragraph::new(crate::text::ellipsize(
            &summary,
            usize::from(footer[0].width),
        ))
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
    let title = if app
        .attachments
        .as_ref()
        .is_some_and(|picker| picker.reply_to.is_some())
    {
        "发送附件 · 回复"
    } else {
        "发送附件"
    };
    let inner = modal(frame, app, centered(area, 86, 25), title);
    let compact_tools = inner.width < 34;
    let wide = inner.width >= 48;
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(if inner.height >= 10 { 2 } else { 1 }),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let tools = if compact_tools {
        Layout::horizontal([
            Constraint::Percentage(35),
            Constraint::Percentage(35),
            Constraint::Percentage(30),
        ])
        .split(parts[0])
    } else {
        Layout::horizontal([
            Constraint::Length(12),
            Constraint::Min(1),
            Constraint::Length(8),
        ])
        .split(parts[0])
    };
    let picker = app.attachments.as_mut().unwrap();
    let focus = picker.focus;
    let mode = match (picker.photos, compact_tools) {
        (true, true) => "[图片]",
        (false, true) => "[文件]",
        (true, false) => "[图片 F5]",
        (false, false) => "[原文件 F5]",
    };
    let paste_label = if compact_tools {
        "[粘贴]"
    } else {
        "[粘贴 F7]"
    };
    let parent_label = if compact_tools {
        "[上级]"
    } else {
        "[上一级]"
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
    let browser_rows = if wide {
        parts[3].height
    } else {
        parts[3].height.saturating_sub(1).max(1)
    };
    picker.visible = usize::from(browser_rows).max(1);
    picker.offset = picker
        .offset
        .min(picker.entries.len().saturating_sub(picker.visible));
    button(frame, app, tools[0], mode, Action::AttachmentMode);
    button(frame, app, tools[1], paste_label, Action::PasteClipboard);
    button(frame, app, tools[2], parent_label, Action::AttachmentParent);
    frame.render_widget(
        Paragraph::new(directory).style(Style::default().fg(palette().muted)),
        parts[1],
    );
    let path_focused = focus == crate::attachments::Focus::Path;
    if path_focused {
        frame.buffer_mut().set_style(parts[2], palette().base());
    }
    let path_cols = Layout::horizontal([Constraint::Length(6), Constraint::Min(1)]).split(parts[2]);
    frame.render_widget(
        Paragraph::new("路径: ").style(
            Style::default()
                .fg(if path_focused {
                    palette().accent
                } else {
                    palette().muted
                })
                .add_modifier(if path_focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        path_cols[0],
    );
    app.hit_targets.push((
        path_cols[0],
        Target::Cursor(Editor::AttachmentPath, path_cursor),
    ));
    editor(
        frame,
        app,
        path_cols[1],
        Editor::AttachmentPath,
        &path,
        path_cursor,
        false,
    );
    if path.is_empty() {
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                "粘贴/拖入路径，或点击下方文件（Tab 切换）",
                usize::from(path_cols[1].width),
            ))
            .style(Style::default().fg(palette().muted)),
            path_cols[1],
        );
    }

    let (browser_area, queue_area) = if wide {
        let cols = Layout::horizontal([
            Constraint::Percentage(60),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(parts[3]);
        (cols[0], Some((cols[1], cols[2])))
    } else {
        let stack = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(parts[3]);
        (stack[0], Some((Rect::default(), stack[1])))
    };

    let browser_focused = focus == crate::attachments::Focus::Browser;
    let picker = app.attachments.as_ref().unwrap();
    let total_entries = picker.entries.len();
    let offset = picker.offset;
    let visible = picker.visible;
    let has_browser_scroll = total_entries > visible && browser_area.width >= 4;
    if let Some((divider, _)) = queue_area
        && wide
        && !has_browser_scroll
    {
        for row in 0..divider.height {
            frame.render_widget(
                Paragraph::new("│").style(Style::default().fg(palette().border)),
                Rect::new(divider.x, divider.y + row, 1, 1),
            );
        }
    }
    let entry_width = if has_browser_scroll {
        browser_area.width.saturating_sub(1)
    } else {
        browser_area.width
    };
    let entries: Vec<_> = picker
        .entries
        .iter()
        .enumerate()
        .skip(offset)
        .take(visible)
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
        let rect = Rect::new(browser_area.x, browser_area.y + row as u16, entry_width, 1);
        let style = if selected && browser_focused {
            palette()
                .selection()
                .fg(palette().accent)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(palette().muted)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(&label, usize::from(rect.width))).style(style),
            rect,
        );
        app.hit_targets.push((rect, Target::AttachmentEntry(index)));
    }
    if has_browser_scroll {
        scrollbar(
            frame,
            app,
            browser_area,
            Pane::Modal,
            offset,
            total_entries.saturating_sub(visible),
        );
    }

    if let Some((_, queue_col)) = queue_area {
        let picker = app.attachments.as_mut().unwrap();
        picker.queue_visible = picker
            .selected
            .len()
            .min(6)
            .min(usize::from(queue_col.height.saturating_sub(1)))
            .max(1);
        picker.queue_offset = picker
            .queue_offset
            .min(picker.selected.len().saturating_sub(picker.queue_visible));
        let queue_offset = picker.queue_offset;
        let queue_visible = picker.queue_visible;
        let preview_path = wide
            .then(|| {
                picker
                    .selected
                    .get(queue_offset)
                    .filter(|p| crate::attachments::photo_paths(&[(*p).clone()]))
                    .cloned()
            })
            .flatten();
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
        if wide {
            frame.render_widget(
                Paragraph::new(format!("已选 {}/10 · 滚动/移除", selected.len()))
                    .style(Style::default().fg(palette().accent)),
                Rect::new(queue_col.x, queue_col.y, queue_col.width, 1),
            );
            app.hit_targets.push((queue_col, Target::AttachmentQueue));
            let queue_rows = selected.len().min(queue_visible);
            for (row, (index, name)) in selected
                .into_iter()
                .skip(queue_offset)
                .take(queue_rows)
                .enumerate()
            {
                let rect = Rect::new(
                    queue_col.x,
                    queue_col.y + 1 + row as u16,
                    queue_col.width,
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
                    queue_col.x,
                    queue_col.y + 1 + queue_rows as u16,
                    queue_col.width,
                    queue_col.height.saturating_sub(1 + queue_rows as u16),
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
        } else {
            let summary = if let Some((_, first)) = selected.first() {
                if selected.len() > 1 {
                    format!("已选 {}/10 · × {first} 等", selected.len())
                } else {
                    format!("已选 1/10 · × {first}")
                }
            } else {
                "已选 0/10 · 点击文件选择".into()
            };
            frame.render_widget(
                Paragraph::new(crate::text::ellipsize(
                    &summary,
                    usize::from(queue_col.width),
                ))
                .style(Style::default().fg(if selected.is_empty() {
                    palette().muted
                } else {
                    palette().accent
                })),
                queue_col,
            );
            if !selected.is_empty() {
                app.hit_targets
                    .push((queue_col, Target::AttachmentRemove(0)));
            } else {
                app.hit_targets.push((queue_col, Target::AttachmentQueue));
            }
        }
    }

    let caption_focused = focus == crate::attachments::Focus::Caption;
    if caption_focused {
        frame.buffer_mut().set_style(parts[4], palette().base());
    }
    let caption_cols =
        Layout::horizontal([Constraint::Length(6), Constraint::Min(1)]).split(parts[4]);
    frame.render_widget(
        Paragraph::new("说明: ").style(
            Style::default()
                .fg(if caption_focused {
                    palette().accent
                } else {
                    palette().muted
                })
                .add_modifier(if caption_focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ),
        caption_cols[0],
    );
    app.hit_targets.push((
        caption_cols[0],
        Target::Cursor(Editor::AttachmentCaption, caption_cursor),
    ));
    editor(
        frame,
        app,
        caption_cols[1],
        Editor::AttachmentCaption,
        &caption,
        caption_cursor,
        false,
    );
    if caption.is_empty() {
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                "添加说明（可选，Tab 或点击输入）",
                usize::from(caption_cols[1].width),
            ))
            .style(Style::default().fg(palette().muted)),
            caption_cols[1],
        );
    }
    frame.render_widget(
        Paragraph::new(crate::text::ellipsize(
            error
                .as_deref()
                .unwrap_or("Enter 选择 · Space 多选 · F5 切换模式 · F8 发送"),
            usize::from(parts[5].width),
        ))
        .style(Style::default().fg(
            if app.attachments.as_ref().unwrap().error.is_some() {
                palette().error
            } else {
                palette().muted
            },
        )),
        parts[5],
    );
    let footer = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(parts[6]);
    button(frame, app, footer[0], "[发送附件]", Action::Confirm);
    button(frame, app, footer[1], "[取消]", Action::Cancel);
}

fn quick_switcher_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let global = app.global_message_mode();
    let title = if global {
        "全局消息搜索 · Tab 切换"
    } else {
        "快速切换会话 · Ctrl+K"
    };
    let inner = modal(frame, app, centered(area, 58, 20), title);
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let query = app
        .quick_switcher
        .as_ref()
        .map(|sw| sw.query.clone())
        .unwrap_or_default();
    let cursor = app.quick_switcher.as_ref().map_or(0, |sw| sw.cursor);
    frame.buffer_mut().set_style(parts[0], palette().base());
    let search_cols =
        Layout::horizontal([Constraint::Length(3), Constraint::Min(1)]).split(parts[0]);
    frame.render_widget(
        Paragraph::new(" ⌕ ").style(
            Style::default()
                .fg(palette().accent)
                .add_modifier(Modifier::BOLD),
        ),
        search_cols[0],
    );
    app.hit_targets
        .push((search_cols[0], Target::Cursor(Editor::QuickSwitch, cursor)));
    editor(
        frame,
        app,
        search_cols[1],
        Editor::QuickSwitch,
        &query,
        cursor,
        false,
    );
    if query.is_empty() {
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(
                if global {
                    "输入关键词搜索所有会话…"
                } else {
                    "输入会话名、@用户名或最近消息…"
                },
                usize::from(search_cols[1].width),
            ))
            .style(Style::default().fg(palette().muted)),
            search_cols[1],
        );
    }
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(parts[1].width)))
            .style(Style::default().fg(palette().border)),
        parts[1],
    );
    let entries = app.quick_entries();
    let list_area = parts[2];
    let visible = usize::from(list_area.height / 2).max(1);
    app.modal_visible = visible;
    let selected = {
        let sw = app.quick_switcher.as_mut().unwrap();
        sw.selected = sw.selected.min(entries.len().saturating_sub(1));
        if sw.selected < sw.scroll {
            sw.scroll = sw.selected;
        } else if sw.selected >= sw.scroll + visible {
            sw.scroll = sw.selected + 1 - visible;
        }
        sw.selected
    };
    let scroll = app.quick_switcher.as_ref().unwrap().scroll;
    if entries.is_empty() {
        empty(
            frame,
            list_area,
            if global {
                "没有匹配的消息"
            } else {
                "没有匹配的会话"
            },
            "尝试其他关键词，或按 Esc 关闭",
        );
    } else {
        let has_scroll = entries.len() > visible && list_area.width >= 6;
        let row_w = if has_scroll {
            list_area.width.saturating_sub(1)
        } else {
            list_area.width
        };
        for (i, (id, title, sub, remote)) in entries.iter().skip(scroll).take(visible).enumerate() {
            let abs = scroll + i;
            let is_sel = abs == selected;
            let y = list_area.y + (i * 2) as u16;
            let h = 2u16.min(list_area.bottom().saturating_sub(y));
            if h == 0 {
                break;
            }
            let rect = Rect::new(list_area.x, y, row_w, h);
            let chat = (!global).then(|| app.store.chat(*id)).flatten();
            let badge = if *remote {
                " 全局 ".to_owned()
            } else if let Some(c) = chat
                && c.unread > 0
            {
                if c.unread > 999 {
                    " 999+ ".to_owned()
                } else {
                    format!(" {} ", c.unread)
                }
            } else {
                String::new()
            };
            let badge_w = text_width(badge.as_str());
            let prefix = if is_sel { "› " } else { "  " };
            let text_w = usize::from(row_w).saturating_sub(2);
            let title_str = crate::text::ellipsize(
                title,
                text_w.saturating_sub(badge_w + usize::from(badge_w > 0)),
            );
            let pad = text_w.saturating_sub(text_width(title_str.as_str()) + badge_w);
            let badge_style = if *remote || chat.is_some_and(|c| c.muted) {
                Style::default().fg(palette().muted).bg(palette().surface)
            } else {
                palette().primary()
            };
            let mut row_lines = vec![Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if is_sel {
                        palette().accent
                    } else {
                        palette().muted
                    }),
                ),
                Span::styled(title_str, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(" ".repeat(pad)),
                Span::styled(badge, badge_style),
            ])];
            if h >= 2 {
                let sub_style = if is_sel {
                    palette().muted_on_selection()
                } else {
                    Style::default().fg(palette().muted)
                };
                row_lines.push(Line::styled(
                    format!("  {}", excerpt(sub, text_w)),
                    sub_style,
                ));
            }
            frame.render_widget(
                Paragraph::new(row_lines).style(if is_sel {
                    palette().selection()
                } else {
                    palette().surface_base()
                }),
                rect,
            );
            if global {
                if let Some((chat, message)) = app.global_hit_key(*id as usize) {
                    app.hit_targets
                        .push((rect, Target::GlobalSearchHit(chat, message)));
                }
            } else {
                app.hit_targets.push((rect, Target::QuickSwitchChat(*id)));
            }
        }
        if has_scroll {
            scrollbar(
                frame,
                app,
                list_area,
                Pane::Modal,
                scroll,
                entries.len().saturating_sub(visible),
            );
        }
    }
    frame.render_widget(
        Paragraph::new(crate::text::ellipsize(
            &format!(
                "{} 个匹配 · Tab 切换会话 / 全局消息 · Enter 打开",
                entries.len()
            ),
            usize::from(parts[3].width),
        ))
        .style(Style::default().fg(palette().muted)),
        parts[3],
    );
}

fn chat_info_body(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(chat_id) = app.store.active_chat else {
        empty(frame, area, "未选择会话", "先打开一个会话查看详情");
        return;
    };
    let Some(chat) = app.store.chat(chat_id).cloned() else {
        empty(frame, area, "暂无会话详情", "等待同步会话信息…");
        return;
    };
    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(2),
    ])
    .split(area);
    let mut lines = vec![Line::styled(
        chat.title.clone(),
        Style::default()
            .fg(palette().accent)
            .add_modifier(Modifier::BOLD),
    )];
    if !chat.subtitle.is_empty() {
        lines.push(muted(chat.subtitle.clone()));
    }
    if !chat.username.is_empty() {
        lines.push(Line::styled(
            format!("@{}", chat.username.trim_start_matches('@')),
            Style::default().fg(palette().incoming),
        ));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled(
        "简介 / 信息",
        Style::default().fg(palette().accent),
    ));
    if chat.bio.is_empty() {
        lines.push(muted("暂无简介（后端接入 FullInfo 后展示）"));
    } else {
        lines.push(Line::from(chat.bio.clone()));
    }
    lines.push(Line::from(""));
    lines.push(Line::styled("状态", Style::default().fg(palette().accent)));
    lines.push(Line::from(format!("会话 ID   {}", chat.id)));
    lines.push(Line::from(format!(
        "通知      {}",
        if chat.muted { "已静音" } else { "已开启" }
    )));
    lines.push(Line::from(format!(
        "置顶      {}",
        if chat.pinned {
            "已置顶"
        } else {
            "未置顶"
        }
    )));
    lines.push(Line::from(format!("未读消息  {}", chat.unread)));
    if let Some(pin) = &chat.pinned_message {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            "置顶消息",
            Style::default().fg(palette().accent),
        ));
        lines.push(muted(excerpt(&pin.excerpt, 120)));
    }
    scrolling_text(frame, app, lines, rows[0]);
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(rows[1].width)))
            .style(Style::default().fg(palette().border)),
        rows[1],
    );
    let row0 = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(rows[2].x, rows[2].y, rows[2].width, 1));
    button(
        frame,
        app,
        row0[0],
        if chat.muted {
            "[取消静音]"
        } else {
            "[静音通知]"
        },
        Action::ToggleMute,
    );
    button(
        frame,
        app,
        row0[1],
        if chat.pinned {
            "[取消置顶]"
        } else {
            "[置顶会话]"
        },
        Action::TogglePin,
    );
    let row1 = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(rows[2].x, rows[2].y + 1, rows[2].width, 1));
    button(frame, app, row1[0], "[搜索消息]", Action::Search);
    button(frame, app, row1[1], "[发送附件]", Action::File);
}

fn help_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 64, 24), "使用帮助");
    let lines = vec![
        Line::styled("浏览会话", Style::default().fg(palette().accent)),
        muted("单击选择 · 右键操作 · 滚轮滚动所在面板"),
        Line::from("Ctrl+K / K 快速切换会话    Shift+I 会话详情    Tab 切换面板"),
        Line::from("↑ ↓ / j k 选择    Enter 打开    [ ] 切换分组"),
        Line::from("g 加载更早消息    m 更多会话    PgUp/PgDn 滚动消息"),
        Line::from(""),
        Line::styled("聊天与附件", Style::default().fg(palette().accent)),
        Line::from("i 写消息    Enter 消息操作/媒体预览    Space 更多操作"),
        Line::from("r 回复    e 编辑    f 转发    x 回应（1-8 快捷）    d 删除"),
        Line::from("V 多选    L 链接与 @提及    B Bot 按钮    S 收藏    D 复读"),
        Line::from("p 图片 / a 文件 / Ctrl+O 附件 · t 贴纸 · / 搜索（Ctrl+F 筛选）"),
        Line::from("F7 / Ctrl+V 读取剪贴板图片、文件或文字；F8 确认发送"),
        Line::from("附件：点击多选 / 拖入路径 · Tab 说明 · F8 / Ctrl+Enter 发送"),
        Line::from("贴纸：缩略图网格 · [ ] 切换包 · / 搜索 · 点击发送"),
        Line::from("[ ] 切换相册图片    o 打开大图或文件    E 另存为    T 话题"),
        Line::from("F9 代理设置（登录页可用，支持 SOCKS5 / HTTP / MTProto / 环境变量）"),
        Line::from(""),
        Line::styled("输入与导航", Style::default().fg(palette().accent)),
        Line::from("↑ ↓ 多行移光标    Alt+Enter 换行    Ctrl+W / Alt+BS 删词"),
        Line::from("Alt+←/→ 按词跳转    Ctrl+U 清空    Ctrl+A 全选"),
        Line::from("Enter 提交    Esc 取消当前操作    s 设置（t 切换配色）"),
        Line::from("c 复制消息 · F6 鼠标开关 · q / Ctrl+Q 退出"),
        Line::from("拖选文字 · 双击选词 / 打开媒体 · 拖动滚动条"),
        Line::from("图片：滚轮或 +/- 缩放 · 拖动或方向键平移 · 0 复位"),
        muted("终端自带的选字修饰键因终端而异，可用 F6 暂停鼠标。"),
        Line::from(""),
        Line::styled("API 凭据与官网注册", Style::default().fg(palette().accent)),
        Line::from("已有应用：登录 my.telegram.org/apps 复制原 API ID / Hash。"),
        Line::from("每个号码只能创建一个 API ID，已有应用无需重新注册。"),
        Line::from("官网 ERROR 是 Telegram 的注册错误，客户端无法代为修复。"),
        Line::from("联系发布者提供含 Teleaf 项目凭据的安装包，可免手动注册。"),
        muted("二维码登录也需要应用凭据；官方示例 ID 不适合发行。"),
    ];
    scrolling_text(frame, app, lines, inner);
}

fn settings_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 70, 21), "设置与连接");
    let rows = Layout::vertical([
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(if crate::notifications::supported() {
            3
        } else {
            2
        }),
    ])
    .split(inner);
    let theme_label = format!("[配色：{} · t]", palette().name);
    let theme_w = (text_width(theme_label.as_str()) as u16 + 2).min(rows[1].width);
    let sep_w = rows[1].width.saturating_sub(theme_w);
    if sep_w > 0 {
        frame.render_widget(
            Paragraph::new("─".repeat(usize::from(sep_w)))
                .style(Style::default().fg(palette().border)),
            Rect::new(rows[1].x, rows[1].y, sep_w, 1),
        );
    }
    button(
        frame,
        app,
        Rect::new(rows[1].x + sep_w, rows[1].y, theme_w, 1),
        &theme_label,
        Action::CycleTheme,
    );
    let tools = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(Rect::new(rows[2].x, rows[2].y, rows[2].width, 1));
    button(frame, app, tools[0], "[修改 API]", Action::ApiSetup);
    button(frame, app, tools[1], "[官网]", Action::OpenLink);
    button(
        frame,
        app,
        Rect::new(rows[2].x, rows[2].y + 1, rows[2].width, 1),
        if app.mouse_enabled {
            "[鼠标：开 · F6 切换]"
        } else {
            "[鼠标：关 · F6 切换]"
        },
        Action::ToggleMouse,
    );
    if crate::notifications::supported() {
        button(
            frame,
            app,
            Rect::new(rows[2].x, rows[2].y + 2, rows[2].width, 1),
            if app.notifications.enabled {
                "[桌面通知：开]"
            } else {
                "[桌面通知：关]"
            },
            Action::ToggleNotifications,
        );
    }
    let (api_id, directory) = app.auth.config_summary();
    let proxy_line = match app.proxy.active_summary() {
        Some(summary) => format!("网络代理   已启用 {summary} · 按 p 或 F9 管理"),
        None => match crate::proxy::detect_env_proxy() {
            Some((var, parsed)) => format!(
                "网络代理   未启用（检测到 {var} {}:{} · 按 p 或 F9 一键导入）",
                parsed.server, parsed.port
            ),
            None => "网络代理   直连 · 按 p 或 F9 配置 SOCKS5 / HTTP / MTProto".into(),
        },
    };
    let lines = vec![
        Line::styled("账号连接", Style::default().fg(palette().accent)),
        Line::from(format!("API ID   {api_id}")),
        Line::from(proxy_line),
        muted("API Hash 已隐藏 · F3 修改 API 配置并重新连接"),
        muted("F2 打开 Telegram API 页面"),
        Line::from(""),
        Line::styled("本机数据", Style::default().fg(palette().accent)),
        Line::from(directory),
        muted("凭据和自动生成的密钥保存在 config.json，权限仅当前用户。"),
        Line::from(""),
        Line::styled("终端与诊断", Style::default().fg(palette().accent)),
        Line::from(format!("图片显示   {}", app.media.protocol_name)),
        Line::from(format!(
            "界面配色   {} · 按 t 或 TG_THEME 切换",
            palette().name
        )),
        Line::from(crate::terminal::summary()),
        Line::from(app.status.clone()),
        muted(app.tdlib_path.as_deref().unwrap_or("TDLib 尚未加载")),
    ];
    let body = if rows[0].height >= 2 {
        let footer = Rect::new(rows[0].x, rows[0].bottom() - 1, rows[0].width, 1);
        if app.auth.state == "authorizationStateReady" {
            if app.parity.storage_confirmation {
                let columns = Layout::horizontal([
                    Constraint::Min(1),
                    Constraint::Length(13),
                    Constraint::Length(10),
                ])
                .split(footer);
                frame.render_widget(
                    Paragraph::new("清理本机缓存？").style(Style::default().fg(palette().accent)),
                    columns[0],
                );
                button(frame, app, columns[1], "[确认清理]", Action::ConfirmStorage);
                button(frame, app, columns[2], "[取消]", Action::CancelStorage);
            } else {
                let columns = Layout::horizontal([
                    Constraint::Min(1),
                    Constraint::Length(16),
                    Constraint::Length(16),
                ])
                .split(footer);
                let size = app.parity.storage_size.map_or("—".into(), format_bytes);
                frame.render_widget(
                    Paragraph::new(format!("TDLib 缓存 {size}"))
                        .style(Style::default().fg(palette().muted)),
                    columns[0],
                );
                button(
                    frame,
                    app,
                    columns[1],
                    "[代理设置 · p]",
                    Action::ProxySettings,
                );
                button(
                    frame,
                    app,
                    columns[2],
                    "[清理缓存 · c]",
                    Action::CleanStorage,
                );
            }
        } else {
            let columns =
                Layout::horizontal([Constraint::Min(1), Constraint::Length(18)]).split(footer);
            let proxy_status = app
                .proxy
                .active_summary()
                .map_or_else(|| "网络：直连".into(), |s| format!("代理：{s}"));
            frame.render_widget(
                Paragraph::new(crate::text::ellipsize(
                    &proxy_status,
                    usize::from(columns[0].width),
                ))
                .style(Style::default().fg(palette().muted)),
                columns[0],
            );
            button(
                frame,
                app,
                columns[1],
                "[代理设置 · p/F9]",
                Action::ProxySettings,
            );
        }
        Rect::new(rows[0].x, rows[0].y, rows[0].width, rows[0].height - 1)
    } else {
        rows[0]
    };
    scrolling_text(frame, app, lines, body);
}

fn proxy_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let title = if app.proxy.form.is_some() {
        "新增代理 · SOCKS5 / HTTP / MTProto"
    } else {
        "代理设置（F9）"
    };
    let inner = modal(frame, app, centered(area, 72, 20), title);
    if let Some(form) = app.proxy.form.clone() {
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(if form.kind == crate::proxy::ProxyKind::Mtproto {
                0
            } else {
                3
            }),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);
        let kind_cols = Layout::horizontal([
            Constraint::Length(10),
            Constraint::Length(6),
            Constraint::Min(10),
            Constraint::Length(6),
        ])
        .split(Rect::new(rows[0].x, rows[0].y, rows[0].width, 1));
        frame.render_widget(
            Paragraph::new("协议类型").style(Style::default().fg(if form.field == 0 {
                palette().accent
            } else {
                palette().muted
            })),
            kind_cols[0],
        );
        button(frame, app, kind_cols[1], "[‹]", Action::ProxyKindPrev);
        frame.render_widget(
            Paragraph::new(format!(" {} ", form.kind.label()))
                .alignment(Alignment::Center)
                .style(if form.field == 0 {
                    palette().selection().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                        .fg(palette().text)
                        .add_modifier(Modifier::BOLD)
                }),
            kind_cols[2],
        );
        app.hit_targets.push((kind_cols[2], Target::ProxyField(0)));
        button(frame, app, kind_cols[3], "[›]", Action::ProxyKindNext);
        if rows[0].height >= 2 {
            frame.render_widget(
                Paragraph::new(
                    "支持粘贴完整代理链接（socks5:// / http:// / tg://proxy?...）自动填充",
                )
                .style(Style::default().fg(palette().muted)),
                Rect::new(rows[0].x, rows[0].y + 1, rows[0].width, 1),
            );
        }
        let draw_field = |frame: &mut Frame,
                          app: &mut App,
                          rect: Rect,
                          idx: usize,
                          label: &str,
                          value: &str,
                          cursor: usize,
                          masked: bool| {
            let focused = form.field == idx;
            let block = panel(label, focused);
            let field_inner = block.inner(rect);
            frame.render_widget(block, rect);
            app.hit_targets.push((rect, Target::ProxyField(idx)));
            if focused {
                editor(
                    frame,
                    app,
                    field_inner,
                    Editor::Proxy(idx),
                    value,
                    cursor,
                    masked,
                );
            } else {
                let shown = if masked {
                    "•".repeat(value.chars().count())
                } else {
                    value.to_owned()
                };
                frame.render_widget(
                    Paragraph::new(crate::text::ellipsize(
                        &shown,
                        usize::from(field_inner.width),
                    )),
                    field_inner,
                );
            }
        };
        draw_field(
            frame,
            app,
            rows[1],
            1,
            "服务器地址（例如 127.0.0.1）",
            &form.server,
            form.server_cursor,
            false,
        );
        draw_field(
            frame,
            app,
            rows[2],
            2,
            "端口（例如 7890 / 1080 / 10808）",
            &form.port,
            form.port_cursor,
            false,
        );
        let user_label = if form.kind == crate::proxy::ProxyKind::Mtproto {
            "MTProto Secret（必填）"
        } else {
            "用户名（可选，无认证留空）"
        };
        draw_field(
            frame,
            app,
            rows[3],
            3,
            user_label,
            &form.user_or_secret,
            form.user_cursor,
            form.kind == crate::proxy::ProxyKind::Mtproto,
        );
        if form.kind != crate::proxy::ProxyKind::Mtproto && rows[4].height >= 3 {
            draw_field(
                frame,
                app,
                rows[4],
                4,
                "密码（可选，已遮罩）",
                &form.password,
                form.password_cursor,
                true,
            );
        }
        if let Some(error) = form.error.as_deref().or(app.proxy.status.as_deref()) {
            let is_err = form.error.is_some() || app.proxy.status_error;
            frame.render_widget(
                Paragraph::new(crate::text::ellipsize(error, usize::from(rows[5].width))).style(
                    Style::default().fg(if is_err {
                        palette().error
                    } else {
                        palette().accent
                    }),
                ),
                rows[5],
            );
        }
        let btns = Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[6]);
        button(
            frame,
            app,
            btns[0],
            "[保存并启用 · Enter]",
            Action::ProxySubmit,
        );
        button(frame, app, btns[1], "[取消 · Esc]", Action::ProxyCancelForm);
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let env_hint = match crate::proxy::detect_env_proxy() {
        Some((name, parsed)) => format!(
            "检测到环境变量 {name} ({} {}:{})，按 e 一键导入；启动时设 TG_PROXY 可自动启用。",
            parsed.kind.label(),
            parsed.server,
            parsed.port
        ),
        None => "支持 SOCKS5 / HTTP / MTProto 代理；也可用 TG_PROXY=socks5://127.0.0.1:7890 启动。"
            .into(),
    };
    frame.render_widget(
        Paragraph::new(env_hint)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette().muted)),
        rows[0],
    );
    if app.proxy.entries.is_empty() {
        let msg = if app.proxy.loading {
            "正在读取 TDLib 代理配置…"
        } else {
            "尚未配置代理 · 按 a 手动新增，或按 e 导入环境变量代理"
        };
        empty(frame, rows[1], "当前使用直连", msg);
    } else {
        let visible = usize::from(rows[1].height).max(1);
        let selected = app
            .proxy
            .selected
            .min(app.proxy.entries.len().saturating_sub(1));
        let start = selected.saturating_sub(visible.saturating_sub(1));
        for (row, (idx, entry)) in app
            .proxy
            .entries
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let is_sel = idx == selected;
            let rect = Rect::new(rows[1].x, rows[1].y + row as u16, rows[1].width, 1);
            let dot = if entry.enabled { "●" } else { "○" };
            let ping = if entry.pinging {
                "测速中…".into()
            } else if let Some(ms) = entry.ping_ms {
                format!("{ms} ms")
            } else if let Some(err) = &entry.ping_error {
                err.clone()
            } else {
                "未测速".into()
            };
            let line = Line::from(vec![
                Span::styled(
                    if is_sel { "› " } else { "  " },
                    Style::default().fg(if is_sel {
                        palette().accent
                    } else {
                        palette().muted
                    }),
                ),
                Span::styled(
                    format!("{dot} "),
                    Style::default().fg(if entry.enabled {
                        palette().accent
                    } else {
                        palette().muted
                    }),
                ),
                Span::styled(
                    format!("{} {}:{}", entry.kind.label(), entry.server, entry.port),
                    Style::default().add_modifier(if entry.enabled || is_sel {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
                ),
                Span::styled(
                    format!("  ·  {ping}"),
                    Style::default().fg(if is_sel {
                        palette().text
                    } else {
                        palette().muted
                    }),
                ),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(if is_sel {
                    palette().selection()
                } else {
                    palette().surface_base()
                }),
                rect,
            );
            app.hit_targets.push((rect, Target::ProxyEntry(idx)));
        }
    }
    if let Some(status) = &app.proxy.status {
        frame.render_widget(
            Paragraph::new(crate::text::ellipsize(status, usize::from(rows[2].width))).style(
                Style::default().fg(if app.proxy.status_error {
                    palette().error
                } else {
                    palette().accent
                }),
            ),
            rows[2],
        );
    }
    let btns = Layout::horizontal([
        Constraint::Percentage(18),
        Constraint::Percentage(22),
        Constraint::Percentage(18),
        Constraint::Percentage(14),
        Constraint::Percentage(14),
        Constraint::Percentage(14),
    ])
    .split(rows[3]);
    button(frame, app, btns[0], "[新增·a]", Action::ProxyAdd);
    button(frame, app, btns[1], "[导环境·e]", Action::ProxyImportEnv);
    button(frame, app, btns[2], "[启停·⏎]", Action::ProxyToggle);
    button(frame, app, btns[3], "[测速·t]", Action::ProxyPing);
    button(frame, app, btns[4], "[删·d]", Action::ProxyDelete);
    button(frame, app, btns[5], "[直连·0]", Action::ProxyDisableAll);
}

fn instant_view_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::instant_view::{Kind, State};

    let inner = modal(frame, app, centered(area, 84, area.height), "即时阅读");
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(inner);
    let footer = Layout::horizontal([Constraint::Min(1), Constraint::Length(9)]).split(rows[1]);
    frame
        .buffer_mut()
        .set_style(rows[1], palette().surface_base());
    button(
        frame,
        app,
        footer[0],
        "[在浏览器打开]",
        Action::OpenInstantViewBrowser,
    );
    button(frame, app, footer[1], "[关闭]", Action::CloseInstantView);
    let Some(reader) = &mut app.instant_view else {
        return;
    };
    let body = rows[0];
    reader.page = usize::from(body.height).max(1);
    let article = match &reader.state {
        State::Loading => {
            empty(frame, body, "正在加载…", "o 在浏览器打开");
            return;
        }
        State::Failed => {
            empty(frame, body, "无法加载即时阅读", "按 o 在浏览器打开");
            return;
        }
        State::Ready(article) => article,
    };

    // Reserve a column for the scrollbar so resizing never changes wrapping twice.
    let width = usize::from(body.width.saturating_sub(1)).max(1);
    let block_layout = |block: &crate::instant_view::Block| {
        let indent = (usize::from(block.indent) * 2).min(width / 3);
        let bar = 2 * usize::from(matches!(block.kind, Kind::Quote | Kind::Code));
        (indent, width.saturating_sub(indent + bar).max(1))
    };
    let skips_gap = |kind: Kind| matches!(kind, Kind::Divider | Kind::Code | Kind::Muted);
    let total: usize = article
        .blocks
        .iter()
        .map(|block| {
            text::row_count(&block.text, block_layout(block).1)
                + usize::from(!skips_gap(block.kind))
        })
        .sum::<usize>()
        + usize::from(article.partial);
    reader.max_scroll = total.saturating_sub(reader.page);
    reader.scroll = reader.scroll.min(reader.max_scroll);
    let top = reader.scroll;
    let end = top + reader.page;
    let max_scroll = reader.max_scroll;
    let mut index = 0;
    for block in &article.blocks {
        if index >= end {
            break;
        }
        let (indent, wrap_width) = block_layout(block);
        let (prefix, style) = match block.kind {
            Kind::Title => (
                "",
                Style::default()
                    .fg(palette().accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Kind::Heading => (
                "",
                Style::default()
                    .fg(palette().incoming)
                    .add_modifier(Modifier::BOLD),
            ),
            Kind::Muted => ("", Style::default().fg(palette().muted)),
            Kind::Quote => ("┃ ", Style::default().fg(palette().muted)),
            Kind::Code => ("┃ ", palette().surface_base().fg(palette().incoming)),
            Kind::Divider => ("", Style::default().fg(palette().border)),
            Kind::Body => ("", Style::default().fg(palette().text)),
        };
        text::scan_rows(&block.text, wrap_width, |row| {
            if (top..end).contains(&index) {
                let y = body.y + (index - top) as u16;
                let line = if matches!(block.kind, Kind::Divider) {
                    Line::raw("─".repeat(wrap_width))
                } else {
                    Line::from(vec![
                        Span::raw(prefix),
                        Span::raw(&block.text[row.start..row.end]),
                    ])
                };
                draw_line(
                    frame,
                    Rect::new(body.x + indent as u16, y, (width - indent) as u16, 1),
                    &line,
                    style,
                );
            }
            index += 1;
        });
        index += usize::from(!skips_gap(block.kind));
    }
    if article.partial && (top..end).contains(&index) {
        let rect = Rect::new(body.x, body.y + (index - top) as u16, width as u16, 1);
        draw_line(
            frame,
            rect,
            &muted("内容未完整显示 · o 在浏览器打开"),
            Style::default(),
        );
    }
    scrollbar(frame, app, body, Pane::Modal, top, max_scroll);
}

fn inline_picker_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let inner = modal(frame, app, centered(area, 60, 16), "Bot 按钮");
    let Some(picker) = &app.inline_picker else {
        return;
    };
    let Some(message) = app
        .active_messages()
        .iter()
        .find(|message| message.id == picker.message)
    else {
        empty(frame, inner, "消息已移出当前窗口", "Esc 关闭");
        return;
    };
    let count = message.inline_buttons().count();
    if count == 0 {
        empty(frame, inner, "按钮已更新", "Esc 关闭");
        return;
    }
    let selected = picker.selected.min(count - 1);
    let start = selected
        .saturating_add(1)
        .saturating_sub(usize::from(inner.height));
    let mut targets = Vec::new();
    for (index, (row, col, button)) in message
        .inline_buttons()
        .enumerate()
        .skip(start)
        .take(usize::from(inner.height))
    {
        let rect = Rect::new(inner.x, inner.y + (index - start) as u16, inner.width, 1);
        let style = if index == selected {
            palette().selection()
        } else {
            palette().surface_base()
        };
        let label = inline_label(button, inner.width.saturating_sub(2));
        let line = Line::from(vec![
            Span::raw(if index == selected { "› " } else { "  " }),
            Span::raw(label),
        ]);
        draw_line(frame, rect, &line, style);
        targets.push((rect, Target::InlineButton(message.id, row, col)));
    }
    app.hit_targets.extend(targets);
}

fn link_picker_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(picker) = &app.link_picker else {
        return;
    };
    let items = picker.items.clone();
    let selected = picker.selected.min(items.len().saturating_sub(1));
    let inner = modal(frame, app, centered(area, 64, 16), "消息中的链接与 @提及");
    let visible = usize::from(inner.height).max(1);
    let start = selected.saturating_sub(visible.saturating_sub(1));
    for (row, (idx, item)) in items
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let is_sel = idx == selected;
        let rect = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
        let label = match item {
            crate::store::LinkItem::Url(url) => format!("浏览器 · {url}"),
            crate::store::LinkItem::InstantView(url) => format!("阅读全文 · {url}"),
            crate::store::LinkItem::Mention(handle) => format!("@ {handle} · 打开会话"),
        };
        let line = Line::from(vec![
            Span::styled(
                if is_sel { "› " } else { "  " },
                Style::default().fg(if is_sel {
                    palette().accent
                } else {
                    palette().muted
                }),
            ),
            Span::raw(crate::text::ellipsize(
                &label,
                usize::from(inner.width.saturating_sub(2)),
            )),
        ]);
        draw_line(
            frame,
            rect,
            &line,
            if is_sel {
                palette().selection()
            } else {
                palette().surface_base()
            },
        );
        app.hit_targets.push((rect, Target::LinkEntry(idx)));
    }
}

fn topics_dialog(frame: &mut Frame, app: &mut App, area: Rect) {
    let topics = app
        .store
        .active_chat
        .and_then(|id| app.store.chat(id))
        .map(|chat| chat.topics.clone())
        .unwrap_or_default();
    let inner = modal(
        frame,
        app,
        centered(area, 56, 18),
        "群组话题（Forum Topics）",
    );
    if topics.is_empty() {
        empty(
            frame,
            inner,
            "正在加载话题列表…",
            "若该群暂无话题，按 Esc 返回",
        );
        return;
    }
    let visible = usize::from(inner.height).max(1);
    let selected = app.topic_cursor.min(topics.len().saturating_sub(1));
    let start = selected.saturating_sub(visible.saturating_sub(1));
    for (row, (idx, topic)) in topics
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .enumerate()
    {
        let is_sel = idx == selected;
        let rect = Rect::new(inner.x, inner.y + row as u16, inner.width, 1);
        let pin = if topic.pinned { "★ " } else { "" };
        let closed = if topic.closed { " [已关闭]" } else { "" };
        let unread = if topic.unread > 0 {
            format!(" ({})", topic.unread)
        } else {
            String::new()
        };
        let line = Line::from(vec![
            Span::styled(
                if is_sel { "› " } else { "  " },
                Style::default().fg(if is_sel {
                    palette().accent
                } else {
                    palette().muted
                }),
            ),
            Span::styled(
                format!("# {pin}{}{closed}", topic.title),
                Style::default().add_modifier(if is_sel {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(unread, Style::default().fg(palette().accent)),
        ]);
        draw_line(
            frame,
            rect,
            &line,
            if is_sel {
                palette().selection()
            } else {
                palette().surface_base()
            },
        );
        app.hit_targets.push((rect, Target::TopicEntry(idx)));
    }
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    crate::text::rows(text, width)
        .into_iter()
        .map(|row| text[row.start..row.end].to_owned())
        .collect()
}

fn scrolling_text(frame: &mut Frame, app: &mut App, lines: Vec<Line<'static>>, area: Rect) {
    app.modal_visible = usize::from(area.height).max(1);
    let initial_width = usize::from(area.width).max(1);
    let initial_total: usize = lines
        .iter()
        .map(|line| wrap_text(&line.to_string(), initial_width).len())
        .sum();
    let has_scroll = initial_total > usize::from(area.height) && area.width >= 4;
    let text_area = if has_scroll {
        Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height)
    } else {
        area
    };
    let total: usize = if has_scroll {
        lines
            .iter()
            .map(|line| wrap_text(&line.to_string(), usize::from(text_area.width).max(1)).len())
            .sum()
    } else {
        initial_total
    };
    let max_scroll = total.saturating_sub(usize::from(area.height));
    app.modal_scroll = app.modal_scroll.min(max_scroll as u16);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.modal_scroll, 0)),
        text_area,
    );
    if has_scroll {
        scrollbar(
            frame,
            app,
            area,
            Pane::Modal,
            usize::from(app.modal_scroll),
            max_scroll,
        );
    }
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
    fn bot_keyboards_share_emoji_width_background_and_hit_areas() {
        use crate::store::{InlineButton, InlineButtonKind};
        let mut app = fixture();
        let name = "✏️ Other (type answer)";
        app.store.messages[1].info.inline_keyboard = vec![
            vec![InlineButton {
                text: name.into(),
                kind: InlineButtonKind::Callback("YQ==".into()),
            }],
            vec![
                InlineButton {
                    text: "网址".repeat(9),
                    kind: InlineButtonKind::Url("https://example.com".into()),
                },
                InlineButton {
                    text: "确认".into(),
                    kind: InlineButtonKind::Callback("Yg==".into()),
                },
            ],
        ];
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let buffer = terminal
            .draw(|frame| draw(frame, &mut app))
            .unwrap()
            .buffer
            .clone();
        let get = |r, c| {
            app.hit_targets
                .iter()
                .find(|(_, target)| *target == Target::InlineButton(2, r, c))
                .unwrap()
                .0
        };
        let single = get(0, 0);
        let left = get(1, 0);
        let right = get(1, 1);
        assert_eq!(single.width, left.width + right.width + 1);
        assert_eq!(left.right() + 1, right.x);
        assert!(left.width.abs_diff(right.width) <= 1);
        let pencil = (single.x..single.right())
            .find(|&x| buffer[(x, single.y)].symbol() == "✏️")
            .unwrap();
        assert_eq!(buffer[(pencil + 2, single.y)].symbol(), " ");
        assert_eq!(buffer[(pencil + 3, single.y)].symbol(), "O");
        assert_eq!(
            buffer[(pencil, single.y)].bg,
            buffer[(pencil + 1, single.y)].bg
        );
        assert_eq!(
            buffer[(pencil, single.y)].bg,
            buffer[(pencil + 2, single.y)].bg
        );
        let previous =
            ratatui::buffer::Buffer::filled(buffer.area, ratatui::buffer::Cell::new("x"));
        assert!(
            !previous
                .diff(&buffer)
                .iter()
                .any(|(x, y, _)| *x == pencil + 1 && *y == single.y)
        );
        let output = snapshot(&mut app, 80, 24);
        assert!(output.contains(name));
        assert!(!output.contains("[✏"));
        assert!(output.contains("… ↗"));

        // Short single-column keyboards stay compact on wide terminals instead of stretching 100+ cols.
        app.store.messages[1].info.inline_keyboard = vec![
            vec![InlineButton {
                text: "1".into(),
                kind: InlineButtonKind::Callback("MQ==".into()),
            }],
            vec![InlineButton {
                text: name.into(),
                kind: InlineButtonKind::Callback("YQ==".into()),
            }],
        ];
        let mut wide = Terminal::new(TestBackend::new(160, 24)).unwrap();
        wide.draw(|frame| draw(frame, &mut app)).unwrap();
        let top_btn = app
            .hit_targets
            .iter()
            .find(|(_, target)| *target == Target::InlineButton(2, 0, 0))
            .unwrap()
            .0;
        let bottom_btn = app
            .hit_targets
            .iter()
            .find(|(_, target)| *target == Target::InlineButton(2, 1, 0))
            .unwrap()
            .0;
        assert_eq!(top_btn.width, bottom_btn.width);
        assert!(top_btn.width <= 36 && top_btn.width >= 26);

        // A VS16 emoji in the bottom-most footer notice must also use ForcedWidth so it never wraps/scrolls the screen.
        app.notice = Some("✏️ Type your answer in the chat.".into());
        let footer_buf = wide
            .draw(|frame| draw(frame, &mut app))
            .unwrap()
            .buffer
            .clone();
        let footer_y = footer_buf.area.bottom() - 1;
        assert_eq!(footer_buf[(0, footer_y)].symbol(), "✏️");
        assert_eq!(footer_buf[(2, footer_y)].symbol(), " ");
        let prev =
            ratatui::buffer::Buffer::filled(footer_buf.area, ratatui::buffer::Cell::new("x"));
        assert!(
            !prev
                .diff(&footer_buf)
                .iter()
                .any(|(x, y, _)| *x == 1 && *y == footer_y)
        );
    }

    #[test]
    fn folded_album_shows_one_caption_and_one_media_hit() {
        let mut app = fixture();
        for (index, message) in app.store.messages.iter_mut().enumerate() {
            message.text = "[图片] 相册共用说明".into();
            message.info.media_album_id = 22;
            message.media = Some(MediaRef {
                kind: MediaKind::Photo,
                file_id: index as i32 + 10,
                path: None,
                detail: None,
            });
        }
        let output = snapshot(&mut app, 80, 24);
        assert_eq!(output.matches("相册共用说明").count(), 1);
        assert!(output.contains("[相册 2/2]"));
        assert!(output.contains("o 大图"));
        assert_eq!(app.timeline_rows.len(), 1);
        assert_eq!(app.visible_media, [11]);
        assert!(
            !app.hit_targets
                .iter()
                .any(|(_, target)| *target == Target::Media(1))
        );
    }

    #[test]
    fn wrapped_quotes_and_code_keep_prefixes_styles_and_text_hits() {
        let mut app = fixture();
        let quote = "这一段引用包含足够多的文字可以在小终端里稳定折行，不丢掉左侧竖线。";
        let code = r#"let 中文 = "✏️";"#;
        let body = format!("{quote}\n{code}");
        app.store.messages[1].text = body;
        app.store.messages[1].info.entities = vec![
            TextEntity {
                start: 0,
                end: quote.len(),
                kind: EntityKind::BlockQuote,
            },
            TextEntity {
                start: quote.len() + 1,
                end: quote.len() + 1 + code.len(),
                kind: EntityKind::Pre("rust".into()),
            },
            TextEntity {
                start: 0,
                end: "这".len(),
                kind: EntityKind::Underline,
            },
        ];
        let mut terminal = Terminal::new(TestBackend::new(45, 22)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let first = app
            .hit_targets
            .iter()
            .find(|(_, target)| {
                *target
                    == Target::Text(Point {
                        source: Source::Message(2),
                        byte: 0,
                    })
            })
            .unwrap()
            .0;
        assert_eq!(buffer[(first.x - 2, first.y)].symbol(), "┃");
        assert_eq!(buffer[(first.x - 2, first.y + 1)].symbol(), "┃");
        assert!(
            buffer[(first.x, first.y)]
                .modifier
                .contains(Modifier::UNDERLINED)
        );
        let code_start = app
            .hit_targets
            .iter()
            .find(|(_, target)| {
                *target
                    == Target::Text(Point {
                        source: Source::Message(2),
                        byte: quote.len() + 1,
                    })
            })
            .unwrap()
            .0;
        assert_eq!(buffer[(code_start.x, code_start.y - 1)].symbol(), "r");
        assert_eq!(buffer[(code_start.x - 2, code_start.y)].symbol(), "┃");
        let end_hit = app
            .hit_targets
            .iter()
            .find(|(_, target)| {
                *target
                    == Target::Text(Point {
                        source: Source::Message(2),
                        byte: quote.len() + 1 + code.len(),
                    })
            })
            .unwrap()
            .0;
        assert_eq!(
            buffer[(end_hit.right() - 1, code_start.y)].bg,
            palette().surface
        );
    }

    #[test]
    fn composer_tools_stay_inside_the_border_and_shortcuts_follow_modal_context() {
        for (width, height) in [(80, 24), (120, 36)] {
            let mut app = fixture();
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
            let composer = app
                .hit_targets
                .iter()
                .find(|(_, target)| *target == Target::Composer)
                .unwrap()
                .0;
            assert!(composer.height >= 5);
            for action in [Action::File, Action::Stickers, Action::Submit] {
                let rect = app
                    .hit_targets
                    .iter()
                    .find(|(rect, target)| {
                        *target == Target::Command(action) && rect.y > composer.y
                    })
                    .unwrap()
                    .0;
                assert_eq!(rect.y, composer.bottom() - 2);
                assert_eq!(
                    terminal.backend().buffer()[(rect.x, rect.y + 1)].symbol(),
                    "─"
                );
            }
            let footer = shortcut_hint(&app, width);
            assert!(
                footer.contains("r 回复") && footer.contains("Ctrl+K") && footer.contains("Enter")
            );
            assert!(!footer.contains("I 详情"));
            app.input_mode = InputMode::Search;
            snapshot(&mut app, width, height);
            assert!(app.hit_targets.iter().any(|(rect, target)| {
                *target == Target::Command(Action::CycleMediaFilter)
                    && rect.y == composer.bottom() - 2
            }));
        }
        let mut app = fixture();
        app.proxy.open = true;
        assert!(shortcut_hint(&app, 40).contains("新增"));
        app.proxy.open = false;
        app.show_topics = true;
        assert!(shortcut_hint(&app, 40).contains("话题"));
        app.show_topics = false;
        app.link_picker = Some(crate::LinkPicker {
            items: Vec::new(),
            selected: 0,
        });
        assert!(shortcut_hint(&app, 40).contains("链接"));
        app.link_picker = None;
        app.instant_view = Some(crate::instant_view::Reader::new(
            "https://graph.org/example".into(),
        ));
        assert!(shortcut_hint(&app, 40).contains("浏览器"));
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
            let column = chat_column(&app, Rect::new(0, 1, width, height - 2));
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
        // Shared Intel CI runs image-heavy tests concurrently; wait for completion
        // under a total deadline rather than imposing a two-second speed requirement.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
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
                    json!({"x":x,"y":y,"text":cell.symbol(),"width":text_width(cell.symbol()).max(1),"fg":format!("{:?}",cell.fg),
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
                x += (text_width(symbol) as u16).max(1);
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
                "quick-switch",
                "chat-info",
                "multi-select",
                "rich-chat",
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
                    "quick-switch" => {
                        app.quick_switcher = Some(crate::QuickSwitcher {
                            global_messages: false,
                            query: "产品".into(),
                            cursor: "产品".len(),
                            selected: 0,
                            scroll: 0,
                        });
                    }
                    "chat-info" => {
                        if let Some(chat) = app.store.chat_mut_by_id(1) {
                            chat.subtitle = "12 位成员 · 3 人在线".into();
                            chat.username = "teleaf_dev".into();
                            chat.bio = "Teleaf 终端客户端产品与设计讨论群。".into();
                            chat.pinned = true;
                        }
                        app.show_chat_info = true;
                    }
                    "multi-select" => {
                        app.multi_select = vec![1, 2];
                    }
                    "rich-chat" => {
                        if let Some(chat) = app.store.chat_mut_by_id(1) {
                            chat.pinned_message = Some(crate::store::PinnedBanner {
                                id: 1,
                                excerpt: "周五前完成桌面端对齐评审".into(),
                            });
                            chat.unread = 2;
                            chat.read_inbox = 1;
                            chat.pinned = true;
                            chat.mention_count = 1;
                            chat.last_stamp = crate::calendar::local(1700000060);
                        }
                        app.store.messages[0].info.reactions = vec![
                            crate::store::Reaction {
                                emoji: "👍".into(),
                                custom_emoji_id: None,
                                count: 3,
                                chosen: true,
                            },
                            crate::store::Reaction {
                                emoji: "🔥".into(),
                                custom_emoji_id: None,
                                count: 1,
                                chosen: false,
                            },
                        ];
                        app.store.messages[0].info.entities = vec![crate::store::TextEntity {
                            start: 0,
                            end: "我们".len(),
                            kind: crate::store::EntityKind::Bold,
                        }];
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
        assert!(lines.iter().all(|line| text_width(line.as_str()) <= 8));
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

    #[test]
    fn folder_modal_renders_distinct_keyboard_cursor_and_keeps_tab_offset() {
        let mut app = fixture();
        app.store.apply(&json!({
            "@type": "updateChatFolders",
            "chat_folders": [
                {"id": 7, "name": {"text": {"text": "工作"}}},
                {"id": 8, "name": {"text": {"text": "朋友"}}}
            ]
        }));
        app.folder_offset = 1;
        app.show_folders = true;
        app.folder_cursor = 2;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let rect = app
            .hit_targets
            .iter()
            .find(|(_, target)| *target == Target::Folder(crate::store::ChatList::Folder(8)))
            .unwrap()
            .0;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(rect.x, rect.y)].symbol(), "›");
        assert_eq!(buffer[(rect.x, rect.y)].bg, palette().selected);
        assert_eq!(app.folder_offset, 1);
    }

    #[test]
    fn error_states_use_error_color_without_tinting_login_hints() {
        let mut app = fixture();
        app.auth.state.clear();
        app.tdlib_connected = false;
        app.status = "无法连接，请在设置中查看 TDLib 状态".into();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut found_error_text = false;
        for y in 1..23 {
            for x in 0..80 {
                if buffer[(x, y)].symbol() == "无" {
                    assert_eq!(buffer[(x, y)].fg, palette().error);
                    found_error_text = true;
                    break;
                }
            }
        }
        assert!(found_error_text, "connection error text not rendered");

        // Login form validation error must not tint the bottom keyboard hint red.
        app.auth.begin_setup();
        app.auth.message = "API ID 应为大于零的数字".into();
        app.auth.is_error = true;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 23)].fg, palette().muted);

        // Non-retryable failed message must still display its failure status in error color.
        let mut app = fixture();
        app.store.messages[1].info.sending = crate::store::Sending::Failed {
            can_retry: false,
            reason: "禁止发送".into(),
        };
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut found_failed_status = false;
        for y in 1..20 {
            let mut x = 0u16;
            while x < 80 {
                if buffer[(x, y)].symbol() == "发"
                    && x + 6 < 80
                    && buffer[(x + 2, y)].symbol() == "送"
                    && buffer[(x + 4, y)].symbol() == "失"
                    && buffer[(x + 6, y)].symbol() == "败"
                {
                    assert_eq!(buffer[(x, y)].fg, palette().error);
                    found_failed_status = true;
                    break;
                }
                x += (text_width(buffer[(x, y)].symbol()) as u16).max(1);
            }
            if found_failed_status {
                break;
            }
        }
        assert!(found_failed_status, "non-retryable failed status missing");
    }

    #[test]
    fn small_sticker_cursor_quote_ellipsis_and_mouse_off_badge_render() {
        let mut app = fixture();
        app.mouse_enabled = false;
        app.store.messages[1].info.reply = Some(crate::store::Reply {
            chat_id: 1,
            message_id: 1,
            excerpt: "这是一段非常非常长的被引用消息内容用于验证末尾省略号不会被裁切掉".into(),
        });
        let output = snapshot(&mut app, 45, 18);
        assert!(output.contains("[鼠标关]"));
        assert!(
            output
                .lines()
                .any(|line| line.contains('│') && line.contains('…')),
            "reply quote ellipsis clipped: {output}"
        );

        app.sticker_picker = true;
        app.sticker_cursor = 0;
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
        let mut terminal = Terminal::new(TestBackend::new(30, 12)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let tile = app
            .hit_targets
            .iter()
            .find(|(_, t)| *t == Target::Sticker(0))
            .unwrap()
            .0;
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(tile.x, tile.y)].bg, palette().selected);
    }

    #[test]
    fn help_and_settings_scrollbars_and_theme_toggle_render() {
        let mut app = fixture();
        app.show_help = true;
        snapshot(&mut app, 45, 18);
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| matches!(t, Target::Scroll(Pane::Modal, _)))
        );
        app.close_overlays();
        app.show_settings = true;
        let output = snapshot(&mut app, 80, 24);
        assert!(output.contains("配色："));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Command(Action::CycleTheme))
        );
    }

    #[test]
    fn desktop_parity_quick_switcher_chat_info_multiselect_and_rich_timeline_render() {
        let mut app = fixture();
        // Quick switcher filters loaded chats and exposes clickable jump targets.
        app.quick_switcher = Some(crate::QuickSwitcher {
            global_messages: false,
            query: "小林".into(),
            cursor: "小林".len(),
            selected: 0,
            scroll: 0,
        });
        let output = snapshot(&mut app, 80, 24);
        assert!(output.contains("快速切换会话") && output.contains("小林"));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::QuickSwitchChat(2))
        );
        app.quick_switcher = None;

        // Chat info is an inline sidebar at >= 120 cols and a modal below 120 cols.
        if let Some(chat) = app.store.chat_mut_by_id(1) {
            chat.subtitle = "12 位成员".into();
            chat.username = "teleaf_dev".into();
            chat.bio = "终端客户端讨论组".into();
            chat.pinned_message = Some(crate::store::PinnedBanner {
                id: 1,
                excerpt: "置顶公告".into(),
            });
            chat.unread = 2;
            chat.read_inbox = 1;
        }
        app.show_chat_info = true;
        let wide = snapshot(&mut app, 130, 32);
        assert!(!app.chat_info_modal);
        assert!(wide.contains("@teleaf_dev") && wide.contains("终端客户端讨论组"));
        let narrow = snapshot(&mut app, 80, 24);
        assert!(app.chat_info_modal);
        assert!(narrow.contains("会话详情") && narrow.contains("@teleaf_dev"));
        app.show_chat_info = false;

        // Pinned banner, unread divider, reactions, media progress card, spoiler & dynamic composer.
        app.store.messages[0].info.reactions = vec![crate::store::Reaction {
            emoji: "👍".into(),
            custom_emoji_id: None,
            count: 4,
            chosen: true,
        }];
        app.store.messages[0].info.entities = vec![crate::store::TextEntity {
            start: 0,
            end: "我们".len(),
            kind: crate::store::EntityKind::Spoiler,
        }];
        app.store.messages[1].outgoing = false;
        app.store.messages[1].media = Some(MediaRef {
            file_id: 90,
            path: None,
            kind: MediaKind::Document,
            detail: None,
        });
        app.store.messages[1].info.media_meta = Some(crate::store::MediaMeta {
            file_name: "design.pdf".into(),
            size_bytes: 2 * 1024 * 1024,
            downloaded_bytes: 1024 * 1024,
            duration_secs: 0,
            downloading: true,
        });
        app.input_mode = InputMode::Send;
        app.draft = "第一行\n第二行\n第三行\n第四行".into();
        let rich = snapshot(&mut app, 110, 32);
        assert!(rich.contains("置顶：置顶公告"));
        assert!(rich.contains("2 条未读消息"));
        assert!(rich.contains("👍 4"));
        assert!(rich.contains("design.pdf") && rich.contains("50%"));
        assert!(rich.contains('░'));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::PinnedBanner(1))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Reaction(1, 0))
        );
        assert!(composer_height(&app, 70, 30) > 4);

        // Multi-select replaces composer with batch action bar.
        app.multi_select = vec![1, 2];
        let multi = snapshot(&mut app, 110, 32);
        assert!(multi.contains("多选模式 · 已选 2 条消息"));
        assert!(multi.contains("[✓]"));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::Command(Action::MultiForward))
        );
    }

    #[test]
    fn proxy_modal_link_preview_inline_keyboard_comments_albums_and_topics_render() {
        let mut app = fixture();
        // 1. Proxy list & add form (masks password).
        app.proxy.open = true;
        app.proxy.entries = vec![crate::proxy::ProxyEntry {
            id: 1,
            server: "127.0.0.1".into(),
            port: 7890,
            kind: crate::proxy::ProxyKind::Socks5,
            enabled: true,
            ping_ms: Some(42),
            pinging: false,
            ping_error: None,
        }];
        let list_view = snapshot(&mut app, 110, 32);
        assert!(list_view.contains("代理设置") && list_view.contains("SOCKS5 127.0.0.1:7890"));
        assert!(list_view.contains("42 ms"));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::ProxyEntry(0))
        );

        app.proxy.form = Some(crate::proxy::ProxyForm {
            password: "super-secret".into(),
            password_cursor: 12,
            ..Default::default()
        });
        let form_view = snapshot(&mut app, 110, 32);
        assert!(form_view.contains("新增代理") && !form_view.contains("super-secret"));
        app.proxy.open = false;
        app.proxy.form = None;

        // 2. Link preview, inline keyboard, comment pill & album grouping.
        app.store.messages[0].info.media_album_id = 999;
        app.store.messages[1].info.media_album_id = 999;
        app.store.messages[0].info.link_preview = Some(crate::store::LinkPreview {
            instant_view_version: 0,
            site_name: "GitHub".into(),
            title: "Teleaf Repo".into(),
            description: "终端客户端仓库".into(),
            url: "https://github.com/YoisakiKnd/teleaf".into(),
        });
        app.store.messages[0].info.inline_keyboard = vec![vec![
            crate::store::InlineButton {
                text: "官网".into(),
                kind: crate::store::InlineButtonKind::Url("https://telegram.org".into()),
            },
            crate::store::InlineButton {
                text: "确认".into(),
                kind: crate::store::InlineButtonKind::Callback("b2s=".into()),
            },
        ]];
        app.store.messages[0].info.can_get_thread = true;
        app.store.messages[0].info.reply_count = 7;
        app.timeline_heights.clear();
        app.selected_message = Some(1);
        let timeline = snapshot(&mut app, 110, 36);
        assert!(timeline.contains("[相册 1/2]"));
        assert_eq!(app.timeline_rows.len(), 1);
        assert!(timeline.contains("GitHub · Teleaf Repo") && timeline.contains("终端客户端仓库"));
        assert!(timeline.contains("官网 ↗") && timeline.contains("确认"));
        assert!(timeline.contains("7 条评论"));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::InlineButton(1, 0, 1))
        );
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::CommentThread(1))
        );

        // 3. Link & mention picker modal.
        app.link_picker = Some(crate::LinkPicker {
            items: vec![
                crate::store::LinkItem::Url("https://example.com".into()),
                crate::store::LinkItem::Mention("@teleaf".into()),
            ],
            selected: 0,
        });
        let picker = snapshot(&mut app, 100, 30);
        assert!(picker.contains("消息中的链接与 @提及") && picker.contains("@teleaf"));
        app.link_picker = None;

        // 4. Forum topics modal.
        if let Some(chat) = app.store.chat_mut_by_id(1) {
            chat.is_forum = true;
            chat.topics = vec![crate::store::ForumTopic {
                thread_id: 42,
                title: "发布公告".into(),
                unread: 3,
                pinned: true,
                order: 0,
                closed: false,
                last_preview: "v0.3.0".into(),
            }];
        }
        app.show_topics = true;
        let topics = snapshot(&mut app, 100, 30);
        assert!(topics.contains("群组话题") && topics.contains("发布公告"));
        assert!(
            app.hit_targets
                .iter()
                .any(|(_, t)| *t == Target::TopicEntry(0))
        );
    }
}
