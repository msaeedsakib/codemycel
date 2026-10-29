#![allow(clippy::unwrap_used)]

use std::net::{Ipv4Addr, TcpListener};
use std::path::{Path, PathBuf};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use codemycel_core::config::Config;
use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::query::{Answer, QueryEngine};
use codemycel_core::query::{Budget, JsonFormatter};
use codemycel_core::store::{Store, meta};
use codemycel_index::{Lock, LockKind, Pipeline, Registry};
use codemycel_ui::UiServer;
use serde_json::Value;
use tempfile::TempDir;
use tower::ServiceExt;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

/// A copy of the Rust fixture with a syntactic index stamped `indexed_at`.
fn indexed(indexed_at: &str) -> (TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rust/workspace-basic"),
        &repo,
    );
    let dir = repo.join(".codemycel");
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = Store::create(&dir.join("index.db")).unwrap();
    let config = Config::load_with_home(&repo, temp.path()).unwrap();
    Pipeline::parse_only(&repo, &config, &mut store, &Registry::default()).unwrap();
    stamp(&repo, indexed_at);
    (temp, repo)
}

fn stamp(repo: &Path, indexed_at: &str) {
    let mut store = Store::open(&repo.join(".codemycel/index.db"), false).unwrap();
    store
        .transaction(|tx| tx.meta_set(meta::INDEXED_AT, indexed_at))
        .unwrap();
}

async fn get(repo: &Path, uri: &str) -> (StatusCode, Value) {
    let response = UiServer::router(repo)
        .oneshot(Request::get(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, body)
}

fn direct(repo: &Path, run: impl FnOnce(&QueryEngine<'_>) -> Answer) -> Value {
    let store = Store::open(&repo.join(".codemycel/index.db"), true).unwrap();
    let registry = Registry::default();
    let engine = QueryEngine::open(repo, store, registry.adapters());
    JsonFormatter::render(&run(&engine))
}

#[test]
fn ui_binds_loopback_only_and_picks_free_port() {
    let taken = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = taken.local_addr().unwrap().port();
    let repo = tempfile::tempdir().unwrap();

    let server = UiServer::bind_from(repo.path(), port).unwrap();
    let addr = server.addr().unwrap();
    assert_eq!(addr.ip(), Ipv4Addr::LOCALHOST);
    assert!(
        addr.port() > port,
        "{addr} should skip the taken port {port}"
    );

    assert!(UiServer::bind(repo.path(), Some(port)).is_err());
}

#[tokio::test]
async fn api_routes_return_json_formatter_output() {
    let (_temp, repo) = indexed("2026-10-29T10:00:00Z");
    let symbol = "crates/core/src/geometry.rs::Area";
    let encoded = "crates/core/src/geometry.rs%3A%3AArea";

    let (status, search) = get(&repo, "/api/search?q=area").await;
    assert_eq!(status, StatusCode::OK);
    let expected = direct(&repo, |e| {
        e.search("area", &[], 20, Budget::new(6000)).unwrap()
    });
    assert_eq!(search, expected);

    let (_, card) = get(&repo, &format!("/api/symbol/{encoded}")).await;
    let options = SymbolOptions {
        include_body: true,
        callers: true,
        callees: true,
        limit: 50,
    };
    let expected = direct(&repo, |e| {
        e.symbol(symbol, options, Budget::new(12000)).unwrap()
    });
    assert_eq!(card, expected);
    assert_eq!(card["outcome"], "ok", "{card}");

    let (_, impact) = get(&repo, &format!("/api/impact/{encoded}?depth=3")).await;
    let expected = direct(&repo, |e| {
        e.impact(symbol, 3, &[], Budget::new(12000)).unwrap()
    });
    assert_eq!(impact, expected);

    let (_, overview) = get(&repo, "/api/overview?scope=crates/core").await;
    let expected = direct(&repo, |e| {
        e.overview(Some("crates/core"), Budget::new(6000)).unwrap()
    });
    assert_eq!(overview, expected);

    let (_, notes) = get(&repo, "/api/notes?max_tokens=100").await;
    let expected = direct(&repo, |e| e.notes(None, false, Budget::new(100)).unwrap());
    assert_eq!(notes, expected);

    let (_, status_answer) = get(&repo, "/api/status").await;
    let expected = direct(&repo, |e| {
        e.status(
            Lock::current_owner(&repo, LockKind::Sync),
            Lock::current_owner(&repo, LockKind::Session),
            Budget::new(2000),
        )
        .unwrap()
    });
    assert_eq!(status_answer, expected);

    let (status, graph) = get(&repo, "/api/communities").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(graph["derived"], true);
    assert!(graph["communities"].is_array() && graph["links"].is_array());

    let (status, missing) = get(&repo, "/api/nothing").await;
    assert_eq!((status, missing), (StatusCode::NOT_FOUND, Value::Null));
}

#[tokio::test]
async fn api_meta_changes_after_sync() {
    let (_temp, repo) = indexed("2026-10-29T10:00:00Z");
    let (_, before) = get(&repo, "/api/meta").await;
    assert_eq!(before["indexed_at"], "2026-10-29T10:00:00Z");

    stamp(&repo, "2026-10-29T10:05:00Z");
    let (_, after) = get(&repo, "/api/meta").await;
    assert_eq!(after["indexed_at"], "2026-10-29T10:05:00Z");
    assert_eq!(after["repo"], repo.display().to_string());
}

#[tokio::test]
async fn api_without_an_index_says_no_index() {
    let repo = tempfile::tempdir().unwrap();
    let (status, body) = get(repo.path(), "/api/status").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["outcome"], "no_index");
}
