#![allow(clippy::unwrap_used)]

mod support;

use std::path::Path;
use std::time::{Duration, Instant};

use codemycel_core::Occurrence;
use codemycel_lsp::LspClient;
use codemycel_lsp::client::{ClientError, LineIndex, path_to_uri};
use lsp_types::Position;
use serde_json::Value;
use support::{alive, fake_lsp, scenario};

fn spawn(name: &str, concurrency: usize) -> LspClient {
    LspClient::spawn(
        &fake_lsp(),
        &[scenario(name).to_str().unwrap()],
        Path::new(env!("CARGO_MANIFEST_DIR")),
        &[],
        concurrency,
    )
    .unwrap()
}

async fn initialized(name: &str, root: &Path, concurrency: usize) -> LspClient {
    let client = spawn(name, concurrency);
    client
        .initialize(root, Value::Null, false, Duration::from_secs(10))
        .await
        .unwrap();
    client.initialized().unwrap();
    client
}

#[tokio::test(flavor = "multi_thread")]
async fn initialize_handshake_completes() {
    let root = tempfile::tempdir().unwrap();
    let client = spawn("empty", 8);
    let result = client
        .initialize(root.path(), Value::Null, false, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(result.capabilities, Default::default());
    client.initialized().unwrap();
    assert!(client.is_alive());
    client.shutdown_and_exit().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn request_times_out_and_client_stays_usable() {
    let root = tempfile::tempdir().unwrap();
    let client = initialized("timeout", root.path(), 8).await;
    let uri = path_to_uri(&root.path().join("src/a.rs")).unwrap();
    let first = client
        .definition(&uri, Position::new(0, 0), Duration::from_millis(300))
        .await;
    assert!(matches!(first, Err(ClientError::Timeout(_))), "{first:?}");
    let second = client
        .definition(&uri, Position::new(0, 0), Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(second.len(), 1);
    assert!(second[0].uri.as_str().ends_with("/src/b.rs"));
    assert!(client.is_alive());
    client.shutdown_and_exit().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn utf16_column_conversion_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let client = initialized("utf16", root.path(), 8).await;
    let source = LineIndex::new("let a = \"😀\"; call();\n");
    let target = LineIndex::new("/* 😀😀 */ call\n");
    let candidate = Occurrence { line: 1, col: 13 };
    let position = source.to_lsp(candidate).unwrap();
    assert_eq!(position, Position::new(0, 14));
    let uri = path_to_uri(&root.path().join("src/emoji.rs")).unwrap();
    let locations = client
        .definition(&uri, position, Duration::from_secs(10))
        .await
        .unwrap();
    assert_eq!(locations.len(), 1, "the server saw the wrong column");
    let landed = target.from_lsp(locations[0].range.start).unwrap();
    assert_eq!(landed, Occurrence { line: 1, col: 9 });
    assert_eq!(target.offset(locations[0].range.start), Some(15));
    assert_eq!(source.from_lsp(position), Some(candidate));
    client.shutdown_and_exit().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn kill_on_drop_leaves_no_child_process() {
    let root = tempfile::tempdir().unwrap();
    let client = initialized("child", root.path(), 8).await;
    let mut notifications = client.notifications().unwrap();
    let child = tokio::time::timeout(Duration::from_secs(10), notifications.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(child.method, "fake/child");
    let child_pid = u32::try_from(child.params["pid"].as_u64().unwrap()).unwrap();
    let server_pid = client.pid().unwrap();
    assert!(alive(server_pid) && alive(child_pid));
    drop(client);
    let deadline = Instant::now() + Duration::from_secs(10);
    while (alive(server_pid) || alive(child_pid)) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(!alive(server_pid), "server {server_pid} survived");
    assert!(!alive(child_pid), "grandchild {child_pid} survived");
}

#[tokio::test(flavor = "multi_thread")]
async fn concurrency_limit_is_respected() {
    let root = tempfile::tempdir().unwrap();
    let client = initialized("slow", root.path(), 2).await;
    let uri = path_to_uri(&root.path().join("src/a.rs")).unwrap();
    let started = Instant::now();
    let requests =
        (0..6).map(|line| client.definition(&uri, Position::new(line, 0), Duration::from_secs(10)));
    for result in futures::future::join_all(requests).await {
        result.unwrap();
    }
    let elapsed = started.elapsed();
    assert!(
        elapsed >= Duration::from_millis(850),
        "six 300 ms requests two at a time finished in {elapsed:?}"
    );
    client.shutdown_and_exit().await;
}
