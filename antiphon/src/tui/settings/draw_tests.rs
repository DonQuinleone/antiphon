use ratatui::Terminal;
use ratatui::backend::TestBackend;

use antiphon_config::ReadingPane;

use crate::tui::settings::draw::*;
use crate::tui::settings::{AccountSummary, ServerKind};
use crate::tui::testkit::app_with_messages;

fn rendered(app: &App) -> ratatui::buffer::Buffer {
    // Wide enough for a full account row with its OAuth state
    // column, and tall enough for the settings modal to frame
    // its whole body.
    let backend = TestBackend::new(100, 30);
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
            kind: ServerKind::Imap,
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
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(text.contains("Accounts"));
    assert!(
        text.contains(" 1 work"),
        "the order position leads the row: {text:?}"
    );
    assert!(text.contains("quin@example.com"));
    assert!(text.contains("imap.example.com"));
}

#[test]
fn an_oauth_account_row_wears_its_state_and_detail() {
    use crate::tui::oauth_status::{OauthInfo, OauthState};

    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: vec![AccountSummary {
            name: "work".to_string(),
            account_name: "work".to_string(),
            address: "quin@example.com".to_string(),
            host: "imap.example.com".to_string(),
            kind: ServerKind::Imap,
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
fn oauth_rows_name_the_provider_not_the_host() {
    use crate::tui::oauth_status::{OauthInfo, OauthState};

    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: vec![
            AccountSummary {
                name: "ms".to_string(),
                account_name: "ms".to_string(),
                address: "josh@example.org".to_string(),
                host: "outlook.office365.com".to_string(),
                kind: ServerKind::Microsoft,
                oauth: Some(OauthInfo {
                    state: OauthState::Ok { minutes_left: 5 },
                    app_only: false,
                    detail: String::new(),
                }),
            },
            AccountSummary {
                name: "gg".to_string(),
                account_name: "gg".to_string(),
                address: "josh@example.com".to_string(),
                host: "imap.gmail.com".to_string(),
                kind: ServerKind::Google,
                oauth: Some(OauthInfo {
                    state: OauthState::Ok { minutes_left: 5 },
                    app_only: false,
                    detail: String::new(),
                }),
            },
        ],
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(
        text.contains("MS365"),
        "microsoft rows read MS365: {text}"
    );
    assert!(text.contains("Google"), "google rows read Google: {text}");
    assert!(
        !text.contains("office365") && !text.contains("gmail"),
        "the provider host never abuts the address: {text}"
    );
}

#[test]
fn a_needs_sign_in_row_wears_the_warning_colour() {
    use crate::tui::oauth_status::{OauthInfo, OauthState};

    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Accounts,
        accounts: vec![AccountSummary {
            name: "ms".to_string(),
            account_name: "ms".to_string(),
            address: "josh@example.org".to_string(),
            host: "outlook.office365.com".to_string(),
            kind: ServerKind::Microsoft,
            oauth: Some(OauthInfo {
                state: OauthState::NeedsSignIn,
                app_only: false,
                detail: String::new(),
            }),
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
    let warned = (0..buffer.area.height).any(|y| {
        (0..buffer.area.width).any(|x| {
            buffer.cell((x, y)).unwrap().fg == app.theme.status_error
        })
    });
    assert!(
        warned,
        "the needs-sign-in state stands out in the warning colour"
    );
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
            kind: ServerKind::Imap,
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
    assert!(text.contains("composer"));
    assert!(text.contains("notify sound"));
    assert!(text.contains("notify speech"));
    // A segmented row draws every option inline, not just the
    // current one.
    assert!(text.contains("embedded") && text.contains("suspend"));
    assert!(
        text.contains("below")
            && text.contains("right")
            && text.contains("off"),
        "the reading pane row shows all its options: {text:?}"
    );
    assert!(text.contains("takes effect when antiphond restarts"));
}

#[test]
fn a_selected_segment_carries_the_accent_highlight() {
    let mut app = app_with_messages(1);
    app.reading_pane = ReadingPane::Right;
    // Land the selection on the reading pane row so its active
    // segment is drawn.
    let reading = crate::tui::settings::cmd::ESSENTIAL_ROWS
        .iter()
        .position(|row| row.label == "reading pane")
        .expect("a reading pane row");
    app.settings = Some(SettingsState {
        tab: SettingsTab::Essentials,
        accounts: Vec::new(),
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: reading,
        daemon_hint: None,
        folders: Vec::new(),
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    let accent = app.theme.accent;
    let highlighted = (0..buffer.area.height).any(|y| {
        (0..buffer.area.width).any(|x| {
            let cell = buffer.cell((x, y)).unwrap();
            cell.symbol() == "r" && cell.bg == accent
        })
    });
    assert!(
        highlighted,
        "the active reading pane segment wears the accent background"
    );
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
                unsynced: false,
            },
            FolderRow {
                account: "work".to_string(),
                folder: "spam".to_string(),
                alias: String::new(),
                hidden: true,
                unsynced: false,
            },
            FolderRow {
                account: "work".to_string(),
                folder: "archive".to_string(),
                alias: String::new(),
                hidden: false,
                unsynced: true,
            },
        ],
        folder_selected: 0,
    });
    app.folder_alias_edit = Some(crate::tui::folder_alias::AliasEdit {
        account: "personal".to_string(),
        folder: "lists/rust".to_string(),
        text: "renamed".to_string(),
        cursor: 7,
    });
    let backend = TestBackend::new(90, 30);
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
        text.contains("spam") && text.contains("hidden"),
        "a hidden folder wears its state marker: {text}"
    );
    assert!(
        text.contains("archive") && text.contains("unsynced"),
        "an unsynced folder wears its state marker: {text}"
    );
    assert!(
        text.contains("aerc-list") && text.contains("visible"),
        "a plain folder reads as visible: {text}"
    );
    assert!(
        text.contains("alias for personal/lists/rust"),
        "the modal names the folder being aliased"
    );
    assert!(text.contains("renamed"), "the edit lives in the modal");
}

#[test]
fn folders_tab_footer_lists_its_keys() {
    let mut app = app_with_messages(1);
    app.settings = Some(SettingsState {
        tab: SettingsTab::Folders,
        accounts: Vec::new(),
        account_selected: 0,
        pending_delete: None,
        pending_revoke: None,
        essentials_selected: 0,
        daemon_hint: None,
        folders: vec![FolderRow {
            account: "work".to_string(),
            folder: "spam".to_string(),
            alias: String::new(),
            hidden: false,
            unsynced: false,
        }],
        folder_selected: 0,
    });
    let buffer = rendered(&app);
    let text: String =
        (0..buffer.area.height).map(|y| row(&buffer, y)).collect();
    assert!(
        text.contains("reorder")
            && text.contains("h hide")
            && text.contains("u unsync")
            && text.contains("enter alias"),
        "the footer lists the folder keys: {text}"
    );
}
