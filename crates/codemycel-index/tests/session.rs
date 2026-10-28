#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use codemycel_core::config::Config;
use codemycel_core::store::{Store, StoreRead};
use codemycel_index::session::{SessionOptions, SessionState};
use codemycel_index::{
    BatchServers, INDEX_FILE_NAME, Lock, LockKind, Pipeline, Registry, SessionLoop, Sources,
    SyncMode,
};
use codemycel_lsp::{EnrichOptions, Provisioner};
use tempfile::TempDir;

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

struct Indexed {
    repo: TempDir,
    home: TempDir,
}

impl Indexed {
    /// A Rust repository indexed in full, with no language server registered.
    fn new() -> Self {
        let repo = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::create_dir_all(repo.path().join(".codemycel")).unwrap();
        std::fs::write(repo.path().join("Cargo.toml"), MANIFEST).unwrap();
        std::fs::write(repo.path().join("src/lib.rs"), "pub fn alpha() {}\n").unwrap();
        let config = Config::load_with_home(repo.path(), home.path()).unwrap();
        let mut store = Store::create(&db(repo.path())).unwrap();
        let provisioner = Provisioner::new(&config, home.path().to_owned(), false);
        let servers = BatchServers {
            provisioner: &provisioner,
            options: EnrichOptions::default(),
        };
        let sources = Sources {
            servers: &servers,
            scip: None,
            progress: &(),
        };
        Pipeline::sync(
            repo.path(),
            &config,
            &mut store,
            &registry(),
            &sources,
            SyncMode::Full,
        )
        .unwrap();
        Self { repo, home }
    }

    fn path(&self) -> &Path {
        self.repo.path()
    }

    fn start(&self) -> SessionLoop {
        let options = SessionOptions {
            allow_download: false,
            debounce: Duration::from_millis(100),
            lock_retry: Duration::from_millis(200),
            sync_poll: Duration::from_millis(50),
            ..SessionOptions::new(self.home.path().to_owned())
        };
        SessionLoop::start(self.path(), registry(), options).unwrap()
    }

    fn add_function(&self, name: &str) {
        let lib = self.path().join("src/lib.rs");
        let mut text = std::fs::read_to_string(&lib).unwrap();
        text.push_str(&format!("pub fn {name}() {{}}\n"));
        std::fs::write(lib, text).unwrap();
    }

    fn indexed(&self, name: &str) -> bool {
        let store = Store::open(&db(self.path()), true).unwrap();
        !store.nodes_named(name).unwrap().is_empty()
    }
}

fn db(repo: &Path) -> PathBuf {
    repo.join(".codemycel").join(INDEX_FILE_NAME)
}

fn registry() -> Registry {
    Registry::default().with_servers(Vec::new())
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn second_opener_is_read_only_until_lock_is_released() {
    let repo = Indexed::new();
    let first = repo.start();
    wait_for("the first session's catch-up", || {
        first.completed_syncs() >= 1
    });
    assert!(!first.is_read_only());

    let second = repo.start();
    wait_for("the second session to find the lock taken", || {
        second.state() == SessionState::ReadOnly
    });
    assert!(second.is_read_only());

    repo.add_function("beta");
    wait_for("the first session to sync the edit", || {
        repo.indexed("beta")
    });
    assert_eq!(
        second.completed_syncs(),
        0,
        "a read-only session never syncs"
    );
    assert_eq!(second.state(), SessionState::ReadOnly);

    first.shutdown();
    wait_for("the second session to take over", || !second.is_read_only());
    wait_for("the second session's catch-up", || {
        second.completed_syncs() >= 1
    });
    repo.add_function("gamma");
    wait_for("the second session to sync the edit", || {
        repo.indexed("gamma")
    });
}

#[test]
fn command_sync_pauses_session_watcher_and_resumes() {
    let repo = Indexed::new();
    let session = repo.start();
    wait_for("the catch-up", || session.completed_syncs() >= 1);

    let command = Lock::try_acquire(repo.path(), LockKind::Sync)
        .unwrap()
        .unwrap();
    assert_eq!(
        session.sync_owner().map(|owner| owner.pid),
        Some(std::process::id())
    );
    repo.add_function("beta");
    wait_for("the session to wait for the command", || {
        session.state() == SessionState::WaitingForSync
    });
    assert_eq!(session.completed_syncs(), 1);
    assert!(!repo.indexed("beta"));

    drop(command);
    wait_for("the deferred sync", || repo.indexed("beta"));
    assert!(session.completed_syncs() >= 2);
    wait_for("the session to watch again", || {
        session.state() == SessionState::Watching
    });
    repo.add_function("gamma");
    wait_for("a sync after resuming", || repo.indexed("gamma"));
    assert_eq!(session.last_error(), None);
}
