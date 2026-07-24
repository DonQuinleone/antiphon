use std::process::Command;

use antiphon_config::Loaded;
use antiphon_sync::{SmtpAccount, SyncAccount};

const IMAPS_PORT: u16 = 993;
const SUBMISSION_PORT: u16 = 587;

pub fn sync_accounts(loaded: &Loaded) -> Vec<SyncAccount> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let account = &entry.account;
            let command = account.imap.password_cmd.as_deref()?;
            let password = resolve_password(command)?;
            Some(SyncAccount {
                name: account.account.name.clone(),
                host: account.imap.host.clone(),
                port: account.imap.port.unwrap_or(IMAPS_PORT),
                user: account.imap.user.clone(),
                password,
            })
        })
        .collect()
}

pub fn smtp_accounts(loaded: &Loaded) -> Vec<(String, SmtpAccount)> {
    loaded
        .accounts
        .iter()
        .filter_map(|entry| {
            let account = &entry.account;
            let smtp = account.smtp.as_ref()?;
            let user = smtp
                .user
                .clone()
                .unwrap_or_else(|| account.imap.user.clone());
            let command = smtp
                .password_cmd
                .as_deref()
                .or(account.imap.password_cmd.as_deref())?;
            let password = resolve_password(command)?;
            Some((
                account.account.name.clone(),
                SmtpAccount {
                    host: smtp.host.clone(),
                    port: smtp.port.unwrap_or(SUBMISSION_PORT),
                    user,
                    password,
                },
            ))
        })
        .collect()
}

fn resolve_password(command: &str) -> Option<String> {
    let output =
        Command::new("sh").args(["-c", command]).output().ok()?;
    if !output.status.success() {
        eprintln!("password_cmd failed: {command}");
        return None;
    }
    let password = String::from_utf8_lossy(&output.stdout)
        .trim_end_matches(['\r', '\n'])
        .to_string();
    if password.is_empty() {
        eprintln!("password_cmd produced nothing: {command}");
        return None;
    }
    Some(password)
}
