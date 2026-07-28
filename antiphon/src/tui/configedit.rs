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

pub(crate) fn with_key(
    contents: &str,
    table: &str,
    key: &str,
    value: &str,
) -> String {
    edited(contents, &format!("[{table}]"), key, value)
}

/// Rewrites every `[[table]]` block to `blocks`: the existing
/// ones are removed and the rendered replacements spliced in
/// where the first sat (or, when there were none, at the end of
/// the file), separated by single blank lines. Reserved for an
/// array of tables the caller fully owns (an account's
/// identities), since each block is regenerated wholesale.
pub(crate) fn set_array_tables(
    contents: &str,
    table: &str,
    blocks: &[Vec<String>],
) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    let ranges = array_table_ranges(&lines, table);
    let anchor = ranges.first().map(|(start, _)| *start);
    for (start, end) in ranges.iter().rev() {
        lines.drain(*start..*end);
    }
    let at = anchor.unwrap_or(lines.len());
    splice_blocks(&mut lines, at, blocks);
    if lines.is_empty() {
        return String::new();
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

/// Every `[[table]]` block as a half-open line range: the header
/// through the line before the next table header.
fn array_table_ranges(
    lines: &[String],
    table: &str,
) -> Vec<(usize, usize)> {
    let header = format!("[[{table}]]");
    let mut ranges = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() != header {
            index += 1;
            continue;
        }
        let end = array_table_end(lines, index);
        ranges.push((index, end));
        index = end;
    }
    ranges
}

fn array_table_end(lines: &[String], start: usize) -> usize {
    lines[start + 1..]
        .iter()
        .position(|line| is_table_header(line))
        .map(|offset| start + 1 + offset)
        .unwrap_or(lines.len())
}

fn splice_blocks(
    lines: &mut Vec<String>,
    at: usize,
    blocks: &[Vec<String>],
) {
    let mut rendered: Vec<String> = Vec::new();
    for block in blocks {
        if !rendered.is_empty() {
            rendered.push(String::new());
        }
        rendered.extend(block.iter().cloned());
    }
    if rendered.is_empty() {
        return;
    }
    if at < lines.len() && !lines[at].is_empty() {
        rendered.push(String::new());
    }
    if at > 0 && !lines[at - 1].is_empty() {
        rendered.insert(0, String::new());
    }
    lines.splice(at..at, rendered);
}

fn edited(
    contents: &str,
    header: &str,
    key: &str,
    value: &str,
) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    match table_range(&lines, header) {
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
            lines.push(header.to_string());
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

/// Drops `[table]` and its whole body; a missing table is a
/// no-op. Reserved for tables the caller fully owns (the
/// account form's [oauth], say), since hand-written keys
/// inside go with it.
pub(super) fn without_table(contents: &str, table: &str) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    let header = format!("[{table}]");
    let Some((start, end)) = table_range(&lines, &header) else {
        return contents.to_string();
    };
    lines.drain(start..end);
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if lines.is_empty() {
        return String::new();
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

pub(super) fn has_table(contents: &str, table: &str) -> bool {
    let lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    table_range(&lines, &format!("[{table}]")).is_some()
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
#[path = "configedit_tests.rs"]
mod tests;
