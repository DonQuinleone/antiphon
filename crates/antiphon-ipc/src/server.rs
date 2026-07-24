use std::fs;
use std::io;
use std::ops::ControlFlow;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

const SOCKET_DIR_MODE: u32 = 0o700;
const SOCKET_FILE_MODE: u32 = 0o600;

#[derive(Debug)]
pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
}

impl IpcServer {
    pub fn bind(path: &Path) -> io::Result<IpcServer> {
        prepare_directory(path)?;
        remove_stale_socket(path)?;
        let listener = UnixListener::bind(path)?;
        fs::set_permissions(
            path,
            fs::Permissions::from_mode(SOCKET_FILE_MODE),
        )?;
        Ok(IpcServer {
            listener,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn accept(&self) -> io::Result<UnixStream> {
        let (stream, _) = self.listener.accept()?;
        Ok(stream)
    }

    pub fn serve(
        &self,
        mut handle: impl FnMut(UnixStream) -> ControlFlow<()>,
    ) -> io::Result<()> {
        loop {
            let stream = self.accept()?;
            if handle(stream).is_break() {
                return Ok(());
            }
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn prepare_directory(path: &Path) -> io::Result<()> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true).mode(SOCKET_DIR_MODE);
    builder.create(dir)?;
    fs::set_permissions(
        dir,
        fs::Permissions::from_mode(SOCKET_DIR_MODE),
    )
}

fn remove_stale_socket(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    // A connect probe distinguishes a live daemon, which must
    // never be displaced, from a socket left by a dead one.
    if UnixStream::connect(path).is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("a live daemon owns {}", path.display()),
        ));
    }
    fs::remove_file(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn socket_in_fresh_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ipc").join("antiphond.sock");
        (dir, path)
    }

    fn mode_of(path: &Path) -> u32 {
        let all_permission_bits = 0o777;
        fs::metadata(path).unwrap().permissions().mode()
            & all_permission_bits
    }

    #[test]
    fn a_stale_socket_file_is_swept_aside() {
        let (_dir, path) = socket_in_fresh_dir();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
        let server = IpcServer::bind(&path).unwrap();
        UnixStream::connect(server.path()).unwrap();
    }

    #[test]
    fn a_live_daemon_is_never_displaced() {
        let (_dir, path) = socket_in_fresh_dir();
        let _first = IpcServer::bind(&path).unwrap();
        let error = IpcServer::bind(&path).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
        UnixStream::connect(&path).unwrap();
    }

    #[test]
    fn the_socket_is_gone_after_drop() {
        let (_dir, path) = socket_in_fresh_dir();
        let server = IpcServer::bind(&path).unwrap();
        drop(server);
        assert!(!path.exists());
    }

    #[test]
    fn directory_and_socket_stay_private() {
        let (_dir, path) = socket_in_fresh_dir();
        let _server = IpcServer::bind(&path).unwrap();
        assert_eq!(mode_of(path.parent().unwrap()), 0o700);
        assert_eq!(mode_of(&path), 0o600);
    }
}
