//! Selection positions refer to original text, independent of wrapping and scrolling.
use crate::{App, text};
use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Composer,
    Message(i64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub source: Source,
    pub byte: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
}

pub fn value(app: &App, source: Source) -> Option<&str> {
    match source {
        Source::Composer => Some(&app.draft),
        Source::Message(id) => app
            .active_messages()
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.text.as_str()),
    }
}
fn rank(app: &App, source: Source) -> Option<usize> {
    match source {
        Source::Composer => Some(0),
        Source::Message(id) => app.active_messages().iter().position(|m| m.id == id),
    }
}
pub fn compatible(a: Source, b: Source) -> bool {
    matches!(
        (a, b),
        (Source::Composer, Source::Composer) | (Source::Message(_), Source::Message(_))
    )
}
pub fn ordered(app: &App) -> Option<(Point, Point)> {
    let s = app.selection?;
    if !compatible(s.anchor.source, s.head.source) {
        return None;
    }
    let normalize = |p: Point| -> Option<Point> {
        Some(Point {
            byte: text::cursor(value(app, p.source)?, p.byte),
            ..p
        })
    };
    let a = normalize(s.anchor)?;
    let b = normalize(s.head)?;
    if (rank(app, a.source)?, a.byte) <= (rank(app, b.source)?, b.byte) {
        Some((a, b))
    } else {
        Some((b, a))
    }
}
pub fn range(app: &App, source: Source) -> Option<Range<usize>> {
    let (a, b) = ordered(app)?;
    if !compatible(a.source, source) {
        return None;
    }
    let index = rank(app, source)?;
    if index < rank(app, a.source)? || index > rank(app, b.source)? {
        return None;
    }
    let start = if source == a.source { a.byte } else { 0 };
    let end = if source == b.source {
        b.byte
    } else {
        value(app, source)?.len()
    };
    Some(start..end)
}
pub fn active(app: &App) -> bool {
    ordered(app).is_some_and(|(a, b)| a != b)
}
pub fn selected_text(app: &App) -> Result<Option<String>, String> {
    let Some((a, b)) = ordered(app) else {
        return Ok(None);
    };
    if a == b {
        return Ok(None);
    }
    if a.source == Source::Composer {
        return Ok(Some(app.draft[a.byte..b.byte].to_owned()));
    }
    let mut result = String::new();
    let from = rank(app, a.source).expect("rank");
    let to = rank(app, b.source).expect("rank");
    for (offset, message) in app.active_messages()[from..=to].iter().enumerate() {
        let span = range(app, Source::Message(message.id)).expect("range");
        if result.len() + span.len() + 1 > 1024 * 1024 {
            return Err("选中文字超过 1 MiB，请缩小选择范围".into());
        }
        if offset > 0 {
            result.push('\n');
        }
        result.push_str(&message.text[span]);
    }
    Ok(Some(result))
}
pub fn word(app: &mut App, point: Point) -> bool {
    let Some(value) = value(app, point.source) else {
        return false;
    };
    let byte = text::cursor(value, point.byte);
    let span = value
        .unicode_word_indices()
        .find(|(start, word)| byte >= *start && byte < start + word.len())
        .map(|(start, word)| start..start + word.len())
        .or_else(|| {
            value
                .grapheme_indices(true)
                .find(|(start, g)| byte >= *start && byte < start + g.len())
                .map(|(start, g)| start..start + g.len())
        });
    let Some(span) = span else {
        return false;
    };
    app.selection = Some(Selection {
        anchor: Point {
            byte: span.start,
            ..point
        },
        head: Point {
            byte: span.end,
            ..point
        },
    });
    if point.source == Source::Composer {
        app.draft_cursor = span.end;
    }
    true
}
pub fn remove_from_draft(app: &mut App) -> bool {
    let Some(span) = range(app, Source::Composer).filter(|r| !r.is_empty()) else {
        app.selection = None;
        return false;
    };
    app.draft_cursor = span.start;
    app.draft.replace_range(span, "");
    app.selection = None;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_preserves_original_newlines_and_handles_reverse_ranges() {
        let mut app = crate::ui::tests::fixture();
        app.store.messages[0].text = "中文\n👨‍👩‍👧‍👦末尾".into();
        app.store.messages[1].text = "第二条 e\u{301} message".into();
        app.selection = Some(Selection {
            anchor: Point {
                source: Source::Message(1),
                byte: 3,
            },
            head: Point {
                source: Source::Message(2),
                byte: "第二条 e\u{301}".len(),
            },
        });
        let expected = Some("文\n👨‍👩‍👧‍👦末尾\n第二条 e\u{301}".to_owned());
        assert_eq!(selected_text(&app).unwrap(), expected);
        let s = app.selection.unwrap();
        app.selection = Some(Selection {
            anchor: s.head,
            head: s.anchor,
        });
        assert_eq!(selected_text(&app).unwrap(), expected);
        app.store.messages.remove(0);
        assert!(!active(&app));
        assert_eq!(selected_text(&app).unwrap(), None);
    }
    #[test]
    fn word_selection_and_replacement_preserve_combined_graphemes() {
        let mut app = crate::ui::tests::fixture();
        app.draft = "hello e\u{301} 👨‍👩‍👧‍👦 中文".into();
        word(
            &mut app,
            Point {
                source: Source::Composer,
                byte: 2,
            },
        );
        assert_eq!(selected_text(&app).unwrap(), Some("hello".into()));
        word(
            &mut app,
            Point {
                source: Source::Composer,
                byte: "hello e\u{301} ".len(),
            },
        );
        assert_eq!(selected_text(&app).unwrap(), Some("👨‍👩‍👧‍👦".into()));
        assert!(remove_from_draft(&mut app));
        crate::text::insert(&mut app.draft, &mut app.draft_cursor, "好", 262144);
        assert_eq!(app.draft, "hello e\u{301} 好 中文");
    }
}
