use std::io;
use std::path::{Path, PathBuf};

use antiphon_ui::Theme;

use super::app::App;

const UI_HEADER: &str = "[ui]";
const THEME_KEY: &str = "theme";
const TMP_SUFFIX: &str = ".tmp";

impl App {
    /// `:theme` with no name lists the gallery; a known name
    /// switches live and persists; an unknown one leaves the
    /// theme untouched and names the gallery instead.
    pub(super) fn theme_command(&mut self, name: &str) {
        if name.is_empty() {
            self.notice = Some(format!("themes: {}", gallery()));
            return;
        }
        let Some(theme) = Theme::by_name(name) else {
            self.notice = Some(format!(
                "unknown theme {name}; themes: {}",
                gallery()
            ));
            return;
        };
        self.theme = theme;
        self.notice = Some(match persist(&self.config_path, name) {
            Ok(()) => format!("theme: {name}"),
            Err(error) => format!("theme: {name} (not saved: {error})"),
        });
    }
}

fn gallery() -> String {
    Theme::names().collect::<Vec<_>>().join(", ")
}

/// Rewrites the `theme` key under `[ui]` in the config file at
/// `path`, leaving every other line untouched; a missing file
/// or table is created rather than treated as an error.
fn persist(path: &Path, name: &str) -> io::Result<()> {
    let existing = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            String::new()
        }
        Err(error) => return Err(error),
    };
    let rewritten = with_theme(&existing, name);
    write_atomically(path, &rewritten)
}

fn with_theme(contents: &str, name: &str) -> String {
    let mut lines: Vec<String> =
        contents.lines().map(str::to_owned).collect();
    match ui_table_range(&lines) {
        Some((header, end)) => match theme_line_in(&lines, header, end)
        {
            Some(index) => {
                lines[index] = replace_value(&lines[index], name)
            }
            None => lines.insert(header + 1, theme_line(name)),
        },
        None => {
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
            lines.push(UI_HEADER.to_string());
            lines.push(theme_line(name));
        }
    }
    let mut rewritten = lines.join("\n");
    rewritten.push('\n');
    rewritten
}

fn theme_line(name: &str) -> String {
    format!("{THEME_KEY} = \"{name}\"")
}

fn is_table_header(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('[') && trimmed.ends_with(']')
}

/// The `[ui]` header's line and the exclusive end of its body:
/// the next table header, or the end of the file.
fn ui_table_range(lines: &[String]) -> Option<(usize, usize)> {
    let header =
        lines.iter().position(|line| line.trim() == UI_HEADER)?;
    let end = lines[header + 1..]
        .iter()
        .position(|line| is_table_header(line))
        .map(|offset| header + 1 + offset)
        .unwrap_or(lines.len());
    Some((header, end))
}

fn theme_line_in(
    lines: &[String],
    start: usize,
    end: usize,
) -> Option<usize> {
    (start + 1..end).find(|&index| is_theme_key(&lines[index]))
}

fn is_theme_key(line: &str) -> bool {
    let Some(rest) = line.trim_start().strip_prefix(THEME_KEY) else {
        return false;
    };
    rest.trim_start().starts_with('=')
}

/// Replaces only the quoted value on a `theme = "..."` line,
/// so indentation and any trailing comment survive untouched.
fn replace_value(line: &str, name: &str) -> String {
    let Some(open) = line.find('"') else {
        return theme_line(name);
    };
    let Some(close_offset) = line[open + 1..].find('"') else {
        return theme_line(name);
    };
    let close = open + 1 + close_offset;
    format!("{}\"{name}\"{}", &line[..open], &line[close + 1..])
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
    fn persist_writes_a_missing_file_and_replaces_an_existing_one() {
        let dir = TempDir::new();
        let path = dir.path.join("config.toml");
        persist(&path, "nord").expect("first write");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[ui]\ntheme = \"nord\"\n"
        );
        persist(&path, "gruvbox-dark").expect("second write");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[ui]\ntheme = \"gruvbox-dark\"\n"
        );
        assert!(!path.with_file_name("config.toml.tmp").exists());
    }

    #[test]
    fn gallery_names_every_theme() {
        assert_eq!(
            gallery(),
            "vespers, kanagawa-wave, catppuccin-mocha, \
             gruvbox-dark, tokyo-night, nord, rose-pine"
        );
    }
}
