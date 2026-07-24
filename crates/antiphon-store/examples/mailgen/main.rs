use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use antiphon_store::StoreLayout;

mod corpus;
mod generator;
mod message;
mod rng;

use generator::generate;

const USAGE: &str = "usage: mailgen --root <dir> --messages <n> \
     [--accounts <n>] [--seed <n>]";

const DEFAULT_ACCOUNTS: usize = 6;
const DEFAULT_SEED: u64 = 1;

struct Config {
    root: PathBuf,
    messages: usize,
    accounts: usize,
    seed: u64,
}

fn main() -> ExitCode {
    let config = match parse_args() {
        Ok(config) => config,
        Err(message) => {
            eprintln!("mailgen: {message}");
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Err(message) = run(&config) {
        eprintln!("mailgen: {message}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn parse_args() -> Result<Config, String> {
    let mut root = None;
    let mut messages = None;
    let mut accounts = DEFAULT_ACCOUNTS;
    let mut seed = DEFAULT_SEED;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--root" => root = Some(PathBuf::from(&value)),
            "--messages" => {
                messages = Some(parse_count(&flag, &value)?);
            }
            "--accounts" => {
                accounts = parse_count(&flag, &value)?;
            }
            "--seed" => {
                seed = value.parse().map_err(|_| {
                    format!(
                        "--seed wants an unsigned integer, \
                         got {value}"
                    )
                })?;
            }
            unknown => {
                return Err(format!("unknown flag {unknown}"));
            }
        }
    }
    let root =
        root.ok_or_else(|| String::from("--root is required"))?;
    let messages = messages
        .ok_or_else(|| String::from("--messages is required"))?;
    Ok(Config {
        root,
        messages,
        accounts,
        seed,
    })
}

fn parse_count(flag: &str, value: &str) -> Result<usize, String> {
    let parsed: usize = value.parse().map_err(|_| {
        format!("{flag} wants a positive integer, got {value}")
    })?;
    if parsed == 0 {
        return Err(format!("{flag} must be at least 1"));
    }
    Ok(parsed)
}

fn run(config: &Config) -> Result<(), String> {
    let layout = StoreLayout::new(&config.root);
    layout.init().map_err(|source| {
        format!(
            "initialising store at {}: {source}",
            layout.root().display()
        )
    })?;
    let generating = Instant::now();
    let written = generate(&layout, config)?;
    let generated_in = generating.elapsed();
    let indexing = Instant::now();
    run_notmuch_new(&layout.notmuch_config_path())?;
    let indexed_in = indexing.elapsed();
    println!(
        "wrote {written} messages across {} accounts \
         in {generated_in:.2?}",
        config.accounts
    );
    println!("notmuch new indexed the store in {indexed_in:.2?}");
    println!("store root: {}", layout.root().display());
    Ok(())
}

fn run_notmuch_new(config: &Path) -> Result<(), String> {
    let out = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", config)
        .output()
        .map_err(|source| format!("running notmuch new: {source}"))?;
    if !out.status.success() {
        return Err(format!(
            "notmuch new failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(())
}
