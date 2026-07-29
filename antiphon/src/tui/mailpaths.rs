//! Deriving the account, folder and the copy-in-view from a
//! message's maildir paths. Kept apart from the operations in
//! `mailops` so the pure path logic stays small and testable.

use std::path::{Path, PathBuf};

use antiphon_store::MessageSummary;

use super::scope::ViewScope;

/// The folder a delivered message sits in, relative to its
/// account maildir: empty for the root inbox, "lists/rust"
/// for a nested folder.
pub(super) fn folder_of(path: &Path) -> String {
    let account = account_of(path);
    let mut parts: Vec<&str> = Vec::new();
    let mut seen_account = false;
    for component in path.components() {
        let text = component.as_os_str().to_str().unwrap_or("");
        if seen_account {
            parts.push(text);
        }
        if !seen_account && text == account {
            seen_account = true;
        }
    }
    // The trailing cur|new/<file> pair never names a folder.
    let folder_parts = parts.len().saturating_sub(2).min(parts.len());
    parts[..folder_parts].join("/")
}

pub(super) fn account_of(path: &Path) -> String {
    let mut components = path.components();
    for component in components.by_ref() {
        if component.as_os_str() == "maildir" {
            break;
        }
    }
    components
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A message delivered to two synced accounts at once (sent from
/// one, Bcc'd to another) has one file per account. This picks
/// the copy in the account being viewed, so every list action
/// lands on the copy the reader sees rather than whichever path
/// notmuch returned first. A received copy wins over a sent one
/// when the scope permits both.
pub(super) fn scoped_path(
    scope: &ViewScope,
    accounts: &[String],
    message: &MessageSummary,
) -> PathBuf {
    if message.paths.len() < 2 {
        return message.path.clone();
    }
    let permitted: Vec<&PathBuf> = message
        .paths
        .iter()
        .filter(|path| {
            scope_permits(scope, accounts, &account_of(path))
        })
        .collect();
    let choices = match permitted.is_empty() {
        true => message.paths.iter().collect(),
        false => permitted,
    };
    choices
        .iter()
        .find(|path| !is_sent_copy(path))
        .or_else(|| choices.first())
        .map(|path| (*path).clone())
        .unwrap_or_else(|| message.path.clone())
}

fn scope_permits(
    scope: &ViewScope,
    accounts: &[String],
    account: &str,
) -> bool {
    match scope {
        ViewScope::Unified => {
            accounts.iter().any(|name| name == account)
        }
        ViewScope::Account(current) => current == account,
    }
}

/// A maildir path whose folder segment names a Sent folder, so
/// the received copy can be preferred over the sent one.
fn is_sent_copy(path: &Path) -> bool {
    path.components().any(|component| {
        component.as_os_str().to_str().is_some_and(|name| {
            name.to_ascii_lowercase().starts_with("sent")
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_of_reads_the_subdir_between_account_and_cur() {
        let cases = [
            ("store/maildir/work/cur/a.eml", ""),
            ("store/maildir/work/lists/rust/new/a.eml", "lists/rust"),
            ("store/maildir/work/archive/cur/a.eml", "archive"),
        ];
        for (path, expected) in cases {
            assert_eq!(folder_of(Path::new(path)), expected, "{path}");
        }
    }

    #[test]
    fn accounts_derive_from_maildir_paths() {
        let cases = [
            ("/store/maildir/work/cur/1.host:2,S", "work"),
            ("/store/maildir/personal/new/2.host", "personal"),
            ("/elsewhere/3.host", ""),
        ];
        for (path, expected) in cases {
            assert_eq!(account_of(Path::new(path)), expected, "{path}");
        }
    }

    fn dual(paths: &[&str]) -> MessageSummary {
        MessageSummary {
            id: "m".to_string(),
            thread_id: String::new(),
            subject: String::new(),
            from: String::new(),
            to: String::new(),
            date_unix: 0,
            tags: Vec::new(),
            unread: false,
            path: PathBuf::from(paths[0]),
            paths: paths.iter().map(PathBuf::from).collect(),
            in_reply_to: None,
            references: Vec::new(),
        }
    }

    #[test]
    fn scoped_path_targets_the_viewed_account_copy() {
        let message = dual(&[
            "store/maildir/work/sent/cur/x.eml",
            "store/maildir/home/cur/x.eml",
        ]);
        let accounts = vec!["work".to_string(), "home".to_string()];
        assert_eq!(
            scoped_path(
                &ViewScope::Account("home".to_string()),
                &accounts,
                &message,
            ),
            PathBuf::from("store/maildir/home/cur/x.eml"),
            "the home scope acts on the home copy",
        );
        assert_eq!(
            scoped_path(
                &ViewScope::Account("work".to_string()),
                &accounts,
                &message,
            ),
            PathBuf::from("store/maildir/work/sent/cur/x.eml"),
            "the work scope acts on the work copy",
        );
        assert_eq!(
            scoped_path(&ViewScope::Unified, &accounts, &message),
            PathBuf::from("store/maildir/home/cur/x.eml"),
            "unified prefers the received copy over the sent one",
        );
    }
}
