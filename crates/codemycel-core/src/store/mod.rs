mod dump;
mod error;
mod open;
mod read;
mod schema;
mod sql;
mod write;

use rusqlite::Connection;

use crate::NodeRef;

pub use error::StoreError;

/// Keys of the `meta` table.
pub mod meta {
    pub const SCHEMA_VERSION: &str = "schema_version";
    pub const CODEMYCEL_VERSION: &str = "codemycel_version";
    /// Git HEAD at the last sync; absent without git.
    pub const INDEXED_COMMIT: &str = "indexed_commit";
    /// RFC 3339 time of the last sync.
    pub const INDEXED_AT: &str = "indexed_at";
    /// Files added, modified, or deleted by the last sync.
    pub const LAST_SYNC_CHANGED_FILES: &str = "last_sync_changed_files";
    /// Hash of the SCIP index the last sync imported; absent without one.
    pub const SCIP_HASH: &str = "scip_hash";
    /// The `[servers.*]` configuration of the last sync, as JSON.
    pub const SERVERS_CONFIG: &str = "servers_config";
}
pub use read::{
    Counts, StoreRead, StoredEdge, StoredFile, StoredNote, StoredRoot, StoredUnresolved,
};
pub use schema::SCHEMA_VERSION;

/// The per-repository SQLite index. Reads come from `StoreRead`; writes happen on a
/// `Tx` inside `Store::transaction`.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    commits: u64,
}

/// What a note is attached to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteTarget<'a> {
    Node(&'a NodeRef),
    /// The repository as a whole, which is no node.
    Repository,
    Orphaned,
}

/// A write transaction. Also implements `StoreRead`, so a phase can read its own writes.
#[derive(Debug)]
pub struct Tx<'a> {
    tx: rusqlite::Transaction<'a>,
}

impl Store {
    /// Runs `f` in one transaction: commits on `Ok`, rolls back on `Err`.
    pub fn transaction<T, E>(&mut self, f: impl FnOnce(&mut Tx<'_>) -> Result<T, E>) -> Result<T, E>
    where
        E: From<StoreError>,
    {
        let mut tx = Tx {
            tx: self.conn.transaction().map_err(StoreError::from)?,
        };
        match f(&mut tx) {
            Ok(value) => {
                tx.tx.commit().map_err(StoreError::from)?;
                self.commits += 1;
                Ok(value)
            }
            Err(error) => {
                tx.tx.rollback().map_err(StoreError::from)?;
                Err(error)
            }
        }
    }

    /// Transactions committed through this handle since it was opened.
    pub fn commit_count(&self) -> u64 {
        self.commits
    }
}

impl StoreRead for Store {
    fn conn(&self) -> &Connection {
        &self.conn
    }
}

impl StoreRead for Tx<'_> {
    fn conn(&self) -> &Connection {
        &self.tx
    }
}
