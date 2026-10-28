use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::error::Result;
use super::schema::{DDL, SCHEMA_VERSION};
use super::{Store, StoreError, meta};

impl Store {
    /// Creates a new database at `db_path`; an error if the file already exists.
    pub fn create(db_path: &Path) -> Result<Self> {
        if db_path.exists() {
            return Err(StoreError::AlreadyExists(db_path.to_path_buf()));
        }
        let mut conn = Connection::open(db_path)?;
        configure(&conn, false)?;
        let tx = conn.transaction()?;
        tx.execute_batch(DDL)?;
        tx.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2), (?3, ?4)",
            params![
                meta::SCHEMA_VERSION,
                SCHEMA_VERSION.to_string(),
                meta::CODEMYCEL_VERSION,
                env!("CARGO_PKG_VERSION")
            ],
        )?;
        tx.commit()?;
        Ok(Self { conn, commits: 0 })
    }

    /// Opens an existing database. A different `schema_version`, or a file that is not a SQLite
    /// database, is `StoreError::SchemaMismatch`; either way the caller rebuilds.
    pub fn open(db_path: &Path, read_only: bool) -> Result<Self> {
        let access = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        };
        let conn = Connection::open_with_flags(
            db_path,
            access | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_URI,
        )?;
        match check_version(&conn) {
            Err(StoreError::Sqlite(error))
                if error.sqlite_error_code() == Some(rusqlite::ErrorCode::NotADatabase) =>
            {
                return Err(StoreError::SchemaMismatch {
                    found: "not a database".to_owned(),
                    expected: SCHEMA_VERSION,
                });
            }
            result => result?,
        }
        configure(&conn, read_only)?;
        Ok(Self { conn, commits: 0 })
    }
}

fn configure(conn: &Connection, read_only: bool) -> Result<()> {
    if !read_only {
        conn.query_row("PRAGMA journal_mode = WAL", [], |row| {
            row.get::<_, String>(0)
        })?;
    }
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(())
}

fn check_version(conn: &Connection) -> Result<()> {
    let has_meta: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'meta')",
        [],
        |row| row.get(0),
    )?;
    let found = if has_meta {
        conn.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    } else {
        None
    };
    match found {
        Some(found) if found == SCHEMA_VERSION.to_string() => Ok(()),
        found => Err(StoreError::SchemaMismatch {
            found: found.unwrap_or_else(|| "none".to_owned()),
            expected: SCHEMA_VERSION,
        }),
    }
}
