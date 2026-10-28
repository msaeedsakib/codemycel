use std::path::PathBuf;

use thiserror::Error;

use crate::{ModelError, NodeRef, NoteId, RepoPath};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("database already exists at {0}")]
    AlreadyExists(PathBuf),
    /// The database was written by another schema version (`none` when it has no meta table);
    /// callers rebuild it.
    #[error("schema version {found} does not match expected {expected}")]
    SchemaMismatch { found: String, expected: u32 },
    #[error("no file row for `{0}`")]
    UnknownFile(RepoPath),
    #[error("no node for {0}")]
    UnknownNode(NodeRef),
    #[error("no note with uuid {}", .0.0)]
    UnknownNote(NoteId),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error("value out of range: {0}")]
    OutOfRange(&'static str),
}

pub type Result<T> = std::result::Result<T, StoreError>;
