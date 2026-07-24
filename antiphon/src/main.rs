mod doctor;

use std::process::ExitCode;

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
    Doctor,
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Some(Command::Doctor) => doctor::run(),
        None => tui_notice(),
    }
}

fn tui_notice() -> ExitCode {
    println!("antiphon {}", env!("ANTIPHON_VERSION"));
    println!(
        "The TUI arrives at milestone M2; until then, \
         `antiphon doctor` checks your setup."
    );
    ExitCode::SUCCESS
}
