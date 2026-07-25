use std::fmt;
use std::path::Path;

use ratatui::style::Color;
use serde::Deserialize;

use super::Theme;

const HEX_DIGITS: usize = 6;

#[derive(Debug)]
pub enum ThemeError {
    Read {
        path: String,
        detail: String,
    },
    Parse {
        source: String,
        detail: String,
    },
    Colour {
        source: String,
        key: String,
        value: String,
    },
}

impl fmt::Display for ThemeError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, detail } => {
                write!(out, "theme {path}: {detail}")
            }
            Self::Parse { source, detail } => {
                write!(out, "theme {source}: {detail}")
            }
            Self::Colour { source, key, value } => write!(
                out,
                "theme {source}: {key} = \"{value}\" is not a \
                 #rrggbb colour"
            ),
        }
    }
}

impl std::error::Error for ThemeError {}

/// The file shape: every colour a "#rrggbb" string, every
/// field required, unknown keys refused so a typo fails
/// loudly instead of silently keeping a default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ThemeFile {
    name: String,
    background: String,
    surface: String,
    border: String,
    text_primary: String,
    text_muted: String,
    accent: String,
    accent_strong: String,
    selection_bg: String,
    selection_fg: String,
    unread_marker: String,
    list_date: String,
    list_time: String,
    list_from: String,
    list_subject: String,
    count: String,
    status_ok: String,
    status_error: String,
    diff_add: String,
    diff_remove: String,
}

pub(super) fn parse_theme(text: &str) -> Result<Theme, ThemeError> {
    parse_named(text, "embedded")
}

pub(super) fn parse_theme_file(
    path: &Path,
) -> Result<Theme, ThemeError> {
    let source = path.display().to_string();
    let text = std::fs::read_to_string(path).map_err(|error| {
        ThemeError::Read {
            path: source.clone(),
            detail: error.to_string(),
        }
    })?;
    parse_named(&text, &source)
}

fn parse_named(text: &str, source: &str) -> Result<Theme, ThemeError> {
    let file: ThemeFile =
        toml::from_str(text).map_err(|error| ThemeError::Parse {
            source: source.to_string(),
            detail: error.message().to_string(),
        })?;
    let colour = |key: &str, value: &String| {
        hex_colour(value).ok_or_else(|| ThemeError::Colour {
            source: source.to_string(),
            key: key.to_string(),
            value: value.clone(),
        })
    };
    Ok(Theme {
        background: colour("background", &file.background)?,
        surface: colour("surface", &file.surface)?,
        border: colour("border", &file.border)?,
        text_primary: colour("text_primary", &file.text_primary)?,
        text_muted: colour("text_muted", &file.text_muted)?,
        accent: colour("accent", &file.accent)?,
        accent_strong: colour("accent_strong", &file.accent_strong)?,
        selection_bg: colour("selection_bg", &file.selection_bg)?,
        selection_fg: colour("selection_fg", &file.selection_fg)?,
        unread_marker: colour("unread_marker", &file.unread_marker)?,
        list_date: colour("list_date", &file.list_date)?,
        list_time: colour("list_time", &file.list_time)?,
        list_from: colour("list_from", &file.list_from)?,
        list_subject: colour("list_subject", &file.list_subject)?,
        count: colour("count", &file.count)?,
        status_ok: colour("status_ok", &file.status_ok)?,
        status_error: colour("status_error", &file.status_error)?,
        diff_add: colour("diff_add", &file.diff_add)?,
        diff_remove: colour("diff_remove", &file.diff_remove)?,
        name: file.name,
    })
}

fn hex_colour(value: &str) -> Option<Color> {
    let digits = value.strip_prefix('#')?;
    if digits.len() != HEX_DIGITS {
        return None;
    }
    let parsed = u32::from_str_radix(digits, 16).ok()?;
    Some(Color::Rgb(
        (parsed >> 16) as u8,
        (parsed >> 8) as u8,
        parsed as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal(name: &str, accent: &str) -> String {
        let mut text = format!("name = \"{name}\"\n");
        for key in [
            "background",
            "surface",
            "border",
            "text_primary",
            "text_muted",
            "accent_strong",
            "selection_bg",
            "selection_fg",
            "unread_marker",
            "list_date",
            "list_time",
            "list_from",
            "list_subject",
            "count",
            "status_ok",
            "status_error",
            "diff_add",
            "diff_remove",
        ] {
            text.push_str(&format!("{key} = \"#101010\"\n"));
        }
        text.push_str(&format!("accent = \"{accent}\"\n"));
        text
    }

    #[test]
    fn a_full_file_parses_to_a_theme() {
        let theme = parse_theme(&minimal("night", "#a0b0c0")).unwrap();
        assert_eq!(theme.name, "night");
        assert_eq!(theme.accent, Color::Rgb(0xa0, 0xb0, 0xc0));
    }

    #[test]
    fn bad_colours_name_the_key_and_value() {
        let error =
            parse_theme(&minimal("night", "a0b0c0")).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("accent"), "{text}");
        assert!(text.contains("a0b0c0"), "{text}");
        assert!(
            parse_theme(&minimal("night", "#a0b0")).is_err(),
            "short colours are refused"
        );
    }

    #[test]
    fn missing_and_unknown_keys_are_refused() {
        assert!(parse_theme("name = \"bare\"\n").is_err());
        let mut text = minimal("night", "#a0b0c0");
        text.push_str("sparkle = \"#ffffff\"\n");
        assert!(parse_theme(&text).is_err());
    }

    #[test]
    fn theme_files_load_from_disk_with_named_errors() {
        let dir = std::env::temp_dir().join(format!(
            "antiphon-theme-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("night.toml");
        std::fs::write(&good, minimal("night", "#a0b0c0")).unwrap();
        assert_eq!(parse_theme_file(&good).unwrap().name, "night");
        let missing = dir.join("absent.toml");
        let error = parse_theme_file(&missing).unwrap_err();
        assert!(error.to_string().contains("absent.toml"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
