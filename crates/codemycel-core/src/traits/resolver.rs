use crate::{Candidate, Node, NodeRef, Occurrence, RepoPath, Verdict};

/// A verified source: the SCIP importer or a language server. Calls block; an
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

impl<R: Resolver + ?Sized> Resolver for &R {
    fn name(&self) -> &'static str {
        (**self).name()
    }
    fn covers(&self, file: &RepoPath) -> bool {
        (**self).covers(file)
    }
    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        (**self).resolve(candidates)
    }
    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        (**self).implementers(node)
    }
    fn outgoing_calls(&self, node: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        (**self).outgoing_calls(node)
    }
}

/// One parsed code file, as a deferred resolver maps its answers onto it.
#[derive(Clone, Copy, Debug)]
pub struct SourceFile<'a> {
    pub path: &'a RepoPath,
    pub text: &'a str,
    pub nodes: &'a [Node],
}

/// A verified source that can resolve only once the parsed files are known, such as an
/// imported SCIP index.
pub trait ResolverSource: Send + Sync {
    fn bind<'a>(&'a self, files: &'a [SourceFile<'a>]) -> Box<dyn Resolver + 'a>;
}
