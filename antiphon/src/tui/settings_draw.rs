use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::settings::{SettingsState, SettingsTab};
use super::settingscmd;

const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const NAME_WIDTH: usize = 16;
const ADDRESS_WIDTH: usize = 28;
const LABEL_WIDTH: usize = 26;

pub(super) fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.settings else {
        return;
    };
    let [tabs_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
            .areas(area);
    frame.render_widget(
        Paragraph::new(tabs_line(app.theme, state.tab)),
        tabs_area,
    );
    match state.tab {
        SettingsTab::Accounts => {
            draw_accounts(frame, app.theme, state, body_area)
        }
        SettingsTab::Essentials => {
            draw_essentials(frame, app, state, body_area)
        }
    }
}

fn tabs_line(theme: &Theme, active: SettingsTab) -> Line<'static> {
    let tabs = [
        (SettingsTab::Accounts, "Accounts"),
        (SettingsTab::Essentials, "Essentials"),
    ];
    let spans = tabs
        .into_iter()
        .map(|(tab, label)| tab_span(theme, active, tab, label))
        .collect::<Vec<_>>();
    Line::from(spans)
}

fn tab_span(
    theme: &Theme,
    active: SettingsTab,
    tab: SettingsTab,
    label: &str,
) -> Span<'static> {
    let style = if tab == active {
        Style::new()
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text_muted)
    };
    Span::styled(format!(" {label} "), style)
}

fn draw_accounts(
    frame: &mut Frame,
    theme: &Theme,
    state: &SettingsState,
    area: Rect,
) {
    let mut lines = Vec::new();
    if state.accounts.is_empty() {
        lines.push(Line::from(Span::styled(
            "no accounts configured \u{b7} a adds one",
            Style::new().fg(theme.text_muted),
        )));
    }
    for (index, account) in state.accounts.iter().enumerate() {
        lines.push(account_line(
            theme,
            account,
            index == state.account_selected,
        ));
    }
    if let Some(name) = &state.pending_delete {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("delete {name}? y/n"),
            Style::new().fg(theme.accent_strong),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn account_line(
    theme: &Theme,
    account: &super::settings::AccountSummary,
    selected: bool,
) -> Line<'static> {
    let marker = mark(selected);
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    Line::from(Span::styled(
        format!(
            "{marker}{:<NAME_WIDTH$}{:<ADDRESS_WIDTH$}{}",
            account.name, account.address, account.host
        ),
        style,
    ))
}

fn draw_essentials(
    frame: &mut Frame,
    app: &App,
    state: &SettingsState,
    area: Rect,
) {
    let theme = app.theme;
    let mut lines = Vec::new();
    for (index, row) in settingscmd::ESSENTIAL_ROWS.iter().enumerate() {
        let selected = index == state.essentials_selected;
        let marker = mark(selected);
        let mut style = Style::new().fg(theme.text_primary);
        if selected {
            style = style.bg(theme.selection_bg).fg(theme.selection_fg);
        }
        let value = (row.render)(app);
        lines.push(Line::from(Span::styled(
            format!("{marker}{:<LABEL_WIDTH$}{value}", row.label),
            style,
        )));
    }
    if let Some(hint) = &state.daemon_hint {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            hint.clone(),
            Style::new().fg(theme.text_muted),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn mark(selected: bool) -> &'static str {
    if selected {
        SELECTED_MARK
    } else {
        UNSELECTED_MARK
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::super::settings::AccountSummary;
    use super::super::testkit::app_with_messages;
    use super::*;

    fn rendered(app: &App) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(70, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw_settings(frame, app, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn accounts_tab_lists_name_address_and_host() {
        let mut app = app_with_messages(1);
        app.settings = Some(SettingsState {
            tab: SettingsTab::Accounts,
            accounts: vec![AccountSummary {
                name: "work".to_string(),
                address: "quin@example.com".to_string(),
                host: "imap.example.com".to_string(),
            }],
            account_selected: 0,
            pending_delete: None,
            essentials_selected: 0,
            daemon_hint: None,
        });
        let buffer = rendered(&app);
        assert!(row(&buffer, 0).contains("Accounts"));
        let account_row = row(&buffer, 1);
        assert!(account_row.contains("work"));
        assert!(account_row.contains("quin@example.com"));
        assert!(account_row.contains("imap.example.com"));
    }

    #[test]
    fn a_pending_delete_shows_the_confirmation() {
        let mut app = app_with_messages(1);
        app.settings = Some(SettingsState {
            tab: SettingsTab::Accounts,
            accounts: vec![AccountSummary {
                name: "work".to_string(),
                address: String::new(),
                host: String::new(),
            }],
            account_selected: 0,
            pending_delete: Some("work".to_string()),
            essentials_selected: 0,
            daemon_hint: None,
        });
        let buffer = rendered(&app);
        let text: String =
            (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
        assert!(text.contains("delete work? y/n"));
    }

    #[test]
    fn essentials_tab_lists_every_row_and_the_daemon_hint() {
        let mut app = app_with_messages(1);
        app.settings = Some(SettingsState {
            tab: SettingsTab::Essentials,
            accounts: Vec::new(),
            account_selected: 0,
            pending_delete: None,
            essentials_selected: 0,
            daemon_hint: Some(
                "takes effect when antiphond restarts".to_string(),
            ),
        });
        let buffer = rendered(&app);
        let text: String =
            (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
        assert!(text.contains("theme"));
        assert!(text.contains("sync interval"));
        assert!(text.contains("sidebar width"));
        assert!(text.contains("takes effect when antiphond restarts"));
    }
}
