use std::io::Read;
use std::process::ExitCode;

use antiphon_config::{Dirs, Loaded, load};
use antiphon_core::{Addr, ParsedIdentity, reply_identity};
use antiphon_ipc::{IpcClient, Request, socket_path};
use antiphon_store::{Envelope, Outbox, StoreLayout};

use crate::autostart;

/// sysexits.h codes; git send-email only distinguishes zero
/// from non-zero, but the classic meanings help everyone else.
const EX_USAGE: u8 = 64;
const EX_NOUSER: u8 = 67;
const EX_TEMPFAIL: u8 = 75;
const EX_CONFIG: u8 = 78;

struct Invocation {
    envelope_from: Option<String>,
    recipients: Vec<String>,
}

/// A sendmail-compatible entry point for git send-email and
/// friends: flags in the classic shape, recipients as
/// arguments, the message on stdin, exit zero once the mail is
/// durably queued for the daemon to send.
pub fn run(args: &[String]) -> ExitCode {
    let invocation = match parse_args(args) {
        Ok(invocation) => invocation,
        Err(message) => return fail(EX_USAGE, &message),
    };
    let mut raw = Vec::new();
    if let Err(error) = std::io::stdin().read_to_end(&mut raw) {
        return fail(EX_TEMPFAIL, &format!("reading stdin: {error}"));
    }
    let Some(dirs) = Dirs::from_process() else {
        return fail(EX_CONFIG, "cannot resolve the home directory");
    };
    let loaded = match load(&dirs) {
        Ok(loaded) => loaded,
        Err(error) => return fail(EX_CONFIG, &error.to_string()),
    };
    let from = match &invocation.envelope_from {
        Some(from) => from.clone(),
        None => match from_header(&raw) {
            Some(from) => from,
            None => {
                return fail(
                    EX_USAGE,
                    "no -f and no From header in the message",
                );
            }
        },
    };
    let Some(account) = account_for(&loaded, &from) else {
        return fail(
            EX_NOUSER,
            &format!("no account identity matches {from}"),
        );
    };
    if let Err(error) = autostart::ensure_daemon(true, &dirs) {
        return fail(EX_TEMPFAIL, &error);
    }
    let layout = StoreLayout::new(dirs.store_root());
    if !layout.exists() {
        return fail(
            EX_TEMPFAIL,
            "the store is unavailable (vault sealed?)",
        );
    }
    let envelope = Envelope {
        account,
        from,
        recipients: invocation.recipients,
        send_after: None,
    };
    if let Err(error) = Outbox::open(&layout).enqueue(&envelope, &raw) {
        return fail(EX_TEMPFAIL, &error.to_string());
    }
    drain_outbox();
    ExitCode::SUCCESS
}

fn fail(code: u8, message: &str) -> ExitCode {
    eprintln!("antiphon sendmail: {message}");
    ExitCode::from(code)
}

fn parse_args(args: &[String]) -> Result<Invocation, String> {
    let mut envelope_from = None;
    let mut recipients = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-i" | "-oi" | "-t" | "--" => {}
            "-f" | "-F" => {
                let value = rest
                    .next()
                    .ok_or(format!("{arg} needs a value"))?;
                if arg == "-f" {
                    envelope_from = Some(value.clone());
                }
            }
            flag if flag.starts_with('-') => {
                eprintln!("antiphon sendmail: ignoring {flag}");
            }
            recipient => recipients.push(recipient.to_owned()),
        }
    }
    if recipients.is_empty() {
        return Err("no recipients given".to_owned());
    }
    Ok(Invocation {
        envelope_from,
        recipients,
    })
}

fn from_header(raw: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\n\n").next().unwrap_or(&text);
    let line = head
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("from:"))?;
    extract_address(&line["from:".len()..])
}

fn extract_address(field: &str) -> Option<String> {
    if let (Some(open), Some(close)) =
        (field.find('<'), field.rfind('>'))
        && open < close
    {
        return Some(field[open + 1..close].trim().to_owned());
    }
    field
        .split_whitespace()
        .find(|token| token.contains('@'))
        .map(|token| token.trim_matches([',', ';']).to_owned())
}

fn account_for(loaded: &Loaded, from: &str) -> Option<String> {
    let target = [Addr::new(from)];
    for entry in &loaded.accounts {
        if entry.account.smtp.is_none() {
            continue;
        }
        let identities: Vec<ParsedIdentity> = entry
            .account
            .identities
            .iter()
            .filter_map(|identity| {
                ParsedIdentity::new(
                    &identity.address,
                    identity.name.as_deref(),
                    identity.signature.as_deref(),
                    &identity.matches,
                )
                .ok()
            })
            .collect();
        if reply_identity(&identities, &target).is_some() {
            return Some(entry.account.account.name.clone());
        }
    }
    None
}

/// Hand the queue to the daemon and wait for the drain, the
/// way a sendmail is expected to block until handover. The
/// message is already durable, so a failed drain only means
/// the sync timer delivers it instead.
fn drain_outbox() {
    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        eprintln!(
            "antiphon sendmail: queued; the daemon will send \
             on its next pass"
        );
        return;
    };
    if client.request(&Request::DrainOutbox).is_err() {
        eprintln!(
            "antiphon sendmail: queued; the daemon will send \
             on its next pass"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_flags_parse_and_unknowns_are_skipped() {
        let args: Vec<String> = [
            "-i",
            "-f",
            "env@example.com",
            "-oi",
            "-oem",
            "a@example.com",
            "b@example.com",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let parsed = parse_args(&args).unwrap();
        assert_eq!(
            parsed.envelope_from.as_deref(),
            Some("env@example.com")
        );
        assert_eq!(
            parsed.recipients,
            ["a@example.com", "b@example.com"]
        );
    }

    #[test]
    fn no_recipients_is_a_usage_error() {
        let args = vec!["-i".to_string()];
        assert!(parse_args(&args).is_err());
    }

    #[test]
    fn from_header_extraction_handles_both_shapes() {
        let cases: [(&[u8], Option<&str>); 3] = [
            (
                b"From: Alice <alice@example.com>\nTo: b\n\nbody",
                Some("alice@example.com"),
            ),
            (b"From: bob@example.com\n\nbody", Some("bob@example.com")),
            (b"To: nobody\n\nFrom: not-a-header", None),
        ];
        for (raw, expected) in cases {
            assert_eq!(
                from_header(raw).as_deref(),
                expected,
                "{raw:?}"
            );
        }
    }
}
