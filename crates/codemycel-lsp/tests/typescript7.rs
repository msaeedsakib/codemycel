#![allow(clippy::unwrap_used)]

mod support;

use std::path::Path;

use codemycel_core::config::{Config, ServerConfig};
use codemycel_core::{LanguageAdapter, RepoPath, ServerAdapter, ServerSource};
use codemycel_lsp::adapters::typescript7::TypeScript7;
use codemycel_lsp::{
    EnrichOptions, Enricher, FileView, IndexView, Provisioner, Readiness, ServerLaunch,
};
use codemycel_parse::typescript::TypeScriptAdapter;
use serde_json::{Value, json};
use support::{FakeAdapter, fake_lsp};

#[cfg(unix)]
fn stub(path: &Path, version: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\necho 'Version {version}'\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
#[test]
fn typescript_5_on_path_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let home = temp.path().join("home");
    let node_tsc = bin.join("tsc");
    stub(&node_tsc, "5.9.2");
    let cached = home.join("servers/typescript/7.0.2/tsc");
    stub(&cached, "7.0.2");
    let provisioner = Provisioner::new(&Config::default(), home, false)
        .with_search_path(Some(bin.into_os_string()));

    let located = provisioner.locate(&TypeScript7::new()).unwrap();

    assert_eq!(located.source, ServerSource::Cache);
    assert_eq!(located.path, cached);
    assert_eq!(located.version, "7.0.2");
    let [rejected] = located.rejected.as_slice() else {
        panic!("expected one rejected candidate: {:?}", located.rejected);
    };
    assert_eq!(rejected.path, node_tsc);
    assert_eq!(rejected.source, ServerSource::Path);
    assert!(
        rejected.reason.contains("5.9.2") && rejected.reason.contains("TypeScript 7"),
        "{}",
        rejected.reason
    );
}

#[test]
fn launch_args_are_lsp_stdio() {
    assert_eq!(TypeScript7::new().launch_args(), ["--lsp", "--stdio"]);
}

const MAIN: &str = "\
import { helper } from \"./util\";

export function run(): number {
    return helper();
}
";

#[tokio::test(flavor = "multi_thread")]
async fn readiness_probe_retries_until_definition_answers() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/main.ts"), MAIN).unwrap();
    std::fs::write(
        dir.path().join("src/util.ts"),
        "export function helper(): number {\n    return 1;\n}\n",
    )
    .unwrap();
    let language = TypeScriptAdapter::new();
    let path = RepoPath::new("src/main.ts").unwrap();
    let parsed = language.parse(&path, MAIN.as_bytes()).unwrap();
    let mut view = IndexView::default();
    view.insert(
        path.clone(),
        FileView::new(&path, MAIN.to_owned(), parsed).unwrap(),
    );
    let log = dir.path().join("messages.log");
    let target = json!({"uri": "$ROOT/src/util.ts",
        "range": {"start": {"line": 0, "character": 16}, "end": {"line": 0, "character": 22}}});
    let scenario = dir.path().join("scenario.json");
    std::fs::write(
        &scenario,
        json!({
            "log_file": log,
            "responses": [{"method": "textDocument/definition", "fail_times": 3, "result": [target]}],
        })
        .to_string(),
    )
    .unwrap();
    let adapter = FakeAdapter::new(&scenario).readiness(TypeScript7::new().readiness());
    let launch = ServerLaunch {
        adapter: &adapter,
        binary: fake_lsp(),
        config: ServerConfig::default(),
        search_path: None,
    };

    let enricher = Enricher::start(
        launch,
        &language,
        &view,
        dir.path(),
        &RepoPath::new("").unwrap(),
        std::slice::from_ref(&path),
        EnrichOptions::default(),
    )
    .await
    .unwrap();
    let readiness = enricher.readiness();
    enricher.shutdown().await;

    assert_eq!(readiness, Readiness::Ready);
    let probes = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|message| message["method"] == "textDocument/definition")
        .count();
    assert_eq!(probes, 4);
}

#[test]
#[ignore = "downloads TypeScript 7 from GitHub"]
fn typescript7_downloads_and_validates() {
    let home = tempfile::tempdir().unwrap();
    let provisioner =
        Provisioner::new(&Config::default(), home.path().to_owned(), true).with_search_path(None);

    let located = provisioner.locate(&TypeScript7::new()).unwrap();

    assert_eq!(located.source, ServerSource::Download);
    assert_eq!(located.version, "7.0.2");
    assert!(located.path.starts_with(home.path()));
}
