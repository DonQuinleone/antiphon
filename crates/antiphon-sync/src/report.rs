#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderReport {
    pub folder: String,
    pub new_messages: usize,
    pub updated_messages: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    pub folders: Vec<FolderReport>,
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
}
