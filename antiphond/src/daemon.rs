use std::ops::ControlFlow;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use antiphon_config::Dirs;
use antiphon_ipc::{
    DaemonStatus, IpcServer, Operation, Request, Response, VaultState,
    read_frame, socket_path, write_frame,
};
use antiphon_store::{
    OpKind, OpLog, SearchIndex, StoreLayout, apply_op,
};

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
    let mut daemon = Daemon { layout, log };
    let outcome = server.serve(|stream| {
        daemon.serve_connection(stream);
        ControlFlow::Continue(())
    });
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("accept: {error}");
            ExitCode::FAILURE
        }
    }
}

struct Daemon {
    layout: StoreLayout,
    log: OpLog,
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
            Request::SyncNow => Response::Error(
                "sync arrives when antiphon-sync lands".to_string(),
            ),
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
        self.apply(&op)
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

    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            version: env!("ANTIPHON_VERSION").to_string(),
            vault: VaultState::Open,
            last_sync_unix: None,
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
