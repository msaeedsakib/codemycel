mod pool;

pub use pool::ServerPool;

use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::{
    DegradeReason, Edge, Language, LanguageAdapter, RepoPath, Root, ServerAdapter, ServerInfo,
    Unresolved, UnresolvedReason,
};
use codemycel_lsp::{EnrichOptions, IndexView, LspResolver, Provisioner, ServerLaunch};

/// One root's share of a verify run.
pub struct RootJob<'a> {
    pub repo_root: &'a Path,
    pub config: &'a Config,
    pub root: &'a Root,
    pub server: &'a dyn ServerAdapter,
    pub language: &'a dyn LanguageAdapter,
    /// Every indexed code file; server answers are mapped onto it.
    pub view: &'a IndexView,
    /// The root's files to enrich, sorted.
    pub files: &'a [RepoPath],
    /// Files this server answers for but does not enrich, whose traits and interfaces are asked
    /// for implementers.
    pub others: &'a [RepoPath],
}

/// What enriching a root produced.
#[derive(Debug, Default)]
pub struct RootOutcome {
    pub server: Option<ServerInfo>,
    pub degraded: Option<DegradeReason>,
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
    pub partially_enriched: BTreeSet<RepoPath>,
    pub config_diagnostics: Vec<String>,
}

impl RootOutcome {
    /// A degraded root: no edges, and every candidate of `files` `server_unavailable`.
    pub fn degraded(
        view: &IndexView,
        files: &[RepoPath],
        reason: DegradeReason,
        server: Option<ServerInfo>,
    ) -> Self {
        Self {
            server,
            degraded: Some(reason),
            unresolved: files
                .iter()
                .filter_map(|file| view.file(file))
                .flat_map(|file| &file.candidates)
                .map(|candidate| Unresolved {
                    candidate: candidate.clone(),
                    reason: UnresolvedReason::ServerUnavailable,
                })
                .collect(),
            ..Self::default()
        }
    }
}

/// What a sync tells the servers before its first job.
pub struct SyncStart<'a> {
    pub repo_root: &'a Path,
    /// The store's `indexed_at` as the sync found it. A server still running from a sync that
    /// did not record it has not seen what the sync that did changed.
    pub indexed_at: Option<&'a str>,
    /// Every path the sync found added, modified, or deleted.
    pub changed: &'a [RepoPath],
    /// The languages whose manifests or project configs are among `changed`.
    pub reconfigured: &'a BTreeSet<Language>,
}

/// Where a root's language server comes from: started and stopped for each run in batch mode,
/// kept warm between syncs by a session.
pub trait Servers {
    fn enrich(&self, job: &RootJob<'_>) -> RootOutcome;

    /// Called once per sync, before its first job, so servers still running from an earlier
    /// sync catch up on the changes, whether or not their root has a job in this sync.
    fn sync_started(&self, _start: &SyncStart<'_>) {}

    /// Called once a sync is written, with the `indexed_at` it recorded.
    fn sync_finished(&self, _indexed_at: &str) {}
}

/// Locates, starts, enriches with, and shuts down a server for every job.
pub struct BatchServers<'a> {
    pub provisioner: &'a Provisioner,
    pub options: EnrichOptions,
}

impl Servers for BatchServers<'_> {
    fn enrich(&self, job: &RootJob<'_>) -> RootOutcome {
        if !self.provisioner.prerequisite_present(job.server) {
            return RootOutcome::degraded(job.view, job.files, DegradeReason::NoCargo, None);
        }
        let located = match self.provisioner.locate(job.server) {
            Ok(located) => located,
            Err(error) => {
                tracing::warn!(root = %job.root.path, "{error}");
                return RootOutcome::degraded(job.view, job.files, DegradeReason::NoServer, None);
            }
        };
        let info = ServerInfo {
            name: job.server.name().to_owned(),
            version: located.version.clone(),
            source: located.source,
        };
        let launch = ServerLaunch {
            adapter: job.server,
            binary: located.path,
            config: job.config.server(job.server.language()),
            search_path: self.provisioner.search_path().map(ToOwned::to_owned),
        };
        let resolver = match LspResolver::start(
            launch,
            job.language,
            job.view,
            job.repo_root,
            &job.root.path,
            job.files,
            self.options,
        ) {
            Ok(resolver) => resolver,
            Err(reason) => return RootOutcome::degraded(job.view, job.files, reason, Some(info)),
        };
        let mut report = resolver.enrich_files(job.files);
        if report.degraded.is_none() {
            let found = resolver.enrich_implementers(job.others);
            report.degraded = found.degraded;
            report.edges.extend(found.edges);
            report.partially_enriched.extend(found.partially_enriched);
        }
        resolver.shutdown();
        if let Some(reason) = report.degraded {
            return RootOutcome::degraded(job.view, job.files, reason, Some(info));
        }
        RootOutcome {
            server: Some(info),
            degraded: None,
            edges: report.edges,
            unresolved: report.unresolved,
            partially_enriched: report.partially_enriched,
            config_diagnostics: report.config_diagnostics,
        }
    }
}
