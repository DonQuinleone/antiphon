//! Preflight checks for the local antiphon setup: doctor v0.

use std::ffi::OsString;
use std::process::{Command, ExitCode};

use antiphon_config::{ConfigError, Dirs, Loaded, load};

const GREEN: &str = "\x1b[32m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";
const STATUS_WIDTH: usize = "FAIL".len();

struct Check {
    name: &'static str,
    arg: &'static str,
    run: fn(&Context, &str) -> Outcome,
}

const CHECKS: &[Check] = &[
    Check {
        name: "config directory",
        arg: "",
        run: config_directory,
    },
    Check {
        name: "config parses",
        arg: "",
        run: config_parses,
    },
    Check {
        name: "accounts",
        arg: "",
        run: accounts,
    },
    Check {
        name: "notmuch",
        arg: "notmuch",
        run: tool_version,
    },
    Check {
        name: "gpg",
        arg: "gpg",
        run: tool_version,
    },
    Check {
        name: "editor",
        arg: "EDITOR",
        run: env_var_set,
    },
];

struct Outcome {
    passed: bool,
    detail: String,
}

impl Outcome {
    fn ok(detail: impl Into<String>) -> Outcome {
        Outcome {
            passed: true,
            detail: detail.into(),
        }
    }

    fn fail(detail: impl Into<String>) -> Outcome {
        Outcome {
            passed: false,
            detail: detail.into(),
        }
    }
}

struct Context {
    dirs: Option<Dirs>,
    loaded: Option<Result<Loaded, ConfigError>>,
}

impl Context {
    fn from_process() -> Context {
        let dirs = Dirs::from_process();
        let loaded = dirs.as_ref().map(load);
        Context { dirs, loaded }
    }
}

pub fn run() -> ExitCode {
    let context = Context::from_process();
    let colour = colour_allowed(std::env::var_os("NO_COLOR"));
    let name_width = CHECKS
        .iter()
        .map(|check| check.name.len())
        .max()
        .unwrap_or(0);
    let results: Vec<bool> = CHECKS
        .iter()
        .map(|check| {
            let outcome = (check.run)(&context, check.arg);
            let line =
                render_line(check.name, &outcome, name_width, colour);
            println!("{line}");
            outcome.passed
        })
        .collect();
    ExitCode::from(exit_code(&results))
}

fn config_directory(context: &Context, _: &str) -> Outcome {
    let Some(dirs) = &context.dirs else {
        return Outcome::fail("cannot resolve the home directory");
    };
    let path = dirs.config.display();
    if !dirs.config.is_dir() {
        return Outcome::fail(format!("{path} does not exist"));
    }
    Outcome::ok(path.to_string())
}

fn config_parses(context: &Context, _: &str) -> Outcome {
    match &context.loaded {
        None => Outcome::fail("no configuration directory"),
        Some(Err(error)) => Outcome::fail(error.to_string()),
        Some(Ok(_)) => {
            Outcome::ok("config.toml and accounts are valid")
        }
    }
}

fn accounts(context: &Context, _: &str) -> Outcome {
    let Some(Ok(loaded)) = &context.loaded else {
        return Outcome::fail(
            "unavailable until the configuration parses",
        );
    };
    if loaded.accounts.is_empty() {
        return Outcome::ok("none configured yet");
    }
    let names: Vec<&str> = loaded
        .accounts
        .iter()
        .map(|entry| entry.account.account.name.as_str())
        .collect();
    Outcome::ok(format!("{} found: {}", names.len(), names.join(", ")))
}

fn tool_version(_: &Context, tool: &str) -> Outcome {
    let output = Command::new(tool).arg("--version").output();
    let Ok(output) = output else {
        return Outcome::fail(format!("{tool} not found on PATH"));
    };
    if !output.status.success() {
        return Outcome::fail(format!("{tool} --version failed"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Outcome::ok(first_line(&text))
}

fn env_var_set(_: &Context, var: &str) -> Outcome {
    match std::env::var(var) {
        Ok(value) if !value.is_empty() => Outcome::ok(value),
        _ => Outcome::fail(format!("${var} is not set")),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim()
}

fn colour_allowed(no_colour: Option<OsString>) -> bool {
    no_colour.is_none_or(|value| value.is_empty())
}

fn render_line(
    name: &str,
    outcome: &Outcome,
    name_width: usize,
    colour: bool,
) -> String {
    let label = if outcome.passed { "ok" } else { "FAIL" };
    let padding = " ".repeat(STATUS_WIDTH - label.len());
    format!(
        "{}{padding}  {name:<name_width$}  {}",
        paint(label, outcome.passed, colour),
        outcome.detail,
    )
}

fn paint(text: &str, passed: bool, colour: bool) -> String {
    if !colour {
        return text.to_string();
    }
    let code = if passed { GREEN } else { RED };
    format!("{code}{text}{RESET}")
}

fn exit_code(results: &[bool]) -> u8 {
    let all_passed = results.iter().all(|passed| *passed);
    u8::from(!all_passed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_line(colour: bool) -> String {
        render_line("gpg", &Outcome::ok("gpg (GnuPG) 2.4"), 8, colour)
    }

    #[test]
    fn coloured_lines_wrap_the_status_in_ansi_codes() {
        assert_eq!(
            ok_line(true),
            "\x1b[32mok\x1b[0m    gpg       gpg (GnuPG) 2.4"
        );
    }

    #[test]
    fn plain_lines_carry_no_escape_codes() {
        assert_eq!(ok_line(false), "ok    gpg       gpg (GnuPG) 2.4");
    }

    #[test]
    fn failures_render_in_red_and_align_with_ok() {
        let outcome = Outcome::fail("$EDITOR is not set");
        assert_eq!(
            render_line("editor", &outcome, 8, true),
            "\x1b[31mFAIL\x1b[0m  editor    $EDITOR is not set"
        );
    }

    #[test]
    fn no_colour_disables_colour_only_when_non_empty() {
        assert!(colour_allowed(None));
        assert!(colour_allowed(Some(OsString::new())));
        assert!(!colour_allowed(Some(OsString::from("1"))));
    }

    #[test]
    fn first_line_takes_the_head_of_version_output() {
        let output = "notmuch 0.38.3\nCopyright blurb\n";
        assert_eq!(first_line(output), "notmuch 0.38.3");
        assert_eq!(first_line(""), "");
        assert_eq!(first_line("bare\n"), "bare");
    }

    #[test]
    fn exit_code_is_zero_only_when_every_check_passes() {
        assert_eq!(exit_code(&[true, true, true]), 0);
        assert_eq!(exit_code(&[true, false, true]), 1);
        assert_eq!(exit_code(&[]), 0);
    }
}
