mod parse;

use std::path::Path;
use std::sync::OnceLock;

use ratatui::style::Color;

pub use parse::ThemeError;

/// One theme file per theme: the shipped gallery is embedded
/// from themes/ in exactly the format users write, so the
/// source tree doubles as the format's documentation.
const BUILTIN_THEMES: [&str; 17] = [
    include_str!("../themes/vespers.toml"),
    include_str!("../themes/kanagawa-wave.toml"),
    include_str!("../themes/catppuccin-mocha.toml"),
    include_str!("../themes/gruvbox-dark.toml"),
    include_str!("../themes/tokyo-night.toml"),
    include_str!("../themes/nord.toml"),
    include_str!("../themes/rose-pine.toml"),
    include_str!("../themes/dracula.toml"),
    include_str!("../themes/solarized-dark.toml"),
    include_str!("../themes/solarized-light.toml"),
    include_str!("../themes/gruvbox-light.toml"),
    include_str!("../themes/catppuccin-latte.toml"),
    include_str!("../themes/one-dark.toml"),
    include_str!("../themes/everforest-dark.toml"),
    include_str!("../themes/ayu-dark.toml"),
    include_str!("../themes/github-dark.toml"),
    include_str!("../themes/monokai.toml"),
];

const VESPERS_NAME: &str = "vespers";
const THEME_EXTENSION: &str = "toml";

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub surface: Color,
    pub border: Color,
    pub text_primary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_strong: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub unread_marker: Color,
    pub list_date: Color,
    pub list_time: Color,
    pub list_from: Color,
    pub list_subject: Color,
    pub count: Color,
    pub status_ok: Color,
    pub status_error: Color,
    pub diff_add: Color,
    pub diff_remove: Color,
}

static REGISTRY: OnceLock<Vec<Theme>> = OnceLock::new();

fn builtins() -> Vec<Theme> {
    BUILTIN_THEMES
        .iter()
        .map(|text| {
            parse::parse_theme(text)
                .expect("embedded themes always parse")
        })
        .collect()
}

/// Loads the registry: the built-in gallery plus every *.toml
/// under `dir`, a user file overriding a built-in of the same
/// name. Called once at startup; a defective user file fails
/// loudly with its path, like config does. A process that
/// never calls this (the daemon, tests) gets the built-ins on
/// first use.
pub fn load_themes(dir: &Path) -> Result<(), ThemeError> {
    let mut themes = builtins();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map(|found| found.filter_map(Result::ok).collect())
        .unwrap_or_default();
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        let is_theme =
            path.extension().is_some_and(|ext| ext == THEME_EXTENSION);
        if !is_theme {
            continue;
        }
        let theme = parse::parse_theme_file(&path)?;
        themes.retain(|known| known.name != theme.name);
        themes.push(theme);
    }
    let _ = REGISTRY.set(themes);
    Ok(())
}

fn registry() -> &'static [Theme] {
    REGISTRY.get_or_init(builtins)
}

impl Theme {
    pub fn by_name(name: &str) -> Option<&'static Theme> {
        registry().iter().find(|theme| theme.name == name)
    }

    pub fn names() -> impl Iterator<Item = &'static str> {
        registry().iter().map(|theme| theme.name.as_str())
    }

    pub fn all() -> impl Iterator<Item = &'static Theme> {
        registry().iter()
    }

    /// The house default; present unless a user file
    /// deliberately overrides it, and the fallback is then
    /// that override.
    pub fn vespers() -> &'static Theme {
        Self::by_name(VESPERS_NAME).unwrap_or_else(|| &registry()[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_parses_and_resolves() {
        let names: Vec<&str> = Theme::names().collect();
        assert_eq!(names.len(), BUILTIN_THEMES.len());
        for expected in [
            "vespers",
            "dracula",
            "solarized-dark",
            "solarized-light",
            "gruvbox-light",
            "catppuccin-latte",
            "one-dark",
            "everforest-dark",
            "ayu-dark",
            "github-dark",
            "monokai",
        ] {
            assert!(names.contains(&expected), "{expected} missing");
        }
        for name in names {
            let theme = Theme::by_name(name)
                .unwrap_or_else(|| panic!("{name} did not resolve"));
            assert_eq!(theme.name, name);
        }
    }

    #[test]
    fn unknown_names_return_none() {
        assert!(Theme::by_name("").is_none());
        assert!(Theme::by_name("solarized").is_none());
        assert!(Theme::by_name("Vespers").is_none());
    }

    #[test]
    fn vespers_matches_the_branding_palette() {
        let vespers = Theme::vespers();
        assert_eq!(vespers.background, Color::Rgb(0x0c, 0x0f, 0x1d));
        assert_eq!(vespers.accent, Color::Rgb(0xd9, 0xad, 0x52));
        assert_eq!(vespers.text_primary, Color::Rgb(0xe9, 0xe4, 0xd4));
    }

    #[test]
    fn light_themes_really_are_light() {
        for name in
            ["solarized-light", "gruvbox-light", "catppuccin-latte"]
        {
            let theme = Theme::by_name(name).unwrap();
            let Color::Rgb(red, green, blue) = theme.background else {
                panic!("{name}: expected an rgb background");
            };
            let brightness =
                u32::from(red) + u32::from(green) + u32::from(blue);
            assert!(brightness > 500, "{name} is not light");
        }
    }
}
