use std::env;
use std::path::Path;

// The notmuch bindings link `libnotmuch` without a search
// path; Homebrew's lib dir is not on the default macOS linker
// path, so add it here when the dylib is actually there.
// Linux distro installs resolve from the default path and get
// no extra flags.
const MACOS_LIB_DIRS: [&str; 2] =
    ["/opt/homebrew/lib", "/usr/local/lib"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }
    for dir in MACOS_LIB_DIRS {
        if !Path::new(dir).join("libnotmuch.dylib").exists() {
            continue;
        }
        println!("cargo:rustc-link-search=native={dir}");
        return;
    }
}
