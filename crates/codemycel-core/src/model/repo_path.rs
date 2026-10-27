use std::fmt;

use serde::{Deserialize, Serialize};

use super::ModelError;

/// A repository-relative path with forward slashes on every platform (spec §9).
/// The empty path is the repository root.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoPath(String);

impl RepoPath {
    pub fn new(path: &str) -> Result<Self, ModelError> {
        let reject = |reason| ModelError::InvalidRepoPath {
            path: path.to_owned(),
            reason,
        };
        let slashed = path.replace('\\', "/");
        let bytes = slashed.as_bytes();
        if slashed.starts_with('/') {
            return Err(reject("absolute path"));
        }
        let drive = bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic();
        if drive && matches!(bytes.get(2), None | Some(b'/')) {
            return Err(reject("absolute path"));
        }
        let mut segments = Vec::new();
        for segment in slashed.split('/') {
            match segment {
                "" | "." => {}
                ".." => return Err(reject("`..` segment")),
                other => segments.push(other),
            }
        }
        Ok(Self(segments.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RepoPath {
    type Error = ModelError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(&value)
    }
}

impl From<RepoPath> for String {
    fn from(value: RepoPath) -> Self {
        value.0
    }
}
