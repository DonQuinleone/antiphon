use crate::urls::scan_urls;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderedBody {
    pub lines: Vec<BodyLine>,
    pub links: Vec<Link>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BodyLine {
    pub text: String,
    pub spans: Vec<LinkSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkSpan {
    pub start: usize,
    pub end: usize,
    pub link: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub index: usize,
    pub url: String,
    pub label: String,
}

impl RenderedBody {
    pub fn wrapped(&self, width: usize) -> RenderedBody {
        if width == 0 {
            return self.clone();
        }
        let lines = self
            .lines
            .iter()
            .flat_map(|line| line.wrapped(width))
            .collect();
        RenderedBody {
            lines,
            links: self.links.clone(),
        }
    }
}

impl BodyLine {
    /// This line cut to the width, spans clipped and
    /// re-based per piece, so callers can wrap line by line
    /// and keep their own per-line state aligned.
    pub fn wrapped(&self, width: usize) -> Vec<BodyLine> {
        if width == 0 {
            return vec![self.clone()];
        }
        wrap_line(self, width)
    }
}

/// Plain text scanned for bare urls: the same shape html
/// rendering produces, for bodies that are already text.
pub fn scan_text(text: &str) -> RenderedBody {
    plain_body(text)
}

pub(crate) struct LinkRegistry {
    links: Vec<Link>,
}

impl LinkRegistry {
    pub(crate) fn new() -> Self {
        Self { links: Vec::new() }
    }

    pub(crate) fn register(&mut self, url: &str) -> usize {
        let found = self.links.iter().position(|link| link.url == url);
        if let Some(id) = found {
            return id;
        }
        self.links.push(Link {
            index: self.links.len() + 1,
            url: url.to_owned(),
            label: String::new(),
        });
        self.links.len() - 1
    }

    pub(crate) fn set_label(&mut self, id: usize, label: &str) {
        let link = &mut self.links[id];
        if !link.label.is_empty() {
            return;
        }
        link.label = label.to_owned();
    }

    pub(crate) fn into_links(self) -> Vec<Link> {
        self.links
    }
}

pub(crate) fn plain_body(text: &str) -> RenderedBody {
    let mut registry = LinkRegistry::new();
    let lines = text
        .lines()
        .map(|line| plain_line(line, &mut registry))
        .collect();
    RenderedBody {
        lines,
        links: registry.into_links(),
    }
}

fn plain_line(line: &str, registry: &mut LinkRegistry) -> BodyLine {
    let spans = scan_urls(line)
        .into_iter()
        .map(|(start, end)| {
            let url = &line[start..end];
            let link = registry.register(url);
            registry.set_label(link, url);
            LinkSpan { start, end, link }
        })
        .collect();
    BodyLine {
        text: line.to_owned(),
        spans,
    }
}

fn wrap_line(line: &BodyLine, width: usize) -> Vec<BodyLine> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        if offset >= line.text.len() && !out.is_empty() {
            return out;
        }
        let rest = &line.text[offset..];
        let Some((end, next)) = cut_point(rest, width) else {
            out.push(slice_line(line, offset, line.text.len()));
            return out;
        };
        out.push(slice_line(line, offset, offset + end));
        offset += next;
    }
}

fn cut_point(rest: &str, width: usize) -> Option<(usize, usize)> {
    let mut space = None;
    let mut over = None;
    for (count, (pos, ch)) in rest.char_indices().enumerate() {
        if ch == ' ' && count <= width {
            space = Some(pos);
        }
        if count == width {
            over = Some(pos);
            break;
        }
    }
    let over = over?;
    match space {
        Some(pos) => Some((pos, skip_spaces(rest, pos))),
        None => Some((over, over)),
    }
}

fn skip_spaces(rest: &str, from: usize) -> usize {
    let run = rest[from..]
        .bytes()
        .take_while(|&byte| byte == b' ')
        .count();
    from + run
}

fn slice_line(line: &BodyLine, start: usize, end: usize) -> BodyLine {
    let spans = line
        .spans
        .iter()
        .filter_map(|span| clip_span(span, start, end))
        .collect();
    BodyLine {
        text: line.text[start..end].to_owned(),
        spans,
    }
}

fn clip_span(
    span: &LinkSpan,
    start: usize,
    end: usize,
) -> Option<LinkSpan> {
    let from = span.start.max(start);
    let to = span.end.min(end);
    if from >= to {
        return None;
    }
    Some(LinkSpan {
        start: from - start,
        end: to - start,
        link: span.link,
    })
}
