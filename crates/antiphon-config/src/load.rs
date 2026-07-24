use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::account::AccountFile;
use crate::error::ConfigError;
use crate::schema::Config;
use crate::xdg::Dirs;

const SUGGESTION_FLOOR: f64 = 0.7;

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

/// A v4 OpenPGP fingerprint: 40 hex digits, spaces allowed,
/// an optional 0x prefix.
const FINGERPRINT_HEX_DIGITS: usize = 40;

fn check_pgp_keys(
    account: &AccountFile,
    text: &str,
    path: &Path,
) -> Result<(), ConfigError> {
    for identity in &account.identities {
        let Some(key) = &identity.pgp_key else {
            continue;
        };
        if valid_fingerprint(key) {
            continue;
        }
        return Err(ConfigError {
            file: path.to_path_buf(),
            line: locate_key(text, "pgp_key"),
            message: format!(
                "pgp_key `{key}` is not an OpenPGP \
                 fingerprint (40 hex digits)"
            ),
            suggestion: Some(
                "gpg --fingerprint <address> shows it".into(),
            ),
        });
    }
    Ok(())
}

fn valid_fingerprint(key: &str) -> bool {
    let hex = key
        .strip_prefix("0x")
        .unwrap_or(key)
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<Vec<char>>();
    hex.len() == FINGERPRINT_HEX_DIGITS
        && hex.iter().all(char::is_ascii_hexdigit)
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

fn parse<T: DeserializeOwned>(
    text: &str,
    path: &Path,
) -> Result<T, ConfigError> {
    toml::from_str(text).map_err(|err| enrich(err, text, path))
}

fn enrich(
    err: toml::de::Error,
    text: &str,
    path: &Path,
) -> ConfigError {
    let message = err.message().to_string();
    ConfigError {
        file: path.to_path_buf(),
        line: err.span().map(|span| line_of(text, span.start)),
        suggestion: suggest(&message),
        message,
    }
}

fn line_of(text: &str, offset: usize) -> usize {
    let upto = offset.min(text.len());
    text[..upto].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn locate_key(text: &str, key: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            let line = line.trim_start();
            line.strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
                || line.starts_with(&format!("[{key}"))
        })
        .map(|index| index + 1)
}

fn suggest(message: &str) -> Option<String> {
    let unknown = unknown_key(message)?;
    let candidates = expected_keys(message);
    candidates
        .into_iter()
        .map(|key| (strsim::jaro_winkler(&unknown, &key), key))
        .filter(|(score, _)| *score >= SUGGESTION_FLOOR)
        .max_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, key)| key)
}

fn unknown_key(message: &str) -> Option<String> {
    let rest = message.strip_prefix("unknown field `")?;
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn expected_keys(message: &str) -> Vec<String> {
    let Some(at) = message.find("expected") else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    let mut rest = &message[at..];
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else {
            break;
        };
        keys.push(rest[..end].to_string());
        rest = &rest[end + 1..];
    }
    keys
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

    struct Rejection {
        name: &'static str,
        text: &'static str,
        line: usize,
        suggestion: Option<&'static str>,
    }

    const REJECTIONS: &[Rejection] = &[
        Rejection {
            name: "top-level typo",
            text: "[notification]\nenabled = true\n",
            line: 1,
            suggestion: Some("notifications"),
        },
        Rejection {
            name: "nested typo",
            text: "[ui]\nreadin_pane = \"below\"\n",
            line: 2,
            suggestion: Some("reading_pane"),
        },
        Rejection {
            name: "unrelated key gets no guess",
            text: "[ui]\nzzz_qqq = 1\n",
            line: 2,
            suggestion: None,
        },
    ];

    #[test]
    fn unknown_keys_fail_with_line_and_suggestion() {
        for case in REJECTIONS {
            let err = config_error(case.text);
            assert_eq!(
                err.line,
                Some(case.line),
                "line for {}",
                case.name
            );
            assert_eq!(
                err.suggestion.as_deref(),
                case.suggestion,
                "suggestion for {}",
                case.name
            );
        }
    }

    #[test]
    fn account_typos_get_suggestions() {
        let text = "[account]\nname = \"a\"\n[imap]\n\
                    host = \"h\"\nuser = \"u\"\n\
                    pasword_cmd = \"pass\"\n";
        let err = parse::<AccountFile>(text, Path::new("a.toml"))
            .expect_err("expected an account error");
        assert_eq!(err.line, Some(6));
        assert_eq!(err.suggestion.as_deref(), Some("password_cmd"));
    }

    #[test]
    fn wrong_types_report_their_line() {
        let err =
            config_error("[vault]\nidle_lock_minutes = \"never\"\n");
        assert_eq!(err.line, Some(2));
        assert!(err.message.contains("invalid type"));
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
        assert!(config.notifications.enabled);
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
    fn pgp_key_fingerprints_are_validated() {
        let cases = [
            ("8F0EA48BF8BE9D3B9E1B2B9C6E5F0D3A1C2B4D5E", true),
            ("0x8F0EA48BF8BE9D3B9E1B2B9C6E5F0D3A1C2B4D5E", true),
            ("8F0E A48B F8BE 9D3B 9E1B 2B9C 6E5F 0D3A 1C2B 4D5E", true),
            ("quin@example.com", false),
            ("8F0EA48B", false),
            ("8F0EA48BF8BE9D3B9E1B2B9C6E5F0D3A1C2B4D5G", false),
        ];
        for (key, expected) in cases {
            assert_eq!(valid_fingerprint(key), expected, "{key}");
        }
    }

    #[test]
    fn a_bad_pgp_key_names_its_line() {
        let text = "[account]\nname = \"a\"\n[imap]\n\
                    host = \"h\"\nuser = \"u\"\n[[identity]]\n\
                    address = \"a@example.com\"\n\
                    pgp_key = \"not-a-fingerprint\"\n";
        let account: AccountFile =
            parse(text, Path::new("a.toml")).unwrap();
        let error = check_pgp_keys(&account, text, Path::new("a.toml"))
            .unwrap_err();
        assert_eq!(error.line, Some(8));
        assert!(error.message.contains("not-a-fingerprint"));
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
