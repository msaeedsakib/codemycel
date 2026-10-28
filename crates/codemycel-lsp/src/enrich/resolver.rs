use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::{
    Candidate, DegradeReason, LanguageAdapter, NodeRef, Occurrence, RepoPath, Resolver, Verdict,
};
use tokio::runtime::Runtime;

use super::{EnrichOptions, EnrichReport, Enricher, IndexView, Readiness, ServerLaunch};

/// One root's language server behind the blocking `Resolver` interface. It owns the runtime its
/// server is driven on, so it must not be created from inside another tokio runtime.
pub struct LspResolver<'a> {
    enricher: Option<Enricher<'a>>,
    files: BTreeSet<RepoPath>,
    runtime: Runtime,
}

impl<'a> LspResolver<'a> {
    /// Starts the server for `root` and waits until it is ready to answer for `files`.
    pub fn start(
        launch: ServerLaunch<'a>,
        language: &'a dyn LanguageAdapter,
        view: &'a IndexView,
        repo_root: &Path,
        root: &RepoPath,
        files: &[RepoPath],
        options: EnrichOptions,
    ) -> Result<Self, DegradeReason> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|_| DegradeReason::StartFailed)?;
        let enricher = runtime.block_on(Enricher::start(
            launch, language, view, repo_root, root, files, options,
        ))?;
        Ok(Self {
            enricher: Some(enricher),
            files: files.iter().cloned().collect(),
            runtime,
        })
    }

    pub fn readiness(&self) -> Readiness {
        self.enricher
            .as_ref()
            .map_or(Readiness::NotReady, Enricher::readiness)
    }

    pub fn enrich_files(&self, files: &[RepoPath]) -> EnrichReport {
        match &self.enricher {
            Some(enricher) => self.runtime.block_on(enricher.enrich_files(files)),
            None => EnrichReport::default(),
        }
    }

    /// See [`Enricher::enrich_implementers`].
    pub fn enrich_implementers(&self, files: &[RepoPath]) -> EnrichReport {
        match &self.enricher {
            Some(enricher) => self.runtime.block_on(enricher.enrich_implementers(files)),
            None => EnrichReport::default(),
        }
    }

    /// `shutdown` and `exit`; dropping the resolver instead kills the server.
    pub fn shutdown(mut self) {
        if let Some(enricher) = self.enricher.take() {
            self.runtime.block_on(enricher.shutdown());
        }
    }
}

impl Resolver for LspResolver<'_> {
    fn name(&self) -> &'static str {
        "lsp"
    }

    fn covers(&self, file: &RepoPath) -> bool {
        self.files.contains(file)
    }

    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        match &self.enricher {
            Some(enricher) => self.runtime.block_on(enricher.resolve(candidates)),
            None => Vec::new(),
        }
    }

    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        match &self.enricher {
            Some(enricher) => self.runtime.block_on(enricher.implementers(node)),
            None => Vec::new(),
        }
    }

    fn outgoing_calls(&self, node: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        let enricher = self.enricher.as_ref()?;
        self.runtime.block_on(enricher.outgoing_calls(node))
    }
}
