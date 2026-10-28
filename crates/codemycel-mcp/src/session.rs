use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use codemycel_core::config::paths::REPO_DIR_NAME;
use codemycel_core::query::{Answer, Budget, QueryEngine, QueryError, TextFormatter};
use codemycel_core::store::{Store, StoreError, StoreRead, meta};
use codemycel_index::{
    INDEX_FILE_NAME, Lock, LockKind, NoteRequest, NoteTool, Registry, RepoRegistry, SessionLoop,
    SessionOptions, SessionState,
};

/// The repository the MCP process serves. Until its index exists and has completed a sync,
/// every answer is the `no index` message and no lock is taken; from then on a `SessionLoop`
/// watches and syncs it, and every call reads it through a fresh read-only store.
pub struct Session {
    repo_root: PathBuf,
    db: PathBuf,
    options: SessionOptions,
    registry: Registry,
    running: Mutex<Option<SessionLoop>>,
}

enum Probe {
    Unindexed,
    Outdated,
    Ready(Store),
    Failed(StoreError),
}

impl Session {
    pub fn new(repo_root: &Path, options: SessionOptions) -> Self {
        Self {
            repo_root: repo_root.to_owned(),
            db: repo_root.join(REPO_DIR_NAME).join(INDEX_FILE_NAME),
            options,
            registry: Registry::default(),
            running: Mutex::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running().is_some()
    }

    /// Starts the session loop if the index has become ready since the last look.
    pub fn refresh(&self) {
        if !self.is_running() {
            let _ = self.open();
        }
    }

    /// Renders what `query` answers from a fresh engine, or why there is nothing to ask.
    pub fn answer(
        &self,
        query: impl FnOnce(&QueryEngine<'_>) -> Result<Answer, QueryError>,
    ) -> String {
        let store = match self.open() {
            Ok(store) => store,
            Err(text) => return text,
        };
        let engine = QueryEngine::open(&self.repo_root, store, self.registry.adapters());
        match query(&engine) {
            Ok(answer) => TextFormatter::render(&answer),
            Err(error) => format!("error: {error}"),
        }
    }

    pub fn status(&self, budget: Budget) -> String {
        let sync_owner = Lock::current_owner(&self.repo_root, LockKind::Sync);
        let session_owner = Lock::current_owner(&self.repo_root, LockKind::Session);
        self.answer(|engine| engine.status(sync_owner, session_owner, budget))
    }

    pub fn note(&self, request: &NoteRequest<'_>, budget: Budget) -> String {
        if let Err(text) = self.open() {
            return text;
        }
        let read_only = self
            .running()
            .as_ref()
            .is_none_or(SessionLoop::is_read_only);
        match NoteTool::write(&self.repo_root, request, read_only, budget) {
            Ok(answer) => TextFormatter::render(&answer),
            Err(error) => format!("error: {error}"),
        }
    }

    /// Stops the session loop: its watcher and servers, and both locks.
    pub fn stop(&self) {
        let running = self.running().take();
        drop(running);
    }

    /// A read-only store over a ready index, starting the session loop on the way; otherwise
    /// the text to answer instead.
    fn open(&self) -> Result<Store, String> {
        match self.probe() {
            Probe::Ready(store) => {
                self.start();
                Ok(store)
            }
            Probe::Outdated => {
                self.start();
                Err(match self.state() {
                    Some(SessionState::ReadOnly) => format!(
                        "index is out of date: run codemycel init in {} to rebuild it",
                        self.repo_root.display()
                    ),
                    _ => self.rebuilding(),
                })
            }
            Probe::Unindexed => Err(match self.state() {
                None | Some(SessionState::ReadOnly | SessionState::Stopped) => self.no_index(),
                Some(_) => self.rebuilding(),
            }),
            Probe::Failed(error) => Err(format!("error: reading the index: {error}")),
        }
    }

    /// The `no index` line, then the repositories indexed elsewhere on this machine, listed for
    /// information only.
    fn no_index(&self) -> String {
        let mut text = format!(
            "no index: run codemycel init in {}",
            self.repo_root.display()
        );
        let registry = RepoRegistry::load(&self.options.home);
        if !registry.entries().is_empty() {
            text.push_str("\nindexed repositories:");
            for entry in registry.entries() {
                text.push_str(&format!("\n  {}", entry.path.display()));
            }
        }
        text
    }

    fn rebuilding(&self) -> String {
        "index is out of date: this session is rebuilding it; ask again shortly".to_owned()
    }

    /// Ready once the first sync has committed `meta.indexed_at`. A database without a `meta`
    /// table is one `init` has only just created.
    fn probe(&self) -> Probe {
        if !self.db.is_file() {
            return Probe::Unindexed;
        }
        match Store::open(&self.db, true) {
            Ok(store) => match store.meta_get(meta::INDEXED_AT) {
                Ok(Some(_)) => Probe::Ready(store),
                Ok(None) => Probe::Unindexed,
                Err(error) => Probe::Failed(error),
            },
            Err(StoreError::SchemaMismatch { found, .. }) if found == "none" => Probe::Unindexed,
            Err(StoreError::SchemaMismatch { .. }) => Probe::Outdated,
            Err(error) => Probe::Failed(error),
        }
    }

    fn start(&self) {
        let mut running = self.running();
        if running.is_some() {
            return;
        }
        match SessionLoop::start(&self.repo_root, Registry::default(), self.options.clone()) {
            Ok(session) => *running = Some(session),
            Err(error) => tracing::warn!("starting the session: {error}"),
        }
    }

    fn state(&self) -> Option<SessionState> {
        self.running().as_ref().map(SessionLoop::state)
    }

    fn running(&self) -> MutexGuard<'_, Option<SessionLoop>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
