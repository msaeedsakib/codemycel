use serde::{Deserialize, Serialize};

use super::{
    ByteRange, Coverage, DegradeReason, FileKind, Language, RepoPath, RootId, ServerSource,
    SkipReason, Span,
};

/// A language project boundary. `Store::set_root` assigns its `RootId`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Root {
    pub path: RepoPath,
    pub language: Language,
}

/// The language server a root was enriched with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub source: ServerSource,
}

/// What the verify phase recorded for a root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootStatus {
    pub server: Option<ServerInfo>,
    pub coverage: Coverage,
    pub degraded: Option<DegradeReason>,
    /// Diagnostics the server published for the root's configuration file; they never degrade it.
    pub config_diagnostics: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRecord {
    pub path: RepoPath,
    pub root_id: Option<RootId>,
    pub hash: blake3::Hash,
    pub size: u64,
    pub language: Option<Language>,
    pub kind: FileKind,
    pub parsed: bool,
    pub skip_reason: Option<SkipReason>,
    pub scip_covered: bool,
}

/// A Rust `impl` block. Not a node; maps server results that land on impl headers.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImplRecord {
    pub file: RepoPath,
    pub self_type_text: String,
    pub trait_text: Option<String>,
    pub span: Span,
    pub header: ByteRange,
    pub body: ByteRange,
    pub self_type_selection: ByteRange,
}
