use std::path::Path;

use crate::account::AccountFile;
use crate::diagnose::locate_key;
use crate::error::ConfigError;

/// A v4 OpenPGP fingerprint: 40 hex digits, spaces allowed,
/// an optional 0x prefix.
const FINGERPRINT_HEX_DIGITS: usize = 40;

pub(crate) fn check_pgp_keys(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::parse;

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
}
