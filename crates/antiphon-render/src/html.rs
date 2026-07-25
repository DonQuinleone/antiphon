use crate::links::{BodyLine, LinkRegistry, LinkSpan, RenderedBody};
use crate::urls::linkable;

const LINE_BREAK: usize = 1;
const PARAGRAPH_BREAK: usize = 2;
const MAX_ENTITY_LEN: usize = 8;

const ENTITIES: [(&str, &str); 6] = [
    ("amp", "&"),
    ("lt", "<"),
    ("gt", ">"),
    ("quot", "\""),
    ("apos", "'"),
    ("nbsp", " "),
];

const SKIP_TAGS: [&str; 5] =
    ["script", "style", "head", "title", "template"];

const PARAGRAPH_TAGS: [&str; 11] = [
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "blockquote",
    "ul",
    "ol",
    "table",
];

const LINE_TAGS: [&str; 8] = [
    "div", "tr", "hr", "section", "article", "header", "footer", "nav",
];

const CELL_TAGS: [&str; 2] = ["td", "th"];

pub(crate) fn html_body(html: &str) -> RenderedBody {
    let mut renderer = Renderer::new();
    let mut rest = html;
    while let Some(pos) = rest.find('<') {
        renderer.text(&rest[..pos]);
        let after = &rest[pos..];
        rest = renderer.markup(after);
    }
    renderer.text(rest);
    renderer.finish()
}

struct Tag {
    name: String,
    attrs: String,
    closing: bool,
}

struct Anchor {
    link: usize,
    from: Option<usize>,
    label: String,
}

struct Renderer {
    lines: Vec<BodyLine>,
    current: String,
    spans: Vec<LinkSpan>,
    registry: LinkRegistry,
    pending_space: bool,
    pending_breaks: usize,
    skip_until: Option<String>,
    anchor: Option<Anchor>,
}

impl Renderer {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            current: String::new(),
            spans: Vec::new(),
            registry: LinkRegistry::new(),
            pending_space: false,
            pending_breaks: 0,
            skip_until: None,
            anchor: None,
        }
    }

    fn markup<'a>(&mut self, after: &'a str) -> &'a str {
        if let Some(skipped) = enclosed_markup(after) {
            return skipped;
        }
        let Some(end) = after.find('>') else {
            self.text("<");
            return &after[1..];
        };
        let Some(tag) = parse_tag(&after[1..end]) else {
            self.text("<");
            return &after[1..];
        };
        self.tag(&tag);
        &after[end + 1..]
    }

    fn text(&mut self, raw: &str) {
        if self.skip_until.is_some() || raw.is_empty() {
            return;
        }
        self.chars(&decode_entities(raw));
    }

    fn chars(&mut self, text: &str) {
        for ch in text.chars() {
            if ch.is_whitespace() {
                self.pending_space = true;
                continue;
            }
            self.push_char(ch);
        }
    }

    fn push_char(&mut self, ch: char) {
        self.apply_breaks();
        self.flush_space();
        self.mark_anchor();
        self.current.push(ch);
        if let Some(anchor) = self.anchor.as_mut() {
            anchor.label.push(ch);
        }
    }

    fn flush_space(&mut self) {
        if !self.pending_space {
            return;
        }
        self.pending_space = false;
        if self.current.is_empty() {
            return;
        }
        self.current.push(' ');
        let Some(anchor) = self.anchor.as_mut() else {
            return;
        };
        if anchor.from.is_some() {
            anchor.label.push(' ');
        }
    }

    fn mark_anchor(&mut self) {
        let offset = self.current.len();
        let Some(anchor) = self.anchor.as_mut() else {
            return;
        };
        if anchor.from.is_none() {
            anchor.from = Some(offset);
        }
    }

    fn request_break(&mut self, depth: usize) {
        self.pending_breaks = self.pending_breaks.max(depth);
    }

    fn forced_break(&mut self) {
        self.pending_breaks =
            (self.pending_breaks + LINE_BREAK).min(PARAGRAPH_BREAK);
    }

    fn apply_breaks(&mut self) {
        if self.pending_breaks == 0 {
            return;
        }
        let paragraph = self.pending_breaks >= PARAGRAPH_BREAK;
        self.pending_breaks = 0;
        self.pending_space = false;
        self.flush_line();
        let separated =
            self.lines.last().is_some_and(|line| !line.text.is_empty());
        if paragraph && separated {
            self.lines.push(BodyLine::default());
        }
    }

    fn flush_line(&mut self) {
        self.anchor_line_break();
        if self.current.is_empty() && self.spans.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.current);
        let spans = std::mem::take(&mut self.spans);
        self.lines.push(BodyLine { text, spans });
    }

    fn anchor_line_break(&mut self) {
        let end = self.current.len();
        let Some(anchor) = self.anchor.as_mut() else {
            return;
        };
        let Some(from) = anchor.from.take() else {
            return;
        };
        if !anchor.label.ends_with(' ') {
            anchor.label.push(' ');
        }
        let link = anchor.link;
        if from >= end {
            return;
        }
        self.spans.push(LinkSpan {
            start: from,
            end,
            link,
        });
    }

    fn tag(&mut self, tag: &Tag) {
        if let Some(active) = &self.skip_until {
            if tag.closing && tag.name == *active {
                self.skip_until = None;
            }
            return;
        }
        let name = tag.name.as_str();
        if SKIP_TAGS.contains(&name) && !tag.closing {
            self.skip_until = Some(tag.name.clone());
            return;
        }
        match name {
            "a" => self.anchor_tag(tag),
            "img" if !tag.closing => self.image(tag),
            "br" if !tag.closing => self.forced_break(),
            "li" => self.list_item(tag),
            _ => self.block_tag(name),
        }
    }

    fn block_tag(&mut self, name: &str) {
        if PARAGRAPH_TAGS.contains(&name) {
            self.request_break(PARAGRAPH_BREAK);
            return;
        }
        if LINE_TAGS.contains(&name) {
            self.request_break(LINE_BREAK);
            return;
        }
        if CELL_TAGS.contains(&name) {
            self.pending_space = true;
        }
    }

    fn list_item(&mut self, tag: &Tag) {
        self.request_break(LINE_BREAK);
        if tag.closing {
            return;
        }
        self.push_char('-');
        self.pending_space = true;
    }

    fn image(&mut self, tag: &Tag) {
        let Some(alt) = attr(&tag.attrs, "alt") else {
            return;
        };
        self.chars(&alt);
    }

    fn anchor_tag(&mut self, tag: &Tag) {
        self.close_anchor();
        if tag.closing {
            return;
        }
        let href = attr(&tag.attrs, "href");
        let Some(url) = href.filter(|url| linkable(url)) else {
            return;
        };
        let link = self.registry.register(&url);
        self.anchor = Some(Anchor {
            link,
            from: None,
            label: String::new(),
        });
    }

    fn close_anchor(&mut self) {
        let Some(anchor) = &self.anchor else {
            return;
        };
        let link = anchor.link;
        if anchor.label.trim().is_empty() {
            let url = self.registry.url(link).to_owned();
            self.chars(&url);
        }
        let Some(anchor) = self.anchor.take() else {
            return;
        };
        let from = anchor.from.unwrap_or(self.current.len());
        self.current.push_str(&marker(link));
        self.spans.push(LinkSpan {
            start: from,
            end: self.current.len(),
            link,
        });
        self.registry.set_label(link, anchor.label.trim());
    }

    fn finish(mut self) -> RenderedBody {
        self.close_anchor();
        self.flush_line();
        RenderedBody {
            lines: self.lines,
            links: self.registry.into_links(),
        }
    }
}

fn marker(link: usize) -> String {
    format!("[{}]", link + 1)
}

fn enclosed_markup(after: &str) -> Option<&str> {
    if let Some(rest) = after.strip_prefix("<!--") {
        let end = rest.find("-->").map_or(rest.len(), |p| p + 3);
        return Some(&rest[end..]);
    }
    if after.starts_with("<!") || after.starts_with("<?") {
        let end = after.find('>').map_or(after.len(), |pos| pos + 1);
        return Some(&after[end..]);
    }
    None
}

fn parse_tag(inner: &str) -> Option<Tag> {
    let inner = inner.strip_suffix('/').unwrap_or(inner);
    let (closing, rest) = match inner.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, inner),
    };
    let mut bytes = rest.bytes();
    if !bytes.next().is_some_and(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let tail = bytes
        .take_while(|byte| byte.is_ascii_alphanumeric())
        .count();
    let name_len = 1 + tail;
    Some(Tag {
        name: rest[..name_len].to_ascii_lowercase(),
        attrs: rest[name_len..].to_owned(),
        closing,
    })
}

fn attr(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return None;
        }
        let key_len = rest
            .bytes()
            .take_while(|byte| {
                byte.is_ascii_alphanumeric() || *byte == b'-'
            })
            .count();
        if key_len == 0 {
            let step = rest.chars().next().map_or(1, char::len_utf8);
            rest = &rest[step..];
            continue;
        }
        let key = &rest[..key_len];
        let (value, next) = attr_value(rest[key_len..].trim_start());
        if key.eq_ignore_ascii_case(name) {
            return Some(decode_entities(&value));
        }
        rest = next;
    }
}

fn attr_value(rest: &str) -> (String, &str) {
    let Some(after) = rest.strip_prefix('=') else {
        return (String::new(), rest);
    };
    let after = after.trim_start();
    match after.chars().next() {
        Some(quote @ ('"' | '\'')) => quoted_value(after, quote),
        _ => bare_value(after),
    }
}

fn quoted_value(after: &str, quote: char) -> (String, &str) {
    let inner = &after[quote.len_utf8()..];
    let Some(end) = inner.find(quote) else {
        return (inner.to_owned(), "");
    };
    (inner[..end].to_owned(), &inner[end + quote.len_utf8()..])
}

fn bare_value(after: &str) -> (String, &str) {
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    (after[..end].to_owned(), &after[end..])
}

fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        let Some((decoded, len)) = decode_entity(tail) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        out.push_str(&decoded);
        rest = &tail[len..];
    }
    out.push_str(rest);
    out
}

fn decode_entity(tail: &str) -> Option<(String, usize)> {
    let end = tail[1..].find(';')? + 1;
    let name = &tail[1..end];
    if name.is_empty() || name.len() > MAX_ENTITY_LEN {
        return None;
    }
    let len = end + 1;
    if let Some(number) = name.strip_prefix('#') {
        let ch = decode_numeric(number)?;
        return Some((ch.to_string(), len));
    }
    ENTITIES
        .iter()
        .find(|(entity, _)| entity.eq_ignore_ascii_case(name))
        .map(|(_, text)| ((*text).to_owned(), len))
}

fn decode_numeric(number: &str) -> Option<char> {
    let hex = number
        .strip_prefix('x')
        .or_else(|| number.strip_prefix('X'));
    let value = match hex {
        Some(digits) => u32::from_str_radix(digits, 16).ok()?,
        None => number.parse().ok()?,
    };
    char::from_u32(value)
}
