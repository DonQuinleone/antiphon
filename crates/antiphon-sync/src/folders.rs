use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

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
