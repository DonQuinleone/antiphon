//! The client's side of the antiphond socket: queued ops,
//! outbox nudges and config reloads, all with short timeouts
//! so the UI never hangs on a busy daemon.

use std::time::Duration;

use antiphon_ipc::{
    IpcClient, OpId, OpKind, Operation, Request, Response, socket_path,
};

use super::IPC_WAIT;
use super::actions::OpIntent;
use super::app::App;

const DAEMON_ASSIGNS_ID: u64 = 0;

pub(super) fn drain_ops(app: &mut App) {
    // A read-only session must never hand ops to a daemon
    // that may be serving the real accounts.
    if app.read_only {
        app.pending_ops.clear();
        return;
    }
    if app.pending_ops.is_empty() {
        return;
    }
    let Some(mut client) = connect() else {
        return;
    };
    while let Some(intent) = app.pending_ops.first().cloned() {
        let request = Request::EnqueueOp(wire_op(intent));
        let Ok(_) = client.request(&request) else {
            return;
        };
        app.pending_ops.remove(0);
    }
}

pub(super) fn nudge_daemon() {
    let Some(mut client) = connect() else {
        return;
    };
    let _ = client.request(&Request::DrainOutbox);
}

fn connect() -> Option<IpcClient> {
    let path = socket_path(|var| std::env::var_os(var));
    let client = IpcClient::connect(&path).ok()?;
    let _ = client.set_read_timeout(IPC_WAIT);
    Some(client)
}

/// A reload restarts the daemon's IDLE watchers, which can
/// take a few seconds to wind down; the usual IPC timeout
/// would misread that as a failure.
const RELOAD_WAIT: Duration = Duration::from_secs(10);

/// Asks antiphond to re-read configuration after an account
/// change. `None` means the daemon took it; `Some` carries the
/// message the caller should show instead.
pub(super) fn request_reload() -> Option<String> {
    let path = socket_path(|var| std::env::var_os(var));
    let Ok(mut client) = IpcClient::connect(&path) else {
        return Some(
            "antiphond is not running; it reads the change when \
             it starts"
                .to_string(),
        );
    };
    let _ = client.set_read_timeout(RELOAD_WAIT);
    match client.request(&Request::Reload) {
        Ok(Response::Ack) => None,
        Ok(Response::Error(error)) => Some(format!("reload: {error}")),
        Ok(_) => Some("reload: unexpected daemon reply".to_string()),
        Err(error) => Some(format!("reload: {error}")),
    }
}

fn wire_op(intent: OpIntent) -> Operation {
    let (account, message_id, kind) = match intent {
        OpIntent::Flag {
            account,
            message_id,
            add,
            remove,
        } => (account, message_id, OpKind::Flag { add, remove }),
        OpIntent::Delete {
            account,
            message_id,
        } => (account, message_id, OpKind::Delete),
        OpIntent::Move {
            account,
            message_id,
            to_folder,
            from_folder,
        } => (
            account,
            message_id,
            OpKind::Move {
                to_folder,
                from_folder: Some(from_folder),
            },
        ),
    };
    Operation {
        op_id: OpId(DAEMON_ASSIGNS_ID),
        account,
        message_id,
        kind,
    }
}
