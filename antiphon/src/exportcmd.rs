use std::path::{Path, PathBuf};
use std::process::ExitCode;

use antiphon_config::{Dirs, Loaded, load};
use antiphon_store::StoreLayout;

use crate::export::{
    ExportKey, archive_file_name, export_account, parse_recipients,
};

pub struct ExportArgs<'a> {
    pub account: Option<&'a str>,
    pub output: &'a Path,
    pub recipients: &'a [String],
    pub passphrase: bool,
}

pub fn run(args: &ExportArgs) -> ExitCode {
    match run_export(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_export(args: &ExportArgs) -> Result<(), String> {
    let key = resolve_key(args)?;
    let (dirs, loaded) = load_config()?;
    let layout = StoreLayout::new(dirs.store_root());
    let accounts = selected_accounts(&loaded, args.account)?;
    let single = args.account.is_some();
    for account in &accounts {
        let dest = destination(args.output, account, single)?;
        let maildir = layout.account_maildir(account);
        let summary = export_account(&maildir, account, &dest, &key)
            .map_err(|err| err.to_string())?;
        println!("{}", summary.line());
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum KeyMode {
    Recipients,
    Passphrase,
}

fn key_mode(
    recipients: &[String],
    passphrase: bool,
) -> Result<KeyMode, String> {
    match (recipients.is_empty(), passphrase) {
        (false, false) => Ok(KeyMode::Recipients),
        (true, true) => Ok(KeyMode::Passphrase),
        (false, true) => Err("use either --recipient or \
             --passphrase, not both"
            .to_string()),
        (true, false) => Err("choose how to encrypt: repeat \
             -r/--recipient <age public key>, or -p/--passphrase \
             to be prompted for one"
            .to_string()),
    }
}

fn resolve_key(args: &ExportArgs) -> Result<ExportKey, String> {
    match key_mode(args.recipients, args.passphrase)? {
        KeyMode::Recipients => parse_recipients(args.recipients)
            .map(ExportKey::Recipients)
            .map_err(|err| err.to_string()),
        KeyMode::Passphrase => prompt_passphrase(),
    }
}

fn prompt_passphrase() -> Result<ExportKey, String> {
    let read = |label| {
        rpassword::prompt_password(label)
            .map_err(|err| format!("cannot read passphrase: {err}"))
    };
    let first = read("export passphrase: ")?;
    if first.is_empty() {
        return Err("the passphrase must not be empty".to_string());
    }
    let again = read("repeat passphrase: ")?;
    if first != again {
        return Err("the passphrases do not match".to_string());
    }
    Ok(ExportKey::Passphrase(first.into()))
}

fn selected_accounts(
    loaded: &Loaded,
    wanted: Option<&str>,
) -> Result<Vec<String>, String> {
    let names: Vec<String> = loaded
        .accounts
        .iter()
        .map(|entry| entry.file_stem.clone())
        .collect();
    if names.is_empty() {
        return Err("no accounts configured; add one under \
             accounts/ first"
            .to_string());
    }
    let Some(wanted) = wanted else {
        return Ok(names);
    };
    if names.iter().any(|name| name == wanted) {
        return Ok(vec![wanted.to_string()]);
    }
    Err(format!(
        "no account named {wanted}; configured accounts: {}",
        names.join(", ")
    ))
}

/// An explicitly named account may target a file directly;
/// everything else treats the output as a directory and names
/// the archives.
fn destination(
    output: &Path,
    account: &str,
    single: bool,
) -> Result<PathBuf, String> {
    if single && !output.is_dir() {
        return Ok(output.to_path_buf());
    }
    std::fs::create_dir_all(output).map_err(|err| {
        format!(
            "cannot create output directory {}: {err}",
            output.display()
        )
    })?;
    Ok(output.join(archive_file_name(account)))
}

fn load_config() -> Result<(Dirs, Loaded), String> {
    let dirs = Dirs::from_process()
        .ok_or("cannot resolve the home directory")?;
    let loaded = load(&dirs).map_err(|err| err.to_string())?;
    Ok((dirs, loaded))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|key| (*key).to_string()).collect()
    }

    #[test]
    fn key_mode_demands_exactly_one_choice() {
        let recipient = keys(&["age1example"]);
        assert_eq!(
            key_mode(&recipient, false),
            Ok(KeyMode::Recipients)
        );
        assert_eq!(key_mode(&[], true), Ok(KeyMode::Passphrase));
        let neither = key_mode(&[], false).unwrap_err();
        assert!(neither.contains("choose how to encrypt"));
        let both = key_mode(&recipient, true).unwrap_err();
        assert!(both.contains("not both"));
    }

    #[test]
    fn a_single_account_may_target_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("work.tar.gz.age");
        assert_eq!(destination(&file, "work", true).unwrap(), file);
    }

    #[test]
    fn directories_get_dated_archive_names() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("exports");
        let dest = destination(&out, "work", false).unwrap();
        assert!(out.is_dir(), "the directory is created");
        assert_eq!(
            dest,
            out.join(archive_file_name("work")),
            "archives are named per account and date"
        );
        let into_existing = destination(&out, "work", true).unwrap();
        assert_eq!(into_existing, dest, "existing dir wins");
    }
}
