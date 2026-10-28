#![allow(clippy::unwrap_used)]

mod support;

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use predicates::prelude::*;
use support::{codemycel, fixture_copy};
use tempfile::TempDir;

/// The Rust fixture, indexed once by `codemycel init --no-download`.
fn indexed() -> &'static (TempDir, PathBuf, TempDir) {
    static REPO: OnceLock<(TempDir, PathBuf, TempDir)> = OnceLock::new();
    REPO.get_or_init(|| {
        let (temp, repo) = fixture_copy("rust/workspace-basic");
        let home = tempfile::tempdir().unwrap();
        let output = codemycel(home.path())
            .args(["init", "--no-download", "--repo"])
            .arg(&repo)
            .output()
            .unwrap();
        assert!(
            matches!(output.status.code(), Some(0 | 2)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (temp, repo, home)
    })
}

fn run(repo: &Path, home: &Path, args: &[&str], json: bool) -> (Option<i32>, String) {
    let mut command = codemycel(home);
    command.args(args).arg("--repo").arg(repo);
    if json {
        command.arg("--json");
    }
    let output = command.output().unwrap();
    (
        output.status.code(),
        String::from_utf8(output.stdout).unwrap(),
    )
}

/// Text ends with the freshness footer; JSON is one object with the same footer data and the
/// expected section.
fn assert_renders(args: &[&str], section: &str) {
    let (_, repo, home) = indexed();
    let (code, text) = run(repo, home.path(), args, false);
    assert!(matches!(code, Some(0 | 2)), "{args:?}: {code:?}\n{text}");
    assert!(
        text.lines().last().unwrap().starts_with("index: "),
        "{args:?}:\n{text}"
    );
    let (code, json) = run(repo, home.path(), args, true);
    assert!(matches!(code, Some(0 | 2)), "{args:?}: {code:?}\n{json}");
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        json[section].is_array(),
        "{args:?}: no `{section}` in {json}"
    );
    assert!(json["freshness"].is_object(), "{args:?}: {json}");
}

#[test]
fn each_tool_subcommand_renders_text_and_json() {
    assert_renders(&["search", "circle"], "results");
    assert_renders(
        &["symbol", "crates/core/src/geometry.rs::Area::area"],
        "symbol",
    );
}

#[test]
#[ignore = "needs Task 4 (PR B)"]
fn each_remaining_tool_subcommand_renders_text_and_json() {
    assert_renders(
        &["impact", "crates/core/src/geometry.rs::Area::area"],
        "depth 1",
    );
    assert_renders(
        &[
            "trace",
            "crates/cli/src/main.rs::main",
            "crates/core/src/geometry.rs::Area::area",
        ],
        "path",
    );
    assert_renders(&["overview"], "packages");
    assert_renders(&["unresolved"], "reasons");
    assert_renders(&["status"], "index");
    assert_renders(&["notes"], "notes");
}

#[test]
fn ambiguous_name_exits_2_with_candidates() {
    let (_, repo, home) = indexed();
    codemycel(home.path())
        .args(["symbol", "new", "--repo"])
        .arg(repo)
        .assert()
        .code(2)
        .stdout(predicate::str::starts_with("candidates:\n"))
        .stdout(predicate::str::contains(
            "crates/core/src/circle.rs::Circle::new (method)",
        ))
        .stdout(predicate::str::contains(
            "crates/core/src/lib.rs::Wrapper<T>::new (method)",
        ));
}

#[test]
fn unknown_name_exits_1() {
    let (_, repo, home) = indexed();
    codemycel(home.path())
        .args(["symbol", "no_such_symbol", "--repo"])
        .arg(repo)
        .assert()
        .code(1)
        .stdout(predicate::str::starts_with(
            "no node named `no_such_symbol`\n",
        ));
}

#[test]
fn queries_without_an_index_say_so() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    codemycel(home.path())
        .args(["search", "circle", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2)
        .stdout(format!(
            "no index: run codemycel init in {}\n",
            repo.path().display()
        ));
}
