use std::io;
use std::path::Path;

use antiphon_ui::Theme;

use super::app::App;
use super::configedit::persist_key;

const UI_TABLE: &str = "ui";
const THEME_KEY: &str = "theme";

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

fn persist(path: &Path, name: &str) -> io::Result<()> {
    persist_key(path, UI_TABLE, THEME_KEY, &quoted(name))
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

#[cfg(test)]
mod tests {
    use super::super::configedit::with_key;
    use super::super::testkit::TempDir;
    use super::*;

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
        let listed = gallery();
        for name in antiphon_ui::Theme::names() {
            assert!(listed.contains(name), "{name} not listed");
        }
    }

    #[test]
    fn theme_edits_go_through_the_generic_config_edit() {
        let before = "[ui]\ntheme = \"vespers\"\nlist_rows = 7\n";
        let after =
            with_key(before, UI_TABLE, THEME_KEY, &quoted("nord"));
        assert_eq!(after, "[ui]\ntheme = \"nord\"\nlist_rows = 7\n");
    }
}
