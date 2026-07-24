use std::process::Command;

pub fn emit_version() {
    println!("cargo:rustc-env=ANTIPHON_VERSION={}", version());
    println!("cargo:rerun-if-changed=../.git/HEAD");
}

fn version() -> String {
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
