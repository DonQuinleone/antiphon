use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::layout::StoreLayout;
use crate::oplog::{Op, OpKind};
use crate::search::{SearchError, SearchIndex};

const INFO_SEPARATOR: &str = ":2,";
const CUR: &str = "cur";
const MAILDIR_SUBDIRS: [&str; 3] = [CUR, "new", "tmp"];

struct FlagMapping {
    tag: &'static str,
    letter: char,
    letter_when_untagged: bool,
}

// unread is the one notmuch tag expressed by the absence of its
// maildir letter: an unread message is one without S.
const FLAG_TABLE: [FlagMapping; 3] = [
    FlagMapping {
        tag: "flagged",
        letter: 'F',
        letter_when_untagged: false,
    },
    FlagMapping {
        tag: "replied",
        letter: 'R',
        letter_when_untagged: false,
    },
    FlagMapping {
        tag: "unread",
        letter: 'S',
        letter_when_untagged: true,
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    Skipped,
}

#[derive(Debug)]
pub enum ApplyError {
    Search(SearchError),
    Io { path: PathBuf, source: io::Error },
    UnknownFlag { flag: String },
    OutsideMaildir { path: PathBuf },
    IndexRefresh { detail: String },
}

impl fmt::Display for ApplyError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Search(source) => {
                write!(out, "locating message: {source}")
            }
            Self::Io { path, source } => {
                write!(out, "applying at {}: {source}", path.display())
            }
            Self::UnknownFlag { flag } => {
                write!(out, "no maildir mapping for flag `{flag}`")
            }
            Self::OutsideMaildir { path } => write!(
                out,
                "{} is not inside a maildir cur/new/tmp",
                path.display()
            ),
            Self::IndexRefresh { detail } => {
                write!(out, "notmuch new failed: {detail}")
            }
        }
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Search(source) => Some(source),
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

pub fn apply_op(
    layout: &StoreLayout,
    index: &SearchIndex,
    op: &Op,
) -> Result<ApplyOutcome, ApplyError> {
    let Some(path) = locate(layout, index, op)? else {
        return Ok(ApplyOutcome::Skipped);
    };
    let outcome = match &op.kind {
        OpKind::Flag { add, remove } => {
            apply_flags(&path, add, remove)?
        }
        OpKind::Move { to_folder, .. } => {
            apply_move(layout, op, &path, to_folder)?
        }
        OpKind::Delete => apply_delete(&path)?,
    };
    if outcome == ApplyOutcome::Applied {
        refresh_index(layout)?;
    }
    Ok(outcome)
}

fn locate(
    layout: &StoreLayout,
    index: &SearchIndex,
    op: &Op,
) -> Result<Option<PathBuf>, ApplyError> {
    let found = existing_path(index, &op.message_id)?;
    if found.is_some() {
        return Ok(found);
    }
    // The index lags the Maildir when a crash lands between a
    // rename and its notmuch refresh; reconcile once before
    // concluding the message is gone.
    refresh_index(layout)?;
    let fresh =
        SearchIndex::open(layout).map_err(ApplyError::Search)?;
    existing_path(&fresh, &op.message_id)
}

fn existing_path(
    index: &SearchIndex,
    message_id: &str,
) -> Result<Option<PathBuf>, ApplyError> {
    index.locate(message_id).map_err(ApplyError::Search)
}

fn apply_flags(
    path: &Path,
    add: &[String],
    remove: &[String],
) -> Result<ApplyOutcome, ApplyError> {
    let (base, mut letters) = split_info(path)?;
    for flag in add {
        set_letter(&mut letters, flag, true)?;
    }
    for flag in remove {
        set_letter(&mut letters, flag, false)?;
    }
    let target = cur_dir(path)?.join(join_info(&base, &letters));
    if target == path {
        return Ok(ApplyOutcome::Skipped);
    }
    fs::rename(path, &target).map_err(io_at(path))?;
    Ok(ApplyOutcome::Applied)
}

fn apply_move(
    layout: &StoreLayout,
    op: &Op,
    path: &Path,
    to_folder: &str,
) -> Result<ApplyOutcome, ApplyError> {
    let folder = layout.account_maildir(&op.account).join(to_folder);
    for sub in MAILDIR_SUBDIRS {
        let dir = folder.join(sub);
        fs::create_dir_all(&dir).map_err(io_at(&dir))?;
    }
    let target = folder.join(CUR).join(cur_name(path)?);
    if target == path {
        return Ok(ApplyOutcome::Skipped);
    }
    fs::rename(path, &target).map_err(io_at(path))?;
    Ok(ApplyOutcome::Applied)
}

fn apply_delete(path: &Path) -> Result<ApplyOutcome, ApplyError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(ApplyOutcome::Applied),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            Ok(ApplyOutcome::Skipped)
        }
        Err(source) => Err(ApplyError::Io {
            path: path.to_owned(),
            source,
        }),
    }
}

fn set_letter(
    letters: &mut BTreeSet<char>,
    flag: &str,
    adding: bool,
) -> Result<(), ApplyError> {
    let mapping = FLAG_TABLE
        .iter()
        .find(|mapping| mapping.tag == flag)
        .ok_or_else(|| ApplyError::UnknownFlag {
            flag: flag.to_owned(),
        })?;
    let want_letter = adding != mapping.letter_when_untagged;
    if want_letter {
        letters.insert(mapping.letter);
        return Ok(());
    }
    letters.remove(&mapping.letter);
    Ok(())
}

fn split_info(
    path: &Path,
) -> Result<(String, BTreeSet<char>), ApplyError> {
    let name = file_name(path)?;
    let Some((base, info)) = name.split_once(INFO_SEPARATOR) else {
        return Ok((name.to_owned(), BTreeSet::new()));
    };
    Ok((base.to_owned(), info.chars().collect()))
}

fn join_info(base: &str, letters: &BTreeSet<char>) -> String {
    let info: String = letters.iter().collect();
    format!("{base}{INFO_SEPARATOR}{info}")
}

fn cur_name(path: &Path) -> Result<String, ApplyError> {
    let name = file_name(path)?;
    if name.contains(INFO_SEPARATOR) {
        return Ok(name.to_owned());
    }
    Ok(format!("{name}{INFO_SEPARATOR}"))
}

fn cur_dir(path: &Path) -> Result<PathBuf, ApplyError> {
    let outside = || ApplyError::OutsideMaildir {
        path: path.to_owned(),
    };
    let dir = path.parent().ok_or_else(outside)?;
    let sub = dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(outside)?;
    if !MAILDIR_SUBDIRS.contains(&sub) {
        return Err(outside());
    }
    Ok(dir.parent().ok_or_else(outside)?.join(CUR))
}

fn file_name(path: &Path) -> Result<&str, ApplyError> {
    path.file_name().and_then(|name| name.to_str()).ok_or(
        ApplyError::OutsideMaildir {
            path: path.to_owned(),
        },
    )
}

fn refresh_index(layout: &StoreLayout) -> Result<(), ApplyError> {
    let output = Command::new("notmuch")
        .arg("new")
        .env("NOTMUCH_CONFIG", layout.notmuch_config_path())
        .output()
        .map_err(|source| ApplyError::IndexRefresh {
            detail: source.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    Err(ApplyError::IndexRefresh {
        detail: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> ApplyError {
    let path = path.to_owned();
    move |source| ApplyError::Io { path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_info_separates_base_and_letters() {
        let (base, letters) =
            split_info(Path::new("/m/cur/171.host:2,FS")).unwrap();
        assert_eq!(base, "171.host");
        assert_eq!(letters, BTreeSet::from(['F', 'S']));
    }

    #[test]
    fn split_info_of_a_new_message_has_no_letters() {
        let (base, letters) =
            split_info(Path::new("/m/new/171.host")).unwrap();
        assert_eq!(base, "171.host");
        assert!(letters.is_empty());
    }

    #[test]
    fn join_info_orders_letters_ascii() {
        let letters = BTreeSet::from(['S', 'F', 'R']);
        assert_eq!(join_info("x", &letters), "x:2,FRS");
        assert_eq!(join_info("x", &BTreeSet::new()), "x:2,");
    }

    #[test]
    fn flag_table_maps_notmuch_tags_to_letters() {
        let mut letters = BTreeSet::new();
        set_letter(&mut letters, "flagged", true).unwrap();
        set_letter(&mut letters, "replied", true).unwrap();
        set_letter(&mut letters, "unread", false).unwrap();
        assert_eq!(letters, BTreeSet::from(['F', 'R', 'S']));
        set_letter(&mut letters, "unread", true).unwrap();
        set_letter(&mut letters, "flagged", false).unwrap();
        assert_eq!(letters, BTreeSet::from(['R']));
    }

    #[test]
    fn unknown_flags_are_rejected() {
        let mut letters = BTreeSet::new();
        let err =
            set_letter(&mut letters, "starred", true).unwrap_err();
        assert!(matches!(
            err,
            ApplyError::UnknownFlag { flag } if flag == "starred"
        ));
    }

    #[test]
    fn cur_dir_resolves_from_new_and_cur() {
        let from_new = cur_dir(Path::new("/m/new/x")).unwrap();
        assert_eq!(from_new, Path::new("/m/cur"));
        let from_cur = cur_dir(Path::new("/m/cur/x:2,S")).unwrap();
        assert_eq!(from_cur, Path::new("/m/cur"));
        assert!(cur_dir(Path::new("/m/elsewhere/x")).is_err());
    }
}
