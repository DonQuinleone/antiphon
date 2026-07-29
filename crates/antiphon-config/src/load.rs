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
    let mut accounts = load_accounts(&dirs.config.join("accounts"))?;
    apply_order(&mut accounts, &config.accounts.order);
    Ok(Loaded { config, accounts })
}

/// Accounts named in `[accounts] order` come first, in that
/// order; the rest keep their filename order behind them, and
/// names matching no account are ignored. Every consumer of
/// `Loaded.accounts` inherits this: the first account is the
/// primary (startup selection, unified compose From).
fn apply_order(accounts: &mut [NamedAccount], order: &[String]) {
    if order.is_empty() {
        return;
    }
    accounts.sort_by_key(|entry| {
        order
            .iter()
            .position(|name| {
                *name == entry.account.account.name
                    || *name == entry.file_stem
            })
            .unwrap_or(order.len())
    });
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

/// A single-line `signature` names a file under `signatures/`; a
/// value carrying a newline can never be a file name, so it is a
/// block typed into the account form's identity editor and is
/// used verbatim rather than dropped as a missing file.
pub fn signature_text(dirs: &Dirs, value: &str) -> Option<String> {
    if value.contains('\n') {
        return Some(value.to_string());
    }
    named_file(dirs, "signatures", value)
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
        assert!(!config.sync.idle);
    }

    #[test]
    fn accounts_order_parses_and_defaults_empty() {
        let config: Config =
            parse("", Path::new("config.toml")).unwrap();
        assert!(config.accounts.order.is_empty());
        let config: Config = parse(
            "[accounts]\norder = [\"work\", \"personal\"]\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.accounts.order, ["work", "personal"]);
    }

    fn named(stem: &str, name: &str) -> NamedAccount {
        let text = format!(
            "[account]\nname = \"{name}\"\n\
             [imap]\nhost = \"h\"\nuser = \"u\"\n"
        );
        NamedAccount {
            file_stem: stem.to_string(),
            account: parse(&text, Path::new("test.toml")).unwrap(),
        }
    }

    fn stems(accounts: &[NamedAccount]) -> Vec<&str> {
        accounts
            .iter()
            .map(|entry| entry.file_stem.as_str())
            .collect()
    }

    #[test]
    fn listed_accounts_lead_and_the_rest_keep_filename_order() {
        let mut accounts =
            vec![named("a", "a"), named("b", "b"), named("c", "c")];
        apply_order(
            &mut accounts,
            &["c".to_string(), "ghost".to_string(), "a".to_string()],
        );
        assert_eq!(stems(&accounts), ["c", "a", "b"]);
    }

    #[test]
    fn order_matches_the_account_name_or_the_file_stem() {
        let mut accounts =
            vec![named("01-work", "work"), named("02-home", "home")];
        apply_order(&mut accounts, &["home".to_string()]);
        assert_eq!(stems(&accounts), ["02-home", "01-work"]);
        apply_order(&mut accounts, &["01-work".to_string()]);
        assert_eq!(stems(&accounts), ["01-work", "02-home"]);
    }

    #[test]
    fn idle_parses_from_the_sync_table() {
        let config: Config =
            parse("[sync]\nidle = true\n", Path::new("config.toml"))
                .unwrap();
        assert!(config.sync.idle);
    }

    #[test]
    fn recipients_parse_from_the_export_table() {
        let config: Config = parse(
            "[export]\nrecipients = [\"age1abc\", \"age1def\"]\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert_eq!(config.export.recipients, ["age1abc", "age1def"]);
        assert!(Config::default().export.recipients.is_empty());
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
    fn inline_images_defaults_on_and_toggles_off() {
        let default: Config = Config::default();
        assert!(default.ui.inline_images, "on unless disabled");
        let config: Config = parse(
            "[ui]\ninline_images = false\n",
            Path::new("config.toml"),
        )
        .unwrap();
        assert!(!config.ui.inline_images);
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
folder_order = ["lists/aerc", "inbox"]
folders_hidden = ["spam"]
folders_unsynced = ["archive/2019"]

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
        assert_eq!(account.folder_order, ["lists/aerc", "inbox"]);
        assert_eq!(account.folders_hidden, ["spam"]);
        assert_eq!(account.folders_unsynced, ["archive/2019"]);
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
    fn a_multi_line_signature_value_is_used_verbatim() {
        let dirs = Dirs {
            config: std::env::temp_dir(),
            data: std::env::temp_dir(),
            state: std::env::temp_dir(),
            cache: std::env::temp_dir(),
        };
        let block = "Kind regards\nQuin";
        assert_eq!(
            signature_text(&dirs, block).as_deref(),
            Some(block),
            "a block typed in the form is sent, not read as a file"
        );
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
