mod client;
mod error;
mod frame;
mod protocol;
mod server;
mod socket;

pub use client::{EventStream, IpcClient};
pub use error::IpcError;
pub use frame::{MAX_FRAME_BYTES, read_frame, write_frame};
pub use protocol::{
    DaemonStatus, Event, NewMailNotice, OpId, OpKind, Operation,
    Request, Response, SyncSummary, VaultState,
};
pub use server::IpcServer;
pub use socket::socket_path;
