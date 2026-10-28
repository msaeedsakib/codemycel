//! Indexing pipeline: walk, root detection, phase orchestration, incremental sync, and the watcher.

pub mod communities;
mod error;
mod locate;
mod lock;
pub mod notes;
pub mod pipeline;
mod registry;
mod repo_registry;
pub mod roots;
pub mod servers;
pub mod session;
pub mod sync;
pub mod walk;
pub mod watch;

pub use communities::CommunityDetector;
pub use error::IndexError;
pub use locate::{INDEX_FILE_NAME, RepoLocation, RepoLocator};
pub use lock::{Lock, LockGuard, LockKind};
pub use notes::NotesReconciler;
pub use pipeline::{
    Phase, Pipeline, PipelineReport, Progress, Scip, Sources, SyncMode, SyncReport,
};
pub use registry::Registry;
pub use repo_registry::{RegistryEntry, RepoRegistry};
pub use roots::RootDetector;
pub use servers::{BatchServers, RootJob, RootOutcome, ServerPool, Servers};
pub use session::{SessionError, SessionLoop, SessionOptions, SessionState};
pub use sync::{ChangeSet, ChangedSymbols, SymbolsDiff, SyncPlan};
pub use walk::Walker;
pub use watch::{Watcher, WatcherHandle};
