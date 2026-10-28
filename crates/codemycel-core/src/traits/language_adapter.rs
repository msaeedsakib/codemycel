use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::{Language, ParsedFile, RepoPath, Root};

#[derive(Debug, Error)]
pub enum AdapterError {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parsing {path}: {message}")]
    Parse { path: RepoPath, message: String },
}

/// The syntactic half of a language. The server half is a `ServerAdapter`; the
/// index registry pairs the two by `Language`.
pub trait LanguageAdapter: Send + Sync {
    fn language(&self) -> Language;

    /// File extensions this adapter claims, without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Roots of this language among `files`. `repo_root` is where manifests are read.
    fn detect_roots(&self, repo_root: &Path, files: &[RepoPath])
    -> Result<Vec<Root>, AdapterError>;

    /// Nodes, impl records, candidates, and structural edges of one file.
    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError>;

    /// Whether a canonicalised definition path lies outside the repository's own code.
    fn external_path_predicate(&self, canonical_path: &Path, repo_root: &Path) -> bool;
}
