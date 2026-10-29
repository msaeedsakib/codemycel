mod debounce;
mod rules;

use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher as _};

use crate::IndexError;
use crate::sync::ChangeSet;
use debounce::Pending;
use rules::Rules;

/// Watches the repository and coalesces file events into change sets.
pub struct Watcher;

impl Watcher {
    /// The quiet period after the last event before its change set is emitted.
    pub const DEBOUNCE: Duration = Duration::from_millis(500);

    /// The platform's file-event backend, as `notify` names it.
    pub fn backend() -> String {
        format!("{:?}", RecommendedWatcher::kind()).to_lowercase()
    }

    /// Watches `repo_root` recursively, skipping what the walk ignores (`ignore` holds the
    /// config's globs) and `.codemycel/`. `on_change` runs on the watcher's thread with each
    /// change set, once `debounce` passes without a kept event.
    pub fn start(
        repo_root: &Path,
        ignore: &[String],
        debounce: Duration,
        on_change: impl FnMut(ChangeSet) + Send + 'static,
    ) -> Result<WatcherHandle, IndexError> {
        let rules = Rules::new(repo_root, ignore)?;
        let (tx, rx) = mpsc::channel();
        let events = tx.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = events.send(Message::Event(event));
        })
        .map_err(|error| watch_error(repo_root, error))?;
        watcher
            .watch(repo_root, RecursiveMode::Recursive)
            .map_err(|error| watch_error(repo_root, error))?;
        let thread = std::thread::Builder::new()
            .name("codemycel-watch".to_owned())
            .spawn(move || run(&rx, rules, debounce, on_change))
            .map_err(|source| IndexError::Io {
                path: repo_root.to_owned(),
                source,
            })?;
        Ok(WatcherHandle {
            watcher: Some(watcher),
            tx,
            thread: Some(thread),
        })
    }
}

/// A running watcher; dropping it stops watching.
pub struct WatcherHandle {
    watcher: Option<RecommendedWatcher>,
    tx: Sender<Message>,
    thread: Option<JoinHandle<()>>,
}

impl WatcherHandle {
    /// Holds change sets back; events keep coalescing until `resume`.
    pub fn pause(&self) {
        let _ = self.tx.send(Message::Pause);
    }

    /// Emits what coalesced during the pause once the debounce passes.
    pub fn resume(&self) {
        let _ = self.tx.send(Message::Resume);
    }

    pub fn stop(self) {}
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        self.watcher.take();
        let _ = self.tx.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

enum Message {
    Event(notify::Result<notify::Event>),
    Pause,
    Resume,
    Stop,
}

fn run(
    rx: &Receiver<Message>,
    mut rules: Rules,
    debounce: Duration,
    mut on_change: impl FnMut(ChangeSet),
) {
    let mut pending = Pending::default();
    let mut due: Option<Instant> = None;
    let mut paused = false;
    loop {
        let message = match due.filter(|_| !paused) {
            Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match message {
            Ok(Message::Event(Ok(event))) => {
                if pending.record(&mut rules, &event) {
                    due = Some(Instant::now() + debounce);
                }
            }
            Ok(Message::Event(Err(error))) => tracing::warn!("watching the repository: {error}"),
            Ok(Message::Pause) => paused = true,
            Ok(Message::Resume) => {
                paused = false;
                if !pending.is_empty() {
                    due = Some(Instant::now() + debounce);
                }
            }
            Ok(Message::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                due = None;
                if let Some(changes) = pending.take(&rules) {
                    on_change(changes);
                }
            }
        }
    }
}

fn watch_error(repo_root: &Path, error: notify::Error) -> IndexError {
    IndexError::Io {
        path: repo_root.to_owned(),
        source: std::io::Error::other(error),
    }
}
