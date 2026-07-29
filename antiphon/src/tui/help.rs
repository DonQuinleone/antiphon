use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use super::app::App;

const HELP_WIDTH: u16 = 40;
const HELP_MAX_ROWS: u16 = 24;

/// The cheatsheet renders the LIVE keymap (defaults merged
/// with the user's [keys] overrides), never a separate list,
/// as a centred modal over whatever is behind it.
pub(super) fn draw_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = app.theme;
    let width = HELP_WIDTH.min(area.width.saturating_sub(2));
    let height = HELP_MAX_ROWS.min(area.height.saturating_sub(2));
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(Clear, modal);
    let block = Block::bordered()
        .title(" keys ")
        .title_bottom(" j/k scroll \u{b7} any other key closes ")
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current = "";
    for (key, action, context) in &app.key_bindings {
        if *context != current {
            current = *context;
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::from(Span::styled(
                format!(" {context}"),
                Style::new()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {key:<11}"),
                Style::new().fg(theme.accent_strong),
            ),
            Span::styled(
                action.clone(),
                Style::new().fg(theme.text_primary),
            ),
        ]));
    }
    let ceiling =
        (lines.len() as u16).saturating_sub(height.saturating_sub(2));
    let scroll = app.help_scroll.min(ceiling);
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll, 0)),
        modal,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::testkit::app_with_messages;

    fn rendered(app: &App) -> String {
        let backend = TestBackend::new(44, 44);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_help(frame, app, frame.area()))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| {
                        buffer
                            .cell((x, y))
                            .unwrap()
                            .symbol()
                            .to_string()
                    })
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_cheatsheet_groups_bindings_under_context_headings() {
        let mut app = app_with_messages(1);
        app.help = true;

        // The global block heads the list, so it shows first.
        let top = rendered(&app);
        assert!(top.contains("global"), "{top}");
        assert!(top.contains("move-down"), "{top}");

        // Scrolling to the end reveals the last context groups
        // under their own headings, with surface-only actions.
        app.help_scroll = u16::MAX;
        let tail = rendered(&app);
        for text in
            ["compose", "compose-submit", "prompt", "prompt-submit"]
        {
            assert!(tail.contains(text), "{text}\n{tail}");
        }
    }
}
