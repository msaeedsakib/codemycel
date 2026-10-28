use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;

use crate::store::{StoreError, StoreRead, meta};
use crate::{Coverage, DegradeReason, Language, RepoPath};

/// How current an answer is: the footer every answer ends with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Freshness {
    /// Git HEAD at the last sync; `None` without git.
    pub indexed_commit: Option<String>,
    pub indexed_at: Option<String>,
    /// The answer's stale files plus the files the last sync changed.
    pub changed_files: u64,
    pub coverage: Vec<LanguageCoverage>,
    /// Files in this answer whose content differs from the indexed content, or that are gone.
    pub stale: Vec<RepoPath>,
}

/// The distinct states of one language's roots.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LanguageCoverage {
    pub language: Language,
    pub states: Vec<CoverageState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CoverageState {
    Verified {
        by: Coverage,
    },
    Degraded {
        reason: DegradeReason,
    },
    /// No verify phase has run over the root.
    NotVerified,
}

impl Freshness {
    /// The footer for an answer showing `files`. Hashes each of them against the index.
    pub fn compute(
        store: &impl StoreRead,
        repo_root: &Path,
        files: &BTreeSet<RepoPath>,
    ) -> Result<Self, StoreError> {
        let mut stale = Vec::new();
        for path in files {
            let Some(stored) = store.file_by_path(path)? else {
                continue;
            };
            let current = std::fs::read(repo_root.join(path.as_str()))
                .ok()
                .map(|bytes| blake3::hash(&bytes));
            if current != Some(stored.record.hash) {
                stale.push(path.clone());
            }
        }
        let last_sync: u64 = store
            .meta_get(meta::LAST_SYNC_CHANGED_FILES)?
            .and_then(|count| count.parse().ok())
            .unwrap_or(0);
        let mut by_language: BTreeMap<Language, BTreeSet<CoverageState>> = BTreeMap::new();
        for root in store.roots()? {
            let state = match root.status {
                Some(status) => match status.degraded {
                    Some(reason) => CoverageState::Degraded { reason },
                    None => CoverageState::Verified {
                        by: status.coverage,
                    },
                },
                None => CoverageState::NotVerified,
            };
            by_language
                .entry(root.root.language)
                .or_default()
                .insert(state);
        }
        Ok(Self {
            indexed_commit: store.meta_get(meta::INDEXED_COMMIT)?,
            indexed_at: store.meta_get(meta::INDEXED_AT)?,
            changed_files: last_sync + stale.len() as u64,
            coverage: by_language
                .into_iter()
                .map(|(language, states)| LanguageCoverage {
                    language,
                    states: states.into_iter().collect(),
                })
                .collect(),
            stale,
        })
    }

    /// Drops stale files no longer in the answer after the budget cut lines.
    pub(crate) fn retain_stale(&mut self, files: &BTreeSet<RepoPath>) {
        let before = self.stale.len();
        self.stale.retain(|path| files.contains(path));
        self.changed_files -= (before - self.stale.len()) as u64;
    }
}
