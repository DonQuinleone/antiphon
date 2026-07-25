use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
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
    let lines: Vec<Line<'static>> = app
        .key_bindings
        .iter()
        .map(|(key, action)| {
            Line::from(vec![
                Span::styled(
                    format!(" {key:<12}"),
                    Style::new().fg(theme.accent_strong),
                ),
                Span::styled(
                    action.clone(),
                    Style::new().fg(theme.text_primary),
                ),
            ])
        })
        .collect();
    let ceiling =
        (lines.len() as u16).saturating_sub(height.saturating_sub(2));
    let scroll = app.help_scroll.min(ceiling);
    frame.render_widget(
        Paragraph::new(lines).block(block).scroll((scroll, 0)),
        modal,
    );
}
