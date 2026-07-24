use std::collections::HashMap;
use std::path::{Path, PathBuf};

use antiphon_store::{Op, OpKind, SearchIndex, StoreLayout};

use crate::engine::{
    RemoteFolder, SyncAccount, TlsSession, connect, selectable_folders,
};
use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::parse_uid;

const UID_ONLY_ITEMS: &str = "(UID)";
const UIDPLUS_CAPABILITY: &str = "UIDPLUS";
const DELETED_FLAG_QUERY: &str = "+FLAGS.SILENT (\\Deleted)";

struct TagFlag {
    tag: &'static str,
    imap_flag: &'static str,
    inverted: bool,
}

// unread is the one tag that inverts on the wire: gaining the
// tag clears \Seen on the server, losing it sets \Seen.
const TAG_FLAGS: [TagFlag; 3] = [
    TagFlag {
        tag: "unread",
        imap_flag: "\\Seen",
        inverted: true,
    },
    TagFlag {
        tag: "flagged",
        imap_flag: "\\Flagged",
        inverted: false,
    },
    TagFlag {
        tag: "replied",
        imap_flag: "\\Answered",
        inverted: false,
    },
];

/// How each op fared against the server, in op order. Dropped
/// ops are resolved under the server-wins rule and must not be
/// retried; unsupported ops (Move, for now) stay pending.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    pub synced: Vec<u64>,
    pub dropped: Vec<u64>,
    pub unsupported: Vec<u64>,
}

enum Verdict {
    Synced,
    Dropped,
    Unsupported,
}

/// Replays local oplog operations to the IMAP server. A message
/// that cannot be found locally, or whose UID no longer exists
/// server-side, is the server-wins conflict case: the op is
/// dropped, never an error for the batch.
pub fn replay(
    account: &SyncAccount,
    layout: &StoreLayout,
    ops: &[Op],
) -> Result<ReplayReport, SyncError> {
    let mut report = ReplayReport::default();
    if ops.is_empty() {
        return Ok(report);
    }
    let index = SearchIndex::open(layout)
        .map_err(|source| SyncError::Index { source })?;
    let mut session = connect(account)?;
    let folders = subdir_map(&selectable_folders(&mut session)?);
    let can_uid_expunge = supports_uid_expunge(&mut session)?;
    let mut replayer = Replayer {
        session,
        index,
        folders,
        maildir_root: layout.account_maildir(&account.name),
        can_uid_expunge,
        selected: None,
    };
    for op in ops {
        match replayer.replay_op(op)? {
            Verdict::Synced => report.synced.push(op.id),
            Verdict::Dropped => report.dropped.push(op.id),
            Verdict::Unsupported => report.unsupported.push(op.id),
        }
    }
    let _ = replayer.session.logout();
    Ok(report)
}

struct Replayer {
    session: TlsSession,
    index: SearchIndex,
    folders: HashMap<PathBuf, String>,
    maildir_root: PathBuf,
    can_uid_expunge: bool,
    selected: Option<String>,
}

impl Replayer {
    fn replay_op(&mut self, op: &Op) -> Result<Verdict, SyncError> {
        match &op.kind {
            OpKind::Move { .. } => Ok(Verdict::Unsupported),
            OpKind::Flag { add, remove } => {
                let Some((set, clear)) = flag_sets(add, remove) else {
                    return Ok(Verdict::Dropped);
                };
                self.with_uid(op, |this, uid| {
                    for query in store_queries(&set, &clear) {
                        this.uid_store(uid, &query)?;
                    }
                    Ok(())
                })
            }
            OpKind::Delete => self.with_uid(op, Self::expunge_uid),
        }
    }

    /// Resolves the op's message to a live server UID and runs
    /// the action on it; an unresolvable message is dropped.
    fn with_uid(
        &mut self,
        op: &Op,
        action: impl FnOnce(&mut Self, u32) -> Result<(), SyncError>,
    ) -> Result<Verdict, SyncError> {
        let Some(uid) = self.locate_uid(op)? else {
            return Ok(Verdict::Dropped);
        };
        action(self, uid)?;
        Ok(Verdict::Synced)
    }

    fn locate_uid(
        &mut self,
        op: &Op,
    ) -> Result<Option<u32>, SyncError> {
        let located = self
            .index
            .locate(&op.message_id)
            .map_err(|source| SyncError::Index { source })?;
        let Some(path) = located else {
            return Ok(None);
        };
        let Some((uid, subdir)) =
            uid_and_subdir(&self.maildir_root, &path)
        else {
            return Ok(None);
        };
        let Some(folder) = self.folders.get(&subdir).cloned() else {
            return Ok(None);
        };
        self.select(&folder)?;
        if !self.uid_exists(uid)? {
            return Ok(None);
        }
        Ok(Some(uid))
    }

    fn select(&mut self, folder: &str) -> Result<(), SyncError> {
        if self.selected.as_deref() == Some(folder) {
            return Ok(());
        }
        self.session
            .select(folder)
            .map_err(SyncError::imap(format!("selecting {folder}")))?;
        self.selected = Some(folder.to_owned());
        Ok(())
    }

    fn uid_exists(&mut self, uid: u32) -> Result<bool, SyncError> {
        let fetches = self
            .session
            .uid_fetch(uid.to_string(), UID_ONLY_ITEMS)
            .map_err(SyncError::imap(format!("probing uid {uid}")))?;
        Ok(fetches.iter().any(|fetch| fetch.uid == Some(uid)))
    }

    fn uid_store(
        &mut self,
        uid: u32,
        query: &str,
    ) -> Result<(), SyncError> {
        self.session.uid_store(uid.to_string(), query).map_err(
            SyncError::imap(format!("storing flags on uid {uid}")),
        )?;
        Ok(())
    }

    fn expunge_uid(&mut self, uid: u32) -> Result<(), SyncError> {
        self.uid_store(uid, DELETED_FLAG_QUERY)?;
        if self.can_uid_expunge {
            self.session.uid_expunge(uid.to_string()).map_err(
                SyncError::imap(format!("expunging uid {uid}")),
            )?;
            return Ok(());
        }
        self.session
            .expunge()
            .map_err(SyncError::imap("expunging"))?;
        Ok(())
    }
}

fn supports_uid_expunge(
    session: &mut TlsSession,
) -> Result<bool, SyncError> {
    let capabilities = session
        .capabilities()
        .map_err(SyncError::imap("reading capabilities"))?;
    Ok(capabilities.has_str(UIDPLUS_CAPABILITY))
}

fn subdir_map(folders: &[RemoteFolder]) -> HashMap<PathBuf, String> {
    folders
        .iter()
        .filter_map(|folder| {
            let subdir = folder_subdir(
                &folder.name,
                folder.delimiter.as_deref(),
            )
            .ok()?;
            Some((subdir, folder.name.clone()))
        })
        .collect()
}

/// Splits a Flag op into the flags to set and to clear on the
/// server; None when any tag has no IMAP equivalent, in which
/// case the op cannot be expressed and is dropped whole.
fn flag_sets(
    add: &[String],
    remove: &[String],
) -> Option<(Vec<&'static str>, Vec<&'static str>)> {
    let mut set = Vec::new();
    let mut clear = Vec::new();
    for (tags, tagged) in [(add, true), (remove, false)] {
        for tag in tags {
            let mapping =
                TAG_FLAGS.iter().find(|mapping| mapping.tag == tag)?;
            if tagged != mapping.inverted {
                set.push(mapping.imap_flag);
            } else {
                clear.push(mapping.imap_flag);
            }
        }
    }
    Some((set, clear))
}

fn store_queries(set: &[&str], clear: &[&str]) -> Vec<String> {
    let mut queries = Vec::new();
    if !set.is_empty() {
        queries.push(format!("+FLAGS.SILENT ({})", set.join(" ")));
    }
    if !clear.is_empty() {
        queries.push(format!("-FLAGS.SILENT ({})", clear.join(" ")));
    }
    queries
}

/// Reads the server UID and folder subdirectory back out of a
/// delivered message's path, the inverse of what the engine
/// wrote: <maildir_root>/<subdir>/{new,cur}/<name>,U=<uid>...
fn uid_and_subdir(
    maildir_root: &Path,
    path: &Path,
) -> Option<(u32, PathBuf)> {
    let name = path.file_name()?.to_str()?;
    let uid = parse_uid(name)?;
    let folder_dir = path.parent()?.parent()?;
    let subdir = folder_dir.strip_prefix(maildir_root).ok()?;
    Some((uid, subdir.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn marking_read_and_flagged_sets_seen_and_flagged() {
        let (set, clear) =
            flag_sets(&tags(&["flagged"]), &tags(&["unread"])).unwrap();
        assert_eq!(set, ["\\Flagged", "\\Seen"]);
        assert!(clear.is_empty());
    }

    #[test]
    fn marking_unread_and_unflagged_clears_both() {
        let (set, clear) =
            flag_sets(&tags(&["unread"]), &tags(&["flagged"])).unwrap();
        assert!(set.is_empty());
        assert_eq!(clear, ["\\Seen", "\\Flagged"]);
    }

    #[test]
    fn replied_maps_to_answered() {
        let (set, clear) = flag_sets(&tags(&["replied"]), &[]).unwrap();
        assert_eq!(set, ["\\Answered"]);
        assert!(clear.is_empty());
    }

    #[test]
    fn an_unmappable_tag_rejects_the_whole_op() {
        assert!(flag_sets(&tags(&["starred"]), &[]).is_none());
        assert!(
            flag_sets(&tags(&["flagged"]), &tags(&["junk"])).is_none()
        );
    }

    #[test]
    fn store_queries_split_into_plus_and_minus() {
        let queries =
            store_queries(&["\\Seen", "\\Flagged"], &["\\Answered"]);
        assert_eq!(
            queries,
            [
                "+FLAGS.SILENT (\\Seen \\Flagged)",
                "-FLAGS.SILENT (\\Answered)",
            ]
        );
        assert!(store_queries(&[], &[]).is_empty());
    }

    #[test]
    fn uid_and_subdir_resolve_from_the_inbox_root() {
        let root = Path::new("/s/maildir/work");
        let path = root.join("cur/1.M2P3Q4.antiphon,U=17:2,S");
        let (uid, subdir) = uid_and_subdir(root, &path).unwrap();
        assert_eq!(uid, 17);
        assert_eq!(subdir, PathBuf::new());
    }

    #[test]
    fn uid_and_subdir_resolve_from_a_nested_folder() {
        let root = Path::new("/s/maildir/work");
        let path = root.join("lists/rust/new/1.M2P3Q4.antiphon,U=9");
        let (uid, subdir) = uid_and_subdir(root, &path).unwrap();
        assert_eq!(uid, 9);
        assert_eq!(subdir, PathBuf::from("lists/rust"));
    }

    #[test]
    fn foreign_paths_and_unmarked_names_yield_nothing() {
        let root = Path::new("/s/maildir/work");
        let elsewhere = Path::new("/s/maildir/home/cur/1.x,U=3:2,S");
        assert!(uid_and_subdir(root, elsewhere).is_none());
        let unmarked = root.join("cur/1700000000.x.host:2,S");
        assert!(uid_and_subdir(root, &unmarked).is_none());
    }

    #[test]
    fn folder_subdirs_map_back_to_server_names() {
        let folders = [
            RemoteFolder {
                name: String::from("INBOX"),
                delimiter: Some(String::from("/")),
            },
            RemoteFolder {
                name: String::from("Lists/Rust"),
                delimiter: Some(String::from("/")),
            },
        ];
        let map = subdir_map(&folders);
        assert_eq!(map[&PathBuf::new()], "INBOX");
        assert_eq!(map[&PathBuf::from("lists/rust")], "Lists/Rust");
    }
}
