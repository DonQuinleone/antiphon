//! The settings Accounts tab: selection, ordering, add/edit,
//! delete and the OAuth sign-in entry point.

use antiphon_config::{Dirs, NamedAccount, OauthProvider};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::App;
use super::settings::{
    AccountSummary, ServerKind, SettingsOutcome, wrapped,
};
use super::settingscmd;

pub(super) fn feed_accounts(
    app: &mut App,
    key: KeyEvent,
) -> SettingsOutcome {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            move_account_selection(app, 1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('k') | KeyCode::Up => {
            move_account_selection(app, -1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('J') => {
            shift_account_order(app, 1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('K') => {
            shift_account_order(app, -1);
            SettingsOutcome::Stay
        }
        KeyCode::Char('a') => {
            app.open_account_form_add();
            SettingsOutcome::Stay
        }
        KeyCode::Char('e') => {
            if let Some(name) = selected_account_name(app) {
                app.open_account_form_edit(&name);
            }
            SettingsOutcome::Stay
        }
        KeyCode::Char('d') => {
            arm_delete(app);
            SettingsOutcome::Stay
        }
        KeyCode::Char('o') => {
            if let Some(name) = selected_account_name(app) {
                super::oauthflow::authorise(app, &name);
            }
            SettingsOutcome::Stay
        }
        KeyCode::Char('x') => {
            super::oauth_status::arm_revoke(app);
            SettingsOutcome::Stay
        }
        _ => SettingsOutcome::Stay,
    }
}

pub(super) fn feed_confirm_delete(
    app: &mut App,
    key: KeyEvent,
    name: &str,
) -> SettingsOutcome {
    if matches!(key.code, KeyCode::Char('y' | 'Y')) {
        remove_account(app, name);
    } else if let Some(state) = app.settings.as_mut() {
        state.pending_delete = None;
    }
    SettingsOutcome::Stay
}

fn move_account_selection(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    if state.accounts.is_empty() {
        return;
    }
    state.account_selected =
        wrapped(state.account_selected, state.accounts.len(), step);
}

const ACCOUNTS_TABLE: &str = "accounts";
const ORDER_KEY: &str = "order";

/// Shift+J/K move the selected account through the list and
/// persist the whole order to `[accounts] order`, so the new
/// order survives a restart and the first account becomes the
/// primary. The running session follows at once.
fn shift_account_order(app: &mut App, step: i32) {
    let Some(state) = app.settings.as_mut() else {
        return;
    };
    let from = state.account_selected;
    let last = state.accounts.len().saturating_sub(1) as i32;
    let to = (from as i32 + step).clamp(0, last) as usize;
    if state.accounts.is_empty() || to == from {
        return;
    }
    state.accounts.swap(from, to);
    state.account_selected = to;
    let order: Vec<String> = state
        .accounts
        .iter()
        .map(|account| account.name.clone())
        .collect();
    let value = super::configedit::toml_string_array(&order);
    let result =
        settingscmd::persist(app, ACCOUNTS_TABLE, ORDER_KEY, &value);
    reorder_live_accounts(app, &order);
    app.notice = Some(match result {
        Ok(()) => format!("account order: {}", order.join(", ")),
        Err(error) => format!("account order not saved: {error}"),
    });
}

/// Mirrors the persisted order onto the running session's
/// account list and sidebar, which every scope cycle, unified
/// query and refresh reads.
fn reorder_live_accounts(app: &mut App, order: &[String]) {
    let rank = |name: &str| {
        order
            .iter()
            .position(|entry| entry == name)
            .unwrap_or(order.len())
    };
    app.accounts.sort_by_key(|name| rank(name));
    app.account_entries.sort_by_key(|entry| rank(&entry.name));
    app.rebuild_sidebar();
}

pub(super) fn selected_account_name(app: &App) -> Option<String> {
    let state = app.settings.as_ref()?;
    state
        .accounts
        .get(state.account_selected)
        .map(|account| account.name.clone())
}

fn arm_delete(app: &mut App) {
    let Some(name) = selected_account_name(app) else {
        return;
    };
    if let Some(state) = app.settings.as_mut() {
        state.pending_delete = Some(name);
    }
}

/// Removes the account's config file only: the mail store
/// under its maildir is left exactly as it was.
fn remove_account(app: &mut App, name: &str) {
    let path = app
        .dirs
        .config
        .join("accounts")
        .join(format!("{name}.toml"));
    app.notice = Some(match std::fs::remove_file(&path) {
        Ok(()) => match super::request_reload() {
            None => format!("removed account {name}"),
            Some(notice) => {
                format!("removed account {name} ({notice})")
            }
        },
        Err(error) => format!("remove {name}: {error}"),
    });
    if let Some(state) = app.settings.as_mut() {
        state.pending_delete = None;
    }
    app.refresh_settings_accounts();
}

pub(super) fn account_summaries(
    dirs: &Dirs,
    auth_failures: &[String],
) -> Vec<AccountSummary> {
    let Ok(loaded) = antiphon_config::load(dirs) else {
        return Vec::new();
    };
    let store = super::oauth_status::open_store_if_present(dirs);
    let now = now_unix();
    loaded
        .accounts
        .iter()
        .map(|entry| {
            summary_of(entry, store.as_ref(), auth_failures, now)
        })
        .collect()
}

fn summary_of(
    entry: &NamedAccount,
    store: Option<&antiphon_oauth::TokenStore>,
    auth_failures: &[String],
    now: u64,
) -> AccountSummary {
    AccountSummary {
        name: entry.file_stem.clone(),
        account_name: entry.account.account.name.clone(),
        address: entry
            .account
            .identities
            .first()
            .map(|identity| identity.address.clone())
            .unwrap_or_default(),
        host: entry.account.imap.host.clone(),
        kind: server_kind(entry),
        oauth: super::oauth_status::info_for(
            entry,
            store,
            auth_failures,
            now,
        ),
    }
}

fn server_kind(entry: &NamedAccount) -> ServerKind {
    match entry.account.oauth.as_ref().map(|oauth| oauth.provider) {
        Some(OauthProvider::Microsoft) => ServerKind::Microsoft,
        Some(OauthProvider::Google) => ServerKind::Google,
        None => ServerKind::Imap,
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::super::settings::{SettingsOutcome, feed};
    use super::super::testkit::{TempDir, app_with_settings};
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(
            code,
            ratatui::crossterm::event::KeyModifiers::NONE,
        )
    }

    #[test]
    fn summaries_carry_the_provider_kind_and_the_imap_host() {
        let dir = TempDir::new();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("gg.toml"),
            "[account]\nname = \"gg\"\n\n\
             [imap]\nhost = \"imap.gmail.com\"\nuser = \"u\"\n\n\
             [oauth]\nprovider = \"google\"\n\n\
             [[identity]]\naddress = \"u@example.com\"\n",
        )
        .unwrap();
        std::fs::write(
            accounts_dir.join("plain.toml"),
            "[account]\nname = \"plain\"\n\n\
             [imap]\nhost = \"imap.example.com\"\nuser = \"u\"\n",
        )
        .unwrap();

        let dirs = Dirs {
            config: dir.path.clone(),
            state: dir.path.join("state"),
            cache: dir.path.join("cache"),
            data: dir.path.join("data"),
        };
        let summaries = account_summaries(&dirs, &[]);
        let google = summaries
            .iter()
            .find(|summary| summary.name == "gg")
            .expect("the google account");
        assert_eq!(google.kind, ServerKind::Google);
        assert_eq!(google.server_label(), "Google");
        let plain = summaries
            .iter()
            .find(|summary| summary.name == "plain")
            .expect("the imap account");
        assert_eq!(plain.kind, ServerKind::Imap);
        assert_eq!(plain.server_label(), "imap.example.com");
    }

    #[test]
    fn accounts_selection_moves_and_wraps() {
        let mut app = app_with_settings(&["a", "b", "c"]);
        feed(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 1);
        feed(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 0);
        feed(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.settings.as_ref().unwrap().account_selected, 2);
    }

    #[test]
    fn shift_j_and_k_reorder_accounts_and_persist_the_order() {
        let dir = TempDir::new();
        let mut app = app_with_settings(&["a", "b", "c"]);
        app.accounts = vec!["a".into(), "b".into(), "c".into()];
        app.config_path = dir.path.join("config.toml");

        feed(&mut app, key(KeyCode::Char('J')));
        let state = app.settings.as_ref().unwrap();
        let names: Vec<&str> = state
            .accounts
            .iter()
            .map(|account| account.name.as_str())
            .collect();
        assert_eq!(names, ["b", "a", "c"]);
        assert_eq!(state.account_selected, 1);
        assert_eq!(app.accounts, ["b", "a", "c"]);
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("[accounts]"), "{text}");
        assert!(
            text.contains("order = [\"b\", \"a\", \"c\"]"),
            "{text}"
        );

        app.settings.as_mut().unwrap().account_selected = 0;
        feed(&mut app, key(KeyCode::Char('K')));
        assert_eq!(
            app.settings.as_ref().unwrap().account_selected,
            0,
            "moving up from the top clamps"
        );
        assert_eq!(app.accounts, ["b", "a", "c"]);
    }

    #[test]
    fn a_opens_a_blank_form_and_e_opens_it_prefilled() {
        let dir = TempDir::new();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("work.toml"),
            "[account]\nname = \"work\"\n\n\
             [imap]\nhost = \"imap.example.com\"\nuser = \"u\"\n\n\
             [[identity]]\naddress = \"u@example.com\"\n",
        )
        .unwrap();

        let mut app = app_with_settings(&["work"]);
        app.dirs.config = dir.path.clone();

        assert_eq!(
            feed(&mut app, key(KeyCode::Char('a'))),
            SettingsOutcome::Stay
        );
        assert!(app.account_form.is_some());
        app.account_form = None;

        feed(&mut app, key(KeyCode::Char('e')));
        let form =
            app.account_form.as_ref().expect("edit opens a form");
        assert_eq!(form.editing.as_deref(), Some("work"));
    }

    #[test]
    fn d_asks_for_confirmation_before_removing() {
        let mut app = app_with_settings(&["work"]);
        feed(&mut app, key(KeyCode::Char('d')));
        assert_eq!(
            app.settings.as_ref().unwrap().pending_delete.as_deref(),
            Some("work")
        );
        feed(&mut app, key(KeyCode::Char('n')));
        assert!(
            app.settings.as_ref().unwrap().pending_delete.is_none()
        );
        assert_eq!(
            app.settings.as_ref().unwrap().accounts.len(),
            1,
            "declining leaves the account alone"
        );
    }

    #[test]
    fn confirming_removal_deletes_only_the_config_file() {
        let dir = TempDir::new();
        let accounts_dir = dir.path.join("accounts");
        std::fs::create_dir_all(&accounts_dir).unwrap();
        std::fs::write(
            accounts_dir.join("work.toml"),
            "[account]\nname = \"work\"\n\n\
             [imap]\nhost = \"h\"\nuser = \"u\"\n",
        )
        .unwrap();
        let maildir = dir.path.join("maildir/work/cur/1.eml");
        std::fs::create_dir_all(maildir.parent().unwrap()).unwrap();
        std::fs::write(&maildir, "kept").unwrap();

        let mut app = app_with_settings(&["work"]);
        app.dirs.config = dir.path.clone();
        feed(&mut app, key(KeyCode::Char('d')));
        feed(&mut app, key(KeyCode::Char('y')));

        assert!(!accounts_dir.join("work.toml").exists());
        assert!(maildir.exists(), "the mail store is never touched");
        assert!(app.settings.as_ref().unwrap().accounts.is_empty());
    }
}
