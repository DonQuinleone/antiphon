use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

const DIR_TMP: &str = "tmp";
const DIR_NEW: &str = "new";
const DIR_CUR: &str = "cur";
const FLAG_SUFFIX: &str = ":2,";
const UID_MARKER: &str = ",U=";
const SEEN_FLAG: char = 'S';
const WRITER_NAME: &str = "antiphon";

static DELIVERY_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Subdir {
    New,
    Cur,
}

impl Subdir {
    fn as_str(self) -> &'static str {
        match self {
            Self::New => DIR_NEW,
            Self::Cur => DIR_CUR,
        }
    }
}

#[derive(Debug)]
pub(crate) struct LocalMessage {
    pub uid: u32,
    pub seen: bool,
    pub subdir: Subdir,
    pub name: String,
}

pub(crate) struct MaildirFolder {
    root: PathBuf,
}

impl MaildirFolder {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn ensure(&self) -> io::Result<()> {
        for sub in [DIR_TMP, DIR_NEW, DIR_CUR] {
            fs::create_dir_all(self.root.join(sub))?;
        }
        Ok(())
    }

    /// Maildir delivery: write into tmp/, then rename into
    /// new/ (or cur/ with the seen flag), so readers never
    /// observe a partial message. Returns the delivered path.
    pub fn deliver(
        &self,
        uid: u32,
        seen: bool,
        content: &[u8],
    ) -> io::Result<PathBuf> {
        let base = unique_name(uid);
        let (subdir, name) = if seen {
            (DIR_CUR, format!("{base}{FLAG_SUFFIX}{SEEN_FLAG}"))
        } else {
            (DIR_NEW, base.clone())
        };
        let staging = self.root.join(DIR_TMP).join(&base);
        fs::write(&staging, content)?;
        let target = self.root.join(subdir).join(name);
        fs::rename(&staging, &target)?;
        Ok(target)
    }

    pub fn scan(&self) -> io::Result<Vec<LocalMessage>> {
        let mut found = Vec::new();
        for subdir in [Subdir::New, Subdir::Cur] {
            let dir = self.root.join(subdir.as_str());
            if !dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let name =
                    entry?.file_name().to_string_lossy().into_owned();
                let Some(uid) = parse_uid(&name) else {
                    continue;
                };
                found.push(LocalMessage {
                    uid,
                    seen: has_seen(&name),
                    subdir,
                    name,
                });
            }
        }
        Ok(found)
    }

    /// Mirrors the server's seen flag onto the local filename;
    /// a message gaining the flag also graduates from new/ to
    /// cur/, per maildir semantics.
    pub fn mirror_seen(
        &self,
        message: &LocalMessage,
        seen: bool,
    ) -> io::Result<()> {
        let renamed = with_seen(&message.name, seen);
        let target_subdir = match message.subdir {
            Subdir::New if seen => Subdir::Cur,
            other => other,
        };
        fs::rename(
            self.path_of(message.subdir, &message.name),
            self.path_of(target_subdir, &renamed),
        )
    }

    pub fn remove(&self, message: &LocalMessage) -> io::Result<()> {
        fs::remove_file(self.path_of(message.subdir, &message.name))
    }

    /// Deletes every message this engine delivered (marked
    /// with the UID field), used when UIDVALIDITY changes and
    /// the folder must be refetched from scratch.
    pub fn remove_delivered(&self) -> io::Result<()> {
        for message in self.scan()? {
            self.remove(&message)?;
        }
        Ok(())
    }

    fn path_of(&self, subdir: Subdir, name: &str) -> PathBuf {
        self.root.join(subdir.as_str()).join(name)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn unique_name(uid: u32) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seq = DELIVERY_SEQ.fetch_add(1, Ordering::Relaxed);
    format!(
        "{}.M{}P{}Q{seq}.{WRITER_NAME}{UID_MARKER}{uid}",
        now.as_secs(),
        now.subsec_micros(),
        std::process::id(),
    )
}

fn split_flags(name: &str) -> (&str, Option<&str>) {
    match name.split_once(FLAG_SUFFIX) {
        Some((base, flags)) => (base, Some(flags)),
        None => (name, None),
    }
}

pub(crate) fn parse_uid(name: &str) -> Option<u32> {
    let (base, _) = split_flags(name);
    let (_, after) = base.rsplit_once(UID_MARKER)?;
    after.parse().ok()
}

pub(crate) fn has_seen(name: &str) -> bool {
    let (_, flags) = split_flags(name);
    flags.is_some_and(|flags| flags.contains(SEEN_FLAG))
}

pub(crate) fn with_seen(name: &str, seen: bool) -> String {
    let (base, flags) = split_flags(name);
    let mut set: Vec<char> = flags
        .unwrap_or_default()
        .chars()
        .filter(|flag| *flag != SEEN_FLAG)
        .collect();
    if seen {
        set.push(SEEN_FLAG);
        set.sort_unstable();
    }
    let joined: String = set.into_iter().collect();
    format!("{base}{FLAG_SUFFIX}{joined}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_field_round_trips_through_the_name() {
        let name = unique_name(4_242);
        assert_eq!(parse_uid(&name), Some(4_242));
        assert_eq!(parse_uid("1700000000.x.host"), None);
    }

    #[test]
    fn uid_survives_the_flag_suffix() {
        assert_eq!(parse_uid("1.M2P3Q4.antiphon,U=17:2,RS"), Some(17));
    }

    #[test]
    fn seen_is_read_from_the_flag_section_only() {
        assert!(has_seen("a,U=1:2,S"));
        assert!(has_seen("a,U=1:2,RS"));
        assert!(!has_seen("a,U=1:2,R"));
        assert!(!has_seen("aSb,U=1"));
    }

    #[test]
    fn with_seen_adds_the_flag_in_sorted_order() {
        assert_eq!(with_seen("a,U=1:2,R", true), "a,U=1:2,RS");
        assert_eq!(with_seen("a,U=1", true), "a,U=1:2,S");
    }

    #[test]
    fn with_seen_removes_only_the_seen_flag() {
        assert_eq!(with_seen("a,U=1:2,RS", false), "a,U=1:2,R");
        assert_eq!(with_seen("a,U=1:2,S", false), "a,U=1:2,");
    }

    #[test]
    fn delivery_lands_in_new_or_cur_and_leaves_tmp_empty() {
        let dir = tempfile::tempdir().unwrap();
        let folder = MaildirFolder::new(dir.path().to_path_buf());
        folder.ensure().unwrap();
        let unseen = folder.deliver(1, false, b"unread mail").unwrap();
        let seen = folder.deliver(2, true, b"read mail").unwrap();
        assert!(unseen.starts_with(dir.path().join(DIR_NEW)));
        assert!(unseen.is_file());
        assert!(seen.starts_with(dir.path().join(DIR_CUR)));
        assert!(seen.is_file());
        let mut messages = folder.scan().unwrap();
        messages.sort_by_key(|message| message.uid);
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].subdir, Subdir::New);
        assert!(!messages[0].seen);
        assert_eq!(messages[1].subdir, Subdir::Cur);
        assert!(messages[1].seen);
        let tmp_entries: Vec<_> =
            fs::read_dir(dir.path().join(DIR_TMP)).unwrap().collect();
        assert!(tmp_entries.is_empty());
    }

    #[test]
    fn mirroring_seen_moves_new_messages_into_cur() {
        let dir = tempfile::tempdir().unwrap();
        let folder = MaildirFolder::new(dir.path().to_path_buf());
        folder.ensure().unwrap();
        folder.deliver(7, false, b"body").unwrap();
        let unseen = folder.scan().unwrap().into_iter().next().unwrap();
        folder.mirror_seen(&unseen, true).unwrap();
        let mirrored =
            folder.scan().unwrap().into_iter().next().unwrap();
        assert_eq!(mirrored.subdir, Subdir::Cur);
        assert!(mirrored.seen);
        assert_eq!(mirrored.uid, 7);
    }

    #[test]
    fn remove_delivered_spares_foreign_files() {
        let dir = tempfile::tempdir().unwrap();
        let folder = MaildirFolder::new(dir.path().to_path_buf());
        folder.ensure().unwrap();
        folder.deliver(1, true, b"ours").unwrap();
        let foreign = dir.path().join(DIR_CUR).join("keep:2,S");
        fs::write(&foreign, b"not ours").unwrap();
        folder.remove_delivered().unwrap();
        assert!(foreign.exists());
        assert!(folder.scan().unwrap().is_empty());
    }
}
