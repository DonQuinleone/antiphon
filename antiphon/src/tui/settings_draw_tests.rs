use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::super::settings::AccountSummary;
use super::super::testkit::app_with_messages;
use super::*;

fn rendered(app: &App) -> ratatui::buffer::Buffer {
    // Wide enough for a full account row with its OAuth
    // state column.
    let backend = TestBackend::new(100, 12);
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
            account_name: "work".to_string(),
            address: "quin@example.com".to_string(),
            host: "imap.example.com".to_string(),
            oauth: None,
        }],
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    assert!(row(&buffer, 0).contains("Accounts"));
    let account_row = row(&buffer, 1);
    assert!(
        account_row.contains(" 1 work"),
        "the order position leads the row: {account_row:?}"
    );
    assert!(account_row.contains("quin@example.com"));
    assert!(account_row.contains("imap.example.com"));
}

#[test]
fn an_oauth_account_row_wears_its_state_and_detail() {
    use super::super::oauth_status::{OauthInfo, OauthState};

    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: vec![AccountSummary {
            name: "work".to_string(),
            account_name: "work".to_string(),
            address: "quin@example.com".to_string(),
            host: "imap.example.com".to_string(),
            oauth: Some(OauthInfo {
                state: OauthState::Ok { minutes_left: 42 },
                app_only: false,
                detail: "imap: scope \u{b7} valid".to_string(),
            }),
        }],
        account_selected: 0,
        pending_delete: None,
        pending_revoke: Some("work".to_string()),
        essentials_selected: 0,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(
        text.contains("oauth: ok (42 min)"),
        "the row carries the state: {text}"
    );
    assert!(
        text.contains("imap: scope"),
        "the detail line follows the selection: {text}"
    );
    assert!(text.contains("revoke the sign-in for work? y/n"));
}

#[test]
fn a_pending_delete_shows_the_confirmation() {
    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: vec![AccountSummary {
            name: "work".to_string(),
            account_name: "work".to_string(),
            address: String::new(),
            host: String::new(),
            oauth: None,
        }],
        account_selected: 0,
        pending_delete: Some("work".to_string()),
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
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
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: Some(
            "takes effect when antiphond restarts".to_string(),
        ),
        folders: Vec::new(),
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(text.contains("theme"));
    assert!(text.contains("sync interval"));
    assert!(text.contains("sidebar width"));
    assert!(text.contains("takes effect when antiphond restarts"));
}

#[test]
fn folders_tab_lists_rows_and_the_selected_ones_edit_shows() {
    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Folders,
        accounts: Vec::new(),
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: vec![
            FolderRow {
                account: "work".to_string(),
                folder: "lists/aerc".to_string(),
                alias: "aerc-list".to_string(),
                hidden: false,
            },
            FolderRow {
                account: "work".to_string(),
                folder: "spam".to_string(),
                alias: String::new(),
                hidden: true,
            },
        ],
        folder_selected: 0,
    });
    app.folder_alias_edit =
        Some(super::super::folder_alias::AliasEdit {
            account: "personal".to_string(),
            folder: "lists/rust".to_string(),
            text: "renamed".to_string(),
            cursor: 7,
        });
    let backend = TestBackend::new(70, 12);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw_settings(frame, &app, frame.area());
            draw_alias_modal(frame, &app, frame.area());
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(text.contains("Folders"));
    assert!(
        text.contains("spam") && text.contains("(hidden)"),
        "hidden folders wear their marker: {text}"
    );
    assert!(
        text.contains("alias for personal/lists/rust"),
        "the modal names the folder being aliased"
    );
    assert!(text.contains("renamed"), "the edit lives in the modal");
}
