use std::collections::HashMap;
use std::path::{Path, PathBuf};

use antiphon_store::{Op, OpKind, SearchIndex, StoreLayout};
use imap_client::imap_types::flag::{Flag, StoreType};

use crate::engine::{RemoteFolder, SyncAccount};
use crate::error::SyncError;
use crate::folders::folder_subdir;
use crate::maildir::parse_uid;
use crate::session::ImapSession;

struct TagFlag {
    tag: &'static str,
    imap_flag: Flag<'static>,
    inverted: bool,
}

// unread is the one tag that inverts on the wire: gaining the
// tag clears \Seen on the server, losing it sets \Seen.
const TAG_FLAGS: [TagFlag; 3] = [
    TagFlag {
        tag: "unread",
        imap_flag: Flag::Seen,
        inverted: true,
    },
    TagFlag {
        tag: "flagged",
        imap_flag: Flag::Flagged,
        inverted: false,
    },
    TagFlag {
        tag: "replied",
        imap_flag: Flag::Answered,
        inverted: false,
    },
];

/// How each op fared against the server, in op order. Dropped
/// ops are resolved under the server-wins rule and must not
/// be retried.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReplayReport {
    pub synced: Vec<u64>,
    pub dropped: Vec<u64>,
}

enum Verdict {
    Synced,
    Dropped,
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
    let mut session = ImapSession::connect(account)?;
    let folders = subdir_map(
        &session
            .list_selectable()
            .map_err(SyncError::imap("listing folders"))?,
    );
    let can_uid_expunge = session.supports_uidplus();
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
        }
    }
    replayer.session.logout();
    Ok(report)
}

struct Replayer {
    session: ImapSession,
    index: SearchIndex,
    folders: HashMap<PathBuf, String>,
    maildir_root: PathBuf,
    can_uid_expunge: bool,
    selected: Option<String>,
}

impl Replayer {
    fn replay_op(&mut self, op: &Op) -> Result<Verdict, SyncError> {
        match &op.kind {
            OpKind::Move {
                to_folder,
                from_folder,
            } => self.replay_move(
                op,
                to_folder.clone(),
                from_folder.clone(),
            ),
            OpKind::Flag { add, remove } => {
                let Some((set, clear)) = flag_sets(add, remove) else {
                    return Ok(Verdict::Dropped);
                };
                self.with_uid(op, |this, uid| {
                    for (kind, flags) in store_actions(set, clear) {
                        this.uid_store(uid, kind, flags)?;
                    }
                    Ok(())
                })
            }
            OpKind::Delete => self.with_uid(op, Self::expunge_uid),
        }
    }

    /// The local apply has already moved the file, so the op's
    /// recorded source mailbox is selected and the UID read
    /// back off the moved file's name. A missing source (an op
    /// from before it was carried), an unknown mailbox on
    /// either side, or a vanished UID all resolve server-wins.
    fn replay_move(
        &mut self,
        op: &Op,
        to_folder: String,
        from_folder: Option<String>,
    ) -> Result<Verdict, SyncError> {
        let Some(source) = from_folder else {
            return Ok(Verdict::Dropped);
        };
        let source_mailbox =
            self.folders.get(Path::new(&source)).cloned();
        let target_mailbox =
            self.folders.get(Path::new(&to_folder)).cloned();
        let (Some(source_mailbox), Some(target_mailbox)) =
            (source_mailbox, target_mailbox)
        else {
            return Ok(Verdict::Dropped);
        };
        let Some(uid) = self.moved_uid(op)? else {
            return Ok(Verdict::Dropped);
        };
        self.select(&source_mailbox)?;
        if !self.uid_exists(uid)? {
            return Ok(Verdict::Dropped);
        }
        self.session.uid_move(uid, &target_mailbox).map_err(
            SyncError::imap(format!(
                "moving uid {uid} to {target_mailbox}"
            )),
        )?;
        Ok(Verdict::Synced)
    }

    /// The moved file keeps its delivery name, so the source
    /// mailbox's UID still rides in it wherever it sits now.
    fn moved_uid(&self, op: &Op) -> Result<Option<u32>, SyncError> {
        let located = self
            .index
            .locate(&op.message_id)
            .map_err(|source| SyncError::Index { source })?;
        let Some(path) = located else {
            return Ok(None);
        };
        let uid = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(parse_uid);
        Ok(uid)
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
        self.session
            .uid_exists(uid)
            .map_err(SyncError::imap(format!("probing uid {uid}")))
    }

    fn uid_store(
        &mut self,
        uid: u32,
        kind: StoreType,
        flags: Vec<Flag<'static>>,
    ) -> Result<(), SyncError> {
        self.session.uid_store(uid, kind, flags).map_err(
            SyncError::imap(format!("storing flags on uid {uid}")),
        )
    }

    fn expunge_uid(&mut self, uid: u32) -> Result<(), SyncError> {
        self.uid_store(uid, StoreType::Add, vec![Flag::Deleted])?;
        if self.can_uid_expunge {
            return self.session.uid_expunge(uid).map_err(
                SyncError::imap(format!("expunging uid {uid}")),
            );
        }
        self.session.expunge().map_err(SyncError::imap("expunging"))
    }
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
) -> Option<(Vec<Flag<'static>>, Vec<Flag<'static>>)> {
    let mut set = Vec::new();
    let mut clear = Vec::new();
    for (tags, tagged) in [(add, true), (remove, false)] {
        for tag in tags {
            let mapping =
                TAG_FLAGS.iter().find(|mapping| mapping.tag == tag)?;
            if tagged != mapping.inverted {
                set.push(mapping.imap_flag.clone());
            } else {
                clear.push(mapping.imap_flag.clone());
            }
        }
    }
    Some((set, clear))
}

fn store_actions(
    set: Vec<Flag<'static>>,
    clear: Vec<Flag<'static>>,
) -> Vec<(StoreType, Vec<Flag<'static>>)> {
    let mut actions = Vec::new();
    if !set.is_empty() {
        actions.push((StoreType::Add, set));
    }
    if !clear.is_empty() {
        actions.push((StoreType::Remove, clear));
    }
    actions
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
        assert_eq!(set, [Flag::Flagged, Flag::Seen]);
        assert!(clear.is_empty());
    }

    #[test]
    fn marking_unread_and_unflagged_clears_both() {
        let (set, clear) =
            flag_sets(&tags(&["unread"]), &tags(&["flagged"])).unwrap();
        assert!(set.is_empty());
        assert_eq!(clear, [Flag::Seen, Flag::Flagged]);
    }

    #[test]
    fn replied_maps_to_answered() {
        let (set, clear) = flag_sets(&tags(&["replied"]), &[]).unwrap();
        assert_eq!(set, [Flag::Answered]);
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
    fn store_actions_split_into_add_and_remove() {
        let actions = store_actions(
            vec![Flag::Seen, Flag::Flagged],
            vec![Flag::Answered],
        );
        assert_eq!(
            actions,
            [
                (StoreType::Add, vec![Flag::Seen, Flag::Flagged]),
                (StoreType::Remove, vec![Flag::Answered]),
            ]
        );
        assert!(store_actions(Vec::new(), Vec::new()).is_empty());
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
