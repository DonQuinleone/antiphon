mod doctor;
mod tui;
mod vaultcmd;

use std::process::ExitCode;

use antiphon_config::{Dirs, load};
use antiphon_store::StoreLayout;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "antiphon",
    version = env!("ANTIPHON_VERSION"),
    about = "A modern mail client for the terminal"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Check the local setup: config, tools and environment.
    Doctor {
        /// Create the message store before running the checks.
        #[arg(long)]
        init_store: bool,
    },
    /// Manage the encrypted vault.
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Create and unlock the vault at the store path.
    Create,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Doctor { init_store }) => doctor::run(init_store),
        Some(Command::Vault { action }) => match action {
            VaultAction::Create => vaultcmd::create(),
        },
        None => open_client(),
    }
}

fn open_client() -> ExitCode {
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
    let layout = StoreLayout::new(dirs.store_root());
    if !layout.exists() {
        eprintln!(
            "no message store at {}; run \
             `antiphon doctor --init-store` to create it",
            layout.root().display()
        );
        return ExitCode::FAILURE;
    }
    tui::run(&loaded, &layout, &dirs)
}
