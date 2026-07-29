use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::configedit::persist_key;
use super::settings::{SettingsOutcome, wrapped};
use super::settingscmd_rows::*;

const IDLE_OPTIONS: [&str; 2] = ["off", "on"];
const ACCOUNTS_BAR_OPTIONS: [&str; 2] = ["sidebar", "tabs"];
const READING_PANE_OPTIONS: [&str; 3] = ["below", "right", "off"];
const COMPOSER_OPTIONS: [&str; 2] = ["embedded", "suspend"];

/// One essentials row: the label shown, the `[table] key`
/// it's stored under, whether a change only takes effect
/// once the daemon restarts, how its current value renders,
/// how it moves under `h`/`l`, and, for a small-set field,
/// the segmented options and which one is currently active.
pub(super) struct EssentialRow {
    pub(super) label: &'static str,
    pub(super) table: &'static str,
    pub(super) key: &'static str,
    pub(super) daemon: bool,
    pub(super) render: fn(&App) -> String,
    pub(super) cycle: fn(&mut App, i32) -> String,
    /// `Some` draws the value as a segmented toggle; `None`
    /// falls back to the plain rendered string (theme, the
    /// numeric fields).
    pub(super) segments: Option<&'static [&'static str]>,
    pub(super) selected: fn(&App) -> usize,
}

pub(super) const ESSENTIAL_ROWS: [EssentialRow; 8] = [
    EssentialRow {
        label: "theme",
        table: "ui",
        key: "theme",
        daemon: false,
        render: render_theme,
        cycle: cycle_theme,
        segments: None,
        selected: |_| 0,
    },
    EssentialRow {
        label: "sync interval (minutes)",
        table: "sync",
        key: "interval_minutes",
        daemon: true,
        render: render_interval_minutes,
        cycle: cycle_interval_minutes,
        segments: None,
        selected: |_| 0,
    },
    EssentialRow {
        label: "idle",
        table: "sync",
        key: "idle",
        daemon: true,
        render: render_idle,
        cycle: cycle_idle,
        segments: Some(&IDLE_OPTIONS),
        selected: |app| usize::from(app.sync_idle),
    },
    EssentialRow {
        label: "accounts bar",
        table: "ui",
        key: "accounts_bar",
        daemon: false,
        render: render_accounts_bar,
        cycle: cycle_accounts_bar,
        segments: Some(&ACCOUNTS_BAR_OPTIONS),
        selected: accounts_bar_index,
    },
    EssentialRow {
        label: "reading pane",
        table: "ui",
        key: "reading_pane",
        daemon: false,
        render: render_reading_pane,
        cycle: cycle_reading_pane,
        segments: Some(&READING_PANE_OPTIONS),
        selected: reading_pane_index,
    },
    EssentialRow {
        label: "composer",
        table: "ui",
        key: "composer",
        daemon: false,
        render: render_composer,
        cycle: cycle_composer,
        segments: Some(&COMPOSER_OPTIONS),
        selected: composer_index,
    },
    EssentialRow {
        label: "list rows",
        table: "ui",
        key: "list_rows",
        daemon: false,
        render: render_list_rows,
        cycle: cycle_list_rows,
        segments: None,
        selected: |_| 0,
    },
    EssentialRow {
        label: "sidebar width",
        table: "ui",
        key: "sidebar_width",
        daemon: false,
        render: render_sidebar_width,
        cycle: cycle_sidebar_width,
        segments: None,
        selected: |_| 0,
    },
];

/// Keys on the Essentials tab: j/k select a row, h/l (or
/// enter) cycle its value and persist it.
pub(super) fn feed(app: &mut App, key: KeyEvent) -> SettingsOutcome {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            move_essentials_selection(app, 1)
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_essentials_selection(app, -1)
        }
        KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
            cycle_essential(app, 1)
        }
        KeyCode::Char('h') | KeyCode::Left => cycle_essential(app, -1),
        _ => {}
    }
    SettingsOutcome::Stay
}

fn move_essentials_selection(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    let len = ESSENTIAL_ROWS.len();
    state.essentials_selected =
        wrapped(state.essentials_selected, len, step);
}

fn cycle_essential(app: &mut App, step: i32) {
    let Some(index) =
        app.settings.as_ref().map(|state| state.essentials_selected)
    else {
        return;
    };
    let row = &ESSENTIAL_ROWS[index];
    let value = (row.cycle)(app, step);
    let result = persist(app, row.table, row.key, &value);
    let base = format!("{}: {value}", row.label);
    app.notice = Some(match result {
        Ok(()) => base,
        Err(error) => format!("{base} (not saved: {error})"),
    });
    if row.daemon {
        super::reload_in_background();
    }
    if let Some(state) = app.settings.as_mut() {
        state.daemon_hint = row
            .daemon
            .then(|| "applying to the running daemon".to_string());
    }
}

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

#[cfg(test)]
mod tests {
    use super::super::settings::SettingsTab;
    use super::super::testkit::{app_with_messages, app_with_settings};
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(
            code,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )
    }

    #[test]
    fn essentials_selection_cycles_a_row_and_persists() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let mut app = app_with_settings(&[]);
        app.config_path = dir.path.join("config.toml");
        app.settings.as_mut().unwrap().tab = SettingsTab::Essentials;

        let target = ESSENTIAL_ROWS
            .iter()
            .position(|row| row.label == "list rows")
            .expect("a list rows row");
        let before = app.list_rows;
        for _ in 0..target {
            feed(&mut app, key(KeyCode::Char('j')));
        }
        feed(&mut app, key(KeyCode::Char('l')));
        assert_ne!(app.list_rows, before);
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(
            text.contains(&format!("list_rows = {}", app.list_rows))
        );
    }

    #[test]
    fn a_daemon_key_sets_the_restart_hint_a_client_key_does_not() {
        use super::super::testkit::TempDir;

        let dir = TempDir::new();
        let mut app = app_with_settings(&[]);
        app.config_path = dir.path.join("config.toml");
        app.settings.as_mut().unwrap().tab = SettingsTab::Essentials;

        feed(&mut app, key(KeyCode::Char('l')));
        assert!(app.settings.as_ref().unwrap().daemon_hint.is_none());

        for _ in 0..2 {
            feed(&mut app, key(KeyCode::Char('j')));
        }
        feed(&mut app, key(KeyCode::Char('l')));
        assert!(app.settings.as_ref().unwrap().daemon_hint.is_some());
    }

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
