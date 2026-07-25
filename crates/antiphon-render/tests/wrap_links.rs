mod common;

use antiphon_render::{
    BodyLine, Link, LinkSpan, RenderedBody, rendered_body,
};
use common::{
    assert_indices_sequential, html_message, line_texts, plain_message,
    span_texts,
};

fn line(text: &str, spans: &[(usize, usize, usize)]) -> BodyLine {
    let spans = spans
        .iter()
        .map(|&(start, end, link)| LinkSpan { start, end, link })
        .collect();
    BodyLine {
        text: text.to_owned(),
        spans,
    }
}

fn link(index: usize, url: &str, label: &str) -> Link {
    Link {
        index,
        url: url.to_owned(),
        label: label.to_owned(),
    }
}

struct Case {
    name: &'static str,
    body: RenderedBody,
    width: usize,
    lines: &'static [&'static str],
    spans: &'static [&'static [(&'static str, usize)]],
}

#[test]
fn wrapping_splits_spans_across_lines() {
    let cases = [
        Case {
            name: "link split at a space inside the label",
            body: RenderedBody {
                lines: vec![line(
                    "read the docs[1] now",
                    &[(5, 16, 0)],
                )],
                links: vec![link(1, "https://example.com/1", "d")],
            },
            width: 10,
            lines: &["read the", "docs[1]", "now"],
            spans: &[&[("the", 1)], &[("docs[1]", 1)], &[]],
        },
        Case {
            name: "long url hard-breaks across three lines",
            body: RenderedBody {
                lines: vec![line(
                    "https://example.com/long/path",
                    &[(0, 29, 0)],
                )],
                links: vec![link(
                    1,
                    "https://example.com/long/path",
                    "u",
                )],
            },
            width: 10,
            lines: &["https://ex", "ample.com/", "long/path"],
            spans: &[
                &[("https://ex", 1)],
                &[("ample.com/", 1)],
                &[("long/path", 1)],
            ],
        },
        Case {
            name: "multibyte text keeps span boundaries valid",
            body: RenderedBody {
                lines: vec![line("héllo wörld[1]", &[(7, 16, 0)])],
                links: vec![link(1, "https://example.com/w", "w")],
            },
            width: 6,
            lines: &["héllo", "wörld[", "1]"],
            spans: &[&[], &[("wörld[", 1)], &[("1]", 1)]],
        },
        Case {
            name: "wide enough lines pass through untouched",
            body: RenderedBody {
                lines: vec![line("short one[1]", &[(6, 12, 0)])],
                links: vec![link(1, "https://example.com/s", "o")],
            },
            width: 40,
            lines: &["short one[1]"],
            spans: &[&[("one[1]", 1)]],
        },
        Case {
            name: "width one splits every character",
            body: RenderedBody {
                lines: vec![line("ab", &[(0, 2, 0)])],
                links: vec![link(1, "https://example.com/a", "ab")],
            },
            width: 1,
            lines: &["a", "b"],
            spans: &[&[("a", 1)], &[("b", 1)]],
        },
        Case {
            name: "trailing spaces leave no ghost line",
            body: RenderedBody {
                lines: vec![line("abcd ", &[])],
                links: vec![],
            },
            width: 4,
            lines: &["abcd"],
            spans: &[&[]],
        },
    ];
    for case in &cases {
        let wrapped = case.body.wrapped(case.width);
        assert_eq!(
            line_texts(&wrapped),
            case.lines,
            "lines for `{}`",
            case.name
        );
        let expected: Vec<Vec<(String, usize)>> = case
            .spans
            .iter()
            .map(|spans| {
                spans
                    .iter()
                    .map(|&(text, number)| (text.to_owned(), number))
                    .collect()
            })
            .collect();
        assert_eq!(
            span_texts(&wrapped),
            expected,
            "spans for `{}`",
            case.name
        );
        assert_eq!(wrapped.links, case.body.links);
    }
}

#[test]
fn wrapping_preserves_span_and_text_integrity() {
    let corpus = [
        rendered_body(&html_message(
            "<p>Read <a href=\"https://example.com/guide\">the \
             long user guide</a> and then \
             <a href=\"https://example.com/faq\">the FAQ</a> \
             before writing in.</p>\
             <p><a href=\"https://example.com/very/long/path/\
             that/never/ends\">https://example.com/very/long/\
             path/that/never/ends</a></p>",
        )),
        rendered_body(&plain_message(
            "opening prose with https://example.com/alpha inside\n\
             and a second line ending in \
             https://example.com/very/long/beta/path/segment",
        )),
    ];
    for body in &corpus {
        assert_indices_sequential(body);
        for width in 1..=30 {
            let wrapped = body.wrapped(width);
            assert_eq!(wrapped.links, body.links);
            for wrapped_line in &wrapped.lines {
                assert!(
                    wrapped_line.text.chars().count() <= width,
                    "line {:?} wider than {width}",
                    wrapped_line.text
                );
            }
            span_texts(&wrapped);
            assert_eq!(
                squash(&wrapped),
                squash(body),
                "text changed at width {width}"
            );
        }
    }
}

fn squash(body: &RenderedBody) -> String {
    body.lines
        .iter()
        .flat_map(|line| line.text.chars())
        .filter(|ch| *ch != ' ')
        .collect()
}
