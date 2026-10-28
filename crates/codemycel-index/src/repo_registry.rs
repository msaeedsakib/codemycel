use std::path::{Path, PathBuf};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::IndexError;

const FILE_NAME: &str = "registry.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub path: PathBuf,
    pub commit: Option<String>,
    pub last_seen: Timestamp,
}

/// `<codemycel home>/registry.json`: the repositories indexed on this machine. Informational
/// only; no query is ever answered from another entry.
#[derive(Debug)]
pub struct RepoRegistry {
    file: PathBuf,
    entries: Vec<RegistryEntry>,
}

impl RepoRegistry {
    /// A missing or unreadable file is an empty registry.
    pub fn load(home: &Path) -> Self {
        let file = home.join(FILE_NAME);
        let entries = std::fs::read(&file)
            .ok()
            .and_then(|bytes| match serde_json::from_slice(&bytes) {
                Ok(entries) => Some(entries),
                Err(error) => {
                    tracing::warn!("ignoring unreadable {}: {error}", file.display());
                    None
                }
            })
            .unwrap_or_default();
        Self { file, entries }
    }

    /// Sorted by path.
    pub fn entries(&self) -> &[RegistryEntry] {
        &self.entries
    }

    /// Upserts the entry for `repo_root` and writes the file, through a rename so a reader never
    /// sees half of it.
    pub fn record(&mut self, repo_root: &Path, commit: Option<&str>) -> Result<(), IndexError> {
        let entry = RegistryEntry {
            path: repo_root.to_owned(),
            commit: commit.map(ToOwned::to_owned),
            last_seen: Timestamp::now(),
        };
        match self.entries.iter_mut().find(|e| e.path == entry.path) {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
        let io = |source| IndexError::Io {
            path: self.file.clone(),
            source,
        };
        if let Some(dir) = self.file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let text = serde_json::to_vec_pretty(&self.entries).map_err(|e| io(e.into()))?;
        let temp = self
            .file
            .with_extension(format!("json.{}", std::process::id()));
        std::fs::write(&temp, text).map_err(io)?;
        std::fs::rename(&temp, &self.file).map_err(io)
    }
}
