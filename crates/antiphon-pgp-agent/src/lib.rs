mod decrypt;
mod error;
mod keyring;

pub use error::AgentError;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use sequoia_gpg_agent::{Agent, KeyPair};
use sequoia_openpgp::armor;
use sequoia_openpgp::cert::ValidCert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::parse::stream::DecryptorBuilder;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::serialize::stream::{Armorer, Message, Signer};
use sequoia_openpgp::{Fingerprint, KeyHandle};
use tokio::runtime::Runtime;

use crate::error::classify;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertSummary {
    pub fingerprint: String,
    pub primary_user_id: Option<String>,
}

pub struct GpgAgent {
    runtime: Runtime,
    agent: Mutex<Agent>,
    gnupg_home: Option<PathBuf>,
}

impl GpgAgent {
    pub fn connect(
        gnupg_home: Option<&Path>,
    ) -> Result<GpgAgent, AgentError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .build()
            .map_err(|error| {
                AgentError::Unreachable(error.to_string())
            })?;
        let agent = runtime
            .block_on(async {
                match gnupg_home {
                    Some(home) => Agent::connect_to(home).await,
                    None => Agent::connect_to_default().await,
                }
            })
            .map_err(|error| {
                AgentError::Unreachable(format!("{error:#}"))
            })?;
        Ok(GpgAgent {
            runtime,
            agent: Mutex::new(agent),
            gnupg_home: gnupg_home.map(Path::to_path_buf),
        })
    }

    pub fn signing_certs(
        &self,
    ) -> Result<Vec<CertSummary>, AgentError> {
        let certs = keyring::export_certs(self.home())?;
        let policy = StandardPolicy::new();
        let mut agent = self.lock_agent();
        let mut summaries = Vec::new();
        for cert in &certs {
            let Ok(valid) = cert.with_policy(&policy, None) else {
                continue;
            };
            if self.signing_keypair(&mut agent, &valid).is_none() {
                continue;
            }
            summaries.push(CertSummary {
                fingerprint: cert.fingerprint().to_string(),
                primary_user_id: valid.primary_userid().ok().map(
                    |uid| {
                        String::from_utf8_lossy(uid.userid().value())
                            .into_owned()
                    },
                ),
            });
        }
        Ok(summaries)
    }

    pub fn sign_detached(
        &self,
        cert_fpr: &str,
        data: &[u8],
    ) -> Result<Vec<u8>, AgentError> {
        let fingerprint =
            cert_fpr.parse::<Fingerprint>().map_err(|error| {
                AgentError::Pgp(format!(
                    "bad fingerprint {cert_fpr}: {error:#}"
                ))
            })?;
        let certs = keyring::export_certs(self.home())?;
        let cert = certs
            .iter()
            .find(|cert| cert.fingerprint() == fingerprint)
            .ok_or_else(|| {
                AgentError::NoSuchCert(fingerprint.to_string())
            })?;
        let policy = StandardPolicy::new();
        let valid = cert
            .with_policy(&policy, None)
            .map_err(|error| AgentError::Pgp(format!("{error:#}")))?;
        let keypair = self
            .signing_keypair(&mut self.lock_agent(), &valid)
            .ok_or_else(|| {
                AgentError::NoSigningKey(fingerprint.to_string())
            })?
            .with_cert(&valid);

        let mut armoured = Vec::new();
        let message = Message::new(&mut armoured);
        let message = Armorer::new(message)
            .kind(armor::Kind::Signature)
            .build()
            .map_err(classify)?;
        let mut signer = Signer::new(message, keypair)
            .map_err(classify)?
            .detached()
            .build()
            .map_err(classify)?;
        signer
            .write_all(data)
            .map_err(|error| classify(error.into()))?;
        signer.finalize().map_err(classify)?;
        Ok(armoured)
    }

    pub fn decrypt(
        &self,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, AgentError> {
        let certs = keyring::export_certs(self.home())?;
        let mut keys: Vec<(KeyHandle, KeyPair)> = Vec::new();
        {
            let mut agent = self.lock_agent();
            for cert in &certs {
                for key in cert.keys() {
                    let key = key.key();
                    let known = self
                        .runtime
                        .block_on(agent.has_key(key))
                        .unwrap_or(false);
                    if !known {
                        continue;
                    }
                    let Ok(keypair) = agent.keypair(key) else {
                        continue;
                    };
                    keys.push((key.key_handle(), keypair));
                }
            }
        }

        let policy = StandardPolicy::new();
        let helper = decrypt::AgentDecryption::new(keys);
        let mut reader = DecryptorBuilder::from_bytes(ciphertext)
            .map_err(classify)?
            .with_policy(&policy, None, helper)
            .map_err(classify)?;
        let mut plaintext = Vec::new();
        reader
            .read_to_end(&mut plaintext)
            .map_err(|error| classify(error.into()))?;
        Ok(plaintext)
    }

    fn home(&self) -> Option<&Path> {
        self.gnupg_home.as_deref()
    }

    fn lock_agent(&self) -> MutexGuard<'_, Agent> {
        self.agent
            .lock()
            .expect("gpg-agent connection lock poisoned")
    }

    fn signing_keypair(
        &self,
        agent: &mut Agent,
        valid: &ValidCert,
    ) -> Option<KeyPair> {
        let candidates = valid
            .keys()
            .alive()
            .revoked(false)
            .for_signing()
            .supported();
        for candidate in candidates {
            let key = candidate.key();
            let known = self
                .runtime
                .block_on(agent.has_key(key))
                .unwrap_or(false);
            if !known {
                continue;
            }
            let Ok(keypair) = agent.keypair(key) else {
                continue;
            };
            return Some(keypair);
        }
        None
    }
}
