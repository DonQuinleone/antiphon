use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::account::AccountFile;
use crate::diagnose::{enrich, locate_key, suggest, unknown_key};
use crate::error::ConfigError;
use crate::pgp::check_pgp_keys;
use crate::schema::Config;
use crate::xdg::Dirs;

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub accounts: Vec<NamedAccount>,
}

#[derive(Debug)]
pub struct NamedAccount {
    pub file_stem: String,
    pub account: AccountFile,
}

pub fn load(dirs: &Dirs) -> Result<Loaded, ConfigError> {
    let config = load_config(&dirs.config)?;
    let accounts = load_accounts(&dirs.config.join("accounts"))?;
    Ok(Loaded { config, accounts })
}

fn load_config(dir: &Path) -> Result<Config, ConfigError> {
    let main_path = dir.join("config.toml");
    let local_path = dir.join("local.toml");
    let main = read_optional(&main_path)?;
    let local = read_optional(&local_path)?;
    match (main, local) {
        (None, None) => Ok(Config::default()),
        (Some(text), None) => parse(&text, &main_path),
        (main, Some(over)) => merged_config(
            main.as_deref().unwrap_or(""),
            &main_path,
            &over,
            &local_path,
        ),
    }
}

fn merged_config(
    main: &str,
    main_path: &Path,
    over: &str,
    over_path: &Path,
) -> Result<Config, ConfigError> {
    // Parse the base strictly first so its own mistakes are
    // reported against its own file and line.
    let _: Config = parse(main, main_path)?;
    let base: toml::Value = parse(main, main_path)?;
    let over_value: toml::Value = parse(over, over_path)?;
    let merged = merge(base, over_value);
    merged.try_into().map_err(|err: toml::de::Error| {
        // The base parsed cleanly, so the defect belongs to the
        // override file; spans are lost in the merge, so find
        // the offending key by scanning its text.
        let message = err.message().to_string();
        let line = unknown_key(&message)
            .and_then(|key| locate_key(over, &key));
        ConfigError {
            file: over_path.to_path_buf(),
            line,
            suggestion: suggest(&message),
            message,
        }
    })
}

fn merge(base: toml::Value, over: toml::Value) -> toml::Value {
    use toml::Value;
    match (base, over) {
        (Value::Table(mut under), Value::Table(over)) => {
            for (key, value) in over {
                let merged = match under.remove(&key) {
                    Some(existing) => merge(existing, value),
                    None => value,
                };
                under.insert(key, merged);
            }
            Value::Table(under)
        }
        (_, over) => over,
    }
}

fn load_accounts(dir: &Path) -> Result<Vec<NamedAccount>, ConfigError> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "toml")
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = read_required(&path)?;
            let account = parse(&text, &path)?;
            check_pgp_keys(&account, &text, &path)?;
            Ok(NamedAccount {
                file_stem: file_stem(&path),
                account,
            })
        })
        .collect()
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn read_optional(path: &Path) -> Result<Option<String>, ConfigError> {
    if !path.exists() {
        return Ok(None);
    }
    read_required(path).map(Some)
}

fn read_required(path: &Path) -> Result<String, ConfigError> {
    fs::read_to_string(path).map_err(|err| ConfigError {
        file: path.to_path_buf(),
        line: None,
        message: format!("cannot read file: {err}"),
        suggestion: None,
    })
}

pub(crate) fn parse<T: DeserializeOwned>(
    text: &str,
    path: &Path,
) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|err| enrich(err, text, path))
}

pub fn signature_text(dirs: &Dirs, name: &str) -> Option<String> {
    named_file(dirs, "signatures", name)
}

pub fn template_text(dirs: &Dirs, name: &str) -> Option<String> {
    named_file(dirs, "templates", name)
}

fn named_file(dirs: &Dirs, kind: &str, name: &str) -> Option<String> {
    let file_name = Path::new(name).file_name()?;
    if file_name != name {
        return None;
    }
    let path = dirs.config.join(kind).join(file_name);
    fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Composer, ReadingPane};

    fn config_error(text: &str) -> ConfigError {
        parse::<Config>(text, Path::new("config.toml"))
            .expect_err("expected a config error")
    }

    #[test]
    fn saved_searches_require_both_fields() {
        let err =
            config_error("[[saved_searches]]\nname = \"unread\"\n");
        assert!(err.message.contains("query"));
    }

    #[test]
    fn defaults_hold_when_config_is_empty() {
        let config: Config =
            parse("", Path::new("config.toml")).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(config.ui.theme, "vespers");
        assert_eq!(config.ui.reading_pane, ReadingPane::Below);
        assert_eq!(config.ui.composer, Composer::Embedded);
        assert_eq!(config.ui.list_rows, 7);
        assert_eq!(config.ui.sidebar_width, 16);
        assert_eq!(config.ui.date_format, "%Y-%m-%d %H:%M");
        assert_eq!(
            config.ui.headers,
            ["from", "to", "date", "subject"]
        );
        assert!(config.notifications.enabled);
    }

    #[test]
    fn headers_parse_from_the_ui_table() {
        let config: Config = parse(
            "[ui]\nheaders = [\"from\", \"x-mailer\"]\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.ui.headers, ["from", "x-mailer"]);
    }

    #[test]
    fn list_rows_and_sidebar_width_parse_from_the_ui_table() {
        let config: Config = parse(
            "[ui]\nlist_rows = 12\nsidebar_width = 24\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.ui.list_rows, 12);
        assert_eq!(config.ui.sidebar_width, 24);
        assert!(
            config_error("[ui]\nlist_rows = -3\n")
                .message
                .contains("invalid value")
        );
    }

    #[test]
    fn composer_accepts_the_suspend_fallback() {
        let config: Config = parse(
            "[ui]\ncomposer = \"suspend\"\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.ui.composer, Composer::Suspend);
        assert!(
            config_error("[ui]\ncomposer = \"floating\"\n")
                .message
                .contains("unknown variant")
        );
    }

    #[test]
    fn local_overrides_win_without_touching_the_rest() {
        let main = "[ui]\ntheme = \"kanagawa\"\n\
                    date_format = \"%H:%M\"\n";
        let over = "[ui]\ntheme = \"gruvbox\"\n";
        let config = merged_config(
            main,
            Path::new("config.toml"),
            over,
            Path::new("local.toml"),
        )
        .unwrap();
        assert_eq!(config.ui.theme, "gruvbox");
        assert_eq!(config.ui.date_format, "%H:%M");
    }

    #[test]
    fn local_defects_are_blamed_on_local() {
        let over = "[ui]\nthene = \"gruvbox\"\n";
        let err = merged_config(
            "",
            Path::new("config.toml"),
            over,
            Path::new("local.toml"),
        )
        .expect_err("expected a local.toml error");
        assert_eq!(err.file, PathBuf::from("local.toml"));
        assert_eq!(err.line, Some(2));
        assert_eq!(err.suggestion.as_deref(), Some("theme"));
    }

    #[test]
    fn account_files_parse_the_full_shape() {
        let text = r#"
[account]
name = "personal"

[imap]
host = "imap.example.com"
user = "quin@example.com"
password_cmd = "pass show mail/personal"

[smtp]
host = "smtp.example.com"

[[identity]]
address = "quin@example.com"
match = ["quin@example.com", "*@quin.example.com"]
pgp_sign = true

[[rules]]
match_list = "~sircmpwn/aerc-devel"
move_to = "lists/aerc"
"#;
        let account: AccountFile =
            parse(text, Path::new("personal.toml")).unwrap();
        assert_eq!(account.account.name, "personal");
        assert_eq!(account.identities.len(), 1);
        assert_eq!(account.identities[0].matches.len(), 2);
        assert!(account.identities[0].pgp_sign);
        assert_eq!(account.rules.len(), 1);
    }

    #[test]
    fn signatures_and_templates_load_by_bare_name_only() {
        let root = std::env::temp_dir()
            .join(format!("antiphon-sig-test-{}", std::process::id()));
        fs::create_dir_all(root.join("signatures")).unwrap();
        fs::write(root.join("signatures/personal"), "Q\n").unwrap();
        let dirs = Dirs {
            config: root.clone(),
            data: root.join("data"),
            state: root.join("state"),
            cache: root.join("cache"),
        };
        assert_eq!(
            signature_text(&dirs, "personal").as_deref(),
            Some("Q\n")
        );
        assert!(signature_text(&dirs, "missing").is_none());
        assert!(signature_text(&dirs, "../secrets").is_none());
        assert!(template_text(&dirs, "any").is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn loading_a_directory_reads_accounts_sorted() {
        let root = std::env::temp_dir().join(format!(
            "antiphon-config-test-{}",
            std::process::id()
        ));
        let accounts = root.join("accounts");
        fs::create_dir_all(&accounts).unwrap();
        fs::write(
            root.join("config.toml"),
            "[ui]\ntheme = \"kanagawa\"\n",
        )
        .unwrap();
        let account = "[account]\nname = \"a\"\n\
                       [imap]\nhost = \"h\"\nuser = \"u\"\n";
        fs::write(accounts.join("b.toml"), account).unwrap();
        fs::write(
            accounts.join("a.toml"),
            account.replace("\"a\"", "\"b\""),
        )
        .unwrap();

        let dirs = Dirs {
            config: root.clone(),
            state: root.join("state"),
            cache: root.join("cache"),
            data: root.join("data"),
        };
        let loaded = load(&dirs).unwrap();
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(loaded.config.ui.theme, "kanagawa");
        let stems: Vec<&str> = loaded
            .accounts
            .iter()
            .map(|acc| acc.file_stem.as_str())
            .collect();
        assert_eq!(stems, ["a", "b"]);
    }
}
