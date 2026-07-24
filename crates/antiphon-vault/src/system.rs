use std::ffi::OsString;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use secrecy::{ExposeSecret, SecretString};

use crate::vault::VaultError;

const STDERR_TAIL_CHARS: usize = 400;

#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

/// One external tool call. Secrets travel only via an
/// environment variable or stdin, never argv, which any local
/// process could list.
#[derive(Debug, Clone)]
pub struct Invocation {
    pub program: &'static str,
    pub args: Vec<OsString>,
    pub secret_env: Option<(&'static str, SecretString)>,
    pub secret_stdin: Option<SecretString>,
}

impl Invocation {
    pub fn new(
        program: &'static str,
        args: Vec<OsString>,
    ) -> Invocation {
        Invocation {
            program,
            args,
            secret_env: None,
            secret_stdin: None,
        }
    }

    pub fn with_secret_env(
        mut self,
        name: &'static str,
        secret: SecretString,
    ) -> Invocation {
        self.secret_env = Some((name, secret));
        self
    }

    pub fn with_secret_stdin(
        mut self,
        secret: SecretString,
    ) -> Invocation {
        self.secret_stdin = Some(secret);
        self
    }
}

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub status_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl RunOutput {
    pub fn success(&self) -> bool {
        self.status_code == Some(0)
    }
}

/// The slice of the operating system the backends touch,
/// abstracted so their logic is testable without a disk.
pub trait System {
    fn run(&self, invocation: &Invocation) -> io::Result<RunOutput>;
    fn ensure_dir(&self, path: &Path) -> io::Result<()>;
    fn path_exists(&self, path: &Path) -> bool;
    fn is_mount_point(&self, path: &Path) -> bool;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl System for SystemRunner {
    fn run(&self, invocation: &Invocation) -> io::Result<RunOutput> {
        let mut command = Command::new(invocation.program);
        command.args(&invocation.args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command.stdin(match invocation.secret_stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        });
        if let Some((name, secret)) = &invocation.secret_env {
            command.env(name, secret.expose_secret());
        }
        let mut child = command.spawn()?;
        feed_secret_stdin(&mut child, invocation)?;
        let output = child.wait_with_output()?;
        Ok(RunOutput {
            status_code: output.status.code(),
            stdout: lossy(output.stdout),
            stderr: lossy(output.stderr),
        })
    }

    fn ensure_dir(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)?;
        restrict_to_owner(path)
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn is_mount_point(&self, path: &Path) -> bool {
        device_differs_from_parent(path)
    }
}

fn feed_secret_stdin(
    child: &mut std::process::Child,
    invocation: &Invocation,
) -> io::Result<()> {
    let Some(secret) = &invocation.secret_stdin else {
        return Ok(());
    };
    let Some(mut stdin) = child.stdin.take() else {
        return Ok(());
    };
    stdin.write_all(secret.expose_secret().as_bytes())
}

fn lossy(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> io::Result<()> {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let mode = Permissions::from_mode(PRIVATE_DIR_MODE);
    std::fs::set_permissions(path, mode)
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn device_differs_from_parent(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(own) = std::fs::metadata(path) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return true;
    };
    let Ok(parents) = std::fs::metadata(parent) else {
        return false;
    };
    own.dev() != parents.dev()
}

#[cfg(not(unix))]
fn device_differs_from_parent(_path: &Path) -> bool {
    false
}

pub(crate) fn run_tool(
    system: &impl System,
    invocation: &Invocation,
) -> Result<RunOutput, VaultError> {
    let output = system.run(invocation)?;
    if !output.success() {
        return Err(tool_failure(invocation.program, &output));
    }
    Ok(output)
}

fn tool_failure(tool: &'static str, output: &RunOutput) -> VaultError {
    VaultError::Tool {
        tool,
        status: output.status_code,
        stderr_tail: tail(&output.stderr),
    }
}

fn tail(text: &str) -> String {
    let trimmed = text.trim_end();
    let chars: Vec<char> = trimmed.chars().collect();
    let start = chars.len().saturating_sub(STDERR_TAIL_CHARS);
    chars[start..].iter().collect()
}

#[cfg(test)]
pub(crate) mod fake {
    use std::cell::RefCell;
    use std::collections::{HashSet, VecDeque};
    use std::io;
    use std::path::{Path, PathBuf};

    use super::{Invocation, RunOutput, System};

    #[derive(Default)]
    pub struct FakeSystem {
        pub existing: HashSet<PathBuf>,
        pub mounted: HashSet<PathBuf>,
        pub scripted: RefCell<VecDeque<RunOutput>>,
        pub calls: RefCell<Vec<Invocation>>,
        pub ensured: RefCell<Vec<PathBuf>>,
    }

    impl FakeSystem {
        pub fn with_paths(
            existing: &[&Path],
            mounted: &[&Path],
        ) -> FakeSystem {
            FakeSystem {
                existing: owned_set(existing),
                mounted: owned_set(mounted),
                ..FakeSystem::default()
            }
        }

        pub fn script(&self, outputs: &[(i32, &str)]) {
            let mut queue = self.scripted.borrow_mut();
            for (code, stderr) in outputs {
                queue.push_back(RunOutput {
                    status_code: Some(*code),
                    stdout: String::new(),
                    stderr: (*stderr).to_owned(),
                });
            }
        }
    }

    fn owned_set(paths: &[&Path]) -> HashSet<PathBuf> {
        paths.iter().map(|path| path.to_path_buf()).collect()
    }

    impl System for FakeSystem {
        fn run(
            &self,
            invocation: &Invocation,
        ) -> io::Result<RunOutput> {
            self.calls.borrow_mut().push(invocation.clone());
            let scripted = self.scripted.borrow_mut().pop_front();
            Ok(scripted.unwrap_or(RunOutput {
                status_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
            }))
        }

        fn ensure_dir(&self, path: &Path) -> io::Result<()> {
            self.ensured.borrow_mut().push(path.to_path_buf());
            Ok(())
        }

        fn path_exists(&self, path: &Path) -> bool {
            self.existing.contains(path)
        }

        fn is_mount_point(&self, path: &Path) -> bool {
            self.mounted.contains(path)
        }
    }
}
