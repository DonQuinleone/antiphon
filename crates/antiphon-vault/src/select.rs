use std::io;
use std::path::PathBuf;

use antiphon_config::VaultBackend;
use antiphon_store::StoreLayout;

use crate::apfs::ApfsVault;
use crate::gocryptfs::GocryptfsVault;
use crate::vault::{Vault, VaultError};

const GOCRYPTFS_DIR: &str = "vault.gocryptfs";
const APFS_IMAGE: &str = "vault.sparseimage";

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
        // The luks2 backend lands from the wt/luks branch;
        // until it merges, selecting it reports itself
        // unsupported rather than pretending to seal anything.
        Resolved::Luks2 => {
            Err(VaultError::UnsupportedOnThisBuild("luks2"))
        }
    }
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
    fn luks2_reports_unsupported_on_this_build() {
        let layout = StoreLayout::new("/data/antiphon/store");
        let Err(err) = build(Resolved::Luks2, &layout) else {
            panic!("luks2 built despite being unsupported");
        };
        assert!(matches!(
            err,
            VaultError::UnsupportedOnThisBuild("luks2")
        ));
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
