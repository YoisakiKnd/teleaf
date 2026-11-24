//! A bounded Markdown subset for outgoing TDLib formattedText.
use serde_json::{Value, json};

const MAX_ENTITIES: usize = 64;
const MAX_DEPTH: usize = 8;

pub fn formatted_text(input: &str) -> Value {
    let mut parser = Parser {
        text: String::with_capacity(input.len()),
        units: 0,
        entities: Vec::new(),
        work: input.len().saturating_mul(12),
    };
    parser.parse(input, 0, false);
    parser.entities.sort_by_key(|entity| {
        (
            entity["offset"].as_i64(),
            std::cmp::Reverse(entity["length"].as_i64()),
        )
    });
    json!({"@type":"formattedText","text":parser.text,"entities":parser.entities})
}

struct Parser {
    text: String,
    units: usize,
    entities: Vec<Value>,
    work: usize,
}

struct Markup<'a> {
    body: &'a str,
    rest: &'a str,
    kind: &'static str,
    argument: &'a str,
    literal: bool,
}

impl Parser {
    fn push(&mut self, value: &str) {
        self.text.push_str(value);
        self.units += value.encode_utf16().count();
    }

    fn parse(&mut self, mut rest: &str, depth: usize, in_link: bool) {
        let mut previous = None;
        while !rest.is_empty() {
            if self.work == 0 || depth >= MAX_DEPTH || self.entities.len() == MAX_ENTITIES {
                self.push(rest);
                return;
            }
            let ch = rest.chars().next().unwrap();
            if ch == '\\'
                && let Some(next) = rest[1..].chars().next()
                && "\\*_`~|[]".contains(next)
            {
                self.push(&rest[..2]);
                rest = &rest[2..];
                previous = Some(next);
                continue;
            }
            let markup = match ch {
                '`' if rest.starts_with("```") => {
                    if depth == 0 && previous.is_none_or(|c| c == '\n') {
                        let Some(fence) = self.fence(rest) else {
                            self.push(rest);
                            return;
                        };
                        Some(fence)
                    } else {
                        None
                    }
                }
                '[' if !in_link => self.link(rest),
                '*' => self.inline(rest, if rest.starts_with("**") { "**" } else { "*" }),
                '_' if !previous.is_some_and(char::is_alphanumeric) => self.inline(rest, "_"),
                '`' if !in_link => self.inline(rest, "`"),
                '~' if rest.starts_with("~~") => self.inline(rest, "~~"),
                '|' if rest.starts_with("||") => self.inline(rest, "||"),
                _ => None,
            };
            if let Some(markup) = markup {
                let start = self.units;
                // Reserve the outer entity before parsing nested styles.
                let index = self.entities.len();
                self.entities.push(Value::Null);
                if markup.literal {
                    self.push(markup.body);
                } else {
                    self.parse(
                        markup.body,
                        depth + 1,
                        in_link || markup.kind == "textEntityTypeTextUrl",
                    );
                }
                let mut kind = json!({"@type":markup.kind});
                match markup.kind {
                    "textEntityTypePreCode" => kind["language"] = json!(markup.argument),
                    "textEntityTypeTextUrl" => kind["url"] = json!(markup.argument),
                    _ => {}
                }
                self.entities[index] = json!({
                    "@type":"textEntity","offset":start,"length":self.units - start,"type":kind
                });
                previous = rest[..rest.len() - markup.rest.len()].chars().next_back();
                rest = markup.rest;
            } else {
                // Consume marker runs together so an unmatched ** does not turn into *.
                let size = if "*_`~|".contains(ch) {
                    rest.bytes().take_while(|&b| b == ch as u8).count()
                } else {
                    ch.len_utf8()
                };
                self.push(&rest[..size]);
                rest = &rest[size..];
                previous = Some(ch);
            }
        }
    }

    // Charge each scan against the same work budget, including recursive parses.
    fn until(&mut self, text: &str, marker: char, multiline: bool) -> Option<usize> {
        let mut escaped = false;
        for (i, ch) in text.char_indices() {
            self.work = self.work.checked_sub(1)?;
            if !multiline && (ch == '\n' || ch == '\r') {
                break;
            }
            if ch == marker && !escaped {
                return Some(i);
            }
            escaped = ch == '\\' && !escaped;
        }
        None
    }

    fn inline<'a>(&mut self, input: &'a str, marker: &str) -> Option<Markup<'a>> {
        let body = &input[marker.len()..];
        let first = body.chars().next()?;
        if first == marker.chars().next()? || (marker != "`" && first.is_whitespace()) {
            return None;
        }
        let ch = marker.chars().next()?;
        let mut offset = 0;
        while let Some(found) = self.until(&body[offset..], ch, false) {
            let end = offset + found;
            let run = body[end..].bytes().take_while(|&b| b == ch as u8).count();
            self.work = self.work.saturating_sub(run.saturating_sub(1));
            if self.work == 0 {
                return None;
            }
            offset = end + run;
            if run == marker.len()
                && end > 0
                && (marker == "`" || !body[..end].ends_with(char::is_whitespace))
                && (marker != "_" || !body[offset..].starts_with(char::is_alphanumeric))
            {
                let kind = match marker {
                    "**" => "textEntityTypeBold",
                    "*" | "_" => "textEntityTypeItalic",
                    "`" => "textEntityTypeCode",
                    "~~" => "textEntityTypeStrikethrough",
                    "||" => "textEntityTypeSpoiler",
                    _ => unreachable!(),
                };
                return Some(Markup {
                    body: &body[..end],
                    rest: &body[offset..],
                    kind,
                    argument: "",
                    literal: marker == "`",
                });
            }
        }
        None
    }

    fn link<'a>(&mut self, input: &'a str) -> Option<Markup<'a>> {
        let input = &input[1..];
        let end = self.until(input, ']', false)?;
        let url_and_rest = input[end + 1..].strip_prefix('(')?;
        let mut parentheses = 0usize;
        let mut url_end = None;
        for (i, ch) in url_and_rest.char_indices() {
            self.work = self.work.checked_sub(1)?;
            match ch {
                '(' => parentheses += 1,
                ')' if parentheses == 0 => {
                    url_end = Some(i);
                    break;
                }
                ')' => parentheses -= 1,
                _ if ch.is_whitespace() || ch.is_control() || ch == '\\' => return None,
                _ => {}
            }
        }
        let url_end = url_end?;
        let url = &url_and_rest[..url_end];
        let body = &input[..end];
        (end > 0 && !body.contains('[') && crate::text::web_url(url)).then_some(Markup {
            body,
            rest: &url_and_rest[url_end + 1..],
            kind: "textEntityTypeTextUrl",
            argument: url,
            literal: false,
        })
    }

    fn fence<'a>(&mut self, input: &'a str) -> Option<Markup<'a>> {
        let input = &input[3..];
        let header_end = self.until(input, '\n', true)?;
        let language = input[..header_end].trim();
        if language.len() > 32
            || !language
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-+#.".contains(&b))
        {
            return None;
        }
        let body = &input[header_end + 1..];
        let mut offset = 0;
        for line in body.split_inclusive('\n') {
            self.work = self.work.saturating_sub(line.len());
            if self.work == 0 {
                return None;
            }
            if line.trim_end_matches(['\r', '\n', ' ', '\t']) == "```" {
                let code = body[..offset].strip_suffix('\n')?;
                let code = code.strip_suffix('\r').unwrap_or(code);
                let rest = &body[offset + line.trim_end_matches(['\r', '\n']).len()..];
                return (!code.is_empty()).then_some(Markup {
                    body: code,
                    rest,
                    kind: if language.is_empty() {
                        "textEntityTypePre"
                    } else {
                        "textEntityTypePreCode"
                    },
                    argument: language,
                    literal: true,
                });
            }
            offset += line.len();
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_markdown_uses_utf16_offsets_after_removing_markers() {
        let input = "中😀 **粗体** *ital* _斜_ `✏️` ~~删~~ ||藏|| [链接😀](https://telegra.ph/demo)\n```rust\nlet 猫 = \"🐈\";\n```";
        let expected = "中😀 粗体 ital 斜 ✏️ 删 藏 链接😀\nlet 猫 = \"🐈\";";
        let result = formatted_text(input);
        assert_eq!(result["text"], expected);
        let entities = result["entities"].as_array().unwrap();
        let types = [
            ("粗体", "Bold"),
            ("ital", "Italic"),
            ("斜", "Italic"),
            ("✏️", "Code"),
            ("删", "Strikethrough"),
            ("藏", "Spoiler"),
            ("链接😀", "TextUrl"),
            ("let 猫 = \"🐈\";", "PreCode"),
        ];
        assert_eq!(entities.len(), types.len());
        assert_eq!(entities[0]["offset"], 4);
        for (entity, (body, kind)) in entities.iter().zip(types) {
            let byte = expected.find(body).unwrap();
            assert_eq!(entity["offset"], expected[..byte].encode_utf16().count());
            assert_eq!(entity["length"], body.encode_utf16().count());
            assert_eq!(entity["type"]["@type"], format!("textEntityType{kind}"));
        }
        assert_eq!(entities[6]["type"]["url"], "https://telegra.ph/demo");
        assert_eq!(entities[7]["type"]["language"], "rust");
    }

    #[test]
    fn ordinary_text_unclosed_markers_and_non_web_links_stay_literal() {
        for plain in [
            "**未闭合\n*bad _plain_text_here `bad\n~~bad ||bad",
            "C:\\_file and snake_case_123",
            "[link](file:///tmp/file) [link](https://example.com",
            "```rust\n**this fence is unclosed**",
            "5 * 4 and a_b_c",
            "prefix \\\\ literal",
        ] {
            let result = formatted_text(plain);
            assert_eq!(result["text"], plain, "{plain}");
            assert_eq!(result["entities"], json!([]));
        }
        let result = formatted_text("**粗体 _斜😀_**\n```\r\n  a\r\n```");
        assert_eq!(result["text"], "粗体 斜😀\n  a");
        assert_eq!(result["entities"][1]["offset"], 3);
        assert_eq!(result["entities"][1]["length"], 3);
        assert_eq!(result["entities"][2]["type"]["@type"], "textEntityTypePre");
    }

    #[test]
    fn link_destinations_can_include_balanced_parentheses_and_have_styled_labels() {
        let result = formatted_text("**参见 [ _条目_](https://example.com/wiki/A_(B))**");
        assert_eq!(result["text"], "参见  条目");
        assert_eq!(
            result["entities"][1]["type"]["url"],
            "https://example.com/wiki/A_(B)"
        );
        assert_eq!(
            result["entities"][2]["type"]["@type"],
            "textEntityTypeItalic"
        );
    }

    #[test]
    fn parsing_has_bounded_work_and_entity_count() {
        let result = formatted_text(&"**bold** ".repeat(MAX_ENTITIES + 2));
        assert_eq!(result["entities"].as_array().unwrap().len(), MAX_ENTITIES);
        assert!(
            result["text"]
                .as_str()
                .unwrap()
                .ends_with("**bold** **bold** ")
        );
        let input = "[".repeat(32 * 1024);
        assert_eq!(formatted_text(&input)["text"], input);
    }
}
