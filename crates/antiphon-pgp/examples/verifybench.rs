use std::time::Instant;

use antiphon_pgp::{Keyring, verify};

const RUNS: u32 = 1000;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(path) = args.next() else {
        eprintln!("usage: verifybench <message> [keyring-dir]");
        std::process::exit(2);
    };
    let raw = std::fs::read(&path).unwrap_or_else(|error| {
        eprintln!("{path}: {error}");
        std::process::exit(1);
    });
    let keyring = match args.next() {
        Some(dir) => Keyring::from_dir(dir),
        None => Keyring::default(),
    };
    let started = Instant::now();
    let mut last = None;
    for _ in 0..RUNS {
        last = Some(verify(&raw, &keyring));
    }
    let per_run =
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(RUNS);
    let status = last
        .map(|signature| format!("{:?}", signature.status))
        .unwrap_or_default();
    println!("{status}: {per_run:.3} ms per verify");
}
