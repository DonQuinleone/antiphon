mod common;

use antiphon_render::rendered_body;
use common::{
    assert_indices_sequential, line_texts, plain_message, span_texts,
};

struct Case {
    name: &'static str,
    body: &'static str,
    spans: &'static [&'static [(&'static str, usize)]],
    links: &'static [&'static str],
}

#[test]
fn plain_text_urls_are_detected_without_rewriting() {
    let cases = [
        Case {
            name: "url at line start",
            body: "https://example.com/a is neat",
            spans: &[&[("https://example.com/a", 1)]],
            links: &["https://example.com/a"],
        },
        Case {
            name: "url mid line",
            body: "see https://example.com/b here",
            spans: &[&[("https://example.com/b", 1)]],
            links: &["https://example.com/b"],
        },
        Case {
            name: "url at line end",
            body: "go to https://example.com/c",
            spans: &[&[("https://example.com/c", 1)]],
            links: &["https://example.com/c"],
        },
        Case {
            name: "trailing full stop excluded",
            body: "read https://example.com/d.",
            spans: &[&[("https://example.com/d", 1)]],
            links: &["https://example.com/d"],
        },
        Case {
            name: "semicolon and comma excluded",
            body: "https://example.com/e; then \
                   https://example.com/f, ok",
            spans: &[&[
                ("https://example.com/e", 1),
                ("https://example.com/f", 2),
            ]],
            links: &["https://example.com/e", "https://example.com/f"],
        },
        Case {
            name: "parenthesised url drops the bracket",
            body: "(see https://example.com/g)",
            spans: &[&[("https://example.com/g", 1)]],
            links: &["https://example.com/g"],
        },
        Case {
            name: "balanced brackets stay in the url",
            body: "https://example.com/x_(y) stays",
            spans: &[&[("https://example.com/x_(y)", 1)]],
            links: &["https://example.com/x_(y)"],
        },
        Case {
            name: "mailto with trailing comma",
            body: "write mailto:bob@example.com, thanks",
            spans: &[&[("mailto:bob@example.com", 1)]],
            links: &["mailto:bob@example.com"],
        },
        Case {
            name: "angle brackets delimit the url",
            body: "at <https://example.com/h> now",
            spans: &[&[("https://example.com/h", 1)]],
            links: &["https://example.com/h"],
        },
        Case {
            name: "duplicate urls share a number",
            body: "https://example.com/i and\n\
                   https://example.com/i again",
            spans: &[
                &[("https://example.com/i", 1)],
                &[("https://example.com/i", 1)],
            ],
            links: &["https://example.com/i"],
        },
        Case {
            name: "prose without a scheme has no links",
            body: "just example.com prose",
            spans: &[&[]],
            links: &[],
        },
        Case {
            name: "a bare scheme is not a link",
            body: "https:// alone",
            spans: &[&[]],
            links: &[],
        },
        Case {
            name: "schemes embedded in a word are skipped",
            body: "xhttps://example.com/j",
            spans: &[&[]],
            links: &[],
        },
        Case {
            name: "quoted lines still link",
            body: "> https://example.com/k quoted",
            spans: &[&[("https://example.com/k", 1)]],
            links: &["https://example.com/k"],
        },
    ];
    for case in &cases {
        let body = rendered_body(&plain_message(case.body));
        let source: Vec<&str> = case.body.lines().collect();
        assert_eq!(
            line_texts(&body),
            source,
            "text rewritten in `{}`",
            case.name
        );
        let expected: Vec<Vec<(String, usize)>> = case
            .spans
            .iter()
            .map(|line| {
                line.iter()
                    .map(|&(text, number)| (text.to_owned(), number))
                    .collect()
            })
            .collect();
        assert_eq!(
            span_texts(&body),
            expected,
            "spans for `{}`",
            case.name
        );
        let urls: Vec<&str> =
            body.links.iter().map(|link| link.url.as_str()).collect();
        assert_eq!(urls, case.links, "links for `{}`", case.name);
        for link in &body.links {
            assert_eq!(
                link.label, link.url,
                "plain labels mirror urls in `{}`",
                case.name
            );
        }
        assert_indices_sequential(&body);
    }
}
