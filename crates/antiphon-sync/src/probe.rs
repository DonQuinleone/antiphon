//! One-shot connection checks for the account form: reach the
//! server (and, for a password account, authenticate), then log
//! out. Every check is bounded by a timeout so a stalled server
//! cannot hang the caller.

use std::time::Duration;

use crate::engine::SyncAccount;
use crate::error::SyncError;
use crate::session::{
    build_runtime, connect_client, logout, tls_connect,
};

/// Connects and authenticates the account, then logs out. A
/// `Connect`/`Timeout` error means the server was unreachable; a
/// `Login` error means it was reached but rejected the
/// credentials.
pub fn probe_login(
    account: &SyncAccount,
    timeout: Duration,
) -> Result<(), SyncError> {
    let runtime = build_runtime()?;
    let bounded = runtime.block_on(async {
        tokio::time::timeout(timeout, connect_client(account)).await
    });
    let client = match bounded {
        Ok(result) => result?,
        Err(_) => {
            return Err(SyncError::Timeout {
                host: account.host.clone(),
                port: account.port,
            });
        }
    };
    logout(&runtime, client);
    Ok(())
}

/// Reaches the server over TLS without authenticating, for an
/// OAuth account whose sign-in supplies credentials separately.
pub fn probe_reachable(
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<(), SyncError> {
    let runtime = build_runtime()?;
    let bounded = runtime.block_on(async {
        tokio::time::timeout(timeout, tls_connect(host, port)).await
    });
    let client = match bounded {
        Ok(result) => result?,
        Err(_) => {
            return Err(SyncError::Timeout {
                host: host.to_string(),
                port,
            });
        }
    };
    logout(&runtime, client);
    Ok(())
}
