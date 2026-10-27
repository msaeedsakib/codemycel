use crate::{Candidate, NodeRef, Occurrence, RepoPath, Verdict};

/// A verified source: the SCIP importer or a language server (spec §7.3). Calls block; an
/// asynchronous implementation drives its own runtime.
pub trait Resolver: Send + Sync {
    /// `"scip"` or `"lsp"`.
    fn name(&self) -> &'static str;

    fn covers(&self, file: &RepoPath) -> bool;

    /// One verdict per candidate, in order. A verdict is final.
    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict>;

    /// Implementers of a trait or interface, or overriders of one of its methods.
    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef>;

    /// Callees of a function or method with their call sites; `None` when unsupported.
    fn outgoing_calls(&self, node: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>>;
}
