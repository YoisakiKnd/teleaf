//! Byte cursors on grapheme boundaries; terminal columns are display widths.
use crossterm::event::KeyCode;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

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
    if UnicodeWidthStr::width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut output = String::new();
    let mut used = 0;
    for g in text.graphemes(true) {
        let size = UnicodeWidthStr::width(g);
        if used + size >= width {
            break;
        }
        output.push_str(g);
        used += size;
    }
    output.push('…');
    output
}

fn scan_rows(text: &str, width: usize, mut emit: impl FnMut(Row)) {
    let width = width.max(1);
    let mut start = 0;
    let mut used = 0;
    for (i, g) in text.grapheme_indices(true) {
        if g == "\n" || g == "\r\n" {
            emit(Row { start, end: i });
            start = i + g.len();
            used = 0;
        } else {
            let size = UnicodeWidthStr::width(g);
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
        let size = if masked { 1 } else { UnicodeWidthStr::width(g) };
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
}
