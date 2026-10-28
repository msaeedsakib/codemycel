#![allow(clippy::unwrap_used)]

mod fake;

use std::path::Path;
use std::time::{Duration, Instant};

use codemycel_core::store::Store;
use codemycel_core::{DegradeReason, Language};
use codemycel_index::{Pipeline, Registry, ServerPool, Sources, SyncMode, SyncReport};
use codemycel_lsp::EnrichOptions;
use fake::{FakeAdapter, Repo};
use serde_json::{Value, json};

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

const LIB: &str = "\
mod b;

pub fn run() {
    b::helper();
}
";

const B: &str = "pub fn helper() {}\n";

fn rust_repo() -> Repo {
    Repo::new(&[
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", LIB),
        ("src/b.rs", B),
    ])
}

fn registry(scenario: &Path) -> Registry {
    Registry::default().with_servers(vec![(Language::Rust, Box::new(FakeAdapter::new(scenario)))])
}

fn sync(
    repo: &Repo,
    store: &mut Store,
    registry: &Registry,
    pool: &ServerPool<'_>,
    mode: SyncMode,
) -> SyncReport {
    let sources = Sources {
        servers: pool,
        scip: None,
        progress: &(),
    };
    Pipeline::sync(repo.path(), &repo.config(), store, registry, &sources, mode).unwrap()
}

fn degraded(report: &SyncReport) -> Option<DegradeReason> {
    let [(_, status)] = report.pipeline.statuses.as_slice() else {
        panic!("one root expected: {:?}", report.pipeline.statuses);
    };
    status.degraded
}

fn methods(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .filter_map(|message| message["method"].as_str())
        .collect()
}

#[test]
fn server_starts_on_first_use_and_stops_after_idle() {
    let repo = rust_repo();
    let registry = registry(&repo.scenario(json!([])));
    let db = tempfile::tempdir().unwrap();
    let mut store = Store::create(&db.path().join("index.db")).unwrap();
    let pool = ServerPool::new(
        &registry,
        repo.home(),
        false,
        EnrichOptions::default(),
        ServerPool::IDLE_TIMEOUT,
    );
    assert!(repo.launches().is_empty());

    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Full);
    assert_eq!(degraded(&report), None);
    assert_eq!(repo.launches().len(), 1);
    assert_eq!(pool.running().len(), 1);

    std::fs::write(
        repo.path().join("src/b.rs"),
        "pub fn helper() {}\npub fn more() {}\n",
    )
    .unwrap();
    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Incremental);
    assert_eq!(degraded(&report), None);
    let launches = repo.launches();
    assert_eq!(launches.len(), 1, "the warm server is reused");
    let received = &launches[0].1;
    let notified = received
        .iter()
        .rposition(|m| m["method"] == "workspace/didChangeWatchedFiles")
        .expect("changed files are announced to the warm server");
    let changes = &received[notified]["params"]["changes"];
    assert_eq!(changes[0]["uri"], repo.uri("src/b.rs"), "{changes}");
    let reopened = received
        .iter()
        .rposition(|m| {
            m["method"] == "textDocument/didOpen"
                && m["params"]["textDocument"]["uri"] == repo.uri("src/b.rs")
        })
        .unwrap();
    assert!(notified < reopened, "{:?}", methods(received));

    pool.reap_idle(Instant::now());
    assert_eq!(pool.running().len(), 1);
    assert!(!methods(&repo.launches()[0].1).contains(&"exit"));

    pool.reap_idle(Instant::now() + ServerPool::IDLE_TIMEOUT);
    assert!(pool.running().is_empty());
    assert!(methods(&repo.launches()[0].1).contains(&"exit"));

    std::fs::write(repo.path().join("src/b.rs"), B).unwrap();
    sync(&repo, &mut store, &registry, &pool, SyncMode::Incremental);
    assert_eq!(
        repo.launches().len(),
        2,
        "an idle-stopped server starts again"
    );
}

#[test]
fn server_without_a_job_in_a_sync_is_told_its_changes() {
    let repo = Repo::new(&[
        (
            "a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        ),
        ("a/src/lib.rs", B),
        (
            "b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        ),
        ("b/src/lib.rs", B),
    ]);
    let registry = registry(&repo.scenario(json!([])));
    let db = tempfile::tempdir().unwrap();
    let mut store = Store::create(&db.path().join("index.db")).unwrap();
    let pool = ServerPool::new(
        &registry,
        repo.home(),
        false,
        EnrichOptions::default(),
        ServerPool::IDLE_TIMEOUT,
    );
    sync(&repo, &mut store, &registry, &pool, SyncMode::Full);
    assert_eq!(pool.running().len(), 2);

    std::fs::write(
        repo.path().join("a/src/lib.rs"),
        "pub fn helper() {}\npub fn more() {}\n",
    )
    .unwrap();
    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Incremental);
    let [(root, _)] = report.pipeline.statuses.as_slice() else {
        panic!("only the edited root runs: {:?}", report.pipeline.statuses);
    };
    assert_eq!(root.path.as_str(), "a");

    let edited = repo.uri("a/src/lib.rs");
    let notified = || {
        repo.log()
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|m| m["method"] == "workspace/didChangeWatchedFiles")
            .filter(|m| {
                m["params"]["changes"]
                    .as_array()
                    .is_some_and(|changes| changes.iter().any(|c| c["uri"] == edited))
            })
            .count()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while notified() < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(notified(), 2, "both running servers hear of the edit once");
}

#[test]
fn crashed_server_restarts_once() {
    let repo = rust_repo();
    let scenario = repo.scratch.path().join("crashing.json");
    let scenario_json = json!({
        "after_initialized": [{"method": "experimental/serverStatus", "params": {"quiescent": true}}],
        "responses": [],
        "log_file": repo.log_path(),
        "state_file": repo.scratch.path().join("launches"),
        "crash_after": [0, 0, 0, null],
    });
    std::fs::write(&scenario, scenario_json.to_string()).unwrap();
    let registry = registry(&scenario);
    let db = tempfile::tempdir().unwrap();
    let mut store = Store::create(&db.path().join("index.db")).unwrap();
    let pool = ServerPool::new(
        &registry,
        repo.home(),
        false,
        EnrichOptions::default(),
        ServerPool::IDLE_TIMEOUT,
    );

    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Full);
    assert_eq!(degraded(&report), Some(DegradeReason::Crashed));
    assert_eq!(repo.launches().len(), 2, "one start and one restart");
    assert!(pool.running().is_empty());

    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Full);
    assert_eq!(degraded(&report), None);
    assert_eq!(
        repo.launches().len(),
        4,
        "a fresh server, which crashes and restarts"
    );
    assert_eq!(pool.running().len(), 1);

    std::fs::write(
        repo.path().join("src/b.rs"),
        "pub fn helper() {}\npub fn more() {}\n",
    )
    .unwrap();
    let report = sync(&repo, &mut store, &registry, &pool, SyncMode::Incremental);
    assert_eq!(degraded(&report), None);
    assert_eq!(repo.launches().len(), 4, "the restarted server stays warm");
}
