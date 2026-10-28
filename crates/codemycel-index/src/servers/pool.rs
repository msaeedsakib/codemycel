use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use codemycel_core::config::ServerConfig;
use codemycel_core::{DegradeReason, Root, ServerAdapter, ServerInfo};
use codemycel_lsp::enrich::absolute;
use codemycel_lsp::{EnrichOptions, EnrichReport, Enricher, Provisioner, ServerLaunch, Session};
use tokio::runtime::Runtime;

use super::{RootJob, RootOutcome, Servers};
use crate::Registry;

/// Language servers kept running between a session's syncs, one per root. A root's server
/// starts on its first job, is told about changed files and reused while warm, and stops once
/// `reap_idle` finds it unused for the idle timeout. Server adapters come from the registry the
/// pool borrows, so a server outlives the jobs that use it.
pub struct ServerPool<'r> {
    registry: &'r Registry,
    home: PathBuf,
    allow_download: bool,
    options: EnrichOptions,
    idle_timeout: Duration,
    state: Mutex<State<'r>>,
}

#[derive(Default)]
struct State<'r> {
    runtime: Option<Runtime>,
    warm: BTreeMap<Root, Warm<'r>>,
}

struct Warm<'r> {
    session: Session<'r>,
    info: ServerInfo,
    config: ServerConfig,
    root_dir: PathBuf,
    last_used: Instant,
}

impl<'r> ServerPool<'r> {
    /// How long a server stays running without a job using it.
    pub const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

    /// Servers are located as `Provisioner::new(config, home, allow_download)` locates them,
    /// with the job's config.
    pub fn new(
        registry: &'r Registry,
        home: PathBuf,
        allow_download: bool,
        options: EnrichOptions,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            registry,
            home,
            allow_download,
            options,
            idle_timeout,
            state: Mutex::new(State::default()),
        }
    }

    /// Shuts down every server unused for the idle timeout as of `now`.
    pub fn reap_idle(&self, now: Instant) {
        let mut state = self.lock();
        let state = &mut *state;
        let idle: Vec<Root> = state
            .warm
            .iter()
            .filter(|(_, warm)| now.saturating_duration_since(warm.last_used) >= self.idle_timeout)
            .map(|(root, _)| root.clone())
            .collect();
        for root in idle {
            if let Some(warm) = state.warm.remove(&root) {
                shut_down(state.runtime.as_ref(), warm);
            }
        }
    }

    /// When the next running server becomes idle.
    pub fn next_idle_deadline(&self) -> Option<Instant> {
        self.lock()
            .warm
            .values()
            .map(|warm| warm.last_used + self.idle_timeout)
            .min()
    }

    /// The roots with a running server.
    pub fn running(&self) -> Vec<Root> {
        self.lock().warm.keys().cloned().collect()
    }

    pub fn shutdown_all(&self) {
        let mut state = self.lock();
        let state = &mut *state;
        for (_, warm) in std::mem::take(&mut state.warm) {
            shut_down(state.runtime.as_ref(), warm);
        }
    }

    fn lock(&self) -> MutexGuard<'_, State<'r>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// A started server for the job's root, or why none started and which server it was.
    fn launch(
        &self,
        runtime: &Runtime,
        job: &RootJob<'_>,
        server: &'r dyn ServerAdapter,
        config: ServerConfig,
        root_dir: PathBuf,
    ) -> Result<Warm<'r>, (DegradeReason, Option<ServerInfo>)> {
        let provisioner = Provisioner::new(job.config, self.home.clone(), self.allow_download);
        if !provisioner.prerequisite_present(server) {
            return Err((DegradeReason::NoCargo, None));
        }
        let located = provisioner.locate(server).map_err(|error| {
            tracing::warn!(root = %job.root.path, "{error}");
            (DegradeReason::NoServer, None)
        })?;
        let info = ServerInfo {
            name: server.name().to_owned(),
            version: located.version.clone(),
            source: located.source,
        };
        let launch = ServerLaunch {
            adapter: server,
            binary: located.path,
            config: config.clone(),
            search_path: provisioner.search_path().map(ToOwned::to_owned),
        };
        match runtime.block_on(Session::start(launch, &root_dir)) {
            Ok(session) => Ok(Warm {
                session,
                info,
                config,
                root_dir,
                last_used: Instant::now(),
            }),
            Err(reason) => Err((reason, Some(info))),
        }
    }
}

impl Servers for ServerPool<'_> {
    /// Reuses the root's running server after telling it which files changed, or starts one.
    /// A server that crashed between syncs, or whose config changed, is replaced. A crash during
    /// the job restarts it once; when that fails too the root is degraded for this job and the
    /// server dropped, so the next job starts a fresh one.
    fn enrich(&self, job: &RootJob<'_>) -> RootOutcome {
        let Some(server) = self.registry.server_for(job.root.language) else {
            return RootOutcome::degraded(job.view, job.files, DegradeReason::NoServer, None);
        };
        let mut state = self.lock();
        let state = &mut *state;
        if state.runtime.is_none() {
            state.runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .ok();
        }
        let Some(runtime) = state.runtime.as_ref() else {
            return RootOutcome::degraded(job.view, job.files, DegradeReason::StartFailed, None);
        };
        let config = job.config.server(server.language());
        let root_dir = absolute(job.repo_root, &job.root.path);
        let stale = state.warm.get(job.root).is_some_and(|warm| {
            !warm.session.is_alive() || warm.config != config || warm.root_dir != root_dir
        });
        if stale && let Some(warm) = state.warm.remove(job.root) {
            shut_down(Some(runtime), warm);
        }
        let warm = match state.warm.entry(job.root.clone()) {
            std::collections::btree_map::Entry::Occupied(entry) => {
                let warm = entry.into_mut();
                if let Err(error) = warm.session.files_changed(job.repo_root, job.changed) {
                    tracing::debug!(root = %job.root.path, "telling the server about changes: {error}");
                }
                warm
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                match self.launch(runtime, job, server, config, root_dir) {
                    Ok(warm) => entry.insert(warm),
                    Err((reason, info)) => {
                        return RootOutcome::degraded(job.view, job.files, reason, info);
                    }
                }
            }
        };
        let report = runtime.block_on(enrich_with(&warm.session, job, self.options));
        let info = warm.info.clone();
        if let Some(reason) = report.degraded {
            if let Some(warm) = state.warm.remove(job.root) {
                shut_down(Some(runtime), warm);
            }
            return RootOutcome::degraded(job.view, job.files, reason, Some(info));
        }
        warm.last_used = Instant::now();
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

impl Drop for ServerPool<'_> {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

async fn enrich_with(
    session: &Session<'_>,
    job: &RootJob<'_>,
    options: EnrichOptions,
) -> EnrichReport {
    let enricher = match Enricher::attach(
        session,
        job.language,
        job.view,
        job.repo_root,
        job.files,
        options,
    )
    .await
    {
        Ok(enricher) => enricher,
        Err(reason) => {
            return EnrichReport {
                degraded: Some(reason),
                ..EnrichReport::default()
            };
        }
    };
    let mut report = enricher.enrich_files(job.files).await;
    if report.degraded.is_none() {
        let found = enricher.enrich_implementers(job.others).await;
        report.degraded = found.degraded;
        report.edges.extend(found.edges);
        report.partially_enriched.extend(found.partially_enriched);
    }
    report
}

/// `shutdown` and `exit`; without a runtime the process is killed when the session drops.
fn shut_down(runtime: Option<&Runtime>, warm: Warm<'_>) {
    if let Some(runtime) = runtime {
        runtime.block_on(warm.session.shutdown());
    }
}
