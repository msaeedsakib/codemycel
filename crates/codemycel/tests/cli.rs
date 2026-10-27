#![allow(clippy::unwrap_used)]

use assert_cmd::cargo::cargo_bin_cmd;
use codemycel_core::store::Store;
use codemycel_core::{FileKind, FileRecord, Language, Node, NodeKind, RepoPath, SemanticPath};
use predicates::prelude::*;

fn not_implemented(command: &str) -> String {
    format!("not implemented: {command}\n")
}

#[test]
fn cli_version_prints_semver() {
    cargo_bin_cmd!("codemycel")
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("codemycel {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn every_subcommand_exists_and_reports_not_implemented() {
    let invocations: &[&[&str]] = &[
        &["init"],
        &["sync"],
        &["mcp"],
        &["ui"],
        &["setup"],
        &["doctor"],
        &["upgrade"],
        &["search", "rate limiter"],
        &["symbol", "Store::open"],
        &["impact", "Store::open"],
        &["trace", "main", "Store::open"],
        &["overview"],
        &["unresolved"],
        &["note", "repo", "a durable finding"],
        &["notes"],
    ];
    for args in invocations {
        cargo_bin_cmd!("codemycel")
            .args(*args)
            .assert()
            .code(1)
            .stdout("")
            .stderr(not_implemented(args[0]));
    }
}

#[test]
fn global_flags_accepted_before_and_after_subcommand() {
    let repo = tempfile::tempdir().unwrap();
    let repo = repo.path().to_str().unwrap();
    for (command, args) in [
        ("search", vec!["--repo", repo, "--json", "search", "store"]),
        ("search", vec!["search", "store", "--repo", repo, "--json"]),
        (
            "impact",
            vec!["--json", "impact", "Store::open", "--repo", repo],
        ),
    ] {
        cargo_bin_cmd!("codemycel")
            .args(&args)
            .assert()
            .code(1)
            .stderr(not_implemented(command));
    }
}

#[test]
fn status_reports_no_index_in_empty_repo() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let nested = repo.path().join("src");
    std::fs::create_dir(&nested).unwrap();
    cargo_bin_cmd!("codemycel")
        .args(["status", "--repo"])
        .arg(&nested)
        .assert()
        .code(2)
        .stdout(format!(
            "no index: run codemycel init in {}\n",
            repo.path().display()
        ));
}

#[test]
fn status_reads_counts_from_created_store() {
    let repo = tempfile::tempdir().unwrap();
    let dir = repo.path().join(".codemycel");
    std::fs::create_dir(&dir).unwrap();
    let mut store = Store::create(&dir.join("index.db")).unwrap();
    let path = RepoPath::new("src/main.rs").unwrap();
    store
        .transaction(|tx| {
            tx.upsert_file(&FileRecord {
                path: path.clone(),
                root_id: None,
                hash: blake3::hash(b"fn main() {}"),
                size: 12,
                language: Some(Language::Rust),
                kind: FileKind::Code,
                parsed: true,
                skip_reason: None,
                scip_covered: false,
            })?;
            tx.insert_nodes(&[Node {
                path: SemanticPath::for_file(&path),
                kind: NodeKind::File,
                name: "main.rs".into(),
                file: Some(path.clone()),
                span: None,
                selection: None,
                signature: None,
                has_explicit_return_type: false,
                body_hash: None,
                doc_comment: None,
                visibility: None,
            }])
        })
        .unwrap();
    drop(store);

    cargo_bin_cmd!("codemycel")
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .code(0)
        .stdout("schema_version: 1\nnodes: 1\nedges: 0\nunresolved: 0\n");
}

#[test]
fn query_flags_parse_core_enums_and_reject_unknown_values() {
    cargo_bin_cmd!("codemycel")
        .args([
            "search",
            "store",
            "--kind",
            "struct,type_alias",
            "--limit",
            "5",
        ])
        .assert()
        .code(1)
        .stderr(not_implemented("search"));
    cargo_bin_cmd!("codemycel")
        .args(["impact", "x", "--kinds", "calls", "--depth", "5"])
        .assert()
        .code(1)
        .stderr(not_implemented("impact"));
    cargo_bin_cmd!("codemycel")
        .args(["unresolved", "--reason", "no_definition"])
        .assert()
        .code(1)
        .stderr(not_implemented("unresolved"));
    cargo_bin_cmd!("codemycel")
        .args(["search", "store", "--kind", "widget"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("widget"));
    cargo_bin_cmd!("codemycel")
        .args(["impact", "x", "--depth", "6"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--depth"));
}
