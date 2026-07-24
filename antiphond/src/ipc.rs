use std::os::unix::net::UnixStream;

use antiphon_ipc::{
    DaemonStatus, Operation, Request, Response, read_frame, write_frame,
};
use antiphon_store::{OpKind, SearchIndex, apply_op};

use crate::daemon::Daemon;

impl Daemon {
    pub(crate) fn serve_connection(&mut self, mut stream: UnixStream) {
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
            Request::DrainOutbox => {
                self.drain_outbox();
                Response::Ack
            }
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

    fn status(&self) -> DaemonStatus {
        DaemonStatus {
            version: env!("ANTIPHON_VERSION").to_string(),
            vault: self.vault,
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
mod tests {
    use antiphon_ipc::OpKind as WireKind;

    use super::*;

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
