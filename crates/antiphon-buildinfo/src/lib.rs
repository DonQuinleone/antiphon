use std::process::Command;

pub fn emit_version() {
    println!("cargo:rustc-env=ANTIPHON_VERSION={}", version());
    // HEAD only changes on branch switches; commits move the
    // ref it points at, so watch that file too or the stamp
    // goes stale while the code stays fresh.
    println!("cargo:rerun-if-changed=../.git/HEAD");
    for ref_file in resolved_refs() {
        println!("cargo:rerun-if-changed={ref_file}");
    }
    println!("cargo:rerun-if-env-changed=ANTIPHON_VERSION");
}

fn resolved_refs() -> Vec<String> {
    let Ok(head) = std::fs::read_to_string("../.git/HEAD") else {
        return Vec::new();
    };
    let Some(reference) = head.trim().strip_prefix("ref: ") else {
        return Vec::new();
    };
    vec![
        format!("../.git/{reference}"),
        "../.git/packed-refs".to_string(),
    ]
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
