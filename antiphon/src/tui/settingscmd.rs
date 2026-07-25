use antiphon_config::ReadingPane;
use antiphon_ui::Theme;

use super::app::App;
use super::draw::{SIDEBAR_WIDTH_MAX, SIDEBAR_WIDTH_MIN};
use super::settings::wrapped;
use super::themecmd::persist_key;

const LIST_ROWS_MIN: u16 = 1;
const LIST_ROWS_MAX: u16 = 60;
const INTERVAL_MINUTES_MIN: u32 = 1;
const INTERVAL_MINUTES_MAX: u32 = 1440;
const STEP: u32 = 1;

const READING_PANES: [(ReadingPane, &str); 3] = [
    (ReadingPane::Below, "below"),
    (ReadingPane::Right, "right"),
    (ReadingPane::Off, "off"),
];

/// One essentials row: the label shown, the `[table] key`
/// it's stored under, whether a change only takes effect
/// once the daemon restarts, how its current value renders,
/// and how it moves under `h`/`l`.
pub(super) struct EssentialRow {
    pub(super) label: &'static str,
    pub(super) table: &'static str,
    pub(super) key: &'static str,
    pub(super) daemon: bool,
    pub(super) render: fn(&App) -> String,
    pub(super) cycle: fn(&mut App, i32) -> String,
}

pub(super) const ESSENTIAL_ROWS: [EssentialRow; 6] = [
    EssentialRow {
        label: "theme",
        table: "ui",
        key: "theme",
        daemon: false,
        render: render_theme,
        cycle: cycle_theme,
    },
    EssentialRow {
        label: "sync interval (minutes)",
        table: "sync",
        key: "interval_minutes",
        daemon: true,
        render: render_interval_minutes,
        cycle: cycle_interval_minutes,
    },
    EssentialRow {
        label: "idle",
        table: "sync",
        key: "idle",
        daemon: true,
        render: render_idle,
        cycle: cycle_idle,
    },
    EssentialRow {
        label: "reading pane",
        table: "ui",
        key: "reading_pane",
        daemon: false,
        render: render_reading_pane,
        cycle: cycle_reading_pane,
    },
    EssentialRow {
        label: "list rows",
        table: "ui",
        key: "list_rows",
        daemon: false,
        render: render_list_rows,
        cycle: cycle_list_rows,
    },
    EssentialRow {
        label: "sidebar width",
        table: "ui",
        key: "sidebar_width",
        daemon: false,
        render: render_sidebar_width,
        cycle: cycle_sidebar_width,
    },
];

/// Writes one essentials key through the same surgical config
/// edit `:theme` uses, so every other line survives untouched.
pub(super) fn persist(
    app: &App,
    table: &str,
    key: &str,
    value: &str,
) -> std::io::Result<()> {
    persist_key(&app.config_path, table, key, value)
}

fn render_theme(app: &App) -> String {
    app.theme.name.clone()
}

fn cycle_theme(app: &mut App, step: i32) -> String {
    let names: Vec<&str> = Theme::names().collect();
    let current = names.iter().position(|name| *name == app.theme.name);
    let next = wrapped(current.unwrap_or(0), names.len(), step);
    let name = names[next];
    app.theme = Theme::by_name(name).unwrap_or_else(Theme::vespers);
    format!("\"{name}\"")
}

fn render_reading_pane(app: &App) -> String {
    reading_pane_name(app.reading_pane).to_string()
}

fn reading_pane_name(pane: ReadingPane) -> &'static str {
    READING_PANES
        .iter()
        .find(|(candidate, _)| *candidate == pane)
        .map_or("below", |(_, name)| name)
}

fn cycle_reading_pane(app: &mut App, step: i32) -> String {
    let current = READING_PANES
        .iter()
        .position(|(pane, _)| *pane == app.reading_pane)
        .unwrap_or(0);
    let next = wrapped(current, READING_PANES.len(), step);
    let (pane, name) = READING_PANES[next];
    app.reading_pane = pane;
    format!("\"{name}\"")
}

fn render_idle(app: &App) -> String {
    on_off(app.sync_idle).to_string()
}

fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn cycle_idle(app: &mut App, _step: i32) -> String {
    app.sync_idle = !app.sync_idle;
    app.sync_idle.to_string()
}

fn render_interval_minutes(app: &App) -> String {
    app.sync_interval_minutes.to_string()
}

fn cycle_interval_minutes(app: &mut App, step: i32) -> String {
    app.sync_interval_minutes = stepped(
        app.sync_interval_minutes,
        step,
        INTERVAL_MINUTES_MIN,
        INTERVAL_MINUTES_MAX,
    );
    app.sync_interval_minutes.to_string()
}

fn render_list_rows(app: &App) -> String {
    app.list_rows.to_string()
}

fn cycle_list_rows(app: &mut App, step: i32) -> String {
    let updated = stepped(
        u32::from(app.list_rows),
        step,
        u32::from(LIST_ROWS_MIN),
        u32::from(LIST_ROWS_MAX),
    );
    app.list_rows = updated as u16;
    app.list_rows.to_string()
}

fn render_sidebar_width(app: &App) -> String {
    app.sidebar_width.to_string()
}

fn cycle_sidebar_width(app: &mut App, step: i32) -> String {
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
    fn every_row_renders_and_cycles_without_panicking() {
        for row in &ESSENTIAL_ROWS {
            let mut app = app_with_messages(1);
            let before = (row.render)(&app);
            (row.cycle)(&mut app, 1);
            assert_ne!(
                before,
                (row.render)(&app),
                "{}: cycling should change something \
                 (idle only has two states, still flips)",
                row.label
            );
        }
    }

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

    #[test]
    fn persist_writes_through_the_generic_config_edit() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let mut app = app_with_messages(1);
        app.config_path = dir.path.join("config.toml");
        persist(&app, "sync", "interval_minutes", "9")
            .expect("persist a fresh file");
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("interval_minutes = 9"));
    }
}
