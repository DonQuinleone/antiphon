use std::ffi::OsString;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

use crate::system::{Invocation, System, SystemRunner, run_tool};
use crate::vault::{
    Auth, CreateOptions, Mounted, Vault, VaultError, VaultStatus,
    passphrase,
};

const CONFIG_FILE: &str = "gocryptfs.conf";
const PASSPHRASE_ENV: &str = "ANTIPHON_VAULT_PASSPHRASE";
const EXTPASS_SHELL: &str = "/bin/sh";

/// The passphrase must stay off argv, where any local process
/// could list it; gocryptfs instead runs this helper, which
/// prints the secret from an environment variable inherited
/// from the gocryptfs process alone.
fn extpass_print() -> String {
    format!("printf %s \"${PASSPHRASE_ENV}\"")
}

pub struct GocryptfsVault<S: System = SystemRunner> {
    cipherdir: PathBuf,
    mount_point: PathBuf,
    system: S,
}

impl GocryptfsVault<SystemRunner> {
    pub fn new(
        cipherdir: impl Into<PathBuf>,
        mount_point: impl Into<PathBuf>,
    ) -> GocryptfsVault<SystemRunner> {
        GocryptfsVault::with_system(
            cipherdir,
            mount_point,
            SystemRunner,
        )
    }
}

impl<S: System> GocryptfsVault<S> {
    pub fn with_system(
        cipherdir: impl Into<PathBuf>,
        mount_point: impl Into<PathBuf>,
        system: S,
    ) -> GocryptfsVault<S> {
        GocryptfsVault {
            cipherdir: cipherdir.into(),
            mount_point: mount_point.into(),
            system,
        }
    }

    pub fn cipherdir(&self) -> &Path {
        &self.cipherdir
    }

    pub fn mount_point(&self) -> &Path {
        &self.mount_point
    }

    fn config_path(&self) -> PathBuf {
        self.cipherdir.join(CONFIG_FILE)
    }

    fn command(
        &self,
        secret: &SecretString,
        trailing: Vec<OsString>,
    ) -> Invocation {
        // Each -extpass adds one argv element of the helper;
        // the joined -extpass=VALUE form is required because
        // gocryptfs's flag parser mangles a bare `-c` value.
        let mut args: Vec<OsString> = vec![
            "-q".into(),
            format!("-extpass={EXTPASS_SHELL}").into(),
            "-extpass=-c".into(),
            format!("-extpass={}", extpass_print()).into(),
        ];
        args.extend(trailing);
        Invocation::new("gocryptfs", args)
            .with_secret_env(PASSPHRASE_ENV, secret.clone())
    }
}

impl<S: System> Vault for GocryptfsVault<S> {
    fn status(&self) -> VaultStatus {
        if !self.system.path_exists(&self.config_path()) {
            return VaultStatus::Absent;
        }
        if self.system.is_mount_point(&self.mount_point) {
            return VaultStatus::Open;
        }
        VaultStatus::Sealed
    }

    fn create(&self, opts: &CreateOptions) -> Result<(), VaultError> {
        if self.status() != VaultStatus::Absent {
            return Err(VaultError::AlreadyExists(
                self.cipherdir.clone(),
            ));
        }
        let secret = passphrase(&opts.auth)?;
        self.system.ensure_dir(&self.cipherdir)?;
        let init = self.command(
            secret,
            vec!["-init".into(), self.cipherdir.clone().into()],
        );
        run_tool(&self.system, &init)?;
        Ok(())
    }

    fn unlock(&self, auth: &Auth) -> Result<Mounted, VaultError> {
        if self.status() == VaultStatus::Absent {
            return Err(VaultError::Absent(self.cipherdir.clone()));
        }
        if self.system.is_mount_point(&self.mount_point) {
            return Ok(Mounted::new(&self.mount_point));
        }
        let secret = passphrase(auth)?;
        self.system.ensure_dir(&self.mount_point)?;
        let mount = self.command(
            secret,
            vec![
                self.cipherdir.clone().into(),
                self.mount_point.clone().into(),
            ],
        );
        run_tool(&self.system, &mount)?;
        Ok(Mounted::new(&self.mount_point))
    }

    fn lock(&self) -> Result<(), VaultError> {
        if !self.system.is_mount_point(&self.mount_point) {
            return Ok(());
        }
        let unmount = Invocation::new(
            "umount",
            vec![self.mount_point.clone().into()],
        );
        let output = self.system.run(&unmount)?;
        if output.success() {
            return Ok(());
        }
        let fallback = Invocation::new(
            "diskutil",
            vec!["unmount".into(), self.mount_point.clone().into()],
        );
        run_tool(&self.system, &fallback)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::system::fake::FakeSystem;

    const PASS: &str = "test-passphrase";

    fn cipherdir() -> PathBuf {
        PathBuf::from("/data/antiphon/vault.gocryptfs")
    }

    fn config() -> PathBuf {
        cipherdir().join(CONFIG_FILE)
    }

    fn mount() -> PathBuf {
        PathBuf::from("/data/antiphon/store")
    }

    fn vault(system: FakeSystem) -> GocryptfsVault<FakeSystem> {
        GocryptfsVault::with_system(cipherdir(), mount(), system)
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
    fn status_reflects_config_and_mount() {
        let cases = [
            (FakeSystem::default(), VaultStatus::Absent),
            (
                FakeSystem::with_paths(&[&config()], &[]),
                VaultStatus::Sealed,
            ),
            (
                FakeSystem::with_paths(&[&config()], &[&mount()]),
                VaultStatus::Open,
            ),
        ];
        for (system, expected) in cases {
            assert_eq!(vault(system).status(), expected);
        }
    }

    #[test]
    fn create_refuses_an_existing_vault() {
        let system = FakeSystem::with_paths(&[&config()], &[]);
        let vault = vault(system);
        let err =
            vault.create(&CreateOptions { auth: auth() }).unwrap_err();
        assert!(matches!(err, VaultError::AlreadyExists(_)));
    }

    #[test]
    fn create_inits_with_the_passphrase_off_argv() {
        let vault = vault(FakeSystem::default());
        vault.create(&CreateOptions { auth: auth() }).unwrap();
        let calls = vault.system.calls.borrow();
        assert_eq!(calls.len(), 1);
        let init = &calls[0];
        assert_eq!(init.program, "gocryptfs");
        let argv = argv_of(init);
        assert!(argv.contains(&"-init".to_owned()));
        assert!(
            argv.contains(&format!("-extpass={}", extpass_print()))
        );
        assert!(!argv.iter().any(|arg| arg.contains(PASS)));
        let (name, secret) = init.secret_env.as_ref().unwrap();
        assert_eq!(*name, PASSPHRASE_ENV);
        assert_eq!(secret.expose_secret(), PASS);
        assert!(init.secret_stdin.is_none());
        assert_eq!(*vault.system.ensured.borrow(), vec![cipherdir()]);
    }

    #[test]
    fn unlock_mounts_the_cipherdir_at_the_store_root() {
        let system = FakeSystem::with_paths(&[&config()], &[]);
        let vault = vault(system);
        let mounted = vault.unlock(&auth()).unwrap();
        assert_eq!(mounted.mount_point(), mount());
        let calls = vault.system.calls.borrow();
        let argv = argv_of(&calls[0]);
        let tail = &argv[argv.len() - 2..];
        assert_eq!(
            tail,
            [
                cipherdir().display().to_string(),
                mount().display().to_string(),
            ]
        );
        assert!(!argv.iter().any(|arg| arg.contains(PASS)));
    }

    #[test]
    fn unlock_of_an_absent_vault_errors() {
        let vault = vault(FakeSystem::default());
        let err = vault.unlock(&auth()).unwrap_err();
        assert!(matches!(err, VaultError::Absent(_)));
    }

    #[test]
    fn unlock_when_already_open_runs_nothing() {
        let system = FakeSystem::with_paths(&[&config()], &[&mount()]);
        let vault = vault(system);
        let mounted = vault.unlock(&auth()).unwrap();
        assert_eq!(mounted.mount_point(), mount());
        assert!(vault.system.calls.borrow().is_empty());
    }

    #[test]
    fn unlock_rejects_unimplemented_auth_methods() {
        let system = FakeSystem::with_paths(&[&config()], &[]);
        let vault = vault(system);
        let err = vault.unlock(&Auth::Touchid).unwrap_err();
        assert!(matches!(err, VaultError::AuthUnsupported("touchid")));
    }

    #[test]
    fn lock_unmounts_the_store_root() {
        let system = FakeSystem::with_paths(&[&config()], &[&mount()]);
        let vault = vault(system);
        vault.lock().unwrap();
        let calls = vault.system.calls.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].program, "umount");
        assert_eq!(
            argv_of(&calls[0]),
            vec![mount().display().to_string()]
        );
    }

    #[test]
    fn lock_falls_back_to_diskutil_when_umount_fails() {
        let system = FakeSystem::with_paths(&[&config()], &[&mount()]);
        system.script(&[(1, "busy"), (0, "")]);
        let vault = vault(system);
        vault.lock().unwrap();
        let calls = vault.system.calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].program, "diskutil");
        assert_eq!(argv_of(&calls[1])[0], "unmount");
    }

    #[test]
    fn lock_when_sealed_is_a_no_op() {
        let system = FakeSystem::with_paths(&[&config()], &[]);
        let vault = vault(system);
        vault.lock().unwrap();
        assert!(vault.system.calls.borrow().is_empty());
    }
}
