use antiphon_config::{Loaded, Unlock};
use antiphon_ipc::VaultState;
use antiphon_store::StoreLayout;
use antiphon_vault::{
    Auth, PassphraseCmdSource, SecretSource, TouchidSource, Vault,
    VaultStatus, YubikeySource, resolve_passphrase, select_backend,
};

/// Ensure the store is readable before the daemon opens it. An
/// absent vault means the store is a plain directory (the
/// pre-vault arrangement); a sealed vault is unlocked with the
/// passphrase from `[vault] passphrase_cmd`.
pub fn ensure_open(
    loaded: &Loaded,
    layout: &StoreLayout,
) -> Result<VaultState, String> {
    let vault = backend(loaded, layout)?;
    match vault.status() {
        VaultStatus::Open => Ok(VaultState::Open),
        VaultStatus::Absent => {
            println!("no vault; using the plain store");
            Ok(VaultState::Absent)
        }
        VaultStatus::Sealed => {
            open_sealed(loaded, layout, vault.as_ref())?;
            Ok(VaultState::Open)
        }
    }
}

pub fn lock(
    loaded: &Loaded,
    layout: &StoreLayout,
) -> Result<(), String> {
    let vault = backend(loaded, layout)?;
    if vault.status() != VaultStatus::Open {
        return Ok(());
    }
    vault
        .lock()
        .map_err(|error| format!("locking the vault: {error}"))
}

fn backend(
    loaded: &Loaded,
    layout: &StoreLayout,
) -> Result<Box<dyn Vault>, String> {
    select_backend(loaded.config.vault.backend, layout)
        .map_err(|error| format!("selecting the vault: {error}"))
}

fn open_sealed(
    loaded: &Loaded,
    layout: &StoreLayout,
    vault: &dyn Vault,
) -> Result<(), String> {
    let sources = unlock_sources(loaded, layout);
    if sources.is_empty() {
        return Err("vault is sealed but no unlock method is \
             configured; set `[vault] passphrase_cmd` or enrol \
             Touch ID"
            .to_string());
    }
    let secret = resolve_passphrase(&sources)
        .map_err(|error| format!("unlocking the vault: {error}"))?;
    vault
        .unlock(&Auth::Passphrase(secret))
        .map_err(|error| format!("unlocking the vault: {error}"))?;
    println!("vault unlocked");
    Ok(())
}

/// The `[vault] unlock` list turned into ordered secret sources.
/// Each is tried in the order listed, so a cancelled touch falls
/// through to the next. A passphrase or YubiKey entry with no
/// `passphrase_cmd` contributes no source: the command yields the
/// vault passphrase for the former and the FIDO2 PIN for the
/// latter.
fn unlock_sources(
    loaded: &Loaded,
    layout: &StoreLayout,
) -> Vec<Box<dyn SecretSource>> {
    let mut sources: Vec<Box<dyn SecretSource>> = Vec::new();
    for method in &loaded.config.vault.unlock {
        match method {
            Unlock::Touchid => sources
                .push(Box::new(TouchidSource::new(layout.root()))),
            Unlock::Passphrase => {
                let Some(command) = &loaded.config.vault.passphrase_cmd
                else {
                    continue;
                };
                sources.push(Box::new(PassphraseCmdSource::new(
                    command.clone(),
                )));
            }
            Unlock::Yubikey => {
                let Some(command) = &loaded.config.vault.passphrase_cmd
                else {
                    continue;
                };
                sources.push(Box::new(YubikeySource::new(
                    layout.root(),
                    command.clone(),
                )));
            }
        }
    }
    sources
}
