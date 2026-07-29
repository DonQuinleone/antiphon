use std::process::Command;

use antiphon_oauth::{TokenStore, refresh};
use antiphon_store::Outbox;
use antiphon_sync::{
    SmtpAccount, SyncAccount, SyncError, append_sent, send,
};

use antiphon_config::GraphAuth;
use secrecy::SecretString;

use crate::accounts::{AccountSet, GraphSend, OauthAccount};
use crate::mailflow::{Mailflow, error_chain, now_unix};
use crate::tokens;

/// A send failure with its retry verdict attached: permanent
/// means no later drain can succeed without the user changing
/// something, so the message leaves the retry queue.
pub(crate) struct ShipError {
    pub(crate) detail: String,
    pub(crate) permanent: bool,
}

impl ShipError {
    pub(crate) fn transient(
        detail: impl std::fmt::Display,
    ) -> ShipError {
        ShipError {
            detail: detail.to_string(),
            permanent: false,
        }
    }
}

/// A rejected message (5xx) or one the server can never parse
/// will fail identically on every retry; connection trouble
/// and 4xx throttling deserve another pass.
fn smtp_permanent(error: &SyncError) -> bool {
    match error {
        SyncError::Smtp { source, .. } => source.is_permanent(),
        SyncError::SmtpMessage { .. } => true,
        _ => false,
    }
}

impl Mailflow {
    pub(crate) fn drain_outbox(&self) {
        self.drain_outbox_of(&self.snapshot());
    }

    pub(crate) fn drain_outbox_of(&self, set: &AccountSet) {
        let outbox = Outbox::open(&self.layout);
        let pending = match outbox.pending() {
            Ok(pending) => pending,
            Err(error) => {
                eprintln!("outbox: {error}");
                return;
            }
        };
        let now = now_unix();
        for queued in pending {
            if !due(&queued.envelope, now) {
                continue;
            }
            self.send_queued(set, &outbox, queued);
        }
    }

    fn send_queued(
        &self,
        set: &AccountSet,
        outbox: &Outbox,
        queued: antiphon_store::QueuedMessage,
    ) {
        let account = queued.envelope.account.clone();
        let raw = match std::fs::read(&queued.message_path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("outbox {}: {error}", queued.id);
                return;
            }
        };
        if let Err(error) = self.ship(set, &queued.envelope, &raw) {
            self.settle_failure(outbox, queued.id, &error);
            return;
        }
        if let Err(error) = self.file_sent(&account, &raw) {
            eprintln!("sent copy {}: {error}", queued.id);
        }
        self.file_sent_on_server(set, &account, &raw);
        if let Err(error) = outbox.remove(queued.id) {
            eprintln!("outbox {}: {error}", queued.id);
            return;
        }
        println!("sent outbox message {}", queued.id);
    }

    /// A transient failure stays queued for the next drain; a
    /// permanent one moves aside into outbox/dead, so a message
    /// no server will ever take stops retrying forever.
    fn settle_failure(
        &self,
        outbox: &Outbox,
        queued_id: u64,
        error: &ShipError,
    ) {
        if !error.permanent {
            eprintln!("send {queued_id}: {}; will retry", error.detail);
            return;
        }
        match outbox.reject(queued_id) {
            Ok(()) => eprintln!(
                "send {queued_id}: {}; giving up, message moved \
                 to outbox/dead",
                error.detail
            ),
            Err(reject_error) => eprintln!(
                "send {queued_id}: {}; also failed to set it \
                 aside: {reject_error}",
                error.detail
            ),
        }
    }

    /// One message leaves the machine here: through Graph
    /// when the account opted in, else through SMTP. The queued
    /// envelope carries the true recipient list, Bcc included.
    fn ship(
        &self,
        set: &AccountSet,
        envelope: &antiphon_store::Envelope,
        raw: &[u8],
    ) -> Result<(), ShipError> {
        let account = &envelope.account;
        if let Some(spec) = set.graph_spec(account) {
            let graph = spec
                .graph
                .as_ref()
                .expect("graph_spec only returns graph senders");
            let token =
                self.graph_token(spec).map_err(ShipError::transient)?;
            let sender = matches!(graph.auth, GraphAuth::AppOnly)
                .then_some(envelope.from.as_str());
            let upload = crate::graph::with_envelope_bcc(
                raw,
                &envelope.recipients,
            );
            return crate::graph::send_raw(
                &token,
                &crate::graph::sendmail_url(sender),
                &upload,
            );
        }
        let Some(smtp) = self.smtp_for(set, account) else {
            return Err(ShipError {
                detail: format!("no smtp account for {account}"),
                permanent: true,
            });
        };
        send(&smtp, &envelope.from, &envelope.recipients, raw).map_err(
            |error| ShipError {
                permanent: smtp_permanent(&error),
                detail: error_chain(&error),
            },
        )
    }

    /// Best-effort mirror of the local sent twin into the
    /// server's own sent folder; Graph accounts skip it since
    /// Graph files Sent Items itself.
    fn file_sent_on_server(
        &self,
        set: &AccountSet,
        account: &str,
        raw: &[u8],
    ) {
        if set.graph_spec(account).is_some() {
            return;
        }
        let Some(sync_account) = self.imap_account(set, account) else {
            return;
        };
        match append_sent(&sync_account, raw) {
            Ok(folder) => {
                println!("sent copy filed on the server ({folder})")
            }
            Err(error) => {
                eprintln!("server sent copy {account}: {error}")
            }
        }
    }

    fn imap_account(
        &self,
        set: &AccountSet,
        account: &str,
    ) -> Option<SyncAccount> {
        if let Some(found) = set
            .accounts
            .iter()
            .find(|candidate| candidate.name == account)
        {
            return Some(found.clone());
        }
        let spec = set
            .oauth
            .iter()
            .find(|candidate| candidate.name == account)?;
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return None;
            }
        };
        Some(spec.sync_account(token))
    }

    fn graph_token(
        &self,
        spec: &OauthAccount,
    ) -> Result<String, String> {
        let graph = spec
            .graph
            .as_ref()
            .ok_or(format!("{}: not a graph sender", spec.name))?;
        if matches!(graph.auth, GraphAuth::AppOnly) {
            return app_only_graph_token(&spec.name, graph);
        }
        let store = TokenStore::open(self.layout.tokens_dir())
            .map_err(|error| {
                format!("{}: token store: {error}", spec.name)
            })?;
        tokens::access_token(
            &store,
            &antiphon_oauth::graph_grant(&spec.name),
            &spec.name,
            now_unix(),
            &refresh,
        )
    }

    fn smtp_for(
        &self,
        set: &AccountSet,
        account: &str,
    ) -> Option<SmtpAccount> {
        let stored = set
            .smtp
            .iter()
            .find(|(name, _)| name == account)
            .map(|(_, smtp)| smtp.clone());
        if stored.is_some() {
            return stored;
        }
        let spec = set
            .oauth
            .iter()
            .find(|spec| spec.name == account && spec.smtp.is_some())?;
        let token = match self.oauth_token(spec, false) {
            Ok(token) => token,
            Err(message) => {
                eprintln!("{message}");
                return None;
            }
        };
        spec.smtp_account(token)
    }

    fn file_sent(
        &self,
        account: &str,
        raw: &[u8],
    ) -> std::io::Result<()> {
        let sent =
            self.layout.account_maildir(account).join("sent/cur");
        std::fs::create_dir_all(&sent)?;
        let name = format!(
            "{}.P{}.antiphon:2,S",
            now_unix(),
            std::process::id()
        );
        std::fs::write(sent.join(name), raw)?;
        let status = Command::new("notmuch")
            .arg("new")
            .env("NOTMUCH_CONFIG", self.layout.notmuch_config_path())
            .output()?;
        if !status.status.success() {
            eprintln!("notmuch new failed after sent copy");
        }
        // new.tags stamps every indexed message unread+inbox; a
        // sent copy is neither.
        let retag = Command::new("notmuch")
            .args([
                "tag",
                "+sent",
                "-inbox",
                "-unread",
                "--",
                &format!("path:{account}/sent/**"),
            ])
            .env("NOTMUCH_CONFIG", self.layout.notmuch_config_path())
            .output()?;
        if !retag.status.success() {
            eprintln!("retagging the sent copy failed");
        }
        Ok(())
    }
}

/// App-only tokens are fetched fresh per send: sends are rare,
/// the grant has no refresh token, and the secret never rests
/// anywhere but the command that prints it.
fn app_only_graph_token(
    account: &str,
    graph: &GraphSend,
) -> Result<String, String> {
    use secrecy::ExposeSecret;

    let tenant = graph.tenant.as_deref().ok_or(format!(
        "{account}: [graph] auth = \"app_only\" needs a tenant"
    ))?;
    let client_id = graph.client_id.as_deref().ok_or(format!(
        "{account}: [graph] app_only needs a client_id \
         (in [graph] or [oauth])"
    ))?;
    let command = graph.secret_cmd.as_deref().ok_or(format!(
        "{account}: [graph] app_only needs secret_cmd"
    ))?;
    let secret = crate::accounts::resolve_password(command).ok_or(
        format!("{account}: secret_cmd produced no client secret"),
    )?;
    let tokens = antiphon_oauth::app_only_token(
        tenant,
        client_id,
        &SecretString::from(secret),
    )
    .map_err(|error| {
        format!("{account}: app-only graph token: {error}")
    })?;
    Ok(tokens.access_token.expose_secret().to_string())
}

/// A message with no schedule, or one whose time has passed,
/// is due now; a future-dated one waits in the outbox.
fn due(envelope: &antiphon_store::Envelope, now: u64) -> bool {
    match envelope.send_after {
        Some(at) => at <= now,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use antiphon_store::Envelope;

    use super::due;

    fn envelope(send_after: Option<u64>) -> Envelope {
        Envelope {
            account: "personal".to_string(),
            from: "quin@example.com".to_string(),
            recipients: vec!["mara@example.com".to_string()],
            send_after,
        }
    }

    #[test]
    fn scheduling_holds_a_message_until_its_time() {
        assert!(due(&envelope(None), 100), "unscheduled sends now");
        assert!(due(&envelope(Some(100)), 100), "due at its time");
        assert!(due(&envelope(Some(50)), 100), "overdue sends");
        assert!(!due(&envelope(Some(200)), 100), "a future time waits");
    }
}
