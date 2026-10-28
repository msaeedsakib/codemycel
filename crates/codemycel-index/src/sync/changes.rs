use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::store::StoredFile;
use codemycel_core::{FileRecord, RepoPath};

/// Files added, modified, or deleted between the index and the working tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChangeSet {
    pub added: BTreeSet<RepoPath>,
    pub modified: BTreeSet<RepoPath>,
    pub deleted: BTreeSet<RepoPath>,
}

impl ChangeSet {
    /// The walk's files against the stored ones. A file is modified when its content hash, size,
    /// kind, language, or skip reason differs; touching a file without changing it is no change.
    pub fn between(stored: &[StoredFile], walked: &[FileRecord]) -> Self {
        let stored: BTreeMap<&RepoPath, &FileRecord> =
            stored.iter().map(|f| (&f.record.path, &f.record)).collect();
        let mut changes = Self::default();
        for file in walked {
            match stored.get(&file.path) {
                None => {
                    changes.added.insert(file.path.clone());
                }
                Some(old) => {
                    let same = old.hash == file.hash
                        && old.size == file.size
                        && old.kind == file.kind
                        && old.language == file.language
                        && old.skip_reason == file.skip_reason;
                    if !same {
                        changes.modified.insert(file.path.clone());
                    }
                }
            }
        }
        let walked: BTreeSet<&RepoPath> = walked.iter().map(|f| &f.path).collect();
        changes.deleted = stored
            .keys()
            .filter(|path| !walked.contains(*path))
            .map(|path| (*path).clone())
            .collect();
        changes
    }

    pub fn len(&self) -> usize {
        self.added.len() + self.modified.len() + self.deleted.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every changed path, sorted.
    pub fn paths(&self) -> Vec<RepoPath> {
        let mut paths: Vec<RepoPath> = self
            .added
            .iter()
            .chain(&self.modified)
            .chain(&self.deleted)
            .cloned()
            .collect();
        paths.sort();
        paths
    }
}
