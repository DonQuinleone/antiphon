use antiphon_config::Loaded;
use antiphon_store::StoreLayout;
use antiphon_vault::{
    Auth, Vault, VaultStatus, passphrase_command, select_backend,
};
use secrecy::SecretString;

/// Ensure the store is readable before the daemon opens it. An
/// absent vault means the store is a plain directory (the
/// pre-vault arrangement); a sealed vault is unlocked with the
/// passphrase from `[vault] passphrase_cmd`.
pub fn ensure_open(
    loaded: &Loaded,
    layout: &StoreLayout,
) -> Result<(), String> {
    let vault = backend(loaded, layout)?;
    match vault.status() {
        VaultStatus::Open => Ok(()),
        VaultStatus::Absent => {
            println!("no vault; using the plain store");
            Ok(())
        }
        VaultStatus::Sealed => open_sealed(loaded, vault.as_ref()),
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
    vault: &dyn Vault,
) -> Result<(), String> {
    let secret = passphrase(loaded)?;
    vault
        .unlock(&Auth::Passphrase(secret))
        .map_err(|error| format!("unlocking the vault: {error}"))?;
    println!("vault unlocked");
    Ok(())
}

fn passphrase(loaded: &Loaded) -> Result<SecretString, String> {
    let Some(command) = &loaded.config.vault.passphrase_cmd else {
        return Err("vault is sealed but no `[vault] passphrase_cmd` \
             is configured to unlock it"
            .to_string());
    };
    passphrase_command(command).map_err(|error| error.to_string())
}
