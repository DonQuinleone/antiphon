//! LUKS2 vault backend (DESIGN.md section 5): a loopback
//! container file holding a LUKS2 volume with ext4 inside,
//! mounted at the store root. Antiphon only orchestrates the
//! system tools; it contains no crypto of its own.
//!
//! Privileged steps run under `sudo -n`; antiphond documents
//! the sudoers entries it needs and never prompts, so a sudo
//! that would ask for a password surfaces as
//! `VaultError::SudoNeedsPassword`. The passphrase travels only
//! on stdin via `--key-file=-` (cryptsetup reads stdin
//! verbatim, so the same bytes unlock here and interactively as
//! long as no newline is appended), never on argv.
//!
//! Command lines verified against cryptsetup(8),
//! cryptsetup-luksFormat(8) and cryptsetup-open(8) at
//! man.archlinux.org: `--key-file=-` reads stdin, a regular
//! file device argument is auto-mapped through a loop device,
//! and `--batch-mode` suppresses the luksFormat confirmation.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use secrecy::SecretString;

use crate::system::{Invocation, System, SystemRunner};
use crate::vault::{
    Auth, CreateOptions, Mounted, Vault, VaultError, VaultStatus,
    passphrase,
};

const SUDO: &str = "sudo";
const MAPPER_DIR: &str = "/dev/mapper";
const SUDO_REFUSAL_MARKS: [&str; 2] =
    ["password is required", "a terminal is required"];

pub enum EnrollMethod {
    Fido2,
    PgpSmartcard,
}

pub struct Luks2Vault<S: System = SystemRunner> {
    container: PathBuf,
    mapper: String,
    mount_point: PathBuf,
    owner: String,
    system: S,
}

impl Luks2Vault<SystemRunner> {
    pub fn new(
        container: impl Into<PathBuf>,
        mapper: impl Into<String>,
        mount_point: impl Into<PathBuf>,
        owner: impl Into<String>,
    ) -> Luks2Vault<SystemRunner> {
        Luks2Vault::with_system(
            container,
            mapper,
            mount_point,
            owner,
            SystemRunner,
        )
    }
}

impl<S: System> Luks2Vault<S> {
    pub fn with_system(
        container: impl Into<PathBuf>,
        mapper: impl Into<String>,
        mount_point: impl Into<PathBuf>,
        owner: impl Into<String>,
        system: S,
    ) -> Luks2Vault<S> {
        Luks2Vault {
            container: container.into(),
            mapper: mapper.into(),
            mount_point: mount_point.into(),
            owner: owner.into(),
            system,
        }
    }

    /// Extra keyslots (FIDO2 token, PGP smartcard) enrol via
    /// systemd-cryptenroll (`--fido2-device=auto`,
    /// `--pkcs11-token-uri=auto`); the design names it for
    /// v1-later, so this is a documented no-op until there is
    /// hardware to test against.
    pub fn enroll(
        &self,
        _method: EnrollMethod,
    ) -> Result<(), VaultError> {
        Err(VaultError::NotYet("keyslot enrolment"))
    }

    fn mapper_device(&self) -> PathBuf {
        Path::new(MAPPER_DIR).join(&self.mapper)
    }

    fn mapper_present(&self) -> bool {
        self.system.path_exists(&self.mapper_device())
    }

    fn mounted(&self) -> bool {
        self.system.is_mount_point(&self.mount_point)
    }

    fn luks_format(&self) -> Vec<OsString> {
        args(&[
            "cryptsetup",
            "luksFormat",
            "--type",
            "luks2",
            "--batch-mode",
            "--key-file=-",
        ])
        .into_iter()
        .chain([self.container.clone().into()])
        .collect()
    }

    fn luks_open(&self) -> Vec<OsString> {
        args(&["cryptsetup", "open", "--type", "luks2", "--key-file=-"])
            .into_iter()
            .chain([
                self.container.clone().into(),
                self.mapper.clone().into(),
            ])
            .collect()
    }

    fn mkfs(&self) -> Vec<OsString> {
        vec![
            "mkfs.ext4".into(),
            "-q".into(),
            self.mapper_device().into(),
        ]
    }

    fn mount(&self) -> Vec<OsString> {
        vec![
            "mount".into(),
            self.mapper_device().into(),
            self.mount_point.clone().into(),
        ]
    }

    fn chown(&self) -> Vec<OsString> {
        vec![
            "chown".into(),
            self.owner.clone().into(),
            self.mount_point.clone().into(),
        ]
    }

    fn umount(&self) -> Vec<OsString> {
        vec!["umount".into(), self.mount_point.clone().into()]
    }

    fn close(&self) -> Vec<OsString> {
        vec![
            "cryptsetup".into(),
            "close".into(),
            self.mapper.clone().into(),
        ]
    }

    fn privileged(
        &self,
        tool_args: Vec<OsString>,
        secret: Option<&SecretString>,
    ) -> Result<(), VaultError> {
        let mut full: Vec<OsString> = vec!["-n".into()];
        full.extend(tool_args);
        let command = display(&full);
        let mut invocation = Invocation::new(SUDO, full);
        if let Some(secret) = secret {
            invocation = invocation.with_secret_stdin(secret.clone());
        }
        let output = self.system.run(&invocation)?;
        if output.success() {
            return Ok(());
        }
        if sudo_refused(&output.stderr) {
            return Err(VaultError::SudoNeedsPassword { command });
        }
        Err(VaultError::Tool {
            tool: SUDO,
            status: output.status_code,
            stderr_tail: output.stderr.trim_end().to_owned(),
        })
    }
}

impl<S: System> Vault for Luks2Vault<S> {
    fn status(&self) -> VaultStatus {
        if !self.system.path_exists(&self.container) {
            return VaultStatus::Absent;
        }
        if self.mounted() && self.mapper_present() {
            return VaultStatus::Open;
        }
        VaultStatus::Sealed
    }

    fn create(&self, opts: &CreateOptions) -> Result<(), VaultError> {
        if self.status() != VaultStatus::Absent {
            return Err(VaultError::AlreadyExists(
                self.container.clone(),
            ));
        }
        let secret = passphrase(&opts.auth)?;
        self.system.allocate(&self.container, opts.size_bytes)?;
        self.privileged(self.luks_format(), Some(secret))?;
        self.privileged(self.luks_open(), Some(secret))?;
        self.privileged(self.mkfs(), None)?;
        self.system.ensure_dir(&self.mount_point)?;
        self.privileged(self.mount(), None)?;
        self.privileged(self.chown(), None)
    }

    fn unlock(&self, auth: &Auth) -> Result<Mounted, VaultError> {
        if !self.system.path_exists(&self.container) {
            return Err(VaultError::Absent(self.container.clone()));
        }
        if self.mounted() {
            return Ok(Mounted::new(&self.mount_point));
        }
        if !self.mapper_present() {
            let secret = passphrase(auth)?;
            self.privileged(self.luks_open(), Some(secret))?;
        }
        self.system.ensure_dir(&self.mount_point)?;
        self.privileged(self.mount(), None)?;
        Ok(Mounted::new(&self.mount_point))
    }

    fn lock(&self) -> Result<(), VaultError> {
        if self.mounted() {
            self.privileged(self.umount(), None)?;
        }
        if self.mapper_present() {
            self.privileged(self.close(), None)?;
        }
        Ok(())
    }
}

fn args(parts: &[&str]) -> Vec<OsString> {
    parts.iter().map(OsString::from).collect()
}

fn display(args: &[OsString]) -> String {
    let mut out = SUDO.to_owned();
    for arg in args {
        out.push(' ');
        out.push_str(&arg.to_string_lossy());
    }
    out
}

fn sudo_refused(stderr: &str) -> bool {
    stderr.lines().any(|line| {
        line.starts_with("sudo:")
            && SUDO_REFUSAL_MARKS.iter().any(|mark| line.contains(mark))
    })
}

#[cfg(test)]
mod tests {
    use secrecy::ExposeSecret;

    use super::*;
    use crate::system::fake::FakeSystem;

    const PASS: &str = "test-passphrase";
    const MAPPER: &str = "antiphon-test";

    fn container() -> PathBuf {
        PathBuf::from("/data/antiphon/vault.luks")
    }

    fn mount() -> PathBuf {
        PathBuf::from("/data/antiphon/store")
    }

    fn mapper_dev() -> PathBuf {
        PathBuf::from("/dev/mapper").join(MAPPER)
    }

    fn vault(system: FakeSystem) -> Luks2Vault<FakeSystem> {
        Luks2Vault::with_system(
            container(),
            MAPPER,
            mount(),
            "quin",
            system,
        )
    }

    fn auth() -> Auth {
        Auth::Passphrase(SecretString::from(PASS.to_owned()))
    }

    fn argv(call: &Invocation) -> Vec<String> {
        call.args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn status_truth_table() {
        let cases = [
            (FakeSystem::default(), VaultStatus::Absent),
            (
                FakeSystem::with_paths(&[&container()], &[]),
                VaultStatus::Sealed,
            ),
            (
                FakeSystem::with_paths(&[&container()], &[&mount()]),
                VaultStatus::Sealed,
            ),
            (
                FakeSystem::with_paths(
                    &[&container(), &mapper_dev()],
                    &[&mount()],
                ),
                VaultStatus::Open,
            ),
        ];
        for (system, expected) in cases {
            assert_eq!(vault(system).status(), expected);
        }
    }

    #[test]
    fn create_runs_the_full_sequence_passphrase_on_stdin() {
        let vault = vault(FakeSystem::default());
        vault.create(&CreateOptions::new(auth())).unwrap();
        let calls = vault.system.calls.borrow();
        let programs: Vec<&str> =
            calls.iter().map(|c| c.program).collect();
        assert_eq!(programs, ["sudo"; 5]);
        let joined: Vec<Vec<String>> = calls.iter().map(argv).collect();
        assert!(joined[0].contains(&"luksFormat".to_owned()));
        assert!(joined[1].contains(&"open".to_owned()));
        assert!(joined[2].contains(&"mkfs.ext4".to_owned()));
        assert!(joined[3].contains(&"mount".to_owned()));
        assert!(joined[4].contains(&"chown".to_owned()));
        for call in calls.iter() {
            assert!(
                !argv(call).iter().any(|arg| arg.contains(PASS)),
                "passphrase leaked to argv"
            );
        }
        let secret_calls: Vec<&Invocation> =
            calls.iter().filter(|c| c.secret_stdin.is_some()).collect();
        assert_eq!(secret_calls.len(), 2);
        for call in secret_calls {
            assert_eq!(
                call.secret_stdin.as_ref().unwrap().expose_secret(),
                PASS
            );
        }
        assert_eq!(
            *vault.system.allocated.borrow(),
            vec![(container(), crate::vault::DEFAULT_VAULT_BYTES)]
        );
    }

    #[test]
    fn create_refuses_a_present_vault() {
        let system = FakeSystem::with_paths(&[&container()], &[]);
        let err = vault(system)
            .create(&CreateOptions::new(auth()))
            .unwrap_err();
        assert!(matches!(err, VaultError::AlreadyExists(_)));
    }

    #[test]
    fn unlock_skips_open_when_the_mapper_lingers() {
        let system =
            FakeSystem::with_paths(&[&container(), &mapper_dev()], &[]);
        let vault = vault(system);
        vault.unlock(&auth()).unwrap();
        let calls = vault.system.calls.borrow();
        let opened =
            calls.iter().any(|c| argv(c).contains(&"open".to_owned()));
        assert!(!opened, "reopened an already-mapped container");
        assert!(
            calls.iter().any(|c| argv(c).contains(&"mount".to_owned()))
        );
    }

    #[test]
    fn unlock_on_a_missing_container_reports_absent() {
        let err =
            vault(FakeSystem::default()).unlock(&auth()).unwrap_err();
        assert!(matches!(err, VaultError::Absent(_)));
    }

    #[test]
    fn lock_umounts_and_closes_when_open() {
        let system = FakeSystem::with_paths(
            &[&container(), &mapper_dev()],
            &[&mount()],
        );
        let vault = vault(system);
        vault.lock().unwrap();
        let calls = vault.system.calls.borrow();
        assert!(
            calls
                .iter()
                .any(|c| argv(c).contains(&"umount".to_owned()))
        );
        assert!(
            calls.iter().any(|c| argv(c).contains(&"close".to_owned()))
        );
    }

    #[test]
    fn a_sudo_password_prompt_is_named_as_such() {
        let system = FakeSystem::default();
        system.script(&[(1, "sudo: a password is required")]);
        let err = vault(system)
            .create(&CreateOptions::new(auth()))
            .unwrap_err();
        assert!(matches!(err, VaultError::SudoNeedsPassword { .. }));
    }

    #[test]
    fn a_tool_error_is_not_mistaken_for_a_sudo_refusal() {
        let system = FakeSystem::default();
        system.script(&[(
            1,
            "cryptsetup: a password is required for the keyslot",
        )]);
        let err = vault(system)
            .create(&CreateOptions::new(auth()))
            .unwrap_err();
        assert!(matches!(err, VaultError::Tool { .. }));
    }

    #[test]
    fn enrolment_is_not_yet() {
        let err = vault(FakeSystem::default())
            .enroll(EnrollMethod::Fido2)
            .unwrap_err();
        assert!(matches!(err, VaultError::NotYet(_)));
    }
}
