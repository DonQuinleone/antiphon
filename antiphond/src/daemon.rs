use std::collections::HashSet;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use antiphon_config::{Dirs, Loaded, load};
use antiphon_ipc::{
    DaemonStatus, IpcServer, Operation, Request, Response, VaultState,
    read_frame, socket_path, write_frame,
};
use antiphon_store::{
    Op, OpKind, OpLog, Outbox, SearchIndex, StoreLayout, apply_op,
};
use antiphon_sync::{SmtpAccount, SyncAccount, replay, send, sync};

pub fn run() -> ExitCode {
    let Some(dirs) = Dirs::from_process() else {
        eprintln!("cannot resolve the home directory");
        return ExitCode::FAILURE;
    };
    let layout = StoreLayout::new(dirs.store_root());
    if !layout.exists() {
        eprintln!(
            "no message store at {}; run \
             `antiphon doctor --init-store` to create it",
            layout.root().display()
        );
        return ExitCode::FAILURE;
    }
    let loaded = match load(&dirs) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let accounts = sync_accounts(&loaded);
    let smtp = smtp_accounts(&loaded);
    let log = match OpLog::open(&layout) {
        Ok(log) => log,
        Err(error) => {
            eprintln!("oplog: {error}");
            return ExitCode::FAILURE;
        }
    };
    let path = socket_path(|var| std::env::var_os(var));
    let server = match IpcServer::bind(&path) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("cannot bind {}: {error}", path.display());
            return ExitCode::FAILURE;
        }
    };
    println!(
        "antiphond {} listening on {}",
        env!("ANTIPHON_VERSION"),
        path.display()
    );
    let mut daemon = Daemon {
        layout,
        log,
        accounts,
        smtp,
        last_sync_unix: None,
    };
    daemon.sync_pass();
    let interval = sync_interval(&loaded);
    if let Err(error) = serve_with_timer(&server, &mut daemon, interval)
    {
        eprintln!("accept: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

struct Daemon {
    layout: StoreLayout,
    log: OpLog,
    accounts: Vec<SyncAccount>,
    smtp: Vec<(String, SmtpAccount)>,
    last_sync_unix: Option<u64>,
}

fn smtp_accounts(loaded: &Loaded) -> Vec<(String, SmtpAccount)> {
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

const SUBMISSION_PORT: u16 = 587;
const ACCEPT_POLL: Duration = Duration::from_millis(200);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(5);

fn sync_interval(loaded: &Loaded) -> Option<Duration> {
    let minutes = loaded.config.sync.interval_minutes;
    if minutes == 0 {
        return None;
    }
    Some(Duration::from_secs(u64::from(minutes) * 60))
}

fn due(last: Instant, interval: Option<Duration>) -> bool {
    interval.is_some_and(|interval| last.elapsed() >= interval)
}

fn serve_with_timer(
    server: &IpcServer,
    daemon: &mut Daemon,
    interval: Option<Duration>,
) -> std::io::Result<()> {
    server.set_nonblocking(true)?;
    let mut last_sync = Instant::now();
    loop {
        match server.accept() {
            Ok(stream) => {
                stream.set_nonblocking(false)?;
                // A silent client must not starve the accept
                // loop and the sync timer behind it.
                stream.set_read_timeout(Some(CLIENT_READ_TIMEOUT))?;
                daemon.serve_connection(stream);
            }
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock =>
            {
                std::thread::sleep(ACCEPT_POLL);
            }
            Err(error) => return Err(error),
        }
        if due(last_sync, interval) {
            daemon.sync_pass();
            last_sync = Instant::now();
        }
    }
}

fn sync_accounts(loaded: &Loaded) -> Vec<SyncAccount> {
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

const IMAPS_PORT: u16 = 993;

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

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

impl Daemon {
    fn serve_connection(&mut self, mut stream: UnixStream) {
        loop {
            let request: Request = match read_frame(&mut stream) {
                Ok(request) => request,
                Err(_) => return,
            };
            let response = self.respond(request);
            if write_frame(&mut stream, &response).is_err() {
                return;
            }
        }
    }

    fn respond(&mut self, request: Request) -> Response {
        match request {
            Request::Ping => Response::Pong,
            Request::EnqueueOp(operation) => self.enqueue(operation),
            Request::Status => Response::Status(self.status()),
            Request::SyncNow => self.sync_now(),
            Request::Subscribe => Response::Error(
                "events arrive with the sync loop".to_string(),
            ),
        }
    }

    fn enqueue(&mut self, operation: Operation) -> Response {
        let kind = store_kind(operation.kind);
        let op = match self.log.append(
            &operation.account,
            &operation.message_id,
            kind,
        ) {
            Ok(op) => op,
            Err(error) => {
                return Response::Error(error.to_string());
            }
        };
        let response = self.apply(&op);
        if response == Response::Ack {
            self.drain_ops();
        }
        response
    }

    fn apply(&mut self, op: &antiphon_store::Op) -> Response {
        let index = match SearchIndex::open(&self.layout) {
            Ok(index) => index,
            Err(error) => {
                return Response::Error(error.to_string());
            }
        };
        if let Err(error) = apply_op(&self.layout, &index, op) {
            return Response::Error(error.to_string());
        }
        if let Err(error) = self.log.mark_applied(op.id) {
            return Response::Error(error.to_string());
        }
        Response::Ack
    }

    fn drain_outbox(&mut self) {
        let outbox = Outbox::open(&self.layout);
        let pending = match outbox.pending() {
            Ok(pending) => pending,
            Err(error) => {
                eprintln!("outbox: {error}");
                return;
            }
        };
        for queued in pending {
            self.send_queued(&outbox, queued);
        }
    }

    fn send_queued(
        &self,
        outbox: &Outbox,
        queued: antiphon_store::QueuedMessage,
    ) {
        let account = queued.envelope.account.clone();
        let Some((_, smtp)) =
            self.smtp.iter().find(|(name, _)| *name == account)
        else {
            eprintln!(
                "outbox {}: no smtp account for {account}",
                queued.id
            );
            return;
        };
        let raw = match std::fs::read(&queued.message_path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!("outbox {}: {error}", queued.id);
                return;
            }
        };
        if let Err(error) = send(smtp, &raw) {
            eprintln!("send {}: {error}", queued.id);
            return;
        }
        if let Err(error) = self.file_sent(&account, &raw) {
            eprintln!("sent copy {}: {error}", queued.id);
        }
        if let Err(error) = outbox.remove(queued.id) {
            eprintln!("outbox {}: {error}", queued.id);
            return;
        }
        println!("sent outbox message {}", queued.id);
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

    fn sync_all(&mut self) -> usize {
        let mut failures = 0;
        for account in &self.accounts {
            match sync(account, &self.layout) {
                Ok(report) => println!(
                    "synced {}: {} new, {} updated",
                    account.name,
                    report.total_new(),
                    report.total_updated(),
                ),
                Err(error) => {
                    failures += 1;
                    eprintln!(
                        "sync {}: {}",
                        account.name,
                        error_chain(&error)
                    );
                }
            }
        }
        self.last_sync_unix = Some(now_unix());
        failures
    }

    fn sync_now(&mut self) -> Response {
        let failures = self.sync_pass();
        if failures == 0 {
            return Response::Ack;
        }
        Response::Error(format!(
            "sync failed for {failures} of {} accounts",
            self.accounts.len()
        ))
    }

    fn sync_pass(&mut self) -> usize {
        self.drain_outbox();
        let failures = self.sync_all();
        self.drain_ops();
        failures
    }

    /// Replays unsynced ops per account and advances the synced
    /// cursor over the resolved prefix. Synced and dropped ops
    /// are resolved (dropped means the server won and the op is
    /// discarded); unsupported ops stay pending and hold the
    /// cursor, since mark_synced covers everything below it.
    fn drain_ops(&mut self) {
        let pending = self.log.unsynced();
        if pending.is_empty() {
            return;
        }
        let mut resolved = HashSet::new();
        for account in &self.accounts {
            let ops: Vec<Op> = pending
                .iter()
                .filter(|op| op.account == account.name)
                .cloned()
                .collect();
            if ops.is_empty() {
                continue;
            }
            match replay(account, &self.layout, &ops) {
                Ok(report) => {
                    println!(
                        "replayed {}: {} synced, {} dropped, \
                         {} deferred",
                        account.name,
                        report.synced.len(),
                        report.dropped.len(),
                        report.unsupported.len(),
                    );
                    if !report.dropped.is_empty() {
                        eprintln!(
                            "replay {}: server wins, dropped \
                             ops {:?}",
                            account.name, report.dropped
                        );
                    }
                    resolved.extend(report.synced);
                    resolved.extend(report.dropped);
                }
                Err(error) => {
                    eprintln!("replay {}: {error}", account.name);
                }
            }
        }
        let mut cursor = None;
        for op in &pending {
            if !resolved.contains(&op.id) {
                break;
            }
            cursor = Some(op.id);
        }
        let Some(id) = cursor else {
            return;
        };
        if let Err(error) = self.log.mark_synced(id) {
            eprintln!("oplog: {error}");
        }
    }

    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            version: env!("ANTIPHON_VERSION").to_string(),
            vault: VaultState::Open,
            last_sync_unix: self.last_sync_unix,
            pending_ops: self.log.unsynced().len() as u64,
        }
    }
}

fn store_kind(kind: antiphon_ipc::OpKind) -> OpKind {
    match kind {
        antiphon_ipc::OpKind::Flag { add, remove } => {
            OpKind::Flag { add, remove }
        }
        antiphon_ipc::OpKind::Move { to_folder } => {
            OpKind::Move { to_folder }
        }
        antiphon_ipc::OpKind::Delete => OpKind::Delete,
    }
}

#[cfg(test)]
mod timer_tests {
    use super::*;

    #[test]
    fn zero_interval_disables_the_timer() {
        assert!(!due(Instant::now(), None));
    }

    #[test]
    fn elapsed_interval_is_due() {
        let past =
            Instant::now() - Duration::from_secs(SIXTY_ONE_SECONDS);
        assert!(due(past, Some(Duration::from_secs(60))));
        assert!(!due(Instant::now(), Some(Duration::from_secs(60))));
    }

    const SIXTY_ONE_SECONDS: u64 = 61;
}

#[cfg(test)]
mod tests {
    use super::*;
    use antiphon_ipc::OpKind as WireKind;

    #[test]
    fn wire_kinds_map_onto_store_kinds() {
        let flag = store_kind(WireKind::Flag {
            add: vec!["flagged".to_string()],
            remove: Vec::new(),
        });
        assert!(matches!(flag, OpKind::Flag { .. }));
        let moved = store_kind(WireKind::Move {
            to_folder: "archive".to_string(),
        });
        assert!(
            matches!(moved, OpKind::Move { to_folder } if to_folder == "archive")
        );
        assert!(matches!(store_kind(WireKind::Delete), OpKind::Delete));
    }
}
