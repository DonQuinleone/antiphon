use std::fmt;

#[derive(Debug)]
pub enum AgentError {
    Unreachable(String),
    Keyring(String),
    NoSuchCert(String),
    NoSigningKey(String),
    NoDecryptionKey,
    Refused(String),
    Pgp(String),
}

impl fmt::Display for AgentError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Unreachable(cause) => {
                write!(out, "gpg-agent unreachable: {cause}")
            }
            AgentError::Keyring(cause) => {
                write!(out, "reading the GnuPG keyring failed: {cause}")
            }
            AgentError::NoSuchCert(fingerprint) => {
                write!(
                    out,
                    "no certificate {fingerprint} in the keyring"
                )
            }
            AgentError::NoSigningKey(fingerprint) => {
                write!(
                    out,
                    "certificate {fingerprint} has no usable \
                     signing key with secret material known to \
                     gpg-agent"
                )
            }
            AgentError::NoDecryptionKey => {
                write!(
                    out,
                    "no decryption key for this message is \
                     known to gpg-agent"
                )
            }
            AgentError::Refused(cause) => {
                write!(out, "gpg-agent refused the operation: {cause}")
            }
            AgentError::Pgp(cause) => {
                write!(out, "OpenPGP operation failed: {cause}")
            }
        }
    }
}

impl std::error::Error for AgentError {}

pub(crate) fn classify(error: anyhow::Error) -> AgentError {
    match error.downcast::<AgentError>() {
        Ok(ours) => ours,
        Err(error) => classify_agent(error),
    }
}

fn classify_agent(error: anyhow::Error) -> AgentError {
    use sequoia_gpg_agent::Error;
    use sequoia_gpg_agent::assuan;

    if let Some(assuan::Error::OperationFailed(message)) =
        error.downcast_ref::<assuan::Error>()
    {
        return AgentError::Refused(message.clone());
    }
    let Some(agent) = error.downcast_ref::<Error>() else {
        return AgentError::Pgp(format!("{error:#}"));
    };
    match agent {
        Error::Assuan(assuan::Error::OperationFailed(message)) => {
            AgentError::Refused(message.clone())
        }
        Error::UnknownKey(keygrip) => {
            AgentError::NoSigningKey(keygrip.to_string())
        }
        other => AgentError::Refused(other.to_string()),
    }
}
