//! Indexing pipeline: walk, root detection, phase orchestration, incremental sync, and the watcher.

mod error;
pub mod pipeline;
mod registry;
pub mod roots;
pub mod walk;

pub use error::IndexError;
pub use pipeline::{Pipeline, PipelineReport, Sources};
pub use registry::Registry;
pub use roots::RootDetector;
pub use walk::Walker;
