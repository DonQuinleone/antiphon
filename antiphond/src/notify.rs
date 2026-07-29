use std::io;
use std::process::{Command, Stdio};

use antiphon_sync::SyncReport;

/// The desktop-notification preferences the daemon acts on,
/// derived from the [notifications] config and swapped on
/// reload with the rest of the account set.
#[derive(Clone, Default)]
pub(crate) struct NotifyPrefs {
    pub enabled: bool,
    pub folders: Vec<String>,
    pub sound: bool,
    pub speech: bool,
}

impl NotifyPrefs {
    /// A folder counts when the watch list names it, or when
    /// the list is empty (watch everything).
    fn watches(&self, folder: &str) -> bool {
        self.folders.is_empty()
            || self.folders.iter().any(|name| name == folder)
    }
}

/// Announce newly arrived mail on the desktop, best-effort: a
/// failed notifier must never fail a sync. Only mail in the
/// watched folders counts, so filing and sent copies stay
/// silent.
pub(crate) fn new_mail(
    account: &str,
    report: &SyncReport,
    prefs: &NotifyPrefs,
) {
    let total: usize = report
        .folders
        .iter()
        .filter(|folder| prefs.watches(&folder.folder))
        .map(|folder| folder.new_messages)
        .sum();
    if total == 0 {
        return;
    }
    let title = format!("{account}: {total} new");
    let body = folder_summary(report, prefs);
    if let Err(error) = show(&title, &body, prefs.sound) {
        eprintln!("notification: {error}");
    }
    if prefs.speech {
        let _ = speak(&title);
    }
}

fn folder_summary(report: &SyncReport, prefs: &NotifyPrefs) -> String {
    let parts: Vec<String> = report
        .folders
        .iter()
        .filter(|folder| {
            folder.new_messages > 0 && prefs.watches(&folder.folder)
        })
        .map(|folder| {
            format!("{} in {}", folder.new_messages, folder.folder)
        })
        .collect();
    parts.join(", ")
}

#[cfg(target_os = "macos")]
fn show(title: &str, body: &str, sound: bool) -> io::Result<()> {
    let sound = if sound { " sound name \"Ping\"" } else { "" };
    let script = format!(
        "display notification \"{}\" with title \"{}\"{sound}",
        applescript_escape(body),
        applescript_escape(title),
    );
    run(Command::new("osascript").args(["-e", &script]))
}

#[cfg(not(target_os = "macos"))]
fn show(title: &str, body: &str, sound: bool) -> io::Result<()> {
    let mut command = Command::new("notify-send");
    command.arg("--app-name=antiphon");
    if sound {
        command.arg("--hint=string:sound-name:message-new-instant");
    }
    run(command.arg(title).arg(body))
}

/// Reads the notice aloud, detached so speech never blocks the
/// sync; the platform text-to-speech does the talking.
#[cfg(target_os = "macos")]
fn speak(text: &str) -> io::Result<()> {
    spawn_detached(Command::new("say").arg(text))
}

#[cfg(not(target_os = "macos"))]
fn speak(text: &str) -> io::Result<()> {
    spawn_detached(Command::new("spd-say").arg(text))
}

fn spawn_detached(command: &mut Command) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(target_os = "macos")]
fn applescript_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

fn run(command: &mut Command) -> io::Result<()> {
    let status = command.status()?;
    if status.success() {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "notifier exited {:?}",
        status.code()
    )))
}

#[cfg(test)]
mod tests {
    use antiphon_sync::FolderReport;

    use super::*;

    fn report(folders: &[(&str, usize)]) -> SyncReport {
        SyncReport {
            folders: folders
                .iter()
                .map(|(name, new_messages)| FolderReport {
                    folder: (*name).to_owned(),
                    new_messages: *new_messages,
                    updated_messages: 0,
                    removed_messages: 0,
                    delivered: Vec::new(),
                })
                .collect(),
            errors: Vec::new(),
        }
    }

    fn watching_all() -> NotifyPrefs {
        NotifyPrefs {
            enabled: true,
            ..NotifyPrefs::default()
        }
    }

    #[test]
    fn summary_names_only_folders_with_new_mail() {
        let report =
            report(&[("INBOX", 2), ("Sent", 0), ("lists/aerc", 1)]);
        assert_eq!(
            folder_summary(&report, &watching_all()),
            "2 in INBOX, 1 in lists/aerc"
        );
    }

    #[test]
    fn the_watch_list_limits_the_summary_and_the_count() {
        let report =
            report(&[("INBOX", 2), ("Archive", 5), ("Sent", 1)]);
        let prefs = NotifyPrefs {
            enabled: true,
            folders: vec!["INBOX".to_string()],
            ..NotifyPrefs::default()
        };
        assert_eq!(folder_summary(&report, &prefs), "2 in INBOX");
        assert!(prefs.watches("INBOX"));
        assert!(!prefs.watches("Archive"));
    }

    #[test]
    fn an_empty_watch_list_watches_every_folder() {
        assert!(NotifyPrefs::default().watches("anything"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn applescript_metacharacters_are_escaped() {
        assert_eq!(
            applescript_escape(r#"a "quoted" \ path"#),
            r#"a \"quoted\" \\ path"#
        );
    }
}
