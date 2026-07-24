use std::io;
use std::process::Command;

use antiphon_sync::SyncReport;

/// Announce newly arrived mail on the desktop, best-effort:
/// a failed notifier must never fail a sync.
pub fn new_mail(account: &str, report: &SyncReport) {
    let total = report.total_new();
    if total == 0 {
        return;
    }
    let title = format!("{account}: {total} new");
    let body = folder_summary(report);
    if let Err(error) = show(&title, &body) {
        eprintln!("notification: {error}");
    }
}

fn folder_summary(report: &SyncReport) -> String {
    let parts: Vec<String> = report
        .folders
        .iter()
        .filter(|folder| folder.new_messages > 0)
        .map(|folder| {
            format!("{} in {}", folder.new_messages, folder.folder)
        })
        .collect();
    parts.join(", ")
}

#[cfg(target_os = "macos")]
fn show(title: &str, body: &str) -> io::Result<()> {
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        applescript_escape(body),
        applescript_escape(title),
    );
    run(Command::new("osascript").args(["-e", &script]))
}

#[cfg(not(target_os = "macos"))]
fn show(title: &str, body: &str) -> io::Result<()> {
    run(Command::new("notify-send").args([
        "--app-name=antiphon",
        title,
        body,
    ]))
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
                    delivered: Vec::new(),
                })
                .collect(),
        }
    }

    #[test]
    fn summary_names_only_folders_with_new_mail() {
        let report =
            report(&[("INBOX", 2), ("Sent", 0), ("lists/aerc", 1)]);
        assert_eq!(
            folder_summary(&report),
            "2 in INBOX, 1 in lists/aerc"
        );
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
