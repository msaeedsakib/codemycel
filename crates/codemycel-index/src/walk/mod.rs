mod classify;
mod filter;

use std::fs::File;
use std::path::{Path, PathBuf};

use codemycel_core::config::Config;
use codemycel_core::{FileRecord, RepoPath, SkipReason};
use rayon::prelude::*;

pub use filter::ALWAYS_IGNORED;

use crate::{IndexError, Registry};

/// Lists the repository's files per spec §7.2. Ignored files are not listed at all; every other
/// file is, with a skip reason when it will not be parsed.
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

    /// Sorted by path. Unreadable directories and paths that are not UTF-8 are left out.
    pub fn walk(&self) -> Result<Vec<FileRecord>, IndexError> {
        let config_ignore = filter::config_matcher(&self.repo_root, &self.config.ignore)?;
        let mut paths = Vec::new();
        for entry in filter::walker(&self.repo_root, config_ignore) {
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
            match relative.to_str() {
                Some(text) => paths.push((RepoPath::new(text)?, entry.into_path())),
                None => tracing::warn!("skipping non-UTF-8 path {}", relative.display()),
            }
        }
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
