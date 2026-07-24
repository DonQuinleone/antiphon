use antiphon_pgp::{Signature, SignatureStatus};
use antiphon_render::PatchLine;
use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::app::App;
use super::draw::{format_date, header_line};

pub(super) fn draw_pager(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let Some(message) = app.selected_message() else {
        return;
    };
    let mut lines = vec![
        header_line(theme, "From:", message.from.clone()),
        header_line(
            theme,
            "Date:",
            format_date(message.date_unix, &app.date_format),
        ),
        header_line(theme, "Subject:", message.subject.clone()),
        header_line(theme, "Tags:", message.tags.join(", ")),
    ];
    if let Some(line) = signature_line(theme, &app.pager_signature) {
        lines.push(line);
    }
    lines.push(Line::default());
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
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((app.pager_scroll, 0));
    frame.render_widget(paragraph, area);
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

fn prose_colour(theme: &Theme, line: &str) -> ratatui::style::Color {
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

    use super::*;

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
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
