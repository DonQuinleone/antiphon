use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::system::{Invocation, System, SystemRunner, run_tool};
use crate::vault::{
    Auth, CreateOptions, Mounted, Vault, VaultError, VaultStatus,
    passphrase,
};

/// Sparse, so disk use grows with the store rather than being
/// reserved up front; this is only the growth ceiling.
const SPARSE_IMAGE_CAPACITY: &str = "32g";
const VOLUME_NAME: &str = "Antiphon Vault";

pub struct ApfsVault<S: System = SystemRunner> {
    image: PathBuf,
    mount_point: PathBuf,
    system: S,
}

impl ApfsVault<SystemRunner> {
    pub fn new(
        image: impl Into<PathBuf>,
        mount_point: impl Into<PathBuf>,
    ) -> ApfsVault<SystemRunner> {
        ApfsVault::with_system(image, mount_point, SystemRunner)
    }
}

impl<S: System> ApfsVault<S> {
    pub fn with_system(
        image: impl Into<PathBuf>,
        mount_point: impl Into<PathBuf>,
        system: S,
    ) -> ApfsVault<S> {
        ApfsVault {
            image: image.into(),
            mount_point: mount_point.into(),
            system,
        }
    }

    pub fn image(&self) -> &Path {
        &self.image
    }

    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }
}

impl<S: System> Vault for ApfsVault<S> {
    fn status(&self) -> VaultStatus {
        if !self.system.path_exists(&self.image) {
            return VaultStatus::Absent;
        }
        if self.system.is_mount_point(&self.mount_point) {
            return VaultStatus::Open;
        }
        VaultStatus::Sealed
    }

    fn create(&self, opts: &CreateOptions) -> Result<(), VaultError> {
        if self.status() != VaultStatus::Absent {
            return Err(VaultError::AlreadyExists(self.image.clone()));
        }
        let secret = passphrase(&opts.auth)?;
        if let Some(parent) = self.image.parent() {
            self.system.ensure_dir(parent)?;
        }
        let args: Vec<OsString> = vec![
            "create".into(),
            "-quiet".into(),
            "-encryption".into(),
            "AES-256".into(),
            "-stdinpass".into(),
            "-type".into(),
            "SPARSE".into(),
            "-fs".into(),
            "APFS".into(),
            "-size".into(),
            SPARSE_IMAGE_CAPACITY.into(),
            "-volname".into(),
            VOLUME_NAME.into(),
            self.image.clone().into(),
        ];
        let create = Invocation::new("hdiutil", args)
            .with_secret_stdin(secret.clone());
        run_tool(&self.system, &create)?;
        Ok(())
    }

    fn unlock(&self, auth: &Auth) -> Result<Mounted, VaultError> {
        if self.status() == VaultStatus::Absent {
            return Err(VaultError::Absent(self.image.clone()));
        }
        if self.system.is_mount_point(&self.mount_point) {
            return Ok(Mounted::new(&self.mount_point));
        }
        let secret = passphrase(auth)?;
        self.system.ensure_dir(&self.mount_point)?;
        let args: Vec<OsString> = vec![
            "attach".into(),
            "-quiet".into(),
            "-stdinpass".into(),
            "-nobrowse".into(),
            "-mountpoint".into(),
            self.mount_point.clone().into(),
            self.image.clone().into(),
        ];
        let attach = Invocation::new("hdiutil", args)
            .with_secret_stdin(secret.clone());
        run_tool(&self.system, &attach)?;
        Ok(Mounted::new(&self.mount_point))
    }

    fn lock(&self) -> Result<(), VaultError> {
        if !self.system.is_mount_point(&self.mount_point) {
            return Ok(());
        }
        let detach = Invocation::new(
            "hdiutil",
            vec![
                "detach".into(),
                "-quiet".into(),
                self.mount_point.clone().into(),
            ],
        );
        run_tool(&self.system, &detach)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use secrecy::{ExposeSecret, SecretString};

    use super::*;
    use crate::system::fake::FakeSystem;

    const PASS: &str = "test-passphrase";

    fn image() -> PathBuf {
        PathBuf::from("/data/antiphon/vault.sparseimage")
    }

    fn mount() -> PathBuf {
        PathBuf::from("/data/antiphon/store")
    }

    fn vault(system: FakeSystem) -> ApfsVault<FakeSystem> {
        ApfsVault::with_system(image(), mount(), system)
    }

    fn auth() -> Auth {
        Auth::Passphrase(SecretString::from(PASS.to_owned()))
    }

    fn argv_of(call: &Invocation) -> Vec<String> {
        call.args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn status_reflects_image_and_mount() {
        let cases = [
            (FakeSystem::default(), VaultStatus::Absent),
            (
                FakeSystem::with_paths(&[&image()], &[]),
                VaultStatus::Sealed,
            ),
            (
                FakeSystem::with_paths(&[&image()], &[&mount()]),
                VaultStatus::Open,
            ),
        ];
        for (system, expected) in cases {
            assert_eq!(vault(system).status(), expected);
        }
    }

    #[test]
    fn create_builds_an_encrypted_sparse_image() {
        let vault = vault(FakeSystem::default());
        vault.create(&CreateOptions { auth: auth() }).unwrap();
        let calls = vault.system.calls.borrow();
        assert_eq!(calls.len(), 1);
        let create = &calls[0];
        assert_eq!(create.program, "hdiutil");
        let argv = argv_of(create);
        for expected in [
            "create",
            "-encryption",
            "AES-256",
            "-stdinpass",
            "-type",
            "SPARSE",
            "-fs",
            "APFS",
        ] {
            assert!(argv.contains(&expected.to_owned()), "{argv:?}");
        }
        assert!(!argv.iter().any(|arg| arg.contains(PASS)));
        assert!(create.secret_env.is_none());
        let secret = create.secret_stdin.as_ref().unwrap();
        assert_eq!(secret.expose_secret(), PASS);
    }

    #[test]
    fn create_refuses_an_existing_image() {
        let system = FakeSystem::with_paths(&[&image()], &[]);
        let vault = vault(system);
        let err =
            vault.create(&CreateOptions { auth: auth() }).unwrap_err();
        assert!(matches!(err, VaultError::AlreadyExists(_)));
    }

    #[test]
    fn unlock_attaches_at_the_store_root() {
        let system = FakeSystem::with_paths(&[&image()], &[]);
        let vault = vault(system);
        let mounted = vault.unlock(&auth()).unwrap();
        assert_eq!(mounted.mount_point(), mount());
        let calls = vault.system.calls.borrow();
        let attach = &calls[0];
        let argv = argv_of(attach);
        assert_eq!(argv[0], "attach");
        assert!(argv.contains(&"-mountpoint".to_owned()));
        assert!(argv.contains(&mount().display().to_string()));
        assert!(!argv.iter().any(|arg| arg.contains(PASS)));
        assert!(attach.secret_stdin.is_some());
    }

    #[test]
    fn unlock_of_an_absent_image_errors() {
        let vault = vault(FakeSystem::default());
        let err = vault.unlock(&auth()).unwrap_err();
        assert!(matches!(err, VaultError::Absent(_)));
    }

    #[test]
    fn unlock_rejects_unimplemented_auth_methods() {
        let system = FakeSystem::with_paths(&[&image()], &[]);
        let vault = vault(system);
        let err = vault.unlock(&Auth::Yubikey).unwrap_err();
        assert!(matches!(err, VaultError::AuthUnsupported("yubikey")));
    }

    #[test]
    fn lock_detaches_the_mounted_volume() {
        let system = FakeSystem::with_paths(&[&image()], &[&mount()]);
        let vault = vault(system);
        vault.lock().unwrap();
        let calls = vault.system.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "hdiutil");
        assert_eq!(argv_of(&calls[0])[0], "detach");
    }

    #[test]
    fn lock_when_sealed_is_a_no_op() {
        let system = FakeSystem::with_paths(&[&image()], &[]);
        let vault = vault(system);
        vault.lock().unwrap();
        assert!(vault.system.calls.borrow().is_empty());
    }
}
