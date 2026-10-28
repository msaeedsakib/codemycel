use std::path::PathBuf;

use codemycel_core::store::StoreError;
use codemycel_core::{AdapterError, ModelError};
use codemycel_scip::ScipError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IndexError {
    #[error("walking the repository: {0}")]
    Walk(#[from] ignore::Error),
    #[error("reading {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Adapter(#[from] AdapterError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Scip(#[from] ScipError),
}
