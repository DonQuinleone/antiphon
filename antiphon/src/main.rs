mod autostart;
mod doctor;
mod oauthcmd;
mod sendmail;
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
    /// Manage OAuth sign-ins for [oauth] accounts.
    Oauth {
        #[command(subcommand)]
        action: OauthAction,
    },
    /// Sendmail-compatible queueing for git send-email.
    #[command(disable_help_flag = true)]
    Sendmail {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}

#[derive(Subcommand)]
enum VaultAction {
    /// Create and unlock the vault at the store path.
    Create,
}

#[derive(Subcommand)]
enum OauthAction {
    /// Run the provider's sign-in flow and store the grants.
    Login { account: String },
    /// Show the stored grants and their expiry.
    Status { account: String },
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Doctor { init_store }) => doctor::run(init_store),
        Some(Command::Vault { action }) => match action {
            VaultAction::Create => vaultcmd::create(),
        },
        Some(Command::Oauth { action }) => match action {
            OauthAction::Login { account } => oauthcmd::login(&account),
            OauthAction::Status { account } => {
                oauthcmd::status(&account)
            }
        },
        Some(Command::Sendmail { args }) => sendmail::run(&args),
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
    // The daemon may hold the only key to the store: behind a
    // sealed vault it mounts it, so start it before the
    // existence check.
    if let Err(error) =
        autostart::ensure_daemon(loaded.config.daemon.autostart, &dirs)
    {
        eprintln!("{error}");
    }
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
