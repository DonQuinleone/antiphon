use std::process::Command;

pub fn emit_version() {
    println!("cargo:rustc-env=ANTIPHON_VERSION={}", version());
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-env-changed=ANTIPHON_VERSION");
}

/// Tag archives carry no .git, so packagers (AUR, Nix, brew)
/// pass the version through the environment; a git checkout
/// derives it from the tag as always.
fn version() -> String {
    if let Ok(given) = std::env::var("ANTIPHON_VERSION")
        && !given.is_empty()
    {
        return given;
    }
    let output = Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .output();
    let Ok(output) = output else {
        return fallback();
    };
    if !output.status.success() {
        return fallback();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let text = text.trim();
    if text.is_empty() {
        return fallback();
    }
    text.to_string()
}

fn fallback() -> String {
    "unversioned".to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_never_empty() {
        assert!(!super::version().is_empty());
    }
}
