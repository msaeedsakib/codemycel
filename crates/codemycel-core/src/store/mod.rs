mod dump;
mod error;
mod open;
mod read;
mod schema;
mod sql;
mod write;

use rusqlite::Connection;

pub use error::StoreError;
pub use read::{Counts, StoreRead, StoredEdge, StoredFile, StoredRoot, StoredUnresolved};
pub use schema::SCHEMA_VERSION;

/// The per-repository SQLite index (spec §9). Reads come from `StoreRead`; writes happen on a
/// `Tx` inside `Store::transaction`.
#[derive(Debug)]
pub struct Store {
    conn: Connection,
    commits: u64,
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
