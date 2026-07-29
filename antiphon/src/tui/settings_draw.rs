use antiphon_ui::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::app::App;
use super::draw::segmented::{self, SegmentStyle};
use super::folders::FolderRow;
use super::headers::with_cursor;
use super::oauth_status::OauthState;
use super::settings::{SettingsState, SettingsTab};
use super::settingscmd::{self, EssentialRow};

const SELECTED_MARK: &str = "\u{25b8} ";
const UNSELECTED_MARK: &str = "  ";
const LABEL_WIDTH: usize = 26;
const STATE_WIDTH: usize = 10;
const COL_GAP: usize = 2;
const MIN_NAME_WIDTH: usize = 6;
const MIN_ADDRESS_WIDTH: usize = 14;
const FOLDERS_HELP: &str = "Shift-J/K reorder \u{b7} h hide \u{b7} \
     u unsync \u{b7} enter alias";

const SETTINGS_MODAL_WIDTH: u16 = 78;
const SETTINGS_MODAL_HEIGHT: u16 = 26;

pub(super) fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let Some(state) = &app.settings else {
        return;
    };
    let theme = app.theme;
    let modal = settings_modal(area);
    frame.render_widget(ratatui::widgets::Clear, modal);
    let block = ratatui::widgets::Block::bordered()
        .title(" settings ")
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    let [tabs_area, body_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
            .areas(inner);
    frame.render_widget(
        Paragraph::new(tabs_line(theme, state.tab)),
        tabs_area,
    );
    match state.tab {
        SettingsTab::Accounts => {
            draw_accounts(frame, theme, state, body_area)
        }
        SettingsTab::Essentials => {
            draw_essentials(frame, app, state, body_area)
        }
        SettingsTab::Folders => {
            draw_folders(frame, app, state, body_area)
        }
    }
}

/// Settings float as a centred modal over the app rather than a
/// full-bleed page, so a border frames the whole panel.
fn settings_modal(area: Rect) -> Rect {
    let width = SETTINGS_MODAL_WIDTH.min(area.width.saturating_sub(4));
    let height =
        SETTINGS_MODAL_HEIGHT.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

fn tabs_line(theme: &Theme, active: SettingsTab) -> Line<'static> {
    let tabs = [
        (SettingsTab::Accounts, "Accounts"),
        (SettingsTab::Essentials, "Essentials"),
        (SettingsTab::Folders, "Folders"),
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
    let name_w = col_width(
        state.accounts.iter().map(|a| a.name.chars().count()),
        MIN_NAME_WIDTH,
    );
    let address_w = col_width(
        state.accounts.iter().map(|a| a.address.chars().count()),
        MIN_ADDRESS_WIDTH,
    );
    for (index, account) in state.accounts.iter().enumerate() {
        lines.push(account_line(
            theme,
            account,
            index,
            index == state.account_selected,
            name_w,
            address_w,
        ));
    }
    push_oauth_detail(&mut lines, theme, state);
    if let Some(name) = &state.pending_delete {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("delete {name}? y/n"),
            Style::new().fg(theme.accent_strong),
        )));
    }
    if let Some(name) = &state.pending_revoke {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("revoke the sign-in for {name}? y/n"),
            Style::new().fg(theme.accent_strong),
        )));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

/// The selected account's grant scopes and expiries, under
/// the list, so the row itself stays one line.
fn push_oauth_detail(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    state: &SettingsState,
) {
    let detail = state
        .accounts
        .get(state.account_selected)
        .and_then(|account| account.oauth.as_ref())
        .map(|info| info.detail.clone())
        .unwrap_or_default();
    if detail.is_empty() {
        return;
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        detail,
        Style::new().fg(theme.text_muted),
    )));
}

fn account_line(
    theme: &Theme,
    account: &super::settings::AccountSummary,
    index: usize,
    selected: bool,
    name_w: usize,
    address_w: usize,
) -> Line<'static> {
    let marker = mark(selected);
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let position = index + 1;
    let mut spans = vec![Span::styled(
        format!(
            "{marker}{position:>2} {:<name_w$}{:<address_w$} {}",
            account.name,
            account.address,
            account.server_label(),
        ),
        style,
    )];
    if let Some(info) = &account.oauth {
        spans.push(Span::styled(
            format!("  {}", info.label()),
            oauth_style(theme, info, style),
        ));
    }
    Line::from(spans)
}

/// The needs-sign-in state is the one the user must act on, so
/// it wears the warning colour; every other state keeps the
/// row's own styling.
fn oauth_style(
    theme: &Theme,
    info: &super::oauth_status::OauthInfo,
    row_style: Style,
) -> Style {
    if matches!(info.state, OauthState::NeedsSignIn) {
        return row_style.fg(theme.status_error);
    }
    row_style
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
        lines.push(essential_line(app, row, selected));
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

/// A small-set row draws its value as a segmented toggle; the
/// rest keep the plain rendered string. Either way the marker
/// and label carry the row's selection highlight.
fn essential_line(
    app: &App,
    row: &EssentialRow,
    selected: bool,
) -> Line<'static> {
    let theme = app.theme;
    let mut label_style = Style::new().fg(theme.text_primary);
    if selected {
        label_style =
            label_style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    let label =
        format!("{}{:<LABEL_WIDTH$}", mark(selected), row.label);
    let Some(options) = row.segments else {
        let value = (row.render)(app);
        return Line::from(Span::styled(
            format!("{label}{value}"),
            label_style,
        ));
    };
    let mut spans = vec![Span::styled(label, label_style)];
    spans.extend(segmented::segments(
        options,
        (row.selected)(app),
        SegmentStyle {
            selected_bg: theme.accent,
            selected_fg: theme.background,
            unselected_fg: theme.text_muted,
        },
    ));
    Line::from(spans)
}

fn draw_folders(
    frame: &mut Frame,
    app: &App,
    state: &SettingsState,
    area: Rect,
) {
    let theme = app.theme;
    let mut lines = Vec::new();
    if state.folders.is_empty() {
        lines.push(Line::from(Span::styled(
            "no folders discovered yet",
            Style::new().fg(theme.text_muted),
        )));
    }
    let account_w = col_width(
        state.folders.iter().map(|f| f.account.chars().count()),
        MIN_NAME_WIDTH,
    );
    let folder_w = col_width(
        state.folders.iter().map(|f| f.folder.chars().count()),
        MIN_ADDRESS_WIDTH,
    );
    for (index, row) in state.folders.iter().enumerate() {
        lines.push(folder_line(
            app,
            row,
            index == state.folder_selected,
            account_w,
            folder_w,
        ));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        FOLDERS_HELP,
        Style::new().fg(theme.text_muted),
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

fn folder_line(
    app: &App,
    row: &FolderRow,
    selected: bool,
    account_w: usize,
    folder_w: usize,
) -> Line<'static> {
    let theme = app.theme;
    let marker = mark(selected);
    let mut style = Style::new().fg(theme.text_primary);
    if selected {
        style = style.bg(theme.selection_bg).fg(theme.selection_fg);
    }
    Line::from(Span::styled(
        format!(
            "{marker}{:<account_w$}{:<folder_w$}{:<STATE_WIDTH$}{}",
            row.account,
            row.folder,
            state_label(row),
            row.alias,
        ),
        style,
    ))
}

/// The row's sync state: unsynced dominates, since an unsynced
/// folder is never downloaded and so a redundant hidden flag on
/// it is moot; a hidden folder is still synced, just off the
/// sidebar.
fn state_label(row: &FolderRow) -> &'static str {
    match (row.unsynced, row.hidden) {
        (true, _) => "unsynced",
        (false, true) => "hidden",
        (false, false) => "visible",
    }
}

const ALIAS_MODAL_WIDTH: u16 = 56;
const ALIAS_HINT: &str =
    " enter saves \u{b7} empty removes \u{b7} esc cancels ";

/// The alias edits in a small modal of its own, named after
/// the folder, so the mode is unmistakable and the keys sit
/// right under the text being typed.
pub(super) fn draw_alias_modal(
    frame: &mut Frame,
    app: &App,
    area: Rect,
) {
    let Some(edit) = &app.folder_alias_edit else {
        return;
    };
    let theme = app.theme;
    let width = ALIAS_MODAL_WIDTH.min(area.width.saturating_sub(2));
    let height = 3u16.min(area.height);
    let modal = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };
    frame.render_widget(ratatui::widgets::Clear, modal);
    let title = format!(
        " alias for {}/{} ",
        edit.account,
        match edit.folder.is_empty() {
            true => "inbox",
            false => edit.folder.as_str(),
        }
    );
    let block = ratatui::widgets::Block::bordered()
        .title(title)
        .title_bottom(ALIAS_HINT)
        .border_style(Style::new().fg(theme.accent))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(modal);
    frame.render_widget(block, modal);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            with_cursor(&edit.text, edit.cursor),
            Style::new().fg(theme.text_primary),
        ))),
        inner,
    );
}

fn mark(selected: bool) -> &'static str {
    if selected {
        SELECTED_MARK
    } else {
        UNSELECTED_MARK
    }
}

/// A column sized to the widest value it holds plus a gap,
/// never below a floor so short content still reads as a
/// column.
fn col_width(
    values: impl Iterator<Item = usize>,
    minimum: usize,
) -> usize {
    values.max().unwrap_or(0).max(minimum) + COL_GAP
}

#[cfg(test)]
#[path = "settings_draw_tests.rs"]
mod tests;
