use std::io;
use std::path::PathBuf;

use antiphon_config::VaultBackend;
use antiphon_store::StoreLayout;

use crate::apfs::ApfsVault;
use crate::gocryptfs::GocryptfsVault;
use crate::luks2::Luks2Vault;
use crate::vault::{Vault, VaultError};

const GOCRYPTFS_DIR: &str = "vault.gocryptfs";
const APFS_IMAGE: &str = "vault.sparseimage";
const LUKS_CONTAINER: &str = "vault.luks";
const MAPPER_PREFIX: &str = "antiphon";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resolved {
    Apfs,
    Gocryptfs,
    Luks2,
}

pub fn select_backend(
    configured: VaultBackend,
    layout: &StoreLayout,
) -> Result<Box<dyn Vault>, VaultError> {
    build(resolve(configured, std::env::consts::OS), layout)
}

fn resolve(configured: VaultBackend, os: &str) -> Resolved {
    match configured {
        VaultBackend::Apfs => Resolved::Apfs,
        VaultBackend::Gocryptfs => Resolved::Gocryptfs,
        VaultBackend::Luks2 => Resolved::Luks2,
        VaultBackend::Auto => default_for(os),
    }
}

fn default_for(os: &str) -> Resolved {
    match os {
        "macos" => Resolved::Apfs,
        "linux" => Resolved::Luks2,
        _ => Resolved::Gocryptfs,
    }
}

fn build(
    resolved: Resolved,
    layout: &StoreLayout,
) -> Result<Box<dyn Vault>, VaultError> {
    let home = vault_home(layout)?;
    let mount = layout.root().to_path_buf();
    match resolved {
        Resolved::Apfs => {
            Ok(Box::new(ApfsVault::new(home.join(APFS_IMAGE), mount)))
        }
        Resolved::Gocryptfs => Ok(Box::new(GocryptfsVault::new(
            home.join(GOCRYPTFS_DIR),
            mount,
        ))),
        Resolved::Luks2 => Ok(Box::new(Luks2Vault::new(
            home.join(LUKS_CONTAINER),
            mapper_name(layout),
            mount,
            current_user(),
        ))),
    }
}

/// A device-mapper name unique to this store, so two stores on
/// one machine never collide on /dev/mapper.
fn mapper_name(layout: &StoreLayout) -> String {
    let mut hash: u64 = 1469598103934665603;
    for byte in layout.root().as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{MAPPER_PREFIX}-{hash:016x}")
}

fn current_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_owned())
}

/// The ciphertext container lives beside the store root, so it
/// is never inside its own mount.
fn vault_home(layout: &StoreLayout) -> Result<PathBuf, VaultError> {
    let root = layout.root();
    let Some(parent) = root.parent() else {
        return Err(VaultError::Io(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "store root {} has no parent directory to \
                 hold the vault container",
                root.display()
            ),
        )));
    };
    Ok(parent.to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn resolution_honours_config_then_platform() {
        let cases = [
            (VaultBackend::Apfs, "linux", Resolved::Apfs),
            (VaultBackend::Gocryptfs, "macos", Resolved::Gocryptfs),
            (VaultBackend::Luks2, "macos", Resolved::Luks2),
            (VaultBackend::Auto, "macos", Resolved::Apfs),
            (VaultBackend::Auto, "linux", Resolved::Luks2),
            (VaultBackend::Auto, "freebsd", Resolved::Gocryptfs),
        ];
        for (configured, os, expected) in cases {
            assert_eq!(
                resolve(configured, os),
                expected,
                "{configured:?} on {os}"
            );
        }
    }

    #[test]
    fn mapper_names_are_stable_and_store_specific() {
        let one = StoreLayout::new("/data/a/store");
        let two = StoreLayout::new("/data/b/store");
        assert_eq!(mapper_name(&one), mapper_name(&one));
        assert_ne!(mapper_name(&one), mapper_name(&two));
        assert!(mapper_name(&one).starts_with("antiphon-"));
    }

    #[test]
    fn containers_sit_beside_the_store_root() {
        let layout = StoreLayout::new("/data/antiphon/store");
        let home = vault_home(&layout).unwrap();
        assert_eq!(home, Path::new("/data/antiphon"));
    }

    #[test]
    fn a_rootless_store_root_is_rejected() {
        let layout = StoreLayout::new("/");
        let err = vault_home(&layout).unwrap_err();
        assert!(matches!(err, VaultError::Io(_)));
    }

    #[test]
    fn built_backends_start_absent_on_missing_paths() {
        let layout = StoreLayout::new("/nonexistent/antiphon/store");
        for backend in [Resolved::Apfs, Resolved::Gocryptfs] {
            let vault = build(backend, &layout).unwrap();
            assert_eq!(
                vault.status(),
                crate::vault::VaultStatus::Absent
            );
        }
    }
}
