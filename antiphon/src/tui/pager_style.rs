use antiphon_render::PatchLine;
use antiphon_ui::Theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::pager_body::{BodyRow, RowKind};

pub(super) fn styled(theme: &Theme, row: &BodyRow) -> Line<'static> {
    let colour = row_colour(theme, row);
    let text = &row.line.text;
    if row.line.spans.is_empty() {
        return Line::from(Span::styled(
            text.clone(),
            Style::new().fg(colour),
        ));
    }
    let base = Style::new().fg(colour);
    let mut spans = Vec::new();
    let mut cursor = 0;
    for span in &row.line.spans {
        if span.start > cursor {
            spans.push(Span::styled(
                text[cursor..span.start].to_string(),
                base,
            ));
        }
        spans.push(Span::styled(
            text[span.start..span.end].to_string(),
            link_style(theme),
        ));
        cursor = span.end;
    }
    if cursor < text.len() {
        spans.push(Span::styled(text[cursor..].to_string(), base));
    }
    Line::from(spans)
}

fn link_style(theme: &Theme) -> Style {
    Style::new()
        .fg(theme.accent)
        .add_modifier(Modifier::UNDERLINED)
}

fn row_colour(theme: &Theme, row: &BodyRow) -> ratatui::style::Color {
    match row.kind {
        RowKind::Invite | RowKind::Image(_) => theme.accent,
        RowKind::Body(kind) => {
            pager_line_colour(theme, kind, &row.line.text)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_classifications_map_to_theme_roles() {
        let theme = Theme::vespers();
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
