//! Bounded, display-only projection of TDLib Instant View pages.
use serde_json::Value;

const MAX_BLOCKS: usize = 256;
const MAX_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 16;
const MAX_NODES: usize = 4096;
const MAX_BLOCK_BYTES: usize = 8 * 1024;

pub struct Reader {
    pub url: String,
    pub state: State,
    pub scroll: usize,
    pub max_scroll: usize,
    pub page: usize,
}

pub enum State {
    Loading,
    Ready(InstantViewArticle),
    Failed,
}

impl Reader {
    pub fn new(url: String) -> Self {
        Self {
            url,
            state: State::Loading,
            scroll: 0,
            max_scroll: 0,
            page: 1,
        }
    }

    pub fn scroll_by(&mut self, delta: isize) -> bool {
        self.set_scroll(self.scroll.saturating_add_signed(delta))
    }

    pub fn set_scroll(&mut self, top: usize) -> bool {
        let next = top.min(self.max_scroll);
        std::mem::replace(&mut self.scroll, next) != next
    }
}

pub struct InstantViewArticle {
    pub blocks: Vec<Block>,
    pub partial: bool,
}

pub struct Block {
    pub text: String,
    pub kind: Kind,
    pub indent: u8,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Title,
    Heading,
    Body,
    Muted,
    Quote,
    Code,
    Divider,
}

impl InstantViewArticle {
    pub fn parse(value: &Value) -> Option<Self> {
        if tag(value) != "webPageInstantView" {
            return None;
        }
        let mut parser = Parser {
            article: Self {
                blocks: Vec::new(),
                partial: value["is_full"] == false,
            },
            bytes: 0,
            nodes: 0,
        };
        parser.blocks(children(value), 0, 0, false);
        (!parser.article.blocks.is_empty()).then_some(parser.article)
    }
}

fn tag(value: &Value) -> &str {
    value["@type"].as_str().unwrap_or_default()
}

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

// TDLib 1.8 uses page_blocks. Newer schemas also use blocks in these objects.
fn children(value: &Value) -> &[Value] {
    array(value.get("page_blocks").unwrap_or(&value["blocks"]))
}

struct Parser {
    article: InstantViewArticle,
    bytes: usize,
    nodes: usize,
}

impl Parser {
    fn visit(&mut self, depth: usize) -> bool {
        if depth >= MAX_DEPTH
            || self.nodes >= MAX_NODES
            || self.bytes >= MAX_BYTES
            || self.article.blocks.len() >= MAX_BLOCKS
        {
            self.article.partial = true;
            return false;
        }
        self.nodes += 1;
        true
    }

    fn add(&mut self, mut text: String, kind: Kind, indent: u8) {
        if text.is_empty() && !matches!(kind, Kind::Divider) {
            return;
        }
        let remaining = MAX_BYTES.saturating_sub(self.bytes).min(MAX_BLOCK_BYTES);
        if text.len() > remaining {
            text.truncate(text.floor_char_boundary(remaining));
            self.article.partial = true;
        }
        if self.article.blocks.len() >= MAX_BLOCKS || (text.is_empty() && remaining == 0) {
            self.article.partial = true;
            return;
        }
        self.bytes += text.len();
        self.article.blocks.push(Block { text, kind, indent });
    }

    fn append(&mut self, output: &mut String, text: &str) {
        for (index, ch) in text.char_indices() {
            if index + ch.len_utf8() > MAX_BLOCK_BYTES
                || output.len() + ch.len_utf8() > MAX_BLOCK_BYTES
            {
                self.article.partial = true;
                break;
            }
            match ch {
                '\n' => output.push(ch),
                '\t' if output.len() + 4 <= MAX_BLOCK_BYTES => output.push_str("    "),
                ch if ch.is_control() => {}
                ch => output.push(ch),
            }
        }
    }

    fn rich(&mut self, value: &Value, depth: usize, output: &mut String) {
        if !self.visit(depth) || value.is_null() {
            return;
        }
        match tag(value) {
            "richTexts" => {
                for item in array(&value["texts"]) {
                    if depth + 1 >= MAX_DEPTH
                        || self.nodes >= MAX_NODES
                        || output.len() >= MAX_BLOCK_BYTES
                    {
                        self.article.partial = true;
                        break;
                    }
                    self.rich(item, depth + 1, output);
                }
            }
            "richTextIcon" => self.append(output, "[图片]"),
            "richTextCustomEmoji" => self.append(
                output,
                value["alternative_text"].as_str().unwrap_or_default(),
            ),
            "richTextMathematicalExpression" => {
                self.append(output, value["expression"].as_str().unwrap_or_default())
            }
            "richTextButton" => self.rich(&value["button"]["text"], depth + 1, output),
            _ => {
                if let Some(text) = value.as_str().or_else(|| value["text"].as_str()) {
                    self.append(output, text);
                } else if tag(value).starts_with("richText") {
                    self.rich(&value["text"], depth + 1, output);
                }
            }
        }
    }

    fn rich_text(&mut self, value: &Value, depth: usize) -> String {
        let mut text = String::new();
        self.rich(value, depth + 1, &mut text);
        text
    }

    fn caption(&mut self, value: &Value, depth: usize, indent: u8) {
        for field in ["text", "credit"] {
            let text = self.rich_text(&value["caption"][field], depth);
            self.add(text, Kind::Muted, indent);
        }
    }

    fn blocks(&mut self, list: &[Value], depth: usize, indent: u8, quoted: bool) {
        for block in list {
            if !self.block(block, depth, indent, quoted) {
                break;
            }
        }
    }

    fn block(&mut self, block: &Value, depth: usize, indent: u8, quoted: bool) -> bool {
        if !self.visit(depth) {
            return false;
        }
        let (field, kind) = match tag(block) {
            "pageBlockTitle" => ("title", Kind::Title),
            "pageBlockSubtitle" => ("subtitle", Kind::Heading),
            "pageBlockHeader" => ("header", Kind::Heading),
            "pageBlockSubheader" => ("subheader", Kind::Heading),
            "pageBlockSectionHeading" => ("text", Kind::Heading),
            "pageBlockKicker" => ("kicker", Kind::Muted),
            "pageBlockParagraph" => ("text", if quoted { Kind::Quote } else { Kind::Body }),
            "pageBlockFooter" => ("footer", Kind::Muted),
            "pageBlockAuthorDate" => {
                let mut text = self.rich_text(&block["author"], depth);
                if let Some(date) = block["publish_date"]
                    .as_i64()
                    .and_then(crate::calendar::local)
                {
                    if !text.is_empty() {
                        self.append(&mut text, " · ");
                    }
                    self.append(&mut text, &date.date());
                }
                self.add(text, Kind::Muted, indent);
                return true;
            }
            "pageBlockDivider" => {
                self.add(String::new(), Kind::Divider, indent);
                return true;
            }
            "pageBlockBlockQuote" | "pageBlockExpandableBlockQuote" | "pageBlockPullQuote" => {
                let text = self.rich_text(&block["text"], depth);
                self.add(text, Kind::Quote, indent);
                self.blocks(children(block), depth + 1, indent, true);
                let credit = self.rich_text(&block["credit"], depth);
                self.add(credit, Kind::Muted, indent);
                return true;
            }
            "pageBlockPreformatted" => {
                let language = self.rich_text(&block["language"], depth);
                self.add(language, Kind::Muted, indent);
                ("text", Kind::Code)
            }
            "pageBlockPhoto" | "pageBlockVideo" | "pageBlockAnimation" | "pageBlockAudio"
            | "pageBlockVoiceNote" | "pageBlockDocument" => {
                let label = match tag(block) {
                    "pageBlockPhoto" => "[图片]",
                    "pageBlockVideo" => "[视频]",
                    "pageBlockAnimation" => "[动画]",
                    "pageBlockDocument" => "[附件]",
                    _ => "[音频]",
                };
                self.add(label.to_owned(), Kind::Muted, indent);
                self.caption(block, depth, indent);
                return true;
            }
            "pageBlockDetails" | "pageBlockRelatedArticles" => {
                let heading = self.rich_text(&block["header"], depth);
                self.add(heading, Kind::Heading, indent);
                self.blocks(children(block), depth + 1, indent + 1, quoted);
                for item in array(&block["articles"]) {
                    if !self.visit(depth + 1) {
                        break;
                    }
                    let title = item["title"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .unwrap_or_else(|| item["url"].as_str().unwrap_or_default());
                    let mut text = String::new();
                    self.append(&mut text, title);
                    self.add(text, Kind::Muted, indent + 1);
                    if let Some(description) = item["description"].as_str() {
                        let mut text = String::new();
                        self.append(&mut text, description);
                        self.add(text, Kind::Muted, indent + 1);
                    }
                }
                return true;
            }
            "pageBlockCollage" | "pageBlockSlideshow" => {
                self.blocks(children(block), depth + 1, indent, quoted);
                self.caption(block, depth, indent);
                return true;
            }
            "pageBlockCover" => {
                self.block(&block["cover"], depth + 1, indent, quoted);
                return true;
            }
            "pageBlockList" => {
                for item in array(&block["items"]) {
                    if !self.visit(depth + 1) {
                        break;
                    }
                    let mut label = String::new();
                    let prefix = item["label"]
                        .as_str()
                        .filter(|text| !text.is_empty())
                        .unwrap_or("•");
                    self.append(&mut label, prefix);
                    let item_blocks = children(item);
                    if let [first, tail @ ..] = item_blocks
                        && tag(first) == "pageBlockParagraph"
                    {
                        self.append(&mut label, " ");
                        self.rich(&first["text"], depth + 2, &mut label);
                        self.add(label, Kind::Body, indent);
                        self.blocks(tail, depth + 2, indent + 1, quoted);
                    } else {
                        self.add(label, Kind::Muted, indent);
                        self.blocks(item_blocks, depth + 2, indent + 1, quoted);
                    }
                }
                return true;
            }
            _ => return true,
        };
        let text = self.rich_text(&block[field], depth);
        self.add(text, kind, indent);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_old_and_new_schema_blocks_and_rich_text_in_reading_order() {
        let article = InstantViewArticle::parse(&json!({
            "@type":"webPageInstantView","is_full":true,"page_blocks":[
                {"@type":"pageBlockTitle","title":{"@type":"richTexts","texts":[
                    {"@type":"richTextBold","text":{"@type":"richTextPlain","text":"你好"}},
                    {"@type":"richTextCustomEmoji","alternative_text":"😀"}
                ]}},
                {"@type":"pageBlockAuthorDate","author":{"@type":"richTextPlain","text":"作者"},"publish_date":1700000000},
                {"@type":"pageBlockBlockQuote","text":{"@type":"richTextPlain","text":"旧引用"},"credit":"来源"},
                {"@type":"pageBlockPullQuote","blocks":[{"@type":"pageBlockParagraph","text":"新引用"}]},
                {"@type":"pageBlockDetails","header":"详情","page_blocks":[
                    {"@type":"pageBlockList","items":[{"@type":"pageBlockListItem","label":"1.","page_blocks":[
                        {"@type":"pageBlockParagraph","text":{"@type":"richTextUrl","text":{"@type":"richTextPlain","text":"条目"}}},
                        {"@type":"pageBlockPreformatted","language":"rust","text":"  let x = 1;"}
                    ]}]}
                ]},
                {"@type":"pageBlockSlideshow","blocks":[
                    {"@type":"pageBlockPhoto","caption":{"text":"照片说明","credit":"摄影"}},
                    {"@type":"pageBlockVideo"},
                    {"@type":"pageBlockAnimation"}
                ],"caption":{"text":"幻灯片"}},
                {"@type":"pageBlockRelatedArticles","header":"相关阅读","articles":[{"title":"相关文","description":"相关摘要"}]},
                {"@type":"pageBlockDivider"},
                {"@type":"pageBlockFooter","footer":"结束"}
            ]
        })).unwrap();
        let texts: Vec<_> = article
            .blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect();
        assert_eq!(texts[0], "你好😀");
        assert!(texts[1].starts_with("作者 · 2023-11-"));
        assert_eq!(
            &texts[2..],
            [
                "旧引用",
                "来源",
                "新引用",
                "详情",
                "1. 条目",
                "rust",
                "  let x = 1;",
                "[图片]",
                "照片说明",
                "摄影",
                "[视频]",
                "[动画]",
                "幻灯片",
                "相关阅读",
                "相关文",
                "相关摘要",
                "",
                "结束"
            ]
        );
        assert!(matches!(article.blocks[4].kind, Kind::Quote));
        assert!(matches!(article.blocks[8].kind, Kind::Code));
        assert!(!article.partial);
    }

    #[test]
    fn article_limits_nodes_depth_blocks_and_utf8_bytes() {
        let mut nested = json!({"@type":"richTextPlain","text":"deep"});
        for _ in 0..MAX_DEPTH + 2 {
            nested = json!({"@type":"richTextBold","text":nested});
        }
        let large = json!({"@type":"pageBlockParagraph","text":"猫".repeat(MAX_BLOCK_BYTES)});
        let mut blocks = vec![json!({"@type":"pageBlockParagraph","text":nested})];
        blocks.extend(std::iter::repeat_n(large, 20));
        let article =
            InstantViewArticle::parse(&json!({"@type":"webPageInstantView","blocks":blocks}))
                .unwrap();
        assert!(article.partial);
        assert!(
            article
                .blocks
                .iter()
                .map(|block| block.text.len())
                .sum::<usize>()
                <= MAX_BYTES
        );
        assert!(
            article
                .blocks
                .iter()
                .all(|block| block.text.len() <= MAX_BLOCK_BYTES && !block.text.contains("deep"))
        );

        let dividers = vec![json!({"@type":"pageBlockDivider"}); MAX_BLOCKS + 10];
        let article =
            InstantViewArticle::parse(&json!({"@type":"webPageInstantView","blocks":dividers}))
                .unwrap();
        assert!(article.partial);
        assert_eq!(article.blocks.len(), MAX_BLOCKS);

        let mut blocks = vec![Value::Null; MAX_NODES];
        blocks.push(json!({"@type":"pageBlockDivider"}));
        assert!(
            InstantViewArticle::parse(&json!({"@type":"webPageInstantView","blocks":blocks}))
                .is_none()
        );
        assert!(
            InstantViewArticle::parse(&json!({"@type":"error","message":"not available"}))
                .is_none()
        );
    }
}
