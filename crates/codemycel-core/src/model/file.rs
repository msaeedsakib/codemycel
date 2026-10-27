use serde::{Deserialize, Serialize};

use super::{ByteRange, FileKind, Language, RepoPath, RootId, SkipReason, Span};

/// A language project boundary (spec §4, §8.2). `Store::set_root` assigns its `RootId`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Root {
    pub path: RepoPath,
    pub language: Language,
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

/// A Rust `impl` block (spec §4). Not a node; maps server results that land on impl headers.
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
