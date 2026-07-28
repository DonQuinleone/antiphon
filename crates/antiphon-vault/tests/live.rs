//! Live round trips against the real platform tools. Ignored
//! by default; run with `cargo test -p antiphon-vault -- \
//! --ignored --nocapture` on a machine with gocryptfs and
//! macFUSE (and, for the APFS test, macOS).

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use antiphon_vault::{
    ApfsVault, Auth, CreateOptions, GocryptfsVault, Vault, VaultStatus,
};
use secrecy::SecretString;

const MARKER: &str =
    "ANTIPHON-LIVE-MARKER-c1a9e4f2b7d05863-DO-NOT-PERSIST";
const PASSPHRASE_ENTROPY_BYTES: usize = 32;
const SCAN_CHUNK_BYTES: usize = 1 << 20;

fn throwaway_passphrase() -> SecretString {
    let mut bytes = vec![0u8; PASSPHRASE_ENTROPY_BYTES];
    fs::File::open("/dev/urandom")
        .and_then(|mut urandom| urandom.read_exact(&mut bytes))
        .expect("read entropy for a throwaway passphrase");
    let hex: String =
        bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    SecretString::from(hex)
}

struct SealOnDrop<'a> {
    vault: &'a dyn Vault,
}

impl Drop for SealOnDrop<'_> {
    fn drop(&mut self) {
        let _ = self.vault.lock();
    }
}

fn round_trip(
    label: &str,
    vault: &dyn Vault,
    ciphertext_holds_marker: &dyn Fn() -> bool,
) {
    let auth = Auth::Passphrase(throwaway_passphrase());
    assert_eq!(vault.status(), VaultStatus::Absent);

    let started = Instant::now();
    vault.create(&CreateOptions::new(auth.clone())).unwrap();
    println!("{label} create: {:?}", started.elapsed());
    assert_eq!(vault.status(), VaultStatus::Sealed);

    let guard = SealOnDrop { vault };

    let started = Instant::now();
    let mounted = vault.unlock(&auth).unwrap();
    println!("{label} unlock: {:?}", started.elapsed());
    assert_eq!(vault.status(), VaultStatus::Open);

    let marker_file = mounted.mount_point().join("marker.txt");
    fs::write(&marker_file, MARKER).unwrap();

    let started = Instant::now();
    vault.lock().unwrap();
    println!("{label} lock: {:?}", started.elapsed());
    assert_eq!(vault.status(), VaultStatus::Sealed);
    assert!(
        !marker_file.exists(),
        "plaintext still visible after lock"
    );
    assert!(
        !ciphertext_holds_marker(),
        "marker found in ciphertext while locked"
    );

    let started = Instant::now();
    let remounted = vault.unlock(&auth).unwrap();
    println!("{label} re-unlock: {:?}", started.elapsed());
    let read_back =
        fs::read_to_string(remounted.mount_point().join("marker.txt"))
            .unwrap();
    assert_eq!(read_back, MARKER);

    vault.lock().unwrap();
    assert_eq!(vault.status(), VaultStatus::Sealed);
    drop(guard);
}

fn file_contains(path: &Path, needle: &[u8]) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut window = Vec::new();
    let mut chunk = vec![0u8; SCAN_CHUNK_BYTES];
    loop {
        let Ok(read) = file.read(&mut chunk) else {
            return false;
        };
        if read == 0 {
            return false;
        }
        window.extend_from_slice(&chunk[..read]);
        if window
            .windows(needle.len())
            .any(|candidate| candidate == needle)
        {
            return true;
        }
        let keep = window.len().saturating_sub(needle.len() - 1);
        window.drain(..keep);
    }
}

fn tree_contains(dir: &Path, needle: &[u8]) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return tree_contains(&path, needle);
        }
        file_contains(&path, needle)
    })
}

#[test]
#[ignore = "mounts a real gocryptfs filesystem"]
fn gocryptfs_round_trip_keeps_plaintext_in_session_only() {
    let dir = tempfile::tempdir().unwrap();
    let cipherdir = dir.path().join("vault.gocryptfs");
    let mount = dir.path().join("store");
    let vault = GocryptfsVault::new(&cipherdir, &mount);
    round_trip("gocryptfs", &vault, &|| {
        tree_contains(&cipherdir, MARKER.as_bytes())
    });
}

#[test]
#[ignore = "attaches a real encrypted APFS sparse image"]
fn apfs_round_trip_keeps_plaintext_in_session_only() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("vault.sparseimage");
    let mount = dir.path().join("store");
    let vault = ApfsVault::new(&image, &mount);
    round_trip("apfs", &vault, &|| {
        file_contains(&image, MARKER.as_bytes())
    });
}

/// A real Touch ID round trip. Enrolment is silent; the read
/// raises the system biometric prompt, so this needs a Mac with
/// an enrolled fingerprint and a human to touch the sensor. The
/// throwaway store root keeps it clear of any real vault item.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "prompts for a real Touch ID and needs a fingerprint"]
fn touchid_round_trip_returns_the_enrolled_passphrase() {
    use std::process::Command;

    use antiphon_vault::{enrol_touchid, touchid};
    use secrecy::ExposeSecret;

    let dir = tempfile::tempdir().unwrap();
    let store_root = dir.path().join("store");
    let secret = throwaway_passphrase();

    enrol_touchid(&store_root, &secret).unwrap();

    let read = touchid::read_passphrase(&store_root).unwrap();
    assert_eq!(read.expose_secret(), secret.expose_secret());

    let _ = Command::new("security")
        .args([
            "delete-generic-password",
            "-s",
            "antiphon vault",
            "-a",
            &store_root.display().to_string(),
        ])
        .status();
}
