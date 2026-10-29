use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use codemycel_core::SyncOwner;
use codemycel_core::config::paths::REPO_DIR_NAME;
use codemycel_core::config::{Config, ConfigError};
use codemycel_lsp::EnrichOptions;
use thiserror::Error;

use crate::{
    INDEX_FILE_NAME, IndexError, Lock, LockGuard, LockKind, Pipeline, Registry, Scip, ServerPool,
    Sources, SyncMode, SyncReport, Watcher, WatcherHandle, open_or_rebuild,
};

#[derive(Debug, Error)]
pub enum SessionError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Index(#[from] IndexError),
}

/// Timings and server settings of a session.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// The codemycel home: the home config and the server cache.
    pub home: PathBuf,
    /// Servers may be downloaded, unless the config says `no_download`.
    pub allow_download: bool,
    pub debounce: Duration,
    /// How often a read-only session tries to take `session.lock` again.
    pub lock_retry: Duration,
    /// How often a session waiting on another process's `sync.lock` checks it again.
    pub sync_poll: Duration,
    pub idle_timeout: Duration,
    pub enrich: EnrichOptions,
}

impl SessionOptions {
    pub fn new(home: PathBuf) -> Self {
        Self {
            home,
            allow_download: true,
            debounce: Watcher::DEBOUNCE,
            lock_retry: Duration::from_secs(30),
            sync_poll: Duration::from_millis(500),
            idle_timeout: ServerPool::IDLE_TIMEOUT,
            enrich: EnrichOptions::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionState {
    #[default]
    Starting,
    /// Another session holds `session.lock`; this one neither watches nor writes.
    ReadOnly,
    Watching,
    /// Holding a change set while another process holds `sync.lock`; the watcher is paused.
    WaitingForSync,
    /// Shut down, or stopped by an error in `last_error`.
    Stopped,
}

#[derive(Default)]
struct Status {
    state: SessionState,
    syncs: u64,
    last_report: Option<SyncReport>,
    last_error: Option<String>,
}

type Shared = Arc<Mutex<Status>>;

enum Message {
    Changes,
    Stop,
}

/// The session process's sync loop, on its own thread. It holds `session.lock` for its
/// lifetime, retrying while another session holds it, then watches the repository, runs a
/// catch-up incremental sync (full when the index is of another schema version and is
/// rebuilt), and syncs again after every change set, each sync under
/// `sync.lock`. Language servers stay warm between syncs. Dropping it shuts it down.
pub struct SessionLoop {
    repo_root: PathBuf,
    status: Shared,
    tx: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl SessionLoop {
    /// `repo_root` must hold an index.
    pub fn start(
        repo_root: &Path,
        registry: Registry,
        options: SessionOptions,
    ) -> Result<Self, IndexError> {
        let status = Shared::default();
        let (tx, rx) = mpsc::channel();
        let thread = {
            let root = repo_root.to_owned();
            let status = status.clone();
            let tx = tx.clone();
            std::thread::Builder::new()
                .name("codemycel-session".to_owned())
                .spawn(move || run(&root, &registry, &options, &status, tx, &rx))
                .map_err(|source| IndexError::Io {
                    path: repo_root.to_owned(),
                    source,
                })?
        };
        Ok(Self {
            repo_root: repo_root.to_owned(),
            status,
            tx,
            thread: Some(thread),
        })
    }

    pub fn state(&self) -> SessionState {
        lock(&self.status).state
    }

    /// True unless this session holds `session.lock` and watches.
    pub fn is_read_only(&self) -> bool {
        !matches!(
            self.state(),
            SessionState::Watching | SessionState::WaitingForSync
        )
    }

    /// The process holding `sync.lock`, this one included while it syncs.
    pub fn sync_owner(&self) -> Option<SyncOwner> {
        Lock::current_owner(&self.repo_root, LockKind::Sync)
    }

    /// Syncs this session completed, the catch-up included.
    pub fn completed_syncs(&self) -> u64 {
        lock(&self.status).syncs
    }

    pub fn last_report(&self) -> Option<SyncReport> {
        lock(&self.status).last_report.clone()
    }

    /// Why the last sync failed, or why the session stopped; cleared by a successful sync.
    pub fn last_error(&self) -> Option<String> {
        lock(&self.status).last_error.clone()
    }

    /// Stops the watcher and the servers, releases both locks, and joins the thread.
    pub fn shutdown(self) {}
}

impl Drop for SessionLoop {
    fn drop(&mut self) {
        let _ = self.tx.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn lock(status: &Shared) -> MutexGuard<'_, Status> {
    status.lock().unwrap_or_else(PoisonError::into_inner)
}

fn set_state(status: &Shared, state: SessionState) {
    lock(status).state = state;
}

fn run(
    repo_root: &Path,
    registry: &Registry,
    options: &SessionOptions,
    status: &Shared,
    tx: Sender<Message>,
    rx: &Receiver<Message>,
) {
    if let Some(_session) = acquire_session(repo_root, options, status, rx)
        && let Err(error) = watch(repo_root, registry, options, status, tx, rx)
    {
        lock(status).last_error = Some(error.to_string());
    }
    set_state(status, SessionState::Stopped);
}

/// `session.lock`, retried every `lock_retry` while another session holds it; `None` when asked
/// to stop first.
fn acquire_session(
    repo_root: &Path,
    options: &SessionOptions,
    status: &Shared,
    rx: &Receiver<Message>,
) -> Option<LockGuard> {
    loop {
        match Lock::try_acquire(repo_root, LockKind::Session) {
            Ok(Some(guard)) => return Some(guard),
            Ok(None) => set_state(status, SessionState::ReadOnly),
            Err(error) => {
                let mut status = lock(status);
                status.state = SessionState::ReadOnly;
                status.last_error = Some(error.to_string());
            }
        }
        if let Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) =
            rx.recv_timeout(options.lock_retry)
        {
            return None;
        }
    }
}

/// Watches and syncs until asked to stop, then stops the watcher and the servers.
fn watch(
    repo_root: &Path,
    registry: &Registry,
    options: &SessionOptions,
    status: &Shared,
    tx: Sender<Message>,
    rx: &Receiver<Message>,
) -> Result<(), SessionError> {
    let config = Config::load_with_home(repo_root, &options.home)?;
    let watcher = Watcher::start(repo_root, &config.ignore, options.debounce, move |_| {
        let _ = tx.send(Message::Changes);
    })?;
    let pool = ServerPool::new(
        registry,
        options.home.clone(),
        options.allow_download,
        options.enrich,
        options.idle_timeout,
    );
    set_state(status, SessionState::Watching);
    let mut due = true;
    loop {
        if due {
            if !sync_when_free(repo_root, registry, &pool, options, status, &watcher, rx) {
                break;
            }
            due = false;
        }
        let message = match pool.next_idle_deadline() {
            Some(deadline) => rx.recv_timeout(deadline.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(Message::Changes) => due = true,
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => pool.reap_idle(Instant::now()),
        }
    }
    drop(watcher);
    pool.shutdown_all();
    Ok(())
}

/// One sync under `sync.lock`. While another process holds it the watcher is paused and the
/// lock polled; change sets already queued are covered by this sync. `false` when asked to stop.
fn sync_when_free(
    repo_root: &Path,
    registry: &Registry,
    pool: &ServerPool<'_>,
    options: &SessionOptions,
    status: &Shared,
    watcher: &WatcherHandle,
    rx: &Receiver<Message>,
) -> bool {
    let mut paused = false;
    let guard = loop {
        match Lock::try_acquire(repo_root, LockKind::Sync) {
            Ok(Some(guard)) => break Some(guard),
            Ok(None) => {}
            Err(error) => {
                lock(status).last_error = Some(error.to_string());
                break None;
            }
        }
        if !paused {
            watcher.pause();
            paused = true;
            set_state(status, SessionState::WaitingForSync);
        }
        if let Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) =
            rx.recv_timeout(options.sync_poll)
        {
            return false;
        }
    };
    if paused {
        watcher.resume();
        set_state(status, SessionState::Watching);
    }
    let Some(_guard) = guard else {
        return true;
    };
    loop {
        match rx.try_recv() {
            Ok(Message::Changes) => {}
            Err(TryRecvError::Empty) => break,
            Ok(Message::Stop) | Err(TryRecvError::Disconnected) => return false,
        }
    }
    let result = sync_once(repo_root, registry, pool, &options.home);
    let mut status = lock(status);
    match result {
        Ok(report) => {
            status.syncs += 1;
            status.last_report = Some(report);
            status.last_error = None;
        }
        Err(error) => status.last_error = Some(error.to_string()),
    }
    true
}

fn sync_once(
    repo_root: &Path,
    registry: &Registry,
    pool: &ServerPool<'_>,
    home: &Path,
) -> Result<SyncReport, SessionError> {
    let config = Config::load_with_home(repo_root, home)?;
    let db = repo_root.join(REPO_DIR_NAME).join(INDEX_FILE_NAME);
    let (mut store, opened) = open_or_rebuild(&db)?;
    let mode = if opened.needs_full_sync() {
        SyncMode::Full
    } else {
        SyncMode::Incremental
    };
    let scip = Pipeline::load_scip(repo_root, &config, &store, None)?;
    let sources = Sources {
        servers: pool,
        scip: scip.as_ref().map(|index| Scip {
            source: index,
            hash: index.hash(),
        }),
        progress: &(),
    };
    Ok(Pipeline::sync(
        repo_root, &config, &mut store, registry, &sources, mode,
    )?)
}
