use html2text::render::{RichAnnotation, TaggedLine};

use crate::links::{BodyLine, LinkRegistry, LinkSpan, RenderedBody};
use crate::urls::linkable;

// The classic e-mail column budget. Rendering is done once,
// width-agnostic from the pager's point of view; panes
// narrower than this re-wrap through BodyLine::wrapped.
const RENDER_WIDTH: usize = 78;

type RichLine = TaggedLine<Vec<RichAnnotation>>;

pub(crate) fn html_body(html: &str) -> RenderedBody {
    let parsed = html2text::config::rich()
        .lines_from_read(html.as_bytes(), RENDER_WIDTH);
    let Ok(lines) = parsed else {
        // html5ever accepts any tag soup; only pathological
        // documents end up here, and raw text beats nothing.
        return crate::links::plain_body(html);
    };
    let mut builder = Builder::new();
    for line in &lines {
        builder.line(line);
    }
    builder.finish()
}

/// An anchor whose marker is still pending: the text may
/// continue on the next wrapped line, so the "[n]" marker
/// waits until something that is not this link appears.
struct OpenAnchor {
    link: usize,
    url: String,
    line: usize,
    label: String,
}

struct Builder {
    lines: Vec<BodyLine>,
    registry: LinkRegistry,
    open: Option<OpenAnchor>,
}

impl Builder {
    fn new() -> Self {
        Self {
            lines: Vec::new(),
            registry: LinkRegistry::new(),
            open: None,
        }
    }

    fn line(&mut self, line: &RichLine) {
        self.lines.push(BodyLine::default());
        for tagged in line.tagged_strings() {
            match anchor_url(&tagged.tag) {
                Some(url) => self.link_text(&tagged.s, url),
                None => self.text(&tagged.s),
            }
        }
    }

    fn text(&mut self, text: &str) {
        self.close_anchor();
        self.current().text.push_str(text);
    }

    fn link_text(&mut self, text: &str, url: &str) {
        if self.open.as_ref().is_none_or(|open| open.url != url) {
            self.close_anchor();
            self.open_anchor(url);
        }
        let line = self.lines.len() - 1;
        self.continue_label(line, text);
        let link = self.anchor().link;
        let start = self.current().text.len();
        self.current().text.push_str(text);
        let end = self.current().text.len();
        self.span(line, LinkSpan { start, end, link });
    }

    fn open_anchor(&mut self, url: &str) {
        self.open = Some(OpenAnchor {
            link: self.registry.register(url),
            url: url.to_owned(),
            line: self.lines.len() - 1,
            label: String::new(),
        });
    }

    fn continue_label(&mut self, line: usize, text: &str) {
        let anchor = self.anchor();
        if anchor.line < line && !anchor.label.is_empty() {
            anchor.label.push(' ');
        }
        anchor.line = line;
        anchor.label.push_str(text);
    }

    fn close_anchor(&mut self) {
        let Some(open) = self.open.take() else {
            return;
        };
        let line = &mut self.lines[open.line];
        line.text.push_str(&marker(open.link));
        let tail =
            line.spans.last_mut().filter(|span| span.link == open.link);
        if let Some(span) = tail {
            span.end = line.text.len();
        }
        self.registry.set_label(open.link, open.label.trim());
    }

    fn span(&mut self, line: usize, span: LinkSpan) {
        let spans = &mut self.lines[line].spans;
        let merged = spans
            .last_mut()
            .filter(|last| {
                last.link == span.link && last.end == span.start
            })
            .map(|last| last.end = span.end);
        if merged.is_none() {
            spans.push(span);
        }
    }

    fn current(&mut self) -> &mut BodyLine {
        self.lines.last_mut().expect("a line under construction")
    }

    fn anchor(&mut self) -> &mut OpenAnchor {
        self.open.as_mut().expect("an open anchor")
    }

    fn finish(mut self) -> RenderedBody {
        self.close_anchor();
        trim_blank_edges(&mut self.lines);
        RenderedBody {
            lines: self.lines,
            links: self.registry.into_links(),
        }
    }
}

fn anchor_url(tags: &[RichAnnotation]) -> Option<&str> {
    tags.iter()
        .find_map(|tag| match tag {
            RichAnnotation::Link(url) => Some(url.as_str()),
            _ => None,
        })
        .filter(|url| linkable(url))
}

fn marker(link: usize) -> String {
    format!("[{}]", link + 1)
}

fn trim_blank_edges(lines: &mut Vec<BodyLine>) {
    while lines.last().is_some_and(blank) {
        lines.pop();
    }
    let lead = lines.iter().take_while(|line| blank(line)).count();
    lines.drain(..lead);
}

fn blank(line: &BodyLine) -> bool {
    line.text.trim().is_empty() && line.spans.is_empty()
}
