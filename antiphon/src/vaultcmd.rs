use std::process::ExitCode;

use antiphon_config::{Dirs, Loaded, load};
use antiphon_store::StoreLayout;
use antiphon_vault::{
    Auth, CreateOptions, VaultStatus, enrol_touchid,
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
    match run_touchid_enrol(&dirs, &loaded) {
        Ok(()) => {
            println!(
                "Touch ID enrolled; add `touchid` to `[vault] \
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
