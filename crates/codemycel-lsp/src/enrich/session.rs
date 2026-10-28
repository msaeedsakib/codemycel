use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use codemycel_core::config::ServerConfig;
use codemycel_core::{DegradeReason, ReadinessStrategy, RepoPath, ServerAdapter};
use lsp_types::{FileChangeType, Position, Uri};
use serde_json::Value;
use tokio::sync::watch;

use super::absolute;
use crate::client::{ClientError, LspClient, path_to_uri, uri_to_path};

/// How long `initialize` may take before the start counts as failed.
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(120);

/// Everything needed to start, and restart, one root's server.
#[derive(Clone)]
pub struct ServerLaunch<'a> {
    pub adapter: &'a dyn ServerAdapter,
    pub binary: PathBuf,
    pub config: ServerConfig,
    /// `PATH` for the server process; it inherits the environment otherwise.
    pub search_path: Option<OsString>,
}

/// The outcome of waiting for a server to load its project.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    /// The probe never answered; enrichment proceeds and per-request handling covers the rest.
    ReadyAfterProbeTimeout,
    NotReady,
}

/// A definition request that answers once the server has loaded the project.
#[derive(Clone, Debug)]
pub struct Probe {
    pub uri: Uri,
    pub language_id: &'static str,
    pub text: String,
    pub position: Position,
}

#[derive(Default)]
struct Collected {
    /// The latest diagnostics published for the root's configuration file.
    config_diagnostics: Vec<String>,
}

/// A running server for one root, restartable once.
pub struct Session<'a> {
    launch: ServerLaunch<'a>,
    root_dir: PathBuf,
    client: Mutex<Arc<LspClient>>,
    quiescent: Mutex<watch::Receiver<bool>>,
    collected: Arc<Mutex<Collected>>,
}

impl<'a> Session<'a> {
    /// Spawns and initializes the server with `root_dir` as its root. Must run inside a tokio
    /// runtime.
    pub async fn start(launch: ServerLaunch<'a>, root_dir: &Path) -> Result<Self, DegradeReason> {
        let collected = Arc::new(Mutex::new(Collected::default()));
        let (client, quiescent) = spawn(&launch, root_dir, collected.clone()).await?;
        Ok(Self {
            launch,
            root_dir: root_dir.to_owned(),
            client: Mutex::new(Arc::new(client)),
            quiescent: Mutex::new(quiescent),
            collected,
        })
    }

    pub fn adapter(&self) -> &'a dyn ServerAdapter {
        self.launch.adapter
    }

    pub fn client(&self) -> Option<Arc<LspClient>> {
        self.client.lock().ok().map(|client| client.clone())
    }

    pub fn is_alive(&self) -> bool {
        self.client().is_some_and(|client| client.is_alive())
    }

    /// `workspace/didChangeWatchedFiles` for repository files under `repo_root`: deleted when
    /// gone from disk, changed otherwise.
    pub fn files_changed(&self, repo_root: &Path, files: &[RepoPath]) -> Result<(), ClientError> {
        let client = self.client().ok_or(ClientError::Closed)?;
        let changes: Vec<(Uri, FileChangeType)> = files
            .iter()
            .filter_map(|file| {
                let path = absolute(repo_root, file);
                let kind = if path.exists() {
                    FileChangeType::CHANGED
                } else {
                    FileChangeType::DELETED
                };
                Some((path_to_uri(&path).ok()?, kind))
            })
            .collect();
        if changes.is_empty() {
            return Ok(());
        }
        client.did_change_watched_files(&changes)
    }

    /// Replaces a dead server with a fresh one and waits for it again.
    pub async fn restart(&self, probe: Option<&Probe>) -> Result<(), DegradeReason> {
        let (client, quiescent) = spawn(&self.launch, &self.root_dir, self.collected.clone())
            .await
            .map_err(|_| DegradeReason::Crashed)?;
        if let Ok(mut slot) = self.client.lock() {
            *slot = Arc::new(client);
        }
        if let Ok(mut slot) = self.quiescent.lock() {
            *slot = quiescent;
        }
        match self.wait_ready(probe).await {
            Readiness::NotReady => Err(DegradeReason::Crashed),
            _ => Ok(()),
        }
    }

    /// Waits per the adapter's readiness strategy.
    pub async fn wait_ready(&self, probe: Option<&Probe>) -> Readiness {
        match self.launch.adapter.readiness() {
            ReadinessStrategy::WaitForQuiescent { timeout } => {
                let Some(mut quiescent) = self.quiescent.lock().ok().map(|q| q.clone()) else {
                    return Readiness::NotReady;
                };
                match tokio::time::timeout(timeout, quiescent.wait_for(|q| *q)).await {
                    Ok(Ok(_)) => Readiness::Ready,
                    _ => Readiness::NotReady,
                }
            }
            ReadinessStrategy::ProbeDefinition { timeout } => match probe {
                Some(probe) => self.probe(probe, timeout).await,
                None => Readiness::Ready,
            },
        }
    }

    async fn probe(&self, probe: &Probe, timeout: Duration) -> Readiness {
        let Some(client) = self.client() else {
            return Readiness::NotReady;
        };
        if client
            .did_open(&probe.uri, probe.language_id, &probe.text)
            .is_err()
        {
            return Readiness::ReadyAfterProbeTimeout;
        }
        let deadline = tokio::time::Instant::now() + timeout;
        let mut backoff = Duration::from_millis(100);
        let readiness = loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break Readiness::ReadyAfterProbeTimeout;
            }
            match client
                .definition(&probe.uri, probe.position, remaining)
                .await
            {
                Ok(locations) if !locations.is_empty() => break Readiness::Ready,
                Err(crate::client::ClientError::Closed) => {
                    break Readiness::ReadyAfterProbeTimeout;
                }
                _ => {}
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            tokio::time::sleep(backoff.min(remaining)).await;
            backoff = (backoff * 2).min(Duration::from_secs(2));
        };
        let _ = client.did_close(&probe.uri);
        readiness
    }

    /// Diagnostics the server published for the root's configuration file, as
    /// `<file>:<line>: <message>`.
    pub fn config_diagnostics(&self) -> Vec<String> {
        self.collected
            .lock()
            .map(|c| c.config_diagnostics.clone())
            .unwrap_or_default()
    }

    pub async fn shutdown(self) {
        let client = self.client.into_inner().ok();
        if let Some(client) = client.and_then(|c| Arc::try_unwrap(c).ok()) {
            client.shutdown_and_exit().await;
        }
    }
}

async fn spawn(
    launch: &ServerLaunch<'_>,
    root_dir: &Path,
    collected: Arc<Mutex<Collected>>,
) -> Result<(LspClient, watch::Receiver<bool>), DegradeReason> {
    let adapter = launch.adapter;
    let env: Vec<(OsString, OsString)> = launch
        .search_path
        .iter()
        .map(|path| (OsString::from("PATH"), path.clone()))
        .collect();
    let client = LspClient::spawn(
        &launch.binary,
        adapter.launch_args(),
        root_dir,
        &env,
        adapter.request_concurrency(),
    )
    .map_err(|_| DegradeReason::StartFailed)?;
    let (quiescent_tx, quiescent_rx) = watch::channel(false);
    if let Some(mut notifications) = client.notifications() {
        let config_file = adapter
            .config_file_name()
            .map(|name| (canonical(&root_dir.join(name)), name));
        tokio::spawn(async move {
            while let Some(notification) = notifications.recv().await {
                match notification.method.as_str() {
                    "experimental/serverStatus" => {
                        if let Some(quiescent) = notification.params["quiescent"].as_bool() {
                            let _ = quiescent_tx.send(quiescent);
                        }
                    }
                    "textDocument/publishDiagnostics" => {
                        let Some((path, name)) = &config_file else {
                            continue;
                        };
                        let published = notification.params["uri"]
                            .as_str()
                            .and_then(|uri| uri.parse::<Uri>().ok())
                            .and_then(|uri| uri_to_path(&uri))
                            .map(|published| canonical(&published));
                        if published.as_ref() != Some(path) {
                            continue;
                        }
                        let messages = diagnostics(name, &notification.params);
                        if let Ok(mut collected) = collected.lock() {
                            collected.config_diagnostics = messages;
                        }
                    }
                    _ => {}
                }
            }
        });
    }
    let server_status = matches!(
        adapter.readiness(),
        ReadinessStrategy::WaitForQuiescent { .. }
    );
    client
        .initialize(
            root_dir,
            adapter.initialization_options(&launch.config),
            server_status,
            INITIALIZE_TIMEOUT,
        )
        .await
        .map_err(|_| DegradeReason::StartFailed)?;
    client
        .initialized()
        .map_err(|_| DegradeReason::StartFailed)?;
    Ok((client, quiescent_rx))
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_owned())
}

fn diagnostics(name: &str, params: &Value) -> Vec<String> {
    params["diagnostics"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|d| {
                    let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
                    let message = d["message"].as_str().unwrap_or_default();
                    format!("{name}:{line}: {message}")
                })
                .collect()
        })
        .unwrap_or_default()
}
