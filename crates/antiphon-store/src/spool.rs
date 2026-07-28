use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) const MESSAGE_EXT: &str = "eml";
pub(crate) const ENVELOPE_EXT: &str = "json";
pub(crate) const DEAD_DIR: &str = "dead";

#[derive(Debug)]
pub enum SpoolError {
    Io { path: PathBuf, source: io::Error },
    Envelope { path: PathBuf, detail: String },
}

impl fmt::Display for SpoolError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(out, "{}: {source}", path.display())
            }
            Self::Envelope { path, detail } => {
                write!(out, "{}: {detail}", path.display())
            }
        }
    }
}

impl std::error::Error for SpoolError {}

/// One durable message queue on disk: a numbered message file
/// plus an envelope naming its handling, envelope written
/// last, so a message without its envelope is a torn enqueue
/// and stays invisible to pending().
pub(crate) struct Spool {
    dir: PathBuf,
}

impl Spool {
    pub fn new(dir: PathBuf) -> Spool {
        Spool { dir }
    }

    /// The message is durable once enqueue returns: both files
    /// are fsynced. The directory is created on first use, so
    /// spools added after a store was initialised need no
    /// re-init.
    pub fn enqueue<E: Serialize>(
        &self,
        envelope: &E,
        raw_message: &[u8],
    ) -> Result<(u64, PathBuf), SpoolError> {
        fs::create_dir_all(&self.dir).map_err(|source| {
            SpoolError::Io {
                path: self.dir.clone(),
                source,
            }
        })?;
        let id = self.next_id()?;
        let message_path = self.path_for(id, MESSAGE_EXT);
        write_synced(&message_path, raw_message)?;
        let json =
            serde_json::to_vec(envelope).expect("envelope serialises");
        write_synced(&self.path_for(id, ENVELOPE_EXT), &json)?;
        Ok((id, message_path))
    }

    pub fn pending<E: DeserializeOwned>(
        &self,
    ) -> Result<Vec<(u64, E, PathBuf)>, SpoolError> {
        let mut ids = self.queued_ids()?;
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .filter_map(|id| self.load(id).transpose())
            .collect()
    }

    /// Moves a permanently failing message aside into dead/,
    /// where pending() never looks: the queue stops retrying
    /// it, but nothing is lost.
    pub fn reject(&self, id: u64) -> Result<(), SpoolError> {
        let dead = self.dir.join(DEAD_DIR);
        fs::create_dir_all(&dead).map_err(|source| SpoolError::Io {
            path: dead.clone(),
            source,
        })?;
        for ext in [MESSAGE_EXT, ENVELOPE_EXT] {
            let from = self.path_for(id, ext);
            if !from.exists() {
                continue;
            }
            let name = format!("{id:016}.{ext}");
            fs::rename(&from, dead.join(name)).map_err(|source| {
                SpoolError::Io { path: from, source }
            })?;
        }
        Ok(())
    }

    pub fn remove(&self, id: u64) -> Result<(), SpoolError> {
        for ext in [ENVELOPE_EXT, MESSAGE_EXT] {
            let path = self.path_for(id, ext);
            let Err(source) = fs::remove_file(&path) else {
                continue;
            };
            if source.kind() == io::ErrorKind::NotFound {
                continue;
            }
            return Err(SpoolError::Io { path, source });
        }
        Ok(())
    }

    fn queued_ids(&self) -> Result<Vec<u64>, SpoolError> {
        if !self.dir.is_dir() {
            return Ok(Vec::new());
        }
        let entries = fs::read_dir(&self.dir).map_err(|source| {
            SpoolError::Io {
                path: self.dir.clone(),
                source,
            }
        })?;
        Ok(entries
            .filter_map(Result::ok)
            .filter_map(|entry| queued_id(&entry.path()))
            .collect())
    }

    fn load<E: DeserializeOwned>(
        &self,
        id: u64,
    ) -> Result<Option<(u64, E, PathBuf)>, SpoolError> {
        let envelope_path = self.path_for(id, ENVELOPE_EXT);
        let message_path = self.path_for(id, MESSAGE_EXT);
        if !envelope_path.exists() || !message_path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&envelope_path).map_err(|source| {
            SpoolError::Io {
                path: envelope_path.clone(),
                source,
            }
        })?;
        let envelope =
            serde_json::from_slice(&bytes).map_err(|error| {
                SpoolError::Envelope {
                    path: envelope_path,
                    detail: error.to_string(),
                }
            })?;
        Ok(Some((id, envelope, message_path)))
    }

    fn next_id(&self) -> Result<u64, SpoolError> {
        let highest = self.queued_ids()?.into_iter().max().unwrap_or(0);
        Ok(highest + 1)
    }

    fn path_for(&self, id: u64, ext: &str) -> PathBuf {
        self.dir.join(format!("{id:016}.{ext}"))
    }
}

fn queued_id(path: &Path) -> Option<u64> {
    path.file_stem()?.to_str()?.parse().ok()
}

fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), SpoolError> {
    let wrap = |source| SpoolError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = fs::File::create(path).map_err(wrap)?;
    io::Write::write_all(&mut file, bytes).map_err(wrap)?;
    file.sync_data().map_err(wrap)
}
