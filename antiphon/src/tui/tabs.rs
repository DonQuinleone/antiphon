//! The tabbed accounts mode (`ui.accounts_bar = "tabs"`): a
//! one-line bar above the sidebar and list naming unified and
//! every account, plus the g1..g9/gu jumps that also work in
//! sidebar mode.

use antiphon_config::AccountsBar;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::scope::ViewScope;

const UNIFIED_LABEL: &str = "unified";
const TAB_BAR_HEIGHT: u16 = 1;

impl App {
    /// g1..g9 jump straight to that account's scope; a number
    /// with no account says so instead of doing nothing.
    pub(super) fn open_account_tab(&mut self, tab: u8) {
        let index = usize::from(tab).saturating_sub(1);
        let Some(account) = self.accounts.get(index).cloned() else {
            self.notice = Some(format!("no account {tab}"));
            return;
        };
        self.switch_scope(ViewScope::Account(account));
    }

    pub(super) fn open_unified_tab(&mut self) {
        self.switch_scope(ViewScope::Unified);
    }

    /// The one path every scope change takes: the current
    /// query re-runs under the new scope, and in tabs mode
    /// the sidebar follows the active tab.
    pub(super) fn switch_scope(&mut self, scope: ViewScope) {
        self.scope = scope;
        self.thread_return = None;
        self.requery = true;
        self.sync_tab_sidebar();
    }

    /// In tabs mode the sidebar lists only the active
    /// account's folders, so a scope change rebuilds it; the
    /// full sidebar never changes with scope.
    pub(super) fn sync_tab_sidebar(&mut self) {
        if self.accounts_bar == AccountsBar::Tabs {
            self.rebuild_sidebar();
        }
    }
}

/// Reserves the bar's line above the given area in tabs mode;
/// sidebar mode passes the area through untouched.
pub(super) fn split_tab_bar(
    area: Rect,
    mode: AccountsBar,
) -> (Option<Rect>, Rect) {
    if mode == AccountsBar::Sidebar {
        return (None, area);
    }
    let [bar, rest] = Layout::vertical([
        Constraint::Length(TAB_BAR_HEIGHT),
        Constraint::Min(0),
    ])
    .areas(area);
    (Some(bar), rest)
}

pub(super) fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    frame.render_widget(Paragraph::new(tab_line(app)), area);
}

fn tab_line(app: &App) -> Line<'static> {
    let mut spans = vec![tab_span(
        app,
        UNIFIED_LABEL.to_string(),
        app.scope == ViewScope::Unified,
    )];
    for (index, account) in app.accounts.iter().enumerate() {
        let active = matches!(
            &app.scope,
            ViewScope::Account(current) if current == account
        );
        let label = format!("{}:{account}", index + 1);
        spans.push(tab_span(app, label, active));
    }
    Line::from(spans)
}

fn tab_span(app: &App, label: String, active: bool) -> Span<'static> {
    let theme = app.theme;
    let style = if active {
        Style::new()
            .fg(theme.accent_strong)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.text_muted)
    };
    Span::styled(format!(" {label} "), style)
}

#[cfg(test)]
mod tests {
    use antiphon_core::Action;

    use super::super::sidebar::SidebarEntry;
    use super::super::testkit::app_with_folders;
    use super::*;

    fn tabbed_app() -> App {
        let mut app = app_with_folders(&[
            ("a", &["archive"][..]),
            ("b", &["lists"][..]),
        ]);
        app.accounts_bar = AccountsBar::Tabs;
        app
    }

    fn labels(app: &App) -> Vec<String> {
        app.sidebar_entries
            .iter()
            .map(|entry| entry.label().to_string())
            .collect()
    }

    #[test]
    fn g_number_jumps_to_the_account_and_requeries() {
        let mut app = tabbed_app();
        app.apply(Action::AccountTab(2));
        assert_eq!(app.scope, ViewScope::Account("b".into()));
        assert!(app.take_requery());
        assert_eq!(
            labels(&app),
            ["inbox", "lists", "all", "inbox", "unread", "flagged"],
            "only the active account's folders, then searches"
        );
        assert!(
            !app.sidebar_entries
                .iter()
                .any(|entry| entry == &SidebarEntry::Unified),
        );

        app.apply(Action::AccountUnified);
        assert_eq!(app.scope, ViewScope::Unified);
        assert!(app.take_requery());
        assert_eq!(
            labels(&app),
            ["all", "inbox", "unread", "flagged"],
            "the unified tab keeps only the searches"
        );
    }

    #[test]
    fn a_number_with_no_account_notices_and_stays() {
        let mut app = tabbed_app();
        app.apply(Action::AccountTab(7));
        assert_eq!(app.scope, ViewScope::Unified);
        assert!(!app.take_requery());
        assert_eq!(app.notice.as_deref(), Some("no account 7"));
    }

    #[test]
    fn the_jumps_switch_scope_in_sidebar_mode_too() {
        let mut app = app_with_folders(&[("a", &[][..])]);
        app.apply(Action::AccountTab(1));
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert!(app.take_requery());
        assert!(
            app.sidebar_entries
                .iter()
                .any(|entry| entry == &SidebarEntry::Unified),
            "sidebar mode keeps the full tree"
        );
    }

    #[test]
    fn cycling_follows_the_tabs_in_tabs_mode() {
        let mut app = tabbed_app();
        app.apply(Action::NextAccount);
        assert_eq!(app.scope, ViewScope::Account("a".into()));
        assert_eq!(
            labels(&app),
            ["inbox", "archive", "all", "inbox", "unread", "flagged"],
        );
    }

    #[test]
    fn the_bar_reserves_one_line_only_in_tabs_mode() {
        let area = Rect::new(0, 0, 40, 10);
        let (bar, rest) = split_tab_bar(area, AccountsBar::Sidebar);
        assert!(bar.is_none());
        assert_eq!(rest, area);
        let (bar, rest) = split_tab_bar(area, AccountsBar::Tabs);
        assert_eq!(bar.unwrap().height, 1);
        assert_eq!(rest.height, 9);
    }

    #[test]
    fn the_tab_line_numbers_accounts_and_marks_the_active_one() {
        let mut app = tabbed_app();
        app.scope = ViewScope::Account("b".into());
        let line = tab_line(&app);
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect();
        assert_eq!(text, " unified  1:a  2:b ");
    }
}
