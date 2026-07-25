use antiphon_pgp::{Signature, SignatureStatus};
use antiphon_render::{MessageHeader, PatchLine};
use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::app::App;

const KEYBAR: &str = "j/k:Scroll  t:Headers  h:Html  r:Reply  \
                      L:List-reply  q:Back  ?:Help";
const KEYBAR_ROWS: u16 = 1;
const RULE_ROWS: u16 = 1;
const MIN_TAG_GAP_COLS: usize = 2;
const ELLIPSIS: char = '\u{2026}';

pub(super) struct PagerChrome {
    pub keybar: Rect,
    pub headers: Rect,
    pub rule: Rect,
    pub body: Rect,
}

pub(super) fn chrome(app: &App, area: Rect) -> PagerChrome {
    let header_rows = header_lines(app, area.width).len() as u16;
    let [keybar, headers, rule, body] = Layout::vertical([
        Constraint::Length(KEYBAR_ROWS),
        Constraint::Length(header_rows),
        Constraint::Length(RULE_ROWS),
        Constraint::Min(0),
    ])
    .areas(area);
    PagerChrome {
        keybar,
        headers,
        rule,
        body,
    }
}

pub(super) fn draw_pager(frame: &mut Frame, app: &App, area: Rect) {
    let chrome = chrome(app, area);
    frame.render_widget(keybar(app.theme, area.width), chrome.keybar);
    frame.render_widget(
        Paragraph::new(header_lines(app, area.width)),
        chrome.headers,
    );
    frame.render_widget(rule(app.theme, area.width), chrome.rule);
    let body = Paragraph::new(body_lines(app))
        .wrap(Wrap { trim: false })
        .scroll((app.pager_scroll, 0));
    frame.render_widget(body, chrome.body);
}

fn keybar(theme: &Theme, width: u16) -> Paragraph<'static> {
    let bar = Style::new()
        .fg(theme.text_primary)
        .bg(theme.surface)
        .add_modifier(Modifier::BOLD);
    Paragraph::new(Line::from(Span::styled(
        format!("{KEYBAR:<width$}", width = width as usize),
        bar,
    )))
}

fn rule(theme: &Theme, width: u16) -> Paragraph<'static> {
    Paragraph::new(Line::from(Span::styled(
        "\u{2500}".repeat(width as usize),
        Style::new().fg(theme.border),
    )))
}

fn header_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let theme = app.theme;
    let tags = app
        .selected_message()
        .map(|message| message.tags.clone())
        .unwrap_or_default();
    let mut lines =
        header_block(theme, app.pager_header_view(), &tags, width);
    if let Some(line) = signature_line(theme, &app.pager_signature) {
        lines.push(line);
    }
    lines
}

/// One row per header, in the order given, with the tags
/// riding the first row; the reading pane shares this shape.
pub(super) fn header_block(
    theme: &Theme,
    headers: &[MessageHeader],
    tags: &[String],
    width: u16,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = headers
        .iter()
        .map(|header| named_header_line(theme, header))
        .collect();
    let Some(top) = lines.first_mut() else {
        return lines;
    };
    *top = with_tags(theme, top.clone(), tags, width);
    lines
}

fn named_header_line(
    theme: &Theme,
    header: &MessageHeader,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{}: ", header.name),
            Style::new().fg(theme.accent),
        ),
        Span::styled(
            header.value.clone(),
            Style::new().fg(theme.text_primary),
        ),
    ])
}

/// The first header row carries the tags right-aligned and
/// muted; a narrow terminal truncates them with an ellipsis
/// rather than wrapping the row.
fn with_tags(
    theme: &Theme,
    mut line: Line<'static>,
    tags: &[String],
    width: u16,
) -> Line<'static> {
    if tags.is_empty() {
        return line;
    }
    let used: usize = line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum();
    let room = (width as usize).saturating_sub(used + MIN_TAG_GAP_COLS);
    if room == 0 {
        return line;
    }
    let tags_text = fitted(&tags.join(", "), room);
    let pad = (width as usize)
        .saturating_sub(used + tags_text.chars().count());
    line.spans.push(Span::raw(" ".repeat(pad)));
    line.spans.push(Span::styled(
        tags_text,
        Style::new().fg(theme.text_muted),
    ));
    line
}

fn fitted(text: &str, room: usize) -> String {
    if text.chars().count() <= room {
        return text.to_string();
    }
    let mut cut: String =
        text.chars().take(room.saturating_sub(1)).collect();
    cut.push(ELLIPSIS);
    cut
}

fn body_lines(app: &App) -> Vec<Line<'static>> {
    let theme = app.theme;
    let mut lines = Vec::new();
    if !app.pager_invite.is_empty() {
        lines.extend(
            app.pager_invite
                .iter()
                .map(|text| invite_line(theme, text)),
        );
        lines.push(Line::default());
    }
    lines.extend(app.pager_body.lines().enumerate().map(
        |(index, body_line)| {
            let kind = app
                .pager_patch
                .get(index)
                .copied()
                .unwrap_or(PatchLine::Text);
            Line::from(Span::styled(
                body_line.to_string(),
                Style::new()
                    .fg(pager_line_colour(theme, kind, body_line)),
            ))
        },
    ));
    lines
}

fn invite_line(theme: &Theme, text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_string(),
        Style::new().fg(theme.accent),
    ))
}

fn pager_line_colour(
    theme: &Theme,
    kind: PatchLine,
    line: &str,
) -> ratatui::style::Color {
    match kind {
        PatchLine::Addition => theme.diff_add,
        PatchLine::Removal => theme.diff_remove,
        PatchLine::FileHeader => theme.accent_strong,
        PatchLine::HunkHeader => theme.accent,
        PatchLine::NoNewline | PatchLine::Envelope => theme.text_muted,
        PatchLine::Text => prose_colour(theme, line),
    }
}

pub(super) fn prose_colour(
    theme: &Theme,
    line: &str,
) -> ratatui::style::Color {
    if line.trim_start().starts_with('>') {
        theme.text_muted
    } else {
        theme.text_primary
    }
}

fn signature_line(
    theme: &Theme,
    signature: &Signature,
) -> Option<Line<'static>> {
    let text = signature.header_line()?;
    let colour = signature_colour(theme, &signature.status);
    Some(Line::from(Span::styled(text, Style::new().fg(colour))))
}

fn signature_colour(
    theme: &Theme,
    status: &SignatureStatus,
) -> ratatui::style::Color {
    match status {
        SignatureStatus::Good { .. } => theme.status_ok,
        SignatureStatus::Bad { .. } => theme.status_error,
        SignatureStatus::Unknown { .. } | SignatureStatus::None => {
            theme.text_muted
        }
    }
}

#[cfg(test)]
mod tests {
    use antiphon_ui::VESPERS;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::super::testkit::app_with_messages;
    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn row_text(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    fn rendered(app: &App, width: u16, height: u16) -> Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::super::draw::draw(frame, app))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    const RAW: &str = concat!(
        "From: alba@example.com\r\n",
        "To: quin@example.com\r\n",
        "Date: Fri, 24 Jul 2026 09:00:00 +0000\r\n",
        "Subject: Rehearsal\r\n",
        "X-Mailer: antiphon\r\n",
        "\r\n",
        "body line\r\n",
    );

    fn pager_app() -> App {
        let mut app = app_with_messages(1);
        app.messages[0].tags =
            vec!["lists".to_string(), "patch".to_string()];
        app.pager_raw = RAW.as_bytes().to_vec();
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        app
    }

    #[test]
    fn the_pager_keeps_its_chrome_and_the_statusline() {
        let app = pager_app();
        let buffer = rendered(&app, 60, 14);
        assert!(
            row_text(&buffer, 0).starts_with("j/k:Scroll"),
            "{:?}",
            row_text(&buffer, 0)
        );
        assert!(row_text(&buffer, 1).contains("From:"));
        assert!(row_text(&buffer, 2).contains("To:"));
        assert!(
            row_text(&buffer, 3)
                .contains("Date: Fri, 24 Jul 2026 09:00:00 +0000")
        );
        assert!(row_text(&buffer, 4).contains("Subject: Rehearsal"));
        assert!(
            !(0..14).any(|y| row_text(&buffer, y).contains("X-Mailer")),
            "unconfigured headers stay hidden"
        );
        let rule = row_text(&buffer, 5);
        assert!(
            rule.chars().all(|ch| ch == '\u{2500}'),
            "rule under the headers: {rule:?}"
        );
        assert!(row_text(&buffer, 6).starts_with("body line"));
        let status = row_text(&buffer, 13);
        assert!(
            status.contains("messages"),
            "statusline stays: {status:?}"
        );
    }

    #[test]
    fn tags_sit_right_aligned_on_the_top_header_row() {
        let app = pager_app();
        let buffer = rendered(&app, 60, 14);
        let top = row_text(&buffer, 1);
        assert!(top.trim_end().ends_with("lists, patch"), "{top:?}");
        assert!(top.starts_with("From: alba@example.com"));
        let x = top.find("lists, patch").unwrap() as u16;
        let cell = buffer.cell((x, 1)).unwrap();
        assert_eq!(cell.style().fg, Some(VESPERS.text_muted));
    }

    #[test]
    fn overlong_tags_truncate_with_an_ellipsis() {
        let theme = &VESPERS;
        let tags = vec![
            "a-very-long-tag".to_string(),
            "another-long-tag".to_string(),
        ];
        let from = MessageHeader {
            name: "From".to_string(),
            value: "someone@example.com".to_string(),
        };
        let base = named_header_line(theme, &from);
        let line = with_tags(theme, base, &tags, 40);
        let text = line_text(&line);
        assert!(text.chars().count() <= 40, "{text:?}");
        assert!(text.ends_with('\u{2026}'), "{text:?}");

        let narrow = named_header_line(theme, &from);
        let untouched = with_tags(theme, narrow.clone(), &tags, 20);
        assert_eq!(line_text(&untouched), line_text(&narrow));
    }

    #[test]
    fn the_configured_set_governs_the_header_rows() {
        use antiphon_core::Action;

        let mut app = app_with_messages(1);
        app.header_names = vec!["subject".to_string()];
        app.pager_raw = RAW.as_bytes().to_vec();
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        let buffer = rendered(&app, 60, 14);
        assert!(row_text(&buffer, 1).starts_with("Subject: Rehearsal"));
        assert!(
            row_text(&buffer, 2).chars().all(|ch| ch == '\u{2500}'),
            "one configured header, then the rule"
        );

        app.apply(Action::ToggleHeaders);
        let buffer = rendered(&app, 60, 14);
        let rows: Vec<String> =
            (0..14).map(|y| row_text(&buffer, y)).collect();
        assert!(
            rows.iter().any(|row| row.starts_with("X-Mailer:")),
            "the toggle shows every header: {rows:?}"
        );

        app.apply(Action::ToggleHeaders);
        let buffer = rendered(&app, 60, 14);
        assert!(
            !(0..14).any(|y| row_text(&buffer, y).contains("X-Mailer")),
            "toggling back restores the configured set"
        );
    }

    #[test]
    fn signature_line_renders_and_colours_per_status() {
        let theme = &VESPERS;
        let cases = [
            (
                SignatureStatus::Good {
                    signer: "Alice <alice@example.com>".to_string(),
                    key_id: "1A2B3C4D5E6F7A8B".to_string(),
                },
                theme.status_ok,
                "Good signature from Alice \
                 <alice@example.com> (0x1A2B3C4D5E6F7A8B)",
            ),
            (
                SignatureStatus::Bad {
                    key_id: "0BADC0DE0BADC0DE".to_string(),
                },
                theme.status_error,
                "BAD signature (0x0BADC0DE0BADC0DE)",
            ),
            (
                SignatureStatus::Unknown {
                    key_id: "DEADBEEFDEADBEEF".to_string(),
                },
                theme.text_muted,
                "Unknown signature from key \
                 0xDEADBEEFDEADBEEF (not in keyring)",
            ),
        ];
        for (status, colour, expected) in cases {
            let signature = Signature::from_status(status);
            let line =
                signature_line(theme, &signature).expect("a line");
            assert_eq!(line_text(&line), expected);
            assert_eq!(line.spans[0].style.fg, Some(colour));
        }
    }

    #[test]
    fn unsigned_message_shows_no_signature_line() {
        assert!(signature_line(&VESPERS, &Signature::none()).is_none());
    }

    #[test]
    fn invite_blocks_lead_the_body() {
        let mut app = app_with_messages(1);
        app.theme = &VESPERS;
        app.open_pager(
            "body line\n".to_string(),
            Signature::none(),
            vec![
                "calendar invite: Stand-up".to_string(),
                "  reply:     accept/decline not yet wired".to_string(),
            ],
        );
        let lines = body_lines(&app);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert_eq!(texts[0], "calendar invite: Stand-up");
        assert_eq!(lines[0].spans[0].style.fg, Some(VESPERS.accent));
        assert_eq!(
            texts[1],
            "  reply:     accept/decline not yet wired"
        );
        assert_eq!(texts[2], "");
        assert_eq!(texts[3], "body line");

        app.open_pager(
            "plain\n".to_string(),
            Signature::none(),
            Vec::new(),
        );
        let texts: Vec<String> =
            body_lines(&app).iter().map(line_text).collect();
        assert_eq!(texts, ["plain"]);
    }

    #[test]
    fn patch_classifications_map_to_theme_roles() {
        let theme = &VESPERS;
        let cases = [
            (PatchLine::Addition, "+new", theme.diff_add),
            (PatchLine::Removal, "-old", theme.diff_remove),
            (
                PatchLine::FileHeader,
                "diff --git a/f b/f",
                theme.accent_strong,
            ),
            (PatchLine::HunkHeader, "@@ -1 +1 @@", theme.accent),
            (
                PatchLine::NoNewline,
                "\\ No newline at end of file",
                theme.text_muted,
            ),
            (PatchLine::Envelope, "---", theme.text_muted),
            (PatchLine::Text, "prose", theme.text_primary),
            (PatchLine::Text, "> quoted", theme.text_muted),
        ];
        for (kind, line, expected) in cases {
            assert_eq!(
                pager_line_colour(theme, kind, line),
                expected,
                "{kind:?} `{line}`"
            );
        }
    }
}
