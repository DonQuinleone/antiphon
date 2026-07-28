//! Surgical TOML edits: rewrite exactly one key in a config
//! file, leaving every other line (comments included) intact.
//! Everything the settings view and the `:theme` command
//! persist goes through here.

use std::io;
use std::path::{Path, PathBuf};

const TMP_SUFFIX: &str = ".tmp";

/// A TOML array of strings, ready to hand to `persist_key`.
pub(super) fn toml_string_array(values: &[String]) -> String {
    let quoted: Vec<String> =
        values.iter().map(|value| format!("\"{value}\"")).collect();
    format!("[{}]", quoted.join(", "))
}

/// Rewrites the `key` under `[table]` in the config file at
/// `path` to `value` (already TOML-formatted: quoted for a
/// string, bare for a number or bool), leaving every other
/// line untouched; a missing file or table is created rather
/// than treated as an error.
pub(super) fn persist_key(
    path: &Path,
    table: &str,
    key: &str,
    value: &str,
) -> io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            String::new()
        }
        Err(error) => return Err(error),
    };
    let rewritten = with_key(&existing, table, key, value);
    write_atomically(path, &rewritten)
}

/// The sibling of `persist_key` for keys that live at the top
/// of the file, before any table header (an account file's
/// `folder_order`, say).
pub(super) fn persist_root_key(
    path: &Path,
    key: &str,
    value: &str,
) -> io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            String::new()
        }
        Err(error) => return Err(error),
    };
    let rewritten = with_root_key(&existing, key, value);
    write_atomically(path, &rewritten)
}

pub(super) fn with_root_key(
    contents: &str,
    key: &str,
    value: &str,
) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    let end = lines
        .iter()
        .position(|line| is_table_header(line))
        .unwrap_or(lines.len());
    let found = (0..end).find(|&index| is_key_line(&lines[index], key));
    match found {
        Some(index) => {
            lines[index] = replace_value(&lines[index], value)
        }
        None => {
            lines.insert(0, key_line(key, value));
            if lines.get(1).is_some_and(|line| is_table_header(line)) {
                lines.insert(1, String::new());
            }
        }
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

pub(super) fn with_key(
    contents: &str,
    table: &str,
    key: &str,
    value: &str,
) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    let header = format!("[{table}]");
    match table_range(&lines, &header) {
        Some((start, end)) => {
            match key_line_in(&lines, key, start, end) {
                Some(index) => {
                    lines[index] = replace_value(&lines[index], value)
                }
                None => lines.insert(start + 1, key_line(key, value)),
            }
        }
        None => {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push(header);
            lines.push(key_line(key, value));
        }
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

/// Rewrites `[table]` in the config file at `path` to drop
/// `key` entirely; a missing file, table or key is a no-op
/// rather than an error, so removal is idempotent.
pub(super) fn remove_key(
    path: &Path,
    table: &str,
    key: &str,
) -> io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let rewritten = without_key(&existing, table, key);
    write_atomically(path, &rewritten)
}

pub(super) fn without_key(
    contents: &str,
    table: &str,
    key: &str,
) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    let header = format!("[{table}]");
    if let Some((start, end)) = table_range(&lines, &header)
        && let Some(index) = key_line_in(&lines, key, start, end)
    {
        lines.remove(index);
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

fn key_line(key: &str, value: &str) -> String {
    format!("{} = {value}", toml_key(key))
}

/// Bare unless `key` holds a character a bare TOML key
/// cannot carry (a folder path's `/`, say): quoting is always
/// safe, so it is used only where it is actually needed.
fn toml_key(key: &str) -> String {
    let bare = key
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_');
    if bare {
        key.to_string()
    } else {
        format!("\"{key}\"")
    }
}

fn is_table_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[') && trimmed.ends_with(']')
}

/// The `[table]` header's line and the exclusive end of its
/// body: the next table header, or the end of the file.
fn table_range(
    lines: &[String],
    header: &str,
) -> Option<(usize, usize)> {
    let start = lines.iter().position(|line| line.trim() == header)?;
    let end = lines[start + 1..]
        .iter()
        .position(|line| is_table_header(line))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len());
    Some((start, end))
}

fn key_line_in(
    lines: &[String],
    key: &str,
    start: usize,
    end: usize,
) -> Option<usize> {
    (start + 1..end).find(|&index| is_key_line(&lines[index], key))
}

/// Matches `key` whether the line spells it bare or quoted,
/// so a hand-written `archive = ...` is still found even
/// though every key this module writes is now quoted.
fn is_key_line(line: &str, key: &str) -> bool {
    let (found, rest) = split_key(line.trim_start());
    found == key && rest.trim_start().starts_with('=')
}

/// The key token starting `line` (quoted or bare) and
/// everything after it.
fn split_key(line: &str) -> (&str, &str) {
    if let Some(body) = line.strip_prefix('"')
        && let Some(end) = body.find('"')
    {
        return (&body[..end], &body[end + 1..]);
    }
    let end = line
        .find(|ch: char| ch == '=' || ch.is_whitespace())
        .unwrap_or(line.len());
    (&line[..end], &line[end..])
}

/// Replaces only the value after `=`, so the key's spelling,
/// indentation and any trailing comment survive untouched.
fn replace_value(line: &str, value: &str) -> String {
    let Some((start, end)) = value_span(line) else {
        return line.to_string();
    };
    format!("{}{value}{}", &line[..start], &line[end..])
}

/// The byte range of the raw value between `=` and a trailing
/// comment (if any) or the end of the line, trimmed of the
/// whitespace around it.
fn value_span(line: &str) -> Option<(usize, usize)> {
    let eq = line.find('=')?;
    let rest = &line[eq + 1..];
    let leading = rest.len() - rest.trim_start().len();
    let start = eq + 1 + leading;
    let comment = rest.find('#').unwrap_or(rest.len());
    let end = eq + 1 + rest[..comment].trim_end().len();
    Some((start, end.max(start)))
}

fn write_atomically(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(path);
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(TMP_SUFFIX);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::TempDir;
    use super::*;

    fn with_theme(contents: &str, name: &str) -> String {
        with_key(contents, "ui", "theme", &format!("\"{name}\""))
    }

    #[test]
    fn toml_string_array_quotes_and_joins() {
        assert_eq!(toml_string_array(&[]), "[]");
        assert_eq!(
            toml_string_array(&["a".to_string(), "b".to_string()]),
            "[\"a\", \"b\"]"
        );
    }

    #[test]
    fn a_root_key_lands_before_the_first_table() {
        let before = "[account]\nname = \"work\"\n";
        let after =
            with_root_key(before, "folder_order", "[\"lists\"]");
        assert_eq!(
            after,
            "folder_order = [\"lists\"]\n\n\
             [account]\nname = \"work\"\n"
        );
        let updated =
            with_root_key(&after, "folder_order", "[\"spam\"]");
        assert!(updated.starts_with("folder_order = [\"spam\"]\n"));
        assert!(
            !updated.contains("lists"),
            "replaced in place: {updated}"
        );
        assert_eq!(
            with_root_key("", "folders_hidden", "[]"),
            "folders_hidden = []\n"
        );
    }

    #[test]
    fn a_root_key_inside_a_table_is_never_matched() {
        let before = "[account]\nname = \"work\"\n";
        let after = with_root_key(before, "name", "\"other\"");
        assert!(after.starts_with("name = \"other\"\n"));
        assert!(
            after.contains("name = \"work\""),
            "the table's own key survives: {after}"
        );
    }

    #[test]
    fn an_existing_key_is_replaced_in_place() {
        let before = "[ui]\ntheme = \"vespers\"  # see docs\n\
                      list_rows = 7\n";
        let after = with_theme(before, "nord");
        assert_eq!(
            after,
            "[ui]\ntheme = \"nord\"  # see docs\n\
             list_rows = 7\n"
        );
    }

    #[test]
    fn a_missing_key_is_inserted_under_the_header() {
        let before = "[ui]\nlist_rows = 7\n\n[sync]\nidle = false\n";
        let after = with_theme(before, "nord");
        assert_eq!(
            after,
            "[ui]\ntheme = \"nord\"\nlist_rows = 7\n\n\
             [sync]\nidle = false\n"
        );
    }

    #[test]
    fn a_missing_table_is_appended() {
        let before = "[sync]\nidle = false\n";
        let after = with_theme(before, "nord");
        assert_eq!(
            after,
            "[sync]\nidle = false\n\n[ui]\ntheme = \"nord\"\n"
        );
    }

    #[test]
    fn an_empty_document_gets_a_fresh_table() {
        let after = with_theme("", "nord");
        assert_eq!(after, "[ui]\ntheme = \"nord\"\n");
    }

    #[test]
    fn a_key_needing_quotes_is_written_and_found_quoted() {
        let before = "[folder_names]\nother = \"x\"\n";
        let after =
            with_key(before, "folder_names", "lists/aerc", "\"aerc\"");
        assert_eq!(
            after,
            "[folder_names]\n\"lists/aerc\" = \"aerc\"\nother = \"x\"\n"
        );
        let updated =
            with_key(&after, "folder_names", "lists/aerc", "\"list\"");
        assert!(updated.contains("\"lists/aerc\" = \"list\""));
        assert!(!updated.contains("\"aerc\"\n\"lists/aerc\""));
    }

    #[test]
    fn a_hand_written_bare_key_is_still_found_and_replaced() {
        let before = "[folder_names]\narchive = \"Archive\"\n";
        let after =
            with_key(before, "folder_names", "archive", "\"Old\"");
        assert_eq!(after, "[folder_names]\narchive = \"Old\"\n");
    }

    #[test]
    fn without_key_drops_only_the_named_entry() {
        let before = "[folder_names]\n\"lists/aerc\" = \"aerc\"\n\
                      archive = \"Archive\"\n";
        let after = without_key(before, "folder_names", "lists/aerc");
        assert_eq!(after, "[folder_names]\narchive = \"Archive\"\n");
    }

    #[test]
    fn without_key_is_a_no_op_when_nothing_matches() {
        let before = "[folder_names]\narchive = \"Archive\"\n";
        assert_eq!(
            without_key(before, "folder_names", "missing"),
            before
        );
        assert_eq!(without_key(before, "elsewhere", "archive"), before);
        assert_eq!(without_key("", "folder_names", "archive"), "");
    }

    #[test]
    fn remove_key_on_a_missing_file_is_a_no_op() {
        let dir = TempDir::new();
        let path = dir.path.join("missing.toml");
        remove_key(&path, "folder_names", "archive")
            .expect("a missing file is not an error");
        assert!(!path.exists());
    }

    #[test]
    fn remove_key_deletes_the_line_from_a_real_file() {
        let dir = TempDir::new();
        let path = dir.path.join("account.toml");
        std::fs::write(
            &path,
            "[folder_names]\n\"lists/aerc\" = \"aerc\"\n",
        )
        .unwrap();
        remove_key(&path, "folder_names", "lists/aerc")
            .expect("remove an existing key");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[folder_names]\n"
        );
    }

    /// Every essentials key that `settingscmd` writes goes
    /// through the very same generic edit, over the four
    /// shapes a config file can be in: key present, key
    /// absent, table absent, file absent.
    #[test]
    fn every_essentials_key_supports_all_four_edit_cases() {
        let cases: &[(&str, &str, &str)] = &[
            ("ui", "theme", "\"nord\""),
            ("sync", "interval_minutes", "5"),
            ("sync", "idle", "true"),
            ("ui", "reading_pane", "\"right\""),
            ("ui", "list_rows", "12"),
            ("ui", "sidebar_width", "20"),
        ];
        for (table, key, value) in cases {
            let want = key_line(key, value);

            let present =
                format!("[{table}]\n{key} = old\nother = 1\n");
            let after = with_key(&present, table, key, value);
            assert!(after.contains(&want), "{table}.{key} present");
            assert!(
                after.contains("other = 1"),
                "{table}.{key} keeps siblings"
            );

            let absent = format!("[{table}]\nother = 1\n");
            let after = with_key(&absent, table, key, value);
            assert!(after.contains(&want), "{table}.{key} key absent");

            let no_table = "[elsewhere]\nx = 1\n";
            let after = with_key(no_table, table, key, value);
            assert!(
                after.contains(&format!("[{table}]")),
                "{table}.{key} table absent"
            );
            assert!(
                after.contains(&want),
                "{table}.{key} table absent"
            );

            let dir = TempDir::new();
            let path = dir.path.join("config.toml");
            persist_key(&path, table, key, value)
                .expect("persist into a missing file");
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains(&want), "{table}.{key} file absent");
        }
    }
}
