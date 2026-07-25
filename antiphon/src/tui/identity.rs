use antiphon_config::{AccountFile, Dirs, Loaded};
use antiphon_core::{Addr, ParsedIdentity, reply_identity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComposeIdentity {
    pub name: Option<String>,
    pub address: String,
    pub signature: Option<String>,
    pub pgp_sign: bool,
    pub pgp_key: Option<String>,
}

/// The pgp settings of one configured identity, keyed by its
/// address so reply resolution can recover them.
struct IdentityPgp {
    address: String,
    sign: bool,
    key: Option<String>,
}

pub struct ComposeContext {
    entries: Vec<(String, ComposeIdentity)>,
    parsed: Vec<(String, Vec<ParsedIdentity>)>,
    pgp: Vec<(String, Vec<IdentityPgp>)>,
    dirs: Dirs,
}

impl ComposeContext {
    pub fn from_loaded(loaded: &Loaded, dirs: &Dirs) -> ComposeContext {
        let entries = loaded
            .accounts
            .iter()
            .flat_map(|named| {
                let name = named.account.account.name.clone();
                account_identities(&named.account, dirs)
                    .into_iter()
                    .map(move |identity| (name.clone(), identity))
            })
            .collect();
        let parsed = loaded
            .accounts
            .iter()
            .map(|named| {
                (
                    named.account.account.name.clone(),
                    parsed_identities(&named.account),
                )
            })
            .collect();
        let pgp = loaded
            .accounts
            .iter()
            .map(|named| {
                (
                    named.account.account.name.clone(),
                    identity_pgp(&named.account),
                )
            })
            .collect();
        ComposeContext {
            entries,
            parsed,
            pgp,
            dirs: dirs.clone(),
        }
    }

    /// Every configured identity paired with its account, in
    /// account order; the From field cycles through these.
    pub fn choices(&self) -> &[(String, ComposeIdentity)] {
        &self.entries
    }

    /// The named account's identity, or the first configured
    /// identity when the account has none of its own.
    pub fn identity_for(
        &self,
        account: &str,
    ) -> Option<(&str, &ComposeIdentity)> {
        self.entries
            .iter()
            .find(|(name, _)| name == account)
            .or_else(|| self.entries.first())
            .map(|(name, identity)| (name.as_str(), identity))
    }

    /// The identity a reply sends from: the resolution engine
    /// over the delivered addresses (verbatim From on pattern
    /// hits), falling back to the account default.
    pub fn reply_identity_for(
        &self,
        account: &str,
        delivered: &[String],
    ) -> Option<(String, ComposeIdentity)> {
        if let Some(identity) = self.resolve(account, delivered) {
            return Some((account.to_string(), identity));
        }
        self.identity_for(account).map(|(name, identity)| {
            (name.to_string(), identity.clone())
        })
    }

    pub fn template(&self, name: &str) -> Option<String> {
        antiphon_config::template_text(&self.dirs, name)
    }

    fn resolve(
        &self,
        account: &str,
        delivered: &[String],
    ) -> Option<ComposeIdentity> {
        let (_, identities) =
            self.parsed.iter().find(|(name, _)| name == account)?;
        let addrs: Vec<Addr> =
            delivered.iter().map(|a| Addr::new(a)).collect();
        let resolved = reply_identity(identities, &addrs)?;
        let pgp = self.pgp_for(account, &resolved.identity.address);
        Some(ComposeIdentity {
            name: resolved.identity.name.clone(),
            address: resolved.from.clone(),
            signature: resolved.identity.signature.as_deref().and_then(
                |name| {
                    antiphon_config::signature_text(&self.dirs, name)
                },
            ),
            pgp_sign: pgp.is_some_and(|pgp| pgp.sign),
            pgp_key: pgp.and_then(|pgp| pgp.key.clone()),
        })
    }

    fn pgp_for(
        &self,
        account: &str,
        address: &str,
    ) -> Option<&IdentityPgp> {
        let (_, identities) =
            self.pgp.iter().find(|(name, _)| name == account)?;
        identities.iter().find(|pgp| pgp.address == address)
    }
}

fn identity_pgp(file: &AccountFile) -> Vec<IdentityPgp> {
    file.identities
        .iter()
        .map(|identity| IdentityPgp {
            address: identity.address.clone(),
            sign: identity.pgp_sign,
            key: identity.pgp_key.clone(),
        })
        .collect()
}

fn parsed_identities(file: &AccountFile) -> Vec<ParsedIdentity> {
    file.identities
        .iter()
        .filter_map(|identity| {
            ParsedIdentity::new(
                &identity.address,
                identity.name.as_deref(),
                identity.signature.as_deref(),
                &identity.matches,
            )
            .ok()
        })
        .collect()
}

fn account_identities(
    file: &AccountFile,
    dirs: &Dirs,
) -> Vec<ComposeIdentity> {
    if !file.identities.is_empty() {
        return file
            .identities
            .iter()
            .map(|identity| ComposeIdentity {
                name: identity.name.clone(),
                address: identity.address.clone(),
                signature: identity.signature.as_deref().and_then(
                    |name| antiphon_config::signature_text(dirs, name),
                ),
                pgp_sign: identity.pgp_sign,
                pgp_key: identity.pgp_key.clone(),
            })
            .collect();
    }
    let user = file.imap.user.clone();
    if !user.contains('@') {
        return Vec::new();
    }
    vec![ComposeIdentity {
        name: None,
        address: user,
        signature: None,
        pgp_sign: false,
        pgp_key: None,
    }]
}
