use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderReport {
    pub folder: String,
    pub new_messages: usize,
    pub updated_messages: usize,
    pub removed_messages: usize,
    pub delivered: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub folders: Vec<FolderReport>,
    /// Folders that failed this pass, one line each; the rest
    /// of the account synced regardless.
    pub errors: Vec<String>,
}

impl SyncReport {
    pub fn total_new(&self) -> usize {
        self.folders.iter().map(|folder| folder.new_messages).sum()
    }

    pub fn total_updated(&self) -> usize {
        self.folders
            .iter()
            .map(|folder| folder.updated_messages)
            .sum()
    }

    pub fn total_removed(&self) -> usize {
        self.folders
            .iter()
            .map(|folder| folder.removed_messages)
            .sum()
    }

    pub fn delivered(&self) -> Vec<PathBuf> {
        self.folders
            .iter()
            .flat_map(|folder| folder.delivered.iter().cloned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivered_flattens_across_folders() {
        let report = SyncReport {
            folders: vec![
                FolderReport {
                    folder: "INBOX".to_owned(),
                    new_messages: 2,
                    updated_messages: 0,
                    removed_messages: 3,
                    delivered: vec![
                        PathBuf::from("/m/new/a"),
                        PathBuf::from("/m/new/b"),
                    ],
                },
                FolderReport {
                    folder: "Sent".to_owned(),
                    new_messages: 0,
                    updated_messages: 1,
                    removed_messages: 1,
                    delivered: Vec::new(),
                },
            ],
            errors: Vec::new(),
        };
        let expected =
            vec![PathBuf::from("/m/new/a"), PathBuf::from("/m/new/b")];
        assert_eq!(report.delivered(), expected);
        assert_eq!(report.total_new(), 2);
        assert_eq!(report.total_updated(), 1);
        assert_eq!(report.total_removed(), 4);
    }
}
