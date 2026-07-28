use std::path::{Path, PathBuf};
use std::process::ExitCode;

use antiphon_config::{
    Account, AccountFile, Config, Dirs, Imap, Loaded, NamedAccount,
};
use antiphon_store::StoreLayout;

use crate::view::{Opened, ViewKey, archive_stem, open_archive};

const VIEW_CACHE_DIR: &str = "view";

pub struct ViewArgs<'a> {
    pub archive: &'a Path,
    pub identities: &'a [PathBuf],
    pub passphrase: bool,
}

pub fn run(args: &ViewArgs) -> ExitCode {
    match run_view(args) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_view(args: &ViewArgs) -> Result<ExitCode, String> {
    let key = resolve_key(args)?;
    if !args.archive.is_file() {
        return Err(format!(
            "no archive at {}",
            args.archive.display()
        ));
    }
    let dirs = Dirs::from_process()
        .ok_or("cannot resolve the home directory")?;
    let account = archive_stem(args.archive);
    let store_root = dirs.cache.join(VIEW_CACHE_DIR).join(&account);
    let opened =
        open_archive(args.archive, &store_root, &account, &key)
            .map_err(|err| err.to_string())?;
    match opened {
        Opened::Unpacked { files } => println!(
            "unpacked {files} files into {}",
            store_root.display()
        ),
        Opened::Reused => println!(
            "reusing the unpacked archive at {}",
            store_root.display()
        ),
    }
    let layout = StoreLayout::new(&store_root);
    let loaded = synthetic_loaded(&account);
    let code = crate::tui::run(&loaded, &layout, &dirs, true);
    println!(
        "the unpacked archive stays at {}\n\
         remove it with: rm -r {}",
        store_root.display(),
        store_root.display()
    );
    Ok(code)
}

#[derive(Debug, PartialEq, Eq)]
enum KeyMode {
    Identities,
    Passphrase,
}

fn key_mode(
    identities: &[PathBuf],
    passphrase: bool,
) -> Result<KeyMode, String> {
    match (identities.is_empty(), passphrase) {
        (false, false) => Ok(KeyMode::Identities),
        (true, true) => Ok(KeyMode::Passphrase),
        (false, true) => Err("use either --identity or \
             --passphrase, not both"
            .to_string()),
        (true, false) => Err("choose how to decrypt: repeat \
             -i/--identity <age identity file>, or -p/--passphrase \
             to be prompted for one"
            .to_string()),
    }
}

fn resolve_key(args: &ViewArgs) -> Result<ViewKey, String> {
    match key_mode(args.identities, args.passphrase)? {
        KeyMode::Identities => {
            load_identities(args.identities).map(ViewKey::Identities)
        }
        KeyMode::Passphrase => prompt_passphrase(),
    }
}

fn load_identities(
    files: &[PathBuf],
) -> Result<Vec<age::x25519::Identity>, String> {
    let mut identities = Vec::new();
    for file in files {
        identities.extend(identities_in(file)?);
    }
    Ok(identities)
}

/// An age identity file: one AGE-SECRET-KEY-1... per line,
/// with empty lines and # comments ignored.
fn identities_in(
    file: &Path,
) -> Result<Vec<age::x25519::Identity>, String> {
    let text = std::fs::read_to_string(file).map_err(|err| {
        format!("cannot read {}: {err}", file.display())
    })?;
    let keys: Result<Vec<age::x25519::Identity>, String> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            line.parse().map_err(|reason: &str| {
                format!("bad identity in {}: {reason}", file.display())
            })
        })
        .collect();
    let keys = keys?;
    if keys.is_empty() {
        return Err(format!("no age identities in {}", file.display()));
    }
    Ok(keys)
}

fn prompt_passphrase() -> Result<ViewKey, String> {
    let passphrase = rpassword::prompt_password("archive passphrase: ")
        .map_err(|err| format!("cannot read passphrase: {err}"))?;
    if passphrase.is_empty() {
        return Err("the passphrase must not be empty".to_string());
    }
    Ok(ViewKey::Passphrase(passphrase.into()))
}

/// A single synthetic account named after the archive, with no
/// servers and no identities: enough for the client to scope,
/// list and read, while compose paths find nothing to send
/// with.
fn synthetic_loaded(account: &str) -> Loaded {
    Loaded {
        config: Config::default(),
        accounts: vec![NamedAccount {
            file_stem: account.to_string(),
            account: AccountFile {
                account: Account {
                    name: account.to_string(),
                    maildir: None,
                    archive: None,
                    trash: None,
                },
                imap: Imap {
                    host: String::new(),
                    port: None,
                    user: String::new(),
                    password_cmd: None,
                },
                smtp: None,
                identities: Vec::new(),
                rules: Vec::new(),
                oauth: None,
                graph: None,
                folder_names: Default::default(),
                folder_order: Vec::new(),
                folders_hidden: Vec::new(),
            },
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_mode_demands_exactly_one_choice() {
        let identity = vec![PathBuf::from("key.txt")];
        assert_eq!(key_mode(&identity, false), Ok(KeyMode::Identities));
        assert_eq!(key_mode(&[], true), Ok(KeyMode::Passphrase));
        let neither = key_mode(&[], false).unwrap_err();
        assert!(neither.contains("choose how to decrypt"));
        let both = key_mode(&identity, true).unwrap_err();
        assert!(both.contains("not both"));
    }

    #[test]
    fn identity_files_parse_keys_and_skip_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.txt");
        let generated = age::x25519::Identity::generate();
        use age::secrecy::ExposeSecret;
        std::fs::write(
            &path,
            format!(
                "# created today\n\n{}\n",
                generated.to_string().expose_secret()
            ),
        )
        .unwrap();
        let keys = load_identities(&[path]).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(
            keys[0].to_public().to_string(),
            generated.to_public().to_string()
        );
    }

    #[test]
    fn a_keyless_identity_file_names_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.txt");
        std::fs::write(&path, "# nothing here\n").unwrap();
        let error = load_identities(&[path])
            .map(|keys| keys.len())
            .unwrap_err();
        assert!(error.contains("no age identities"), "{error}");
        assert!(error.contains("empty.txt"), "{error}");

        std::fs::write(dir.path().join("bad.txt"), "not-a-key\n")
            .unwrap();
        let error = load_identities(&[dir.path().join("bad.txt")])
            .map(|keys| keys.len())
            .unwrap_err();
        assert!(error.contains("bad identity"), "{error}");
        assert!(error.contains("bad.txt"), "{error}");
    }

    #[test]
    fn the_synthetic_account_is_named_after_the_archive() {
        let loaded = synthetic_loaded("work-2026-07-28");
        assert_eq!(loaded.accounts.len(), 1);
        let entry = &loaded.accounts[0];
        assert_eq!(entry.file_stem, "work-2026-07-28");
        assert_eq!(entry.account.account.name, "work-2026-07-28");
        assert!(entry.account.smtp.is_none());
        assert!(entry.account.identities.is_empty());
    }
}
