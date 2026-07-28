mod common;

use antiphon_render::rendered_body;
use common::{
    assert_indices_sequential, html_message, line_texts, span_texts,
};

struct Case {
    name: &'static str,
    html: &'static str,
    lines: &'static [&'static str],
    spans: &'static [&'static [(&'static str, usize)]],
    links: &'static [(&'static str, &'static str)],
}

#[test]
fn html_anchors_render_with_markers() {
    let cases = [
        Case {
            name: "text anchor keeps its label",
            html: "<p>See <a href=\"https://example.com/page\">\
                   the docs</a> now</p>",
            lines: &["See the docs[1] now"],
            spans: &[&[("the docs[1]", 1)]],
            links: &[("https://example.com/page", "the docs")],
        },
        Case {
            name: "url as text is not repeated",
            html: "<p><a href=\"https://example.com/\">\
                   https://example.com</a></p>",
            lines: &["https://example.com[1]"],
            spans: &[&[("https://example.com[1]", 1)]],
            links: &[("https://example.com/", "https://example.com")],
        },
        Case {
            name: "duplicate urls share a number",
            html: "<p><a href=\"https://example.com/a\">one</a> \
                   and <a href=\"https://example.com/a\">two</a>\
                   </p>",
            lines: &["one[1] and two[1]"],
            spans: &[&[("one[1]", 1), ("two[1]", 1)]],
            links: &[("https://example.com/a", "one")],
        },
        Case {
            name: "image-only anchor uses the alt text",
            html: "<a href=\"https://example.com/i\">\
                   <img src=\"l.png\" alt=\"Logo\"></a>",
            lines: &["Logo[1]"],
            spans: &[&[("Logo[1]", 1)]],
            links: &[("https://example.com/i", "Logo")],
        },
        Case {
            name: "image-only anchor without alt renders nothing",
            html: "<a href=\"https://example.com/i\">\
                   <img src=\"l.png\"></a>",
            lines: &[],
            spans: &[],
            links: &[],
        },
        Case {
            name: "nested markup stays in the label",
            html: "<p><a href=\"https://example.com/n\">\
                   <b>bold</b> and <i>italic</i></a></p>",
            lines: &["bold and italic[1]"],
            spans: &[&[("bold and italic[1]", 1)]],
            links: &[("https://example.com/n", "bold and italic")],
        },
        Case {
            name: "entities decode in label and href",
            html: "<p><a href=\"https://example.com/?a=1&amp;b=2\">\
                   Q &amp; A</a></p>",
            lines: &["Q & A[1]"],
            spans: &[&[("Q & A[1]", 1)]],
            links: &[("https://example.com/?a=1&b=2", "Q & A")],
        },
        Case {
            name: "marker hugs the label before punctuation",
            html: "<p>Read <a href=\"https://example.com/d\">\
                   this</a>, please</p>",
            lines: &["Read this[1], please"],
            spans: &[&[("this[1]", 1)]],
            links: &[("https://example.com/d", "this")],
        },
        Case {
            name: "fragment and javascript anchors stay plain",
            html: "<p><a href=\"#top\">top</a> \
                   <a href=\"javascript:x()\">run</a></p>",
            lines: &["top run"],
            spans: &[&[]],
            links: &[],
        },
        Case {
            name: "numbering follows document order",
            html: "<p><a href=\"https://example.com/1\">one</a> \
                   then <a href=\"https://example.com/2\">two</a>\
                   </p>",
            lines: &["one[1] then two[2]"],
            spans: &[&[("one[1]", 1), ("two[2]", 2)]],
            links: &[
                ("https://example.com/1", "one"),
                ("https://example.com/2", "two"),
            ],
        },
        Case {
            name: "paragraphs and breaks shape lines",
            html: "<p>one</p><p>two<br>three</p>",
            lines: &["one", "", "two", "three"],
            spans: &[&[], &[], &[], &[]],
            links: &[],
        },
        Case {
            name: "double break opens a blank line",
            html: "<p>a<br><br>b</p>",
            lines: &["a", "", "b"],
            spans: &[&[], &[], &[]],
            links: &[],
        },
        Case {
            name: "style and script content is skipped",
            html: "<style>p { color: red }</style>\
                   <script>var x = 1;</script><p>seen</p>",
            lines: &["seen"],
            spans: &[&[]],
            links: &[],
        },
        Case {
            name: "anchor split by a break spans two lines",
            html: "<p><a href=\"https://example.com/s\">first\
                   <br>second</a></p>",
            lines: &["first", "second[1]"],
            spans: &[&[("first", 1)], &[("second[1]", 1)]],
            links: &[("https://example.com/s", "first second")],
        },
        Case {
            name: "unquoted uppercase attributes work",
            html: "<P><A HREF=https://example.com/u>up</A></P>",
            lines: &["up[1]"],
            spans: &[&[("up[1]", 1)]],
            links: &[("https://example.com/u", "up")],
        },
        Case {
            name: "list items get bullets",
            html: "<ul><li>alpha</li><li>beta</li></ul>",
            lines: &["* alpha", "* beta"],
            spans: &[&[], &[]],
            links: &[],
        },
        Case {
            name: "mailto anchor links",
            html: "<p>Contact <a href=\"mailto:bob@example.com\">\
                   Bob</a></p>",
            lines: &["Contact Bob[1]"],
            spans: &[&[("Bob[1]", 1)]],
            links: &[("mailto:bob@example.com", "Bob")],
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
        let found: Vec<(&str, &str)> = body
            .links
            .iter()
            .map(|link| (link.url.as_str(), link.label.as_str()))
            .collect();
        assert_eq!(found, case.links, "links for `{}`", case.name);
        assert_indices_sequential(&body);
    }
}
