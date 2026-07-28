use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum Request {
    Ping,
    SyncNow,
    Reload,
    DrainOutbox,
    Unsubscribe { url: String },
    EnqueueOp(Operation),
    Status,
    Subscribe,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Ack,
    Status(DaemonStatus),
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "body", rename_all = "snake_case")]
pub enum Event {
    SyncStarted,
    SyncFinished(SyncSummary),
    NewMail(NewMailNotice),
    OpApplied(OpId),
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize,
)]
pub struct OpId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub op_id: OpId,
    pub account: String,
    pub message_id: String,
    pub kind: OpKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OpKind {
    Flag {
        add: Vec<String>,
        remove: Vec<String>,
    },
    Move {
        to_folder: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from_folder: Option<String>,
    },
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultState {
    Open,
    Sealed,
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub vault: VaultState,
    pub last_sync_unix: Option<u64>,
    pub pending_ops: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSummary {
    pub account: String,
    pub new_messages: u64,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewMailNotice {
    pub account: String,
    pub folder: String,
    pub count: u64,
}

#[cfg(test)]
mod tests {
    use std::fmt::Debug;

    use serde::de::DeserializeOwned;

    use super::*;

    fn round_trips<T>(value: T)
    where
        T: Serialize + DeserializeOwned + PartialEq + Debug,
    {
        let json = serde_json::to_string(&value).unwrap();
        let back: T = serde_json::from_str(&json).unwrap();
        assert_eq!(back, value, "via {json}");
    }

    fn operation(kind: OpKind) -> Operation {
        Operation {
            op_id: OpId(41),
            account: "example".into(),
            message_id: "<one@example.com>".into(),
            kind,
        }
    }

    fn status() -> DaemonStatus {
        DaemonStatus {
            version: "0.0.0".into(),
            vault: VaultState::Open,
            last_sync_unix: Some(1_700_000_000),
            pending_ops: 3,
        }
    }

    #[test]
    fn requests_round_trip() {
        let requests = [
            Request::Ping,
            Request::SyncNow,
            Request::Reload,
            Request::DrainOutbox,
            Request::EnqueueOp(operation(OpKind::Delete)),
            Request::Status,
            Request::Subscribe,
        ];
        for request in requests {
            round_trips(request);
        }
    }

    #[test]
    fn responses_round_trip() {
        let responses = [
            Response::Pong,
            Response::Ack,
            Response::Status(status()),
            Response::Error("vault locked".into()),
        ];
        for response in responses {
            round_trips(response);
        }
    }

    #[test]
    fn events_round_trip() {
        let events = [
            Event::SyncStarted,
            Event::SyncFinished(SyncSummary {
                account: "example".into(),
                new_messages: 12,
                errors: vec!["timeout".into()],
            }),
            Event::NewMail(NewMailNotice {
                account: "example".into(),
                folder: "INBOX".into(),
                count: 2,
            }),
            Event::OpApplied(OpId(41)),
        ];
        for event in events {
            round_trips(event);
        }
    }

    #[test]
    fn every_operation_kind_round_trips() {
        let kinds = [
            OpKind::Flag {
                add: vec!["flagged".into()],
                remove: vec!["unread".into()],
            },
            OpKind::Move {
                to_folder: "lists/example".into(),
                from_folder: None,
            },
            OpKind::Delete,
        ];
        for kind in kinds {
            round_trips(operation(kind));
        }
    }

    #[test]
    fn a_bare_status_round_trips() {
        round_trips(status());
        round_trips(DaemonStatus {
            last_sync_unix: None,
            ..status()
        });
    }

    #[test]
    fn the_wire_shape_stays_readable() {
        let ping = serde_json::to_string(&Request::Ping).unwrap();
        assert_eq!(ping, r#"{"type":"ping"}"#);
        let kind = OpKind::Move {
            to_folder: "Archive".into(),
            from_folder: None,
        };
        let moved = serde_json::to_string(&kind).unwrap();
        assert_eq!(moved, r#"{"kind":"move","to_folder":"Archive"}"#);
    }
}
