#![allow(dead_code)]

use antiphon_render::RenderedBody;

pub fn html_message(body: &str) -> Vec<u8> {
    format!(
        "From: alice@example.com\r\n\
         Subject: html\r\n\
         MIME-Version: 1.0\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         \r\n\
         {body}\r\n"
    )
    .into_bytes()
}

pub fn plain_message(body: &str) -> Vec<u8> {
    format!(
        "From: alice@example.com\r\n\
         Subject: plain\r\n\
         Content-Type: text/plain; charset=utf-8\r\n\
         \r\n\
         {body}\r\n"
    )
    .into_bytes()
}

pub fn line_texts(body: &RenderedBody) -> Vec<&str> {
    body.lines.iter().map(|line| line.text.as_str()).collect()
}

pub fn span_texts(body: &RenderedBody) -> Vec<Vec<(String, usize)>> {
    body.lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| {
                    assert!(span.start <= span.end, "{span:?}");
                    assert!(
                        span.end <= line.text.len(),
                        "{span:?} outside {:?}",
                        line.text
                    );
                    assert!(line.text.is_char_boundary(span.start));
                    assert!(line.text.is_char_boundary(span.end));
                    (
                        line.text[span.start..span.end].to_owned(),
                        body.links[span.link].index,
                    )
                })
                .collect()
        })
        .collect()
}

pub fn assert_indices_sequential(body: &RenderedBody) {
    for (position, link) in body.links.iter().enumerate() {
        assert_eq!(
            link.index,
            position + 1,
            "link {:?} out of order",
            link.url
        );
    }
}
