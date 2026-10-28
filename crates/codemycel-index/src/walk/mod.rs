mod classify;
mod filter;

use std::collections::BTreeMap;
use std::fs::File;
use std::path::{Path, PathBuf};

use codemycel_core::config::Config;
use codemycel_core::{FileRecord, RepoPath, SkipReason};
use rayon::prelude::*;

pub use filter::ALWAYS_IGNORED;

use crate::{IndexError, Registry};

/// Lists the repository's files. Ignored files are not listed at all unless git tracks them;
/// every other file is, with a skip reason when it will not be parsed.
pub struct Walker<'a> {
    repo_root: PathBuf,
    config: &'a Config,
    registry: &'a Registry,
}

impl<'a> Walker<'a> {
    pub fn new(repo_root: &Path, config: &'a Config, registry: &'a Registry) -> Self {
        Self {
            repo_root: repo_root.to_owned(),
            config,
            registry,
        }
    }

    /// Sorted by path. Unreadable directories and paths with no repository path are left out.
    pub fn walk(&self) -> Result<Vec<FileRecord>, IndexError> {
        let config_ignore = filter::config_matcher(&self.repo_root, &self.config.ignore)?;
        let mut paths = BTreeMap::new();
        for entry in filter::walker(&self.repo_root, config_ignore.clone()) {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    tracing::warn!("skipping unreadable entry: {error}");
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&self.repo_root)
                .unwrap_or(entry.path());
            if let Some(path) = repo_path(relative) {
                paths.insert(path, entry.into_path());
            }
        }
        for relative in filter::tracked_ignored(&self.repo_root, &config_ignore) {
            if let Some(path) = repo_path(&relative) {
                paths
                    .entry(path)
                    .or_insert_with(|| self.repo_root.join(&relative));
            }
        }
        let paths: Vec<_> = paths.into_iter().collect();
        let mut records = paths
            .par_iter()
            .map(|(path, absolute)| self.record(path, absolute))
            .collect::<Result<Vec<_>, _>>()?;
        records.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(records)
    }

    fn record(&self, path: &RepoPath, absolute: &Path) -> Result<FileRecord, IndexError> {
        let io = |source| IndexError::Io {
            path: absolute.to_owned(),
            source,
        };
        let size = std::fs::metadata(absolute).map_err(io)?.len();
        let (kind, language) = classify::kind(path, self.registry);
        let (hash, skip_reason) = if size > self.config.max_file_size {
            let mut hasher = blake3::Hasher::new();
            hasher
                .update_reader(File::open(absolute).map_err(io)?)
                .map_err(io)?;
            (hasher.finalize(), Some(SkipReason::Oversize))
        } else {
            let bytes = std::fs::read(absolute).map_err(io)?;
            (
                blake3::hash(&bytes),
                classify::skip_reason(path, kind, &bytes),
            )
        };
        Ok(FileRecord {
            path: path.clone(),
            root_id: None,
            hash,
            size,
            language,
            kind,
            parsed: false,
            skip_reason,
            scip_covered: false,
        })
    }
}

/// `relative` as a repository path, or `None` with a warning when it is not UTF-8, holds a
/// backslash outside Windows (where it is part of a file name), or is otherwise rejected.
fn repo_path(relative: &Path) -> Option<RepoPath> {
    let Some(text) = relative.to_str() else {
        tracing::warn!("skipping non-UTF-8 path {}", relative.display());
        return None;
    };
    if cfg!(not(windows)) && text.contains('\\') {
        tracing::warn!("skipping {text}: a backslash cannot appear in a repository path");
        return None;
    }
    match RepoPath::new(text) {
        Ok(path) => Some(path),
        Err(error) => {
            tracing::warn!("skipping {text}: {error}");
            None
        }
    }
}
