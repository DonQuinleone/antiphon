mod common;

use antiphon_render::rendered_body;
use common::{html_message, line_texts};

struct Case {
    name: &'static str,
    html: &'static str,
    lines: &'static [&'static str],
}

#[test]
fn html_structures_render_as_text() {
    let cases = [
        Case {
            name: "entities decode beyond the basics",
            html: "<p>Q &amp; A &mdash; caf&eacute; \
                   &copy;&nbsp;2026 &hellip;</p>",
            lines: &["Q & A \u{2014} caf\u{e9} \u{a9}\u{a0}2026 \
                      \u{2026}"],
        },
        Case {
            name: "unordered lists get bullets",
            html: "<ul><li>alpha</li><li>beta</li></ul>",
            lines: &["* alpha", "* beta"],
        },
        Case {
            name: "ordered lists count",
            html: "<ol><li>first</li><li>second</li></ol>",
            lines: &["1. first", "2. second"],
        },
        Case {
            name: "nested lists indent",
            html: "<ul><li>outer<ul><li>inner</li></ul></li>\
                   </ul>",
            lines: &["* outer", "  * inner"],
        },
        Case {
            name: "blockquotes keep the quote prefix",
            html: "<blockquote><p>quoted words</p>\
                   </blockquote><p>reply</p>",
            lines: &["> quoted words", "", "reply"],
        },
        Case {
            name: "tables draw aligned cells",
            html: "<table><tr><th>name</th><th>qty</th></tr>\
                   <tr><td>bolt</td><td>40</td></tr>\
                   <tr><td>nut</td><td>7</td></tr></table>",
            lines: &[
                "\u{2500}\u{2500}\u{2500}\u{2500}\u{252c}\
                 \u{2500}\u{2500}\u{2500}",
                "name\u{2502}qty",
                "\u{2500}\u{2500}\u{2500}\u{2500}\u{253c}\
                 \u{2500}\u{2500}\u{2500}",
                "bolt\u{2502}40 ",
                "\u{2500}\u{2500}\u{2500}\u{2500}\u{253c}\
                 \u{2500}\u{2500}\u{2500}",
                "nut \u{2502}7  ",
                "\u{2500}\u{2500}\u{2500}\u{2500}\u{2534}\
                 \u{2500}\u{2500}\u{2500}",
            ],
        },
        Case {
            name: "headings stand alone",
            html: "<h1>Title</h1><p>body</p>",
            lines: &["# Title", "", "body"],
        },
        Case {
            name: "preformatted text keeps its shape",
            html: "<pre>a  b\n  c</pre>",
            lines: &["a  b", "  c"],
        },
    ];
    for case in &cases {
        let body = rendered_body(&html_message(case.html));
        assert_eq!(
            line_texts(&body),
            case.lines,
            "lines for `{}`",
            case.name
        );
    }
}

/// A marketing-shaped body: nested layout tables, entities in
/// text and hrefs, links wrapped in styling, list items.
#[test]
fn a_gnarly_marketing_body_stays_readable() {
    let html = concat!(
        "<html><head><style>td{color:red}</style></head><body>",
        "<table width=\"100%\"><tr><td>",
        "<table><tr><td><h2>Caf&eacute; Digest</h2>",
        "<p>Bonjour &amp; welcome &mdash; see ",
        "<a href=\"https://example.com/offers?a=1&amp;b=2\">",
        "<b>this week&rsquo;s offers</b></a>.</p>",
        "<ul><li>Croissants &ndash; 2&nbsp;for&nbsp;1</li>",
        "<li><a href=\"mailto:shop@example.com\">Order by ",
        "e-mail</a></li></ul>",
        "</td></tr></table>",
        "</td></tr></table>",
        "<blockquote>You wrote:<br>please stop</blockquote>",
        "</body></html>",
    );
    let body = rendered_body(&html_message(html));
    let text = line_texts(&body).join("\n");
    assert!(
        text.contains("Caf\u{e9} Digest"),
        "heading entity: {text}"
    );
    assert!(
        text.contains("Bonjour & welcome \u{2014} see"),
        "prose entities: {text}"
    );
    assert!(
        text.contains("this week\u{2019}s offers[1]."),
        "marked link label: {text}"
    );
    assert!(
        text.contains("* Croissants \u{2013} 2\u{a0}for\u{a0}1"),
        "list entities: {text}"
    );
    assert!(
        text.contains("* Order by e-mail[2]"),
        "second link marker: {text}"
    );
    assert!(
        text.lines().any(|line| line.starts_with("> You wrote:")),
        "quote prefix: {text}"
    );
    let links: Vec<(&str, &str)> = body
        .links
        .iter()
        .map(|link| (link.url.as_str(), link.label.as_str()))
        .collect();
    assert_eq!(
        links,
        [
            (
                "https://example.com/offers?a=1&b=2",
                "this week\u{2019}s offers"
            ),
            ("mailto:shop@example.com", "Order by e-mail"),
        ]
    );
}
