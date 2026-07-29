use std::path::PathBuf;

use crate::engine::RemoteFolder;

const INBOX: &str = "inbox";

/// Maps an IMAP mailbox name to a path relative to the
/// account's maildir root: INBOX is the root itself, every
/// other mailbox becomes a lowercased subdirectory, with the
/// server's hierarchy delimiter opening nested directories.
pub(crate) fn folder_subdir(
    name: &str,
    delimiter: Option<&str>,
) -> Result<PathBuf, String> {
    let lowered = name.to_lowercase();
    if lowered == INBOX {
        return Ok(PathBuf::new());
    }
    let segments: Vec<&str> = match delimiter {
        Some(sep) if !sep.is_empty() => lowered.split(sep).collect(),
        _ => vec![lowered.as_str()],
    };
    let mut path = PathBuf::new();
    for segment in segments {
        validate_segment(segment)?;
        path.push(segment);
    }
    Ok(path)
}

fn validate_segment(segment: &str) -> Result<(), String> {
    if segment.is_empty() {
        return Err(String::from("empty path segment"));
    }
    if segment == "." || segment == ".." {
        return Err(format!("unsafe path segment `{segment}`"));
    }
    let forbidden = |ch| ch == '/' || ch == '\\' || ch == '\0';
    if segment.contains(forbidden) {
        return Err(format!(
            "forbidden character in segment `{segment}`"
        ));
    }
    Ok(())
}

/// Whether the mailbox is excluded from syncing: compared on
/// the maildir-relative path `folder_subdir` produces, case
/// insensitively. INBOX maps to the empty path and so can
/// never match; a folder whose name does not map at all is
/// left for `sync_folder` to report.
pub(crate) fn excluded(
    folder: &RemoteFolder,
    exclusions: &[String],
) -> bool {
    if exclusions.is_empty() {
        return false;
    }
    let Ok(subdir) =
        folder_subdir(&folder.name, folder.delimiter.as_deref())
    else {
        return false;
    };
    let subdir = subdir.to_string_lossy();
    if subdir.is_empty() {
        return false;
    }
    exclusions
        .iter()
        .any(|pattern| matches_exclusion(pattern, &subdir))
}

/// A `folders_unsynced` entry matches a mailbox by its
/// case-insensitive maildir path. A trailing `*` matches the
/// named folder and every mailbox beneath it, so `calendar*`
/// covers `calendar` and `calendar/birthdays` alike.
fn matches_exclusion(pattern: &str, subdir: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let Some(prefix) = pattern.strip_suffix('*') else {
        return pattern == subdir;
    };
    let prefix = prefix.trim_end_matches('/');
    subdir == prefix || subdir.starts_with(&format!("{prefix}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, delimiter: Option<&str>) -> RemoteFolder {
        RemoteFolder {
            name: name.to_string(),
            delimiter: delimiter.map(str::to_string),
        }
    }

    fn exclusions(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn exclusions_match_the_maildir_relative_name() {
        let listed = exclusions(&["archive/2019", "spam"]);
        let cases = [
            ("Archive/2019", Some("/"), true),
            ("Archive.2019", Some("."), true),
            ("SPAM", Some("/"), true),
            ("archive/2020", Some("/"), false),
            ("archive", Some("/"), false),
        ];
        for (name, delimiter, want) in cases {
            assert_eq!(
                excluded(&folder(name, delimiter), &listed),
                want,
                "{name}"
            );
        }
    }

    #[test]
    fn a_trailing_star_excludes_the_folder_and_its_children() {
        let listed = exclusions(&["calendar*"]);
        let cases = [
            ("Calendar", Some("/"), true),
            ("Calendar/Birthdays", Some("/"), true),
            ("Calendar.Work", Some("."), true),
            ("Calendars", Some("/"), false),
            ("inbox/calendar", Some("/"), false),
        ];
        for (name, delimiter, want) in cases {
            assert_eq!(
                excluded(&folder(name, delimiter), &listed),
                want,
                "{name}"
            );
        }
    }

    #[test]
    fn exclusions_compare_case_insensitively_both_ways() {
        let listed = exclusions(&["Lists/Aerc"]);
        assert!(excluded(&folder("lists/aerc", Some("/")), &listed));
    }

    #[test]
    fn the_inbox_is_never_excludable() {
        let listed = exclusions(&["inbox", "INBOX", ""]);
        assert!(!excluded(&folder("INBOX", Some("/")), &listed));
    }

    #[test]
    fn no_exclusions_and_bad_names_never_exclude() {
        assert!(!excluded(&folder("spam", Some("/")), &[]));
        let listed = exclusions(&["spam"]);
        assert!(!excluded(&folder("..", Some("/")), &listed));
    }

    #[test]
    fn inbox_maps_to_the_maildir_root() {
        for name in ["INBOX", "Inbox", "inbox"] {
            let path = folder_subdir(name, Some("/")).unwrap();
            assert_eq!(path, PathBuf::new());
        }
    }

    #[test]
    fn other_folders_are_lowercased() {
        let path = folder_subdir("Sent", Some("/")).unwrap();
        assert_eq!(path, PathBuf::from("sent"));
    }

    #[test]
    fn hierarchy_delimiter_opens_subdirectories() {
        let path = folder_subdir("Archive/2024", Some("/")).unwrap();
        assert_eq!(path, PathBuf::from("archive/2024"));
        let dotted = folder_subdir("Archive.2024", Some(".")).unwrap();
        assert_eq!(dotted, PathBuf::from("archive/2024"));
    }

    #[test]
    fn missing_delimiter_keeps_the_name_whole() {
        let path = folder_subdir("Sent.Items", None).unwrap();
        assert_eq!(path, PathBuf::from("sent.items"));
    }

    #[test]
    fn traversal_and_separator_abuse_is_rejected() {
        assert!(folder_subdir("..", Some("/")).is_err());
        assert!(folder_subdir("a/../b", Some("/")).is_err());
        assert!(folder_subdir("a//b", Some("/")).is_err());
        assert!(folder_subdir("bad/name", Some(".")).is_err());
        assert!(folder_subdir("bad\\name", Some("/")).is_err());
    }
}
