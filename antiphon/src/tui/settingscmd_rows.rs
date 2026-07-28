use antiphon_config::{AccountsBar, Composer, ReadingPane};
use antiphon_ui::Theme;

use super::app::App;
use super::draw::{SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN};
use super::settings::wrapped;

const LIST_ROWS_MIN: u16 = 1;
const LIST_ROWS_MAX: u16 = 60;
const INTERVAL_MINUTES_MIN: u32 = 1;
const INTERVAL_MINUTES_MAX: u32 = 1440;
const STEP: u32 = 1;

const ACCOUNTS_BARS: [(AccountsBar, &str); 2] = [
    (AccountsBar::Sidebar, "sidebar"),
    (AccountsBar::Tabs, "tabs"),
];

const READING_PANES: [(ReadingPane, &str); 3] = [
    (ReadingPane::Below, "below"),
    (ReadingPane::Right, "right"),
    (ReadingPane::Off, "off"),
];

const COMPOSERS: [(Composer, &str); 2] = [
    (Composer::Embedded, "embedded"),
    (Composer::Suspend, "suspend"),
];

pub(super) fn render_theme(app: &App) -> String {
    app.theme.name.clone()
}

pub(super) fn cycle_theme(app: &mut App, step: i32) -> String {
    let names: Vec<&str> = Theme::names().collect();
    let current = names.iter().position(|name| *name == app.theme.name);
    let next = wrapped(current.unwrap_or(0), names.len(), step);
    let name = names[next];
    app.theme = Theme::by_name(name).unwrap_or_else(Theme::vespers);
    format!("\"{name}\"")
}

pub(super) fn render_accounts_bar(app: &App) -> String {
    accounts_bar_name(app.accounts_bar).to_string()
}

pub(super) fn accounts_bar_index(app: &App) -> usize {
    ACCOUNTS_BARS
        .iter()
        .position(|(bar, _)| *bar == app.accounts_bar)
        .unwrap_or(0)
}

fn accounts_bar_name(bar: AccountsBar) -> &'static str {
    ACCOUNTS_BARS
        .iter()
        .find(|(candidate, _)| *candidate == bar)
        .map_or("sidebar", |(_, name)| name)
}

/// Toggling the mode live also rebuilds the sidebar, so the
/// tab bar and the trimmed folder list appear at once.
pub(super) fn cycle_accounts_bar(app: &mut App, step: i32) -> String {
    let current = ACCOUNTS_BARS
        .iter()
        .position(|(bar, _)| *bar == app.accounts_bar)
        .unwrap_or(0);
    let next = wrapped(current, ACCOUNTS_BARS.len(), step);
    let (bar, name) = ACCOUNTS_BARS[next];
    app.accounts_bar = bar;
    app.rebuild_sidebar();
    format!("\"{name}\"")
}

pub(super) fn render_reading_pane(app: &App) -> String {
    reading_pane_name(app.reading_pane).to_string()
}

pub(super) fn reading_pane_index(app: &App) -> usize {
    READING_PANES
        .iter()
        .position(|(pane, _)| *pane == app.reading_pane)
        .unwrap_or(0)
}

fn reading_pane_name(pane: ReadingPane) -> &'static str {
    READING_PANES
        .iter()
        .find(|(candidate, _)| *candidate == pane)
        .map_or("below", |(_, name)| name)
}

pub(super) fn cycle_reading_pane(app: &mut App, step: i32) -> String {
    let current = READING_PANES
        .iter()
        .position(|(pane, _)| *pane == app.reading_pane)
        .unwrap_or(0);
    let next = wrapped(current, READING_PANES.len(), step);
    let (pane, name) = READING_PANES[next];
    app.reading_pane = pane;
    format!("\"{name}\"")
}

pub(super) fn render_composer(app: &App) -> String {
    composer_name(app.composer).to_string()
}

fn composer_name(composer: Composer) -> &'static str {
    COMPOSERS
        .iter()
        .find(|(candidate, _)| *candidate == composer)
        .map_or("embedded", |(_, name)| name)
}

pub(super) fn composer_index(app: &App) -> usize {
    COMPOSERS
        .iter()
        .position(|(composer, _)| *composer == app.composer)
        .unwrap_or(0)
}

pub(super) fn cycle_composer(app: &mut App, step: i32) -> String {
    let current = composer_index(app);
    let next = wrapped(current, COMPOSERS.len(), step);
    let (composer, name) = COMPOSERS[next];
    app.composer = composer;
    format!("\"{name}\"")
}

pub(super) fn render_idle(app: &App) -> String {
    on_off(app.sync_idle).to_string()
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

pub(super) fn cycle_idle(app: &mut App, _step: i32) -> String {
    app.sync_idle = !app.sync_idle;
    app.sync_idle.to_string()
}

pub(super) fn render_interval_minutes(app: &App) -> String {
    app.sync_interval_minutes.to_string()
}

pub(super) fn cycle_interval_minutes(
    app: &mut App,
    step: i32,
) -> String {
    app.sync_interval_minutes = stepped(
        app.sync_interval_minutes,
        step,
        INTERVAL_MINUTES_MIN,
        INTERVAL_MINUTES_MAX,
    );
    app.sync_interval_minutes.to_string()
}

pub(super) fn render_list_rows(app: &App) -> String {
    app.list_rows.to_string()
}

pub(super) fn cycle_list_rows(app: &mut App, step: i32) -> String {
    let updated = stepped(
        u32::from(app.list_rows),
        step,
        u32::from(LIST_ROWS_MIN),
        u32::from(LIST_ROWS_MAX),
    );
    app.list_rows = updated as u16;
    app.list_rows.to_string()
}

pub(super) fn render_sidebar_width(app: &App) -> String {
    app.sidebar_width.to_string()
}

pub(super) fn cycle_sidebar_width(app: &mut App, step: i32) -> String {
    let updated = stepped(
        u32::from(app.sidebar_width),
        step,
        u32::from(SIDEBAR_WIDTH_MIN),
        u32::from(SIDEBAR_WIDTH_MAX),
    );
    app.sidebar_width = updated as u16;
    app.sidebar_width.to_string()
}

/// `value` moved by one `STEP` in the direction of `step`,
/// clamped to `[min, max]`.
fn stepped(value: u32, step: i32, min: u32, max: u32) -> u32 {
    let delta = i64::from(STEP) * i64::from(step.signum());
    let updated = i64::from(value) + delta;
    updated.clamp(i64::from(min), i64::from(max)) as u32
}

#[cfg(test)]
mod tests {
    use super::super::testkit::app_with_messages;
    use super::*;

    #[test]
    fn theme_cycles_forward_and_backward_with_wraparound() {
        let mut app = app_with_messages(1);
        let names: Vec<&str> = Theme::names().collect();
        let start = app.theme.name.clone();
        cycle_theme(&mut app, 1);
        assert_ne!(app.theme.name, start);
        cycle_theme(&mut app, -1);
        assert_eq!(app.theme.name, start);
        for _ in 0..names.len() {
            cycle_theme(&mut app, 1);
        }
        assert_eq!(app.theme.name, start, "a full lap returns home");
    }

    #[test]
    fn reading_pane_cycles_through_all_three() {
        let mut app = app_with_messages(1);
        app.reading_pane = ReadingPane::Below;
        cycle_reading_pane(&mut app, 1);
        assert_eq!(app.reading_pane, ReadingPane::Right);
        cycle_reading_pane(&mut app, 1);
        assert_eq!(app.reading_pane, ReadingPane::Off);
        cycle_reading_pane(&mut app, 1);
        assert_eq!(app.reading_pane, ReadingPane::Below);
        cycle_reading_pane(&mut app, -1);
        assert_eq!(app.reading_pane, ReadingPane::Off);
    }

    #[test]
    fn idle_toggles_either_direction() {
        let mut app = app_with_messages(1);
        app.sync_idle = false;
        assert_eq!(cycle_idle(&mut app, 1), "true");
        assert_eq!(cycle_idle(&mut app, -1), "false");
    }

    #[test]
    fn numeric_rows_clamp_at_their_bounds() {
        let mut app = app_with_messages(1);
        app.sync_interval_minutes = INTERVAL_MINUTES_MIN;
        cycle_interval_minutes(&mut app, -1);
        assert_eq!(app.sync_interval_minutes, INTERVAL_MINUTES_MIN);
        app.sync_interval_minutes = INTERVAL_MINUTES_MAX;
        cycle_interval_minutes(&mut app, 1);
        assert_eq!(app.sync_interval_minutes, INTERVAL_MINUTES_MAX);

        app.list_rows = LIST_ROWS_MAX;
        cycle_list_rows(&mut app, 1);
        assert_eq!(app.list_rows, LIST_ROWS_MAX);
        app.list_rows = LIST_ROWS_MIN;
        cycle_list_rows(&mut app, -1);
        assert_eq!(app.list_rows, LIST_ROWS_MIN);

        app.sidebar_width = SIDEBAR_WIDTH_MAX;
        cycle_sidebar_width(&mut app, 1);
        assert_eq!(app.sidebar_width, SIDEBAR_WIDTH_MAX);
        app.sidebar_width = SIDEBAR_WIDTH_MIN;
        cycle_sidebar_width(&mut app, -1);
        assert_eq!(app.sidebar_width, SIDEBAR_WIDTH_MIN);
    }
}
