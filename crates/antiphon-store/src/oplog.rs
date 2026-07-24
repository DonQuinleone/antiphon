use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::layout::StoreLayout;

const LOG_FILE: &str = "ops.jsonl";
const STATE_FILE: &str = "state.json";
const STATE_TMP_FILE: &str = "state.json.tmp";
const FIRST_ID: u64 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    pub id: u64,
    pub account: String,
    pub message_id: String,
    pub kind: OpKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OpKind {
    Flag {
        add: Vec<String>,
        remove: Vec<String>,
    },
    Move {
        to_folder: String,
    },
    Delete,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
struct Cursors {
    applied_up_to: u64,
    synced_up_to: u64,
}

#[derive(Debug)]
pub enum OpLogError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    CorruptLine {
        path: PathBuf,
        line: usize,
    },
    CorruptState {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl fmt::Display for OpLogError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(out, "oplog io at {}: {source}", path.display())
            }
            Self::CorruptLine { path, line } => write!(
                out,
                "oplog {} corrupt at line {line}",
                path.display()
            ),
            Self::CorruptState { path, source } => write!(
                out,
                "oplog state {} unreadable: {source}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for OpLogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::CorruptLine { .. } => None,
            Self::CorruptState { source, .. } => Some(source),
        }
    }
}

#[derive(Debug)]
pub struct OpLog {
    file: File,
    log_path: PathBuf,
    state_path: PathBuf,
    state_tmp_path: PathBuf,
    ops: Vec<Op>,
    cursors: Cursors,
}

impl OpLog {
    pub fn open(layout: &StoreLayout) -> Result<Self, OpLogError> {
        let dir = layout.oplog_dir();
        fs::create_dir_all(&dir).map_err(io_at(&dir))?;
        let log_path = dir.join(LOG_FILE);
        let ops = recover(&log_path)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(io_at(&log_path))?;
        let state_path = dir.join(STATE_FILE);
        let cursors = read_cursors(&state_path)?;
        Ok(Self {
            file,
            log_path,
            state_path,
            state_tmp_path: dir.join(STATE_TMP_FILE),
            ops,
            cursors,
        })
    }

    pub fn append(
        &mut self,
        account: &str,
        message_id: &str,
        kind: OpKind,
    ) -> Result<Op, OpLogError> {
        let op = Op {
            id: self.next_id(),
            account: account.to_owned(),
            message_id: message_id.to_owned(),
            kind,
        };
        let mut line =
            serde_json::to_vec(&op).expect("op record serialises");
        line.push(b'\n');
        let wrap = io_at(&self.log_path);
        self.file
            .write_all(&line)
            .and_then(|()| self.file.sync_data())
            .map_err(wrap)?;
        self.ops.push(op.clone());
        Ok(op)
    }

    pub fn unapplied(&self) -> Vec<Op> {
        self.after(self.cursors.applied_up_to)
    }

    pub fn unsynced(&self) -> Vec<Op> {
        self.after(self.cursors.synced_up_to)
    }

    pub fn mark_applied(&mut self, id: u64) -> Result<(), OpLogError> {
        if id <= self.cursors.applied_up_to {
            return Ok(());
        }
        let mut next = self.cursors;
        next.applied_up_to = id;
        self.write_cursors(next)
    }

    pub fn mark_synced(&mut self, id: u64) -> Result<(), OpLogError> {
        if id <= self.cursors.synced_up_to {
            return Ok(());
        }
        let mut next = self.cursors;
        next.synced_up_to = id;
        self.write_cursors(next)
    }

    fn next_id(&self) -> u64 {
        self.ops.last().map_or(FIRST_ID, |op| op.id + 1)
    }

    fn after(&self, cursor: u64) -> Vec<Op> {
        self.ops
            .iter()
            .filter(|op| op.id > cursor)
            .cloned()
            .collect()
    }

    fn write_cursors(
        &mut self,
        next: Cursors,
    ) -> Result<(), OpLogError> {
        let bytes =
            serde_json::to_vec(&next).expect("cursors serialise");
        write_atomic(&self.state_tmp_path, &self.state_path, &bytes)?;
        self.cursors = next;
        Ok(())
    }
}

/// An op is only acknowledged once its full line, newline
/// included, has been fsynced, so an unterminated or unparseable
/// final line is a torn crash artefact and is safe to drop; a bad
/// line anywhere earlier is real corruption and refuses to open.
fn recover(path: &Path) -> Result<Vec<Op>, OpLogError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read(path).map_err(io_at(path))?;
    let mut ops = Vec::new();
    let mut intact_end = 0usize;
    let mut start = 0usize;
    let mut line_number = 0usize;
    while start < data.len() {
        line_number += 1;
        let Some(offset) =
            data[start..].iter().position(|byte| *byte == b'\n')
        else {
            break;
        };
        let end = start + offset;
        let parsed = serde_json::from_slice::<Op>(&data[start..end]);
        let is_final_line = end + 1 == data.len();
        match parsed {
            Ok(op) => {
                ops.push(op);
                intact_end = end + 1;
                start = end + 1;
            }
            Err(_) if is_final_line => break,
            Err(_) => {
                return Err(OpLogError::CorruptLine {
                    path: path.to_owned(),
                    line: line_number,
                });
            }
        }
    }
    if intact_end < data.len() {
        truncate_torn(path, intact_end, data.len())?;
    }
    Ok(ops)
}

fn truncate_torn(
    path: &Path,
    intact_end: usize,
    total: usize,
) -> Result<(), OpLogError> {
    eprintln!(
        "antiphon-store: oplog {}: discarding torn final line \
         ({} bytes) left by a crash",
        path.display(),
        total - intact_end,
    );
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(io_at(path))?;
    file.set_len(intact_end as u64)
        .and_then(|()| file.sync_all())
        .map_err(io_at(path))
}

fn read_cursors(path: &Path) -> Result<Cursors, OpLogError> {
    if !path.exists() {
        return Ok(Cursors::default());
    }
    let data = fs::read(path).map_err(io_at(path))?;
    serde_json::from_slice(&data).map_err(|source| {
        OpLogError::CorruptState {
            path: path.to_owned(),
            source,
        }
    })
}

fn write_atomic(
    tmp_path: &Path,
    final_path: &Path,
    bytes: &[u8],
) -> Result<(), OpLogError> {
    let mut tmp = File::create(tmp_path).map_err(io_at(tmp_path))?;
    tmp.write_all(bytes)
        .and_then(|()| tmp.sync_all())
        .map_err(io_at(tmp_path))?;
    drop(tmp);
    fs::rename(tmp_path, final_path).map_err(io_at(final_path))?;
    sync_parent_dir(final_path)
}

fn sync_parent_dir(path: &Path) -> Result<(), OpLogError> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    File::open(dir)
        .and_then(|handle| handle.sync_all())
        .map_err(io_at(dir))
}

fn io_at(path: &Path) -> impl FnOnce(io::Error) -> OpLogError {
    let path = path.to_owned();
    move |source| OpLogError::Io { path, source }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_in(dir: &tempfile::TempDir) -> StoreLayout {
        let layout = StoreLayout::new(dir.path().join("store"));
        layout.init().unwrap();
        layout
    }

    fn flag_unread_off() -> OpKind {
        OpKind::Flag {
            add: Vec::new(),
            remove: vec!["unread".to_owned()],
        }
    }

    fn append_n(log: &mut OpLog, count: u64) -> Vec<Op> {
        (0..count)
            .map(|n| {
                log.append(
                    "acct",
                    &format!("m{n}@example.com"),
                    flag_unread_off(),
                )
                .unwrap()
            })
            .collect()
    }

    fn log_path(layout: &StoreLayout) -> PathBuf {
        layout.oplog_dir().join(LOG_FILE)
    }

    fn append_raw(layout: &StoreLayout, bytes: &[u8]) {
        let mut file = OpenOptions::new()
            .append(true)
            .open(log_path(layout))
            .unwrap();
        file.write_all(bytes).unwrap();
    }

    #[test]
    fn ids_are_monotonic_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        let first = append_n(&mut log, 2);
        assert_eq!(first[0].id, FIRST_ID);
        assert_eq!(first[1].id, FIRST_ID + 1);
        drop(log);
        let mut log = OpLog::open(&layout).unwrap();
        let next = append_n(&mut log, 1);
        assert_eq!(next[0].id, FIRST_ID + 2);
    }

    #[test]
    fn torn_unterminated_tail_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 2);
        drop(log);
        let intact = fs::read(log_path(&layout)).unwrap();
        append_raw(&layout, b"{\"id\":3,\"account\":\"ac");
        let mut log = OpLog::open(&layout).unwrap();
        assert_eq!(log.unsynced().len(), 2);
        assert_eq!(fs::read(log_path(&layout)).unwrap(), intact);
        let next = append_n(&mut log, 1);
        assert_eq!(next[0].id, FIRST_ID + 2);
    }

    #[test]
    fn torn_terminated_garbage_tail_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 1);
        drop(log);
        append_raw(&layout, b"{\"id\":2,\"acc\n");
        let log = OpLog::open(&layout).unwrap();
        assert_eq!(log.unsynced().len(), 1);
        let content = fs::read(log_path(&layout)).unwrap();
        assert!(content.ends_with(b"}\n"));
    }

    #[test]
    fn corruption_before_the_final_line_refuses_to_open() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 1);
        drop(log);
        append_raw(&layout, b"garbage\n");
        append_raw(&layout, b"more garbage\n");
        let err = OpLog::open(&layout).unwrap_err();
        assert!(matches!(err, OpLogError::CorruptLine { line: 2, .. }));
    }

    #[test]
    fn cursors_partition_unapplied_and_unsynced() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 3);
        assert_eq!(log.unapplied().len(), 3);
        assert_eq!(log.unsynced().len(), 3);
        log.mark_applied(2).unwrap();
        log.mark_synced(1).unwrap();
        assert_eq!(log.unapplied().len(), 1);
        assert_eq!(log.unapplied()[0].id, 3);
        assert_eq!(log.unsynced().len(), 2);
        drop(log);
        let log = OpLog::open(&layout).unwrap();
        assert_eq!(log.unapplied().len(), 1);
        assert_eq!(log.unsynced().len(), 2);
    }

    #[test]
    fn marks_never_regress() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 3);
        log.mark_applied(3).unwrap();
        log.mark_applied(1).unwrap();
        assert!(log.unapplied().is_empty());
    }

    #[test]
    fn state_writes_leave_no_tmp_and_survive_a_stale_tmp() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        append_n(&mut log, 2);
        log.mark_applied(1).unwrap();
        let state_path = layout.oplog_dir().join(STATE_FILE);
        let tmp_path = layout.oplog_dir().join(STATE_TMP_FILE);
        assert!(!tmp_path.exists());
        let state = fs::read(&state_path).unwrap();
        let parsed: Cursors = serde_json::from_slice(&state).unwrap();
        assert_eq!(parsed.applied_up_to, 1);
        fs::write(&tmp_path, b"half-writ").unwrap();
        drop(log);
        let mut log = OpLog::open(&layout).unwrap();
        assert_eq!(log.unapplied().len(), 1);
        log.mark_applied(2).unwrap();
        assert!(!tmp_path.exists());
        let state = fs::read(&state_path).unwrap();
        let parsed: Cursors = serde_json::from_slice(&state).unwrap();
        assert_eq!(parsed.applied_up_to, 2);
    }

    #[test]
    fn an_op_round_trips_through_its_log_line() {
        let dir = tempfile::tempdir().unwrap();
        let layout = layout_in(&dir);
        let mut log = OpLog::open(&layout).unwrap();
        let appended = log
            .append(
                "work",
                "id@example.com",
                OpKind::Move {
                    to_folder: "archive".to_owned(),
                },
            )
            .unwrap();
        drop(log);
        let log = OpLog::open(&layout).unwrap();
        assert_eq!(log.unapplied(), vec![appended]);
    }
}
