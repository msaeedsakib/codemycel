use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::{Language, RepoPath, Root};

use crate::{IndexError, Registry};

/// The roots of every language (spec §8.2) and the mapping of files to them.
pub struct RootDetector {
    roots: Vec<Root>,
}

impl RootDetector {
    /// Asks every adapter for its roots, then applies the `[[root]]` overrides as set operations:
    /// an entry adds its (path, language) root and a `disabled` entry removes it, so re-typing a
    /// root is a disabled entry for the old language plus an entry for the new one.
    pub fn detect(
        repo_root: &Path,
        files: &[RepoPath],
        config: &Config,
        registry: &Registry,
    ) -> Result<Self, IndexError> {
        let mut roots = BTreeSet::new();
        for adapter in registry.adapters() {
            roots.extend(adapter.detect_roots(repo_root, files)?);
        }
        for entry in &config.roots {
            let root = Root {
                path: entry.path.clone(),
                language: entry.language,
            };
            if entry.disabled {
                roots.remove(&root);
            } else {
                roots.insert(root);
            }
        }
        Ok(Self {
            roots: roots.into_iter().collect(),
        })
    }

    /// Sorted by (path, language), the order `Tx::set_root` assigns ids in.
    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    /// The deepest root of `language` whose directory contains `file`.
    pub fn assign(&self, file: &RepoPath, language: Language) -> Option<&Root> {
        self.roots
            .iter()
            .filter(|root| root.language == language && contains(&root.path, file))
            .max_by_key(|root| root.path.as_str().len())
    }
}

fn contains(dir: &RepoPath, file: &RepoPath) -> bool {
    dir.is_root()
        || file
            .as_str()
            .strip_prefix(dir.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
}
