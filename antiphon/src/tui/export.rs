use std::path::{Path, PathBuf};

use antiphon_store::StoreLayout;

use super::app::App;
use super::commands::ExportCommand;
use crate::export::{
    ExportKey, archive_file_name, export_account, parse_recipients,
};

/// Consume a `:export <account> <path>` armed by the command
/// prompt. Encryption targets come from `[export] recipients`
/// in the global config; passphrase prompting has no place
/// inside the TUI.
pub(super) fn run_pending(app: &mut App, layout: &StoreLayout) {
    let Some(command) = app.pending_export.take() else {
        return;
    };
    app.notice = Some(run(app, layout, &command));
}

fn run(
    app: &App,
    layout: &StoreLayout,
    command: &ExportCommand,
) -> String {
    if !app.accounts.contains(&command.account) {
        return format!("no account named {}", command.account);
    }
    if app.export_recipients.is_empty() {
        return "set [export] recipients in config.toml first"
            .to_string();
    }
    let recipients = match parse_recipients(&app.export_recipients) {
        Ok(keys) => keys,
        Err(error) => return error.to_string(),
    };
    let key = ExportKey::Recipients(recipients);
    let destination = destination(&command.path, &command.account);
    let maildir = layout.account_maildir(&command.account);
    match export_account(&maildir, &command.account, &destination, &key)
    {
        Ok(summary) => summary.line(),
        Err(error) => error.to_string(),
    }
}

fn destination(path: &Path, account: &str) -> PathBuf {
    if path.is_dir() {
        return path.join(archive_file_name(account));
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::super::testkit::app_with_messages;
    use super::*;

    fn layout(dir: &tempfile::TempDir) -> StoreLayout {
        StoreLayout::new(dir.path().join("store"))
    }

    fn export_to(app: &mut App, layout: &StoreLayout, path: &Path) {
        app.run_command(&format!("export work {}", path.display()));
        run_pending(app, layout);
    }

    #[test]
    fn an_unknown_account_is_named_in_the_notice() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(&dir);
        let mut app = app_with_messages(1);
        app.run_command("export absent /tmp/out");
        run_pending(&mut app, &layout);
        assert_eq!(
            app.notice.as_deref(),
            Some("no account named absent")
        );
    }

    #[test]
    fn missing_recipients_point_at_the_config_key() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(&dir);
        let mut app = app_with_messages(1);
        app.accounts = vec!["work".to_string()];
        let out = dir.path().join("out.tar.gz.age");
        export_to(&mut app, &layout, &out);
        assert_eq!(
            app.notice.as_deref(),
            Some("set [export] recipients in config.toml first")
        );
    }

    #[test]
    fn a_configured_recipient_yields_an_archive() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout(&dir);
        let maildir = layout.account_maildir("work");
        std::fs::create_dir_all(maildir.join("cur")).unwrap();
        std::fs::write(maildir.join("cur/1.a"), "mail\n").unwrap();
        let identity = age::x25519::Identity::generate();
        let mut app = app_with_messages(1);
        app.accounts = vec!["work".to_string()];
        app.export_recipients = vec![identity.to_public().to_string()];
        let out = dir.path().join("out.tar.gz.age");
        export_to(&mut app, &layout, &out);
        let notice = app.notice.as_deref().unwrap();
        assert!(notice.starts_with("exported work"), "{notice}");
        assert!(out.is_file());
        assert!(app.pending_export.is_none(), "consumed once armed");
    }
}
