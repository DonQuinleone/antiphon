use sequoia_gpg_agent::KeyPair;
use sequoia_openpgp::crypto::{SessionKey, SymmetricAlgorithm};
use sequoia_openpgp::packet::{PKESK, SKESK};
use sequoia_openpgp::parse::stream::{
    DecryptionHelper, MessageStructure, VerificationHelper,
};
use sequoia_openpgp::{Cert, KeyHandle, Result};

use crate::error::AgentError;

pub(crate) struct AgentDecryption {
    keys: Vec<(KeyHandle, KeyPair)>,
}

impl AgentDecryption {
    pub(crate) fn new(
        keys: Vec<(KeyHandle, KeyPair)>,
    ) -> AgentDecryption {
        AgentDecryption { keys }
    }
}

impl VerificationHelper for AgentDecryption {
    fn get_certs(&mut self, _ids: &[KeyHandle]) -> Result<Vec<Cert>> {
        Ok(Vec::new())
    }

    fn check(&mut self, _structure: MessageStructure) -> Result<()> {
        Ok(())
    }
}

impl DecryptionHelper for AgentDecryption {
    fn decrypt(
        &mut self,
        pkesks: &[PKESK],
        _skesks: &[SKESK],
        sym_algo: Option<SymmetricAlgorithm>,
        decrypt: &mut dyn FnMut(
            Option<SymmetricAlgorithm>,
            &SessionKey,
        ) -> bool,
    ) -> Result<Option<Cert>> {
        for pkesk in pkesks {
            let recipient = pkesk.recipient();
            for (handle, keypair) in &mut self.keys {
                let addressed = recipient
                    .as_ref()
                    .map(|wanted| wanted.aliases(&*handle))
                    .unwrap_or(true);
                if !addressed {
                    continue;
                }
                let Some((algo, session_key)) =
                    pkesk.decrypt(keypair, sym_algo)
                else {
                    continue;
                };
                if decrypt(algo, &session_key) {
                    return Ok(None);
                }
            }
        }
        Err(AgentError::NoDecryptionKey.into())
    }
}
