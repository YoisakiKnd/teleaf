//! Byte cursors on grapheme boundaries; terminal columns are display widths.
use crossterm::event::KeyCode;
use unicode_segmentation::UnicodeSegmentation;

pub fn grapheme_width(grapheme: &str) -> usize {
    use ratatui::buffer::CellWidth;

    if grapheme.chars().any(char::is_control) {
        return 0;
    }
    let width = usize::from(grapheme.cell_width());
    if width == 1
        && grapheme.contains('\u{fe0f}')
        && grapheme.starts_with(|ch: char| !ch.is_ascii())
    {
        2
    } else {
        width
    }
}

pub fn width(text: &str) -> usize {
    text.graphemes(true).map(grapheme_width).sum()
}

pub const MAX_WEB_URL_BYTES: usize = 4096;

fn web_host(url: &str) -> Option<&str> {
    let (scheme, rest) = url.split_once("://")?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        || url.len() > MAX_WEB_URL_BYTES
        || url.contains(|ch: char| ch.is_whitespace() || ch.is_control() || ch == '\\')
    {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = if let Some(ip) = authority.strip_prefix('[') {
        let (ip, suffix) = ip.split_once(']')?;
        ip.parse::<std::net::Ipv6Addr>().ok()?;
        (
            ip,
            if suffix.is_empty() {
                None
            } else {
                Some(suffix.strip_prefix(':')?)
            },
        )
    } else {
        authority
            .split_once(':')
            .map_or((authority, None), |(host, port)| (host, Some(port)))
    };
    if host.is_empty()
        || host.contains(['@', '[', ']'])
        || port.is_some_and(|port| {
            port.is_empty()
                || !port.bytes().all(|byte| byte.is_ascii_digit())
                || port.parse::<u16>().is_err()
        })
    {
        return None;
    }
    Some(host)
}

pub fn web_url(url: &str) -> bool {
    web_host(url).is_some()
}

pub fn is_telegraph(url: &str) -> bool {
    web_host(url).is_some_and(|host| {
        let host = host.strip_suffix('.').unwrap_or(host);
        host.eq_ignore_ascii_case("telegra.ph") || host.eq_ignore_ascii_case("graph.org")
    })
}

pub fn cursor(text: &str, position: usize) -> usize {
    if position >= text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(i, _)| i)
        .take_while(|i| *i <= position)
        .last()
        .unwrap_or(0)
}

pub fn insert(text: &mut String, position: &mut usize, value: &str, limit: usize) {
    *position = cursor(text, *position);
    let mut end = 0;
    for (i, g) in value.grapheme_indices(true) {
        if text.len() + i + g.len() > limit {
            break;
        }
        end = i + g.len();
    }
    text.insert_str(*position, &value[..end]);
    *position += end;
}

pub fn edit(text: &mut String, position: &mut usize, key: KeyCode) -> bool {
    *position = cursor(text, *position);
    let before = *position;
    let prev = text[..*position]
        .grapheme_indices(true)
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0);
    let next = text[*position..]
        .graphemes(true)
        .next()
        .map(|g| *position + g.len())
        .unwrap_or(*position);
    match key {
        KeyCode::Left => *position = prev,
        KeyCode::Right => *position = next,
        KeyCode::Home => *position = text[..*position].rfind('\n').map(|i| i + 1).unwrap_or(0),
        KeyCode::End => {
            *position = text[*position..]
                .find('\n')
                .map(|i| *position + i)
                .unwrap_or(text.len())
        }
        KeyCode::Backspace if prev < *position => {
            text.replace_range(prev..*position, "");
            *position = prev;
            return true;
        }
        KeyCode::Delete if next > *position => {
            text.replace_range(*position..next, "");
            return true;
        }
        KeyCode::Char(c) => {
            insert(text, position, &c.to_string(), 262144);
            return true;
        }
        _ => {}
    }
    before != *position
}

pub fn prev_word(text: &str, position: usize) -> usize {
    let pos = cursor(text, position);
    if pos == 0 {
        return 0;
    }
    let slice = &text[..pos];
    let mut graphemes: Vec<(usize, &str)> = slice.grapheme_indices(true).collect();
    while let Some(&(_, g)) = graphemes.last() {
        if g.chars().all(char::is_whitespace) {
            graphemes.pop();
        } else {
            break;
        }
    }
    let Some(&(_, last)) = graphemes.last() else {
        return 0;
    };
    let is_word = |g: &str| {
        g.chars()
            .any(|c| c.is_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace()))
    };
    let target_word = is_word(last);
    let mut start = graphemes.last().unwrap().0;
    while let Some(&(i, g)) = graphemes.last() {
        if is_word(g) == target_word && !g.chars().all(char::is_whitespace) {
            start = i;
            graphemes.pop();
        } else {
            break;
        }
    }
    start
}

pub fn next_word(text: &str, position: usize) -> usize {
    let pos = cursor(text, position);
    if pos >= text.len() {
        return text.len();
    }
    let mut iter = text[pos..].grapheme_indices(true).peekable();
    while let Some(&(_, g)) = iter.peek() {
        if g.chars().all(char::is_whitespace) {
            iter.next();
        } else {
            break;
        }
    }
    let Some(&(_, first)) = iter.peek() else {
        return text.len();
    };
    let is_word = |g: &str| {
        g.chars()
            .any(|c| c.is_alphanumeric() || c == '_' || (!c.is_ascii() && !c.is_whitespace()))
    };
    let target_word = is_word(first);
    let mut end = pos;
    for (i, g) in iter {
        if is_word(g) == target_word && !g.chars().all(char::is_whitespace) {
            end = pos + i + g.len();
        } else {
            break;
        }
    }
    end
}

pub fn delete_prev_word(text: &mut String, position: &mut usize) -> bool {
    *position = cursor(text, *position);
    let target = prev_word(text, *position);
    if target < *position {
        text.replace_range(target..*position, "");
        *position = target;
        true
    } else {
        false
    }
}

pub fn move_vertical(text: &str, position: usize, width: usize, delta: isize) -> usize {
    let pos = cursor(text, position);
    let list = rows(text, width);
    if list.len() <= 1 || delta == 0 {
        return pos;
    }
    let current = list.iter().rposition(|row| row.start <= pos).unwrap_or(0);
    let target = current.saturating_add_signed(delta).min(list.len() - 1);
    if target == current {
        return pos;
    }
    let col = self::width(&text[list[current].start..pos.min(list[current].end)]);
    let row = list[target];
    let mut used = 0;
    for (i, g) in text[row.start..row.end].grapheme_indices(true) {
        let size = grapheme_width(g);
        if used + size > col {
            return row.start + i;
        }
        used += size;
    }
    row.end
}

#[derive(Clone, Copy)]
pub struct Row {
    pub start: usize,
    pub end: usize,
}

pub fn rows(text: &str, width: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    scan_rows(text, width, |row| rows.push(row));
    rows
}

pub fn row_count(text: &str, width: usize) -> usize {
    let mut count = 0;
    scan_rows(text, width, |_| count += 1);
    count
}

pub fn ellipsize(text: &str, width: usize) -> String {
    if self::width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        let size = grapheme_width(g);
        if used + size >= width {
            break;
        }
        output.push_str(g);
        used += size;
    }
    output.push('…');
    output
}

pub fn scan_rows(text: &str, width: usize, mut emit: impl FnMut(Row)) {
    let width = width.max(1);
    let mut start = 0;
    let mut used = 0;
    for (i, g) in text.grapheme_indices(true) {
        if g == "\n" || g == "\r\n" {
            emit(Row { start, end: i });
            start = i + g.len();
            used = 0;
        } else {
            let size = grapheme_width(g);
            if used > 0 && used + size > width {
                emit(Row { start, end: i });
                start = i;
                used = 0;
            }
            used += size;
        }
    }
    emit(Row {
        start,
        end: text.len(),
    });
}

#[cfg(test)]
pub fn hit(text: &str, row: Row, column: usize, masked: bool) -> usize {
    let mut used = 0;
    for (i, g) in text[row.start..row.end].grapheme_indices(true) {
        let size = if masked { 1 } else { grapheme_width(g) };
        if column < used + size {
            return row.start + i;
        }
        used += size;
    }
    row.end
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emoji_presentation_uses_the_same_columns_in_wrap_hit_and_ellipsis() {
        let pencil = "✏️";
        let value = "✏️中a";
        assert_eq!(width(pencil), 2);
        assert_eq!(width(value), 5);
        let rows = rows(value, 3);
        assert_eq!(&value[rows[0].start..rows[0].end], pencil);
        assert_eq!(&value[rows[1].start..rows[1].end], "中a");
        assert_eq!(
            hit(
                value,
                Row {
                    start: 0,
                    end: value.len()
                },
                1,
                false
            ),
            0
        );
        assert_eq!(
            hit(
                value,
                Row {
                    start: 0,
                    end: value.len()
                },
                2,
                false
            ),
            pencil.len()
        );
        assert_eq!(ellipsize(value, 3), "✏️…");
        assert_eq!(width("a\u{fe0f}"), 1);
    }

    #[test]
    fn telegraph_capability_matches_the_url_host() {
        for url in [
            "HTTPS://Telegra.Ph/article",
            "http://graph.org:443/article",
            "https://graph.org./article",
        ] {
            assert!(is_telegraph(url));
        }
        for url in [
            "https://telegra.ph.example.com/test",
            "https://telegra.ph@other.test/test",
            "https://other.test/?page=https://graph.org",
            "https://graph.org:bad/test",
            "file://graph.org/test",
        ] {
            assert!(!is_telegraph(url), "{url}");
        }
    }

    #[test]
    fn unicode_cursor_insert_delete_and_click_use_graphemes() {
        let mut text = "中👨‍👩‍👧‍👦e\u{301}文".to_owned();
        let mut pos = usize::MAX;
        edit(&mut text, &mut pos, KeyCode::Left);
        edit(&mut text, &mut pos, KeyCode::Backspace);
        assert_eq!(text, "中👨‍👩‍👧‍👦文");
        insert(&mut text, &mut pos, "好", 100);
        assert_eq!(text, "中👨‍👩‍👧‍👦好文");
        let row = rows(&text, 20)[0];
        assert_eq!(hit(&text, row, 2, false), "中".len());
        assert_eq!(hit(&text, row, 3, false), "中".len());
        assert_eq!(row_count("ab\n中文\n", 2), 4);
    }

    #[test]
    fn vertical_movement_and_word_editing_respect_graphemes_and_widths() {
        let mut text = "hello world\n中文测试行".to_owned();
        let mut pos = text.len();
        let up = move_vertical(&text, pos, 20, -1);
        assert!(up <= "hello world".len());
        let down = move_vertical(&text, up, 20, 1);
        assert!(down > "hello world".len());
        assert!(delete_prev_word(&mut text, &mut pos));
        assert_eq!(text, "hello world\n");
        let word_end = next_word("hello world", 0);
        assert_eq!(word_end, 5);
        assert_eq!(prev_word("hello world", 11), 6);
    }
}
