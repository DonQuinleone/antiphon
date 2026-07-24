use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::SyncError;

const FIELDS_PER_LINE: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FolderState {
    pub uid_validity: u32,
    pub last_uid: u32,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct AccountState {
    folders: BTreeMap<String, FolderState>,
}

impl AccountState {
    pub fn load(path: &Path) -> Result<Self, SyncError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            fs::read_to_string(path).map_err(SyncError::io(path))?;
        parse(&text).map_err(|(line, detail)| SyncError::State {
            path: path.to_path_buf(),
            line,
            detail,
        })
    }

    /// Writes via a sibling temp file and rename, so a crash
    /// leaves either the old state or the new, never a torn
    /// file.
    pub fn save(&self, path: &Path) -> Result<(), SyncError> {
        let temp = temp_path(path);
        fs::write(&temp, self.serialise())
            .map_err(SyncError::io(&temp))?;
        fs::rename(&temp, path).map_err(SyncError::io(path))
    }

    pub fn folder(&self, name: &str) -> Option<FolderState> {
        self.folders.get(name).copied()
    }

    pub fn set_folder(&mut self, name: &str, state: FolderState) {
        self.folders.insert(name.to_owned(), state);
    }

    fn serialise(&self) -> String {
        let mut out = String::new();
        for (name, state) in &self.folders {
            out.push_str(&format!(
                "{} {} {name}\n",
                state.uid_validity, state.last_uid
            ));
        }
        out
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".tmp");
    PathBuf::from(name)
}

fn parse(text: &str) -> Result<AccountState, (usize, String)> {
    let mut folders = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let fields: Vec<&str> =
            line.splitn(FIELDS_PER_LINE, ' ').collect();
        if fields.len() != FIELDS_PER_LINE {
            return Err((
                number,
                format!(
                    "expected `uidvalidity uid folder`, got `{line}`"
                ),
            ));
        }
        let uid_validity = parse_number(fields[0], number)?;
        let last_uid = parse_number(fields[1], number)?;
        folders.insert(
            fields[2].to_owned(),
            FolderState {
                uid_validity,
                last_uid,
            },
        );
    }
    Ok(AccountState { folders })
}

fn parse_number(
    field: &str,
    line: usize,
) -> Result<u32, (usize, String)> {
    field.parse().map_err(|_| {
        (line, format!("`{field}` is not an unsigned integer"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AccountState {
        let mut state = AccountState::default();
        state.set_folder(
            "INBOX",
            FolderState {
                uid_validity: 17,
                last_uid: 204,
            },
        );
        state.set_folder(
            "Archive/Old Mail",
            FolderState {
                uid_validity: 3,
                last_uid: 9,
            },
        );
        state
    }

    #[test]
    fn round_trips_through_the_state_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acct.state");
        let state = sample();
        state.save(&path).unwrap();
        let reloaded = AccountState::load(&path).unwrap();
        assert_eq!(reloaded, state);
    }

    #[test]
    fn folder_names_with_spaces_survive() {
        let reloaded = parse("3 9 Archive/Old Mail\n").unwrap();
        assert_eq!(
            reloaded.folder("Archive/Old Mail"),
            Some(FolderState {
                uid_validity: 3,
                last_uid: 9,
            })
        );
    }

    #[test]
    fn a_missing_file_is_an_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("absent.state");
        let state = AccountState::load(&path).unwrap();
        assert_eq!(state, AccountState::default());
    }

    #[test]
    fn garbage_lines_are_rejected_with_the_line_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acct.state");
        std::fs::write(&path, "1 2 ok\nnonsense\n").unwrap();
        let error = AccountState::load(&path).unwrap_err();
        let SyncError::State { line, .. } = error else {
            panic!("expected a state error, got {error}");
        };
        assert_eq!(line, 2);
    }

    #[test]
    fn saving_twice_overwrites_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acct.state");
        sample().save(&path).unwrap();
        let mut updated = sample();
        updated.set_folder(
            "INBOX",
            FolderState {
                uid_validity: 17,
                last_uid: 300,
            },
        );
        updated.save(&path).unwrap();
        let reloaded = AccountState::load(&path).unwrap();
        assert_eq!(reloaded.folder("INBOX").unwrap().last_uid, 300);
    }
}
