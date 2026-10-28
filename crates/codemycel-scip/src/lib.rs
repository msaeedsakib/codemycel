//! SCIP index importer and resolver.

mod coverage;
mod mapping;
mod reader;
mod resolver;

use std::path::PathBuf;

use codemycel_core::config::ConfigError;
use thiserror::Error;

pub use reader::{ScipImporter, ScipIndex};
pub use resolver::ScipResolver;

#[derive(Debug, Error)]
pub enum ScipError {
    #[error("reading {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("decoding {}: {source}", path.display())]
    Decode {
        path: PathBuf,
        source: protobuf::Error,
    },
    #[error("no git HEAD to record as the SCIP commit; pass --scip-commit")]
    NoHead,
    #[error(transparent)]
    Config(#[from] ConfigError),
}
