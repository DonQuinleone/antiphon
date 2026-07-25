mod accounts;
mod daemon;
mod drafts;
mod idle;
mod ipc;
mod mailflow;
mod notify;
mod tokens;
mod unsubscribe;
mod vaultctl;
mod worker;

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => daemon::run(),
        Some("--version") | Some("-V") => {
            println!("antiphond {}", env!("ANTIPHON_VERSION"));
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown argument: {other}");
            ExitCode::from(2)
        }
    }
}
