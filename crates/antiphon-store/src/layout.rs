use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
const PRIVATE_DIR_MODE: u32 = 0o700;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreLayout {
    root: PathBuf,
}

impl StoreLayout {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn maildir_root(&self) -> PathBuf {
        self.root.join("maildir")
    }

    pub fn account_maildir(&self, account: &str) -> PathBuf {
        self.maildir_root().join(account)
    }

    pub fn notmuch_dir(&self) -> PathBuf {
        self.root.join("notmuch")
    }

    pub fn oplog_dir(&self) -> PathBuf {
        self.root.join("oplog")
    }

    pub fn outbox_dir(&self) -> PathBuf {
        self.root.join("outbox")
    }

    pub fn tokens_dir(&self) -> PathBuf {
        self.root.join("tokens")
    }

    pub fn contacts_dir(&self) -> PathBuf {
        self.root.join("contacts")
    }

    pub fn sync_state_dir(&self) -> PathBuf {
        self.root.join("sync_state")
    }

    pub fn notmuch_config_path(&self) -> PathBuf {
        self.root.join("notmuch.conf")
    }

    pub fn exists(&self) -> bool {
        self.directories().iter().all(|dir| dir.is_dir())
    }

    /// Idempotent; re-runs also repair drifted permissions and
    /// rewrite the store's notmuch config, which pins mail_root
    /// so message filenames resolve identically for every
    /// opener.
    pub fn init(&self) -> io::Result<()> {
        for dir in self.directories() {
            create_private_dir(&dir)?;
        }
        std::fs::write(
            self.notmuch_config_path(),
            self.notmuch_config(),
        )?;
        self.create_index()
    }

    /// The first sync would create the index anyway, but a
    /// fresh store must open cleanly before that sync has
    /// happened.
    fn create_index(&self) -> io::Result<()> {
        if self.notmuch_dir().join(".notmuch").exists() {
            return Ok(());
        }
        notmuch::Database::create(self.notmuch_dir())
            .map(|_| ())
            .map_err(io::Error::other)
    }

    fn notmuch_config(&self) -> String {
        format!(
            "[database]\n\
             path={}\n\
             mail_root={}\n\
             [new]\n\
             tags=unread;inbox\n\
             [maildir]\n\
             synchronize_flags=true\n",
            self.notmuch_dir().display(),
            self.maildir_root().display(),
        )
    }

    fn directories(&self) -> [PathBuf; 8] {
        [
            self.root.clone(),
            self.maildir_root(),
            self.notmuch_dir(),
            self.oplog_dir(),
            self.outbox_dir(),
            self.tokens_dir(),
            self.contacts_dir(),
            self.sync_state_dir(),
        ]
    }
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    restrict_to_owner(dir)
}

#[cfg(unix)]
fn restrict_to_owner(dir: &Path) -> io::Result<()> {
    use std::fs::Permissions;
    use std::os::unix::fs::PermissionsExt;

    let mode = Permissions::from_mode(PRIVATE_DIR_MODE);
    std::fs::set_permissions(dir, mode)
}

#[cfg(not(unix))]
fn restrict_to_owner(_dir: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_in(dir: &tempfile::TempDir) -> StoreLayout {
        StoreLayout::new(dir.path().join("store"))
    }

    #[test]
    fn paths_hang_off_the_root() {
        let layout = StoreLayout::new("/vault/store");
        let root = Path::new("/vault/store");
        assert_eq!(layout.root(), root);
        assert_eq!(layout.maildir_root(), root.join("maildir"));
        assert_eq!(
            layout.account_maildir("work"),
            root.join("maildir/work")
        );
        assert_eq!(layout.notmuch_dir(), root.join("notmuch"));
        assert_eq!(layout.oplog_dir(), root.join("oplog"));
        assert_eq!(layout.outbox_dir(), root.join("outbox"));
        assert_eq!(layout.tokens_dir(), root.join("tokens"));
        assert_eq!(layout.contacts_dir(), root.join("contacts"));
        assert_eq!(layout.sync_state_dir(), root.join("sync_state"));
    }

    #[test]
    fn init_creates_all_directories() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        layout.init().unwrap();
        for path in [
            layout.maildir_root(),
            layout.notmuch_dir(),
            layout.oplog_dir(),
            layout.outbox_dir(),
            layout.tokens_dir(),
            layout.contacts_dir(),
            layout.sync_state_dir(),
        ] {
            assert!(path.is_dir(), "missing {}", path.display());
        }
    }

    #[test]
    fn exists_only_once_the_full_layout_is_present() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        assert!(!layout.exists());
        layout.init().unwrap();
        assert!(layout.exists());
        std::fs::remove_dir(layout.oplog_dir()).unwrap();
        assert!(!layout.exists());
    }

    #[test]
    fn a_fresh_init_yields_an_openable_index() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        layout.init().unwrap();
        crate::SearchIndex::open(&layout).unwrap();
    }

    #[test]
    fn init_twice_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        layout.init().unwrap();
        layout.init().unwrap();
        assert!(layout.oplog_dir().is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn init_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        layout.init().unwrap();
        for path in [layout.root().to_path_buf(), layout.tokens_dir()] {
            let mode =
                std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                PRIVATE_DIR_MODE,
                "wrong mode on {}",
                path.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn init_reasserts_permissions_on_existing_dirs() {
        use std::fs::Permissions;
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        layout.init().unwrap();
        std::fs::set_permissions(
            layout.tokens_dir(),
            Permissions::from_mode(0o755),
        )
        .unwrap();
        layout.init().unwrap();
        let mode = std::fs::metadata(layout.tokens_dir())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, PRIVATE_DIR_MODE);
    }
}
