use std::process::ExitCode;

use antiphon_config::{Dirs, Loaded, load};
use antiphon_store::StoreLayout;
use antiphon_vault::{
    Auth, CreateOptions, VaultStatus, enrol_touchid, enrol_yubikey,
    passphrase_command, select_backend,
};

pub fn create() -> ExitCode {
    let Some(dirs) = Dirs::from_process() else {
        eprintln!("cannot resolve the home directory");
        return ExitCode::FAILURE;
    };
    let loaded = match load(&dirs) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match run_create(&dirs, &loaded) {
        Ok(path) => {
            println!("vault created and unlocked at {path}");
            println!(
                "now run `antiphon doctor --init-store`, then \
                 start antiphond"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

pub fn touchid_enrol() -> ExitCode {
    run_enrol("Touch ID", "touchid", run_touchid_enrol)
}

pub fn yubikey_enrol() -> ExitCode {
    run_enrol("YubiKey", "yubikey", run_yubikey_enrol)
}

/// The shared shape of an enrolment command: resolve the config,
/// run the method, and report how to switch it on. The method
/// itself is the only part that differs.
fn run_enrol(
    label: &str,
    method: &str,
    run: impl FnOnce(&Dirs, &Loaded) -> Result<(), String>,
) -> ExitCode {
    let (dirs, loaded) = match load_env() {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    match run(&dirs, &loaded) {
        Ok(()) => {
            println!(
                "{label} enrolled; add `{method}` to `[vault] \
                 unlock` to use it"
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn load_env() -> Result<(Dirs, Loaded), String> {
    let dirs = Dirs::from_process().ok_or_else(|| {
        "cannot resolve the home directory".to_owned()
    })?;
    let loaded = load(&dirs).map_err(|error| error.to_string())?;
    Ok((dirs, loaded))
}

fn run_touchid_enrol(
    dirs: &Dirs,
    loaded: &Loaded,
) -> Result<(), String> {
    let Some(command) = &loaded.config.vault.passphrase_cmd else {
        return Err(
            "set `[vault] passphrase_cmd` first; enrolment stores \
             the passphrase it yields behind Touch ID"
                .to_string(),
        );
    };
    let layout = StoreLayout::new(dirs.store_root());
    let secret =
        passphrase_command(command).map_err(|e| e.to_string())?;
    enrol_touchid(layout.root(), &secret)
        .map_err(|error| format!("enrolling Touch ID: {error}"))
}

/// Enrolment reads the vault passphrase from `passphrase_cmd`
/// and the YubiKey PIN from `yubikey_pin_cmd`, two separate
/// sources so the secrets never mix, then seals the passphrase
/// under the key's hmac-secret. The passphrase is never stored
/// in the clear.
fn run_yubikey_enrol(
    dirs: &Dirs,
    loaded: &Loaded,
) -> Result<(), String> {
    let Some(passphrase_cmd) = &loaded.config.vault.passphrase_cmd
    else {
        return Err(
            "set `[vault] passphrase_cmd` first; it supplies the \
             vault passphrase to seal"
                .to_string(),
        );
    };
    let Some(pin_cmd) = &loaded.config.vault.yubikey_pin_cmd else {
        return Err(
            "set `[vault] yubikey_pin_cmd` first; it supplies the \
             YubiKey's FIDO2 PIN"
                .to_string(),
        );
    };
    let secret = passphrase_command(passphrase_cmd)
        .map_err(|e| e.to_string())?;
    let pin = passphrase_command(pin_cmd).map_err(|e| e.to_string())?;
    println!("touch the YubiKey when it blinks (twice)");
    // The enrolment lives outside the vault (the state dir, like
    // the socket): it must be readable while the vault is sealed,
    // since it is what unlocks it.
    enrol_yubikey(&dirs.state, &secret, &pin)
        .map_err(|error| format!("enrolling the YubiKey: {error}"))
}

pub(crate) fn run_create(
    dirs: &Dirs,
    loaded: &Loaded,
) -> Result<String, String> {
    let layout = StoreLayout::new(dirs.store_root());
    if layout.exists() {
        return Err(format!(
            "a store already exists at {}; move it aside before \
             creating a vault over it",
            layout.root().display()
        ));
    }
    let Some(command) = &loaded.config.vault.passphrase_cmd else {
        return Err(
            "set `[vault] passphrase_cmd` first; it supplies the \
             vault passphrase"
                .to_string(),
        );
    };
    let vault = select_backend(loaded.config.vault.backend, &layout)
        .map_err(|error| error.to_string())?;
    if vault.status() != VaultStatus::Absent {
        return Err("a vault already exists here".to_string());
    }
    let secret =
        passphrase_command(command).map_err(|e| e.to_string())?;
    let auth = Auth::Passphrase(secret);
    vault
        .create(&CreateOptions::new(auth.clone()))
        .map_err(|error| format!("creating the vault: {error}"))?;
    vault
        .unlock(&auth)
        .map_err(|error| format!("unlocking the vault: {error}"))?;
    Ok(layout.root().display().to_string())
}
