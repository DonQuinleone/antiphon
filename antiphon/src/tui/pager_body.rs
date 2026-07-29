use antiphon_render::{BodyLine, Link, PatchLine};
use ratatui::layout::Rect;

use super::app::App;
pub(super) use super::pager_style::{prose_colour, styled};

/// One visual row of the pager body: the wrapped line with
/// its link spans, plus how the row as a whole is coloured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BodyRow {
    pub line: BodyLine,
    pub kind: RowKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowKind {
    Invite,
    Image(usize),
    Body(PatchLine),
}

pub(super) fn rows(app: &App) -> Vec<BodyRow> {
    let mut rows = Vec::new();
    if !app.pager_invite.is_empty() {
        rows.extend(app.pager_invite.iter().map(|text| BodyRow {
            line: BodyLine {
                text: text.clone(),
                spans: Vec::new(),
            },
            kind: RowKind::Invite,
        }));
        rows.push(BodyRow {
            line: BodyLine::default(),
            kind: RowKind::Body(PatchLine::Text),
        });
    }
    rows.extend(app.pager_rendered.lines.iter().enumerate().map(
        |(index, line)| {
            BodyRow {
                line: line.clone(),
                kind: RowKind::Body(
                    app.pager_patch
                        .get(index)
                        .copied()
                        .unwrap_or(PatchLine::Text),
                ),
            }
        },
    ));
    rows.extend(image_marker_rows(app));
    rows
}

/// One `[image: <name>]` marker per image part, a render-only
/// block trailing the body: the message's bytes are untouched,
/// so the markers vanish from any forward. Gated by
/// ui.inline_images; off, the images live only in the drawer.
fn image_marker_rows(app: &App) -> Vec<BodyRow> {
    if !app.inline_images || app.pager_images.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![BodyRow {
        line: BodyLine::default(),
        kind: RowKind::Body(PatchLine::Text),
    }];
    rows.extend(app.pager_images.iter().enumerate().map(
        |(index, image)| BodyRow {
            line: BodyLine {
                text: format!("[image: {}]", image.name),
                spans: Vec::new(),
            },
            kind: RowKind::Image(index),
        },
    ));
    rows
}

/// The rows cut to the pane width. Drawing and mouse
/// hit-testing both come through here, so a click always
/// lands on exactly what was rendered.
pub(super) fn wrapped_rows(app: &App, width: usize) -> Vec<BodyRow> {
    rows(app)
        .into_iter()
        .flat_map(|row| {
            row.line.wrapped(width).into_iter().map(move |line| {
                BodyRow {
                    line,
                    kind: row.kind,
                }
            })
        })
        .collect()
}

/// The wrapped row under a click in the body pane, with the
/// click's column within it: the same rows the draw produced,
/// offset by the scroll, so a click lands on what was drawn.
fn row_at(
    app: &App,
    body: Rect,
    column: u16,
    row: u16,
) -> Option<(BodyRow, usize)> {
    let inside = column >= body.x
        && column < body.x.saturating_add(body.width)
        && row >= body.y
        && row < body.y.saturating_add(body.height);
    if !inside {
        return None;
    }
    let visual = (row - body.y) as usize + app.pager_scroll as usize;
    let column = (column - body.x) as usize;
    let rows = wrapped_rows(app, body.width as usize);
    rows.get(visual).cloned().map(|target| (target, column))
}

/// The url under a click in the body pane, if any.
pub(super) fn link_url_at(
    app: &App,
    body: Rect,
    column: u16,
    row: u16,
) -> Option<String> {
    let (target, column) = row_at(app, body, column, row)?;
    link_in(&target.line, column, &app.pager_rendered.links)
        .map(|link| link.url.clone())
}

/// The image index under a click on an `[image: ...]` marker.
pub(super) fn image_index_at(
    app: &App,
    body: Rect,
    column: u16,
    row: u16,
) -> Option<usize> {
    let (target, _) = row_at(app, body, column, row)?;
    match target.kind {
        RowKind::Image(index) => Some(index),
        _ => None,
    }
}

fn link_in<'a>(
    line: &BodyLine,
    column: usize,
    links: &'a [Link],
) -> Option<&'a Link> {
    let (offset, _) = line.text.char_indices().nth(column)?;
    let span = line
        .spans
        .iter()
        .find(|span| span.start <= offset && offset < span.end)?;
    links.get(span.link)
}

#[cfg(test)]
mod tests {
    use antiphon_pgp::Signature;
    use antiphon_ui::Theme;
    use ratatui::style::Modifier;
    use ratatui::text::Line;

    use super::super::testkit::app_with_messages;
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn image(name: &str) -> antiphon_render::MessageImage {
        antiphon_render::MessageImage {
            name: name.to_string(),
            cid: None,
            inline: false,
            content_type: "image/png".to_string(),
            bytes: vec![0],
        }
    }

    #[test]
    fn image_markers_trail_the_body_when_enabled() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        app.pager_images = vec![image("logo.png"), image("chart.gif")];
        let rows = rows(&app);
        let texts: Vec<&str> =
            rows.iter().map(|row| row.line.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "body line",
                "",
                "[image: logo.png]",
                "[image: chart.gif]"
            ],
        );
        assert_eq!(rows[2].kind, RowKind::Image(0));
        assert_eq!(rows[3].kind, RowKind::Image(1));
    }

    #[test]
    fn the_toggle_off_drops_the_markers() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        app.pager_images = vec![image("logo.png")];
        app.inline_images = false;
        let rows = rows(&app);
        assert!(
            rows.iter()
                .all(|row| !matches!(row.kind, RowKind::Image(_))),
            "no markers with the toggle off"
        );
    }

    #[test]
    fn a_click_lands_on_the_image_marker() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        app.pager_images = vec![image("logo.png")];
        let body = Rect::new(0, 3, 40, 10);
        assert_eq!(image_index_at(&app, body, 0, 5), Some(0));
        assert_eq!(image_index_at(&app, body, 0, 3), None);
        assert!(image_index_at(&app, body, 0, 20).is_none());
    }

    #[test]
    fn invite_blocks_lead_the_body() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            vec![
                "calendar invite: Stand-up".to_string(),
                "  reply:     :accept, :tentative or :decline"
                    .to_string(),
            ],
        );
        let rows = rows(&app);
        let texts: Vec<&str> =
            rows.iter().map(|row| row.line.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "calendar invite: Stand-up",
                "  reply:     :accept, :tentative or :decline",
                "",
                "body line",
            ],
        );
        assert_eq!(rows[0].kind, RowKind::Invite);
        let styled = styled(Theme::vespers(), &rows[0]);
        assert_eq!(
            styled.spans[0].style.fg,
            Some(Theme::vespers().accent)
        );
    }

    #[test]
    fn link_spans_colour_accent_underlined() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "see https://example.com/x now\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        let rows = rows(&app);
        let line = styled(Theme::vespers(), &rows[0]);
        assert_eq!(line_text(&line), "see https://example.com/x now");
        assert_eq!(line.spans.len(), 3);
        let link = &line.spans[1];
        assert_eq!(link.content.as_ref(), "https://example.com/x");
        assert_eq!(link.style.fg, Some(Theme::vespers().accent));
        assert!(link.style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn hit_tests_follow_the_wrapped_spans() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "aaa https://example.com/long/wrapped/path bbb\n"
                .to_string(),
            Signature::none(),
            Vec::new(),
        );
        let body = Rect::new(0, 3, 20, 10);
        let wrapped = wrapped_rows(&app, 20);
        let texts: Vec<&str> =
            wrapped.iter().map(|row| row.line.text.as_str()).collect();
        assert_eq!(
            texts,
            ["aaa", "https://example.com/", "long/wrapped/path", "bbb",],
        );
        let cases = [
            (0, 3, false),
            (4, 4, true),
            (2, 5, true),
            (1, 6, false),
            (19, 12, false),
        ];
        for (column, row, hits) in cases {
            let url = link_url_at(&app, body, column, row);
            assert_eq!(
                url.is_some(),
                hits,
                "column {column} row {row}: {url:?}"
            );
            if hits {
                assert_eq!(
                    url.as_deref(),
                    Some("https://example.com/long/wrapped/path")
                );
            }
        }
        assert!(
            link_url_at(&app, body, 25, 3).is_none(),
            "outside the pane"
        );
    }

    #[test]
    fn scrolling_offsets_the_hit_test() {
        let mut app = app_with_messages(1);
        app.open_pager(
            "one\ntwo\nhttps://example.com/x\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        let body = Rect::new(0, 0, 40, 5);
        assert!(link_url_at(&app, body, 0, 0).is_none());
        app.pager_scroll = 2;
        assert_eq!(
            link_url_at(&app, body, 0, 0).as_deref(),
            Some("https://example.com/x")
        );
    }
}
