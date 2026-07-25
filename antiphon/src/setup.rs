use std::io::{BufRead, Write};
#[cfg(target_os = "macos")]
use std::process::Command;
use std::process::ExitCode;

use antiphon_config::{Dirs, load};
use antiphon_store::StoreLayout;

use crate::autostart;
use crate::vaultcmd;

/// One command from nothing to reading mail: ask for the
/// account, store the secrets, write the config, create the
/// vault, initialise the store, start the daemon.
pub fn run() -> ExitCode {
    let Some(dirs) = Dirs::from_process() else {
        eprintln!("cannot resolve the home directory");
        return ExitCode::FAILURE;
    };
    if dirs.config.join("config.toml").exists() {
        eprintln!(
            "{} already exists; setup only builds a fresh \
             configuration",
            dirs.config.join("config.toml").display()
        );
        return ExitCode::FAILURE;
    }
    match wizard(&dirs) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("setup: {error}");
            ExitCode::FAILURE
        }
    }
}

fn wizard(dirs: &Dirs) -> Result<(), String> {
    println!("Antiphon setup. Enter accepts the [default].\n");
    let address = required("e-mail address", validate_address)?;
    let domain = address.split('@').next_back().unwrap_or_default();
    let name = prompt("account name", "personal")?;
    let imap_host = prompt("imap host", &format!("imap.{domain}"))?;
    let imap_user = prompt("imap user", &address)?;
    let smtp_host = prompt("smtp host", &format!("smtp.{domain}"))?;
    let password_cmd = mail_secret(&name, &address)?;
    let passphrase_cmd = vault_secret()?;

    write_config(dirs, &passphrase_cmd)?;
    write_account(
        dirs,
        &name,
        &address,
        &imap_host,
        &imap_user,
        &smtp_host,
        &password_cmd,
    )?;
    let loaded = load(dirs).map_err(|error| error.to_string())?;
    println!("\nconfiguration written and valid");

    vaultcmd::run_create(dirs, &loaded)?;
    println!("vault created and mounted");
    let layout = StoreLayout::new(dirs.store_root());
    layout
        .init()
        .map_err(|error| format!("initialising the store: {error}"))?;
    println!("store initialised");
    autostart::ensure_daemon(true, dirs)?;
    println!(
        "daemon running; the first sync is under way\n\n\
         run `antiphon` to start reading"
    );
    Ok(())
}

fn prompt(label: &str, default: &str) -> Result<String, String> {
    print!("{label} [{default}]: ");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|error| error.to_string())?;
    let answer = line.trim();
    if answer.is_empty() {
        return Ok(default.to_string());
    }
    Ok(answer.to_string())
}

fn required(
    label: &str,
    validate: fn(&str) -> Result<(), String>,
) -> Result<String, String> {
    loop {
        let answer = prompt(label, "")?;
        if answer.is_empty() {
            println!("{label} is required");
            continue;
        }
        match validate(&answer) {
            Ok(()) => return Ok(answer),
            Err(error) => println!("{error}"),
        }
    }
}

fn validate_address(address: &str) -> Result<(), String> {
    if address.contains('@') && !address.contains(char::is_whitespace) {
        return Ok(());
    }
    Err("that does not look like an e-mail address".to_string())
}

#[cfg(target_os = "macos")]
fn mail_secret(name: &str, address: &str) -> Result<String, String> {
    let service = format!("antiphon-mail-{name}");
    println!(
        "storing the mail password in your Keychain as \
         `{service}`"
    );
    keychain_store(&service, address)?;
    Ok(format!("security find-generic-password -w -s {service}"))
}

#[cfg(target_os = "macos")]
fn vault_secret() -> Result<String, String> {
    println!(
        "choose a vault passphrase; it is stored in your \
         Keychain as `antiphon-vault` and the daemon reads it \
         from there"
    );
    keychain_store("antiphon-vault", &whoami())?;
    Ok("security find-generic-password -w -s antiphon-vault"
        .to_string())
}

/// `security` prompts for the secret itself, hidden, so it
/// never touches our argv or environment.
#[cfg(target_os = "macos")]
fn keychain_store(service: &str, account: &str) -> Result<(), String> {
    let status = Command::new("security")
        .args(["add-generic-password", "-U", "-s", service, "-a"])
        .arg(account)
        .arg("-w")
        .status()
        .map_err(|error| format!("running security: {error}"))?;
    if !status.success() {
        return Err(format!(
            "storing `{service}` in the Keychain failed"
        ));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn whoami() -> String {
    std::env::var("USER").unwrap_or_else(|_| "antiphon".to_string())
}

#[cfg(not(target_os = "macos"))]
fn mail_secret(_name: &str, _address: &str) -> Result<String, String> {
    required(
        "command that prints the mail password \
         (e.g. pass show mail/personal)",
        non_empty,
    )
}

#[cfg(not(target_os = "macos"))]
fn vault_secret() -> Result<String, String> {
    required(
        "command that prints the vault passphrase \
         (e.g. pass show antiphon/vault)",
        non_empty,
    )
}

#[cfg(not(target_os = "macos"))]
fn non_empty(_answer: &str) -> Result<(), String> {
    Ok(())
}

fn write_config(
    dirs: &Dirs,
    passphrase_cmd: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(&dirs.config)
        .map_err(|error| error.to_string())?;
    let text = format!(
        "[vault]\n\
         backend = \"auto\"\n\
         passphrase_cmd = \"{passphrase_cmd}\"\n"
    );
    write_new(&dirs.config.join("config.toml"), &text)
}

#[allow(clippy::too_many_arguments)]
fn write_account(
    dirs: &Dirs,
    name: &str,
    address: &str,
    imap_host: &str,
    imap_user: &str,
    smtp_host: &str,
    password_cmd: &str,
) -> Result<(), String> {
    let accounts = dirs.config.join("accounts");
    std::fs::create_dir_all(&accounts)
        .map_err(|error| error.to_string())?;
    let text = format!(
        "[account]\n\
         name = \"{name}\"\n\n\
         [imap]\n\
         host = \"{imap_host}\"\n\
         user = \"{imap_user}\"\n\
         password_cmd = \"{password_cmd}\"\n\n\
         [smtp]\n\
         host = \"{smtp_host}\"\n\n\
         [[identity]]\n\
         address = \"{address}\"\n\
         match = [\"{address}\"]\n"
    );
    write_new(&accounts.join(format!("{name}.toml")), &text)
}

fn write_new(path: &std::path::Path, text: &str) -> Result<(), String> {
    if path.exists() {
        return Err(format!("{} already exists", path.display()));
    }
    std::fs::write(path, text)
        .map_err(|error| format!("{}: {error}", path.display()))
}
