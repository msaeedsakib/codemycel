use std::ffi::OsString;
use std::path::{Path, PathBuf};

use codemycel_core::store::{Store, StoreError};

use crate::IndexError;

/// How `open_or_rebuild` came by its store.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opened {
    Existing,
    Created,
    /// The database was of schema `found`, or not a database, and was replaced by an empty one.
    Rebuilt {
        found: String,
    },
}

impl Opened {
    /// Whether the next sync must be full: anything but an existing index.
    pub fn needs_full_sync(&self) -> bool {
        *self != Opened::Existing
    }
}

/// The writable store at `db`: opened, created when missing, or deleted with its WAL and SHM
/// files and created again when of another schema version. Call it under `sync.lock`.
pub fn open_or_rebuild(db: &Path) -> Result<(Store, Opened), IndexError> {
    if !db.exists() {
        return Ok((Store::create(db)?, Opened::Created));
    }
    match Store::open(db, false) {
        Ok(store) => Ok((store, Opened::Existing)),
        Err(StoreError::SchemaMismatch { found, .. }) => {
            for path in [db.to_owned(), sibling(db, "-wal"), sibling(db, "-shm")] {
                match std::fs::remove_file(&path) {
                    Err(source) if source.kind() != std::io::ErrorKind::NotFound => {
                        return Err(IndexError::Io { path, source });
                    }
                    _ => {}
                }
            }
            Ok((Store::create(db)?, Opened::Rebuilt { found }))
        }
        Err(error) => Err(error.into()),
    }
}

/// `db` with `suffix` appended to its file name.
pub fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(db.as_os_str());
    name.push(suffix);
    PathBuf::from(name)
}
