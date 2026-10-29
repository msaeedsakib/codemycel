#![allow(clippy::unwrap_used)]

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn cli() -> Command {
    let mut command = cargo_bin_cmd!("codemycel");
    command.env("CODEMYCEL_NO_UPDATE_CHECK", "1");
    command
}

fn no_index(repo: &std::path::Path) -> String {
    format!("no index: run codemycel init in {}\n", repo.display())
}

#[test]
fn cli_version_prints_semver() {
    cli()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("codemycel {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn ui_reports_not_implemented() {
    cli()
        .arg("ui")
        .assert()
        .code(1)
        .stdout("")
        .stderr("not implemented: ui\n");
}

#[test]
fn note_body_is_a_flag_not_a_positional() {
    cli()
        .args(["note", "repo", "a durable finding"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--body"));
}

#[test]
fn global_flags_accepted_before_and_after_subcommand() {
    let repo = tempfile::tempdir().unwrap();
    let path = repo.path().to_str().unwrap();
    for args in [
        vec!["--repo", path, "--json", "search", "store"],
        vec!["search", "store", "--repo", path, "--json"],
        vec!["--json", "impact", "Store::open", "--repo", path],
    ] {
        let output = cli()
            .args(&args)
            .assert()
            .code(2)
            .get_output()
            .stdout
            .clone();
        let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(json["outcome"], "no_index", "{args:?}: {json}");
        assert_eq!(
            json["message"],
            no_index(repo.path()).trim_end(),
            "{args:?}: {json}"
        );
    }
}

#[test]
fn status_reports_no_index_in_empty_repo() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".git")).unwrap();
    let nested = repo.path().join("src");
    std::fs::create_dir(&nested).unwrap();
    cli()
        .args(["status", "--repo"])
        .arg(&nested)
        .assert()
        .code(2)
        .stdout(no_index(repo.path()));
}

#[test]
fn status_resolves_relative_repo_from_its_own_directory() {
    let root = tempfile::tempdir().unwrap();
    let here = root.path().join("here");
    let there = root.path().join("there");
    std::fs::create_dir_all(here.join(".git")).unwrap();
    std::fs::create_dir_all(there.join(".git")).unwrap();
    cli()
        .current_dir(&here)
        .args(["status", "--repo", "../there"])
        .assert()
        .code(2)
        .stdout(
            predicate::str::starts_with("no index: run codemycel init in ").and(
                predicate::str::ends_with(format!("{}there\n", std::path::MAIN_SEPARATOR)),
            ),
        );
}

#[test]
fn status_prefers_an_indexed_ancestor_over_a_nearer_git() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".codemycel")).unwrap();
    std::fs::write(root.path().join(".codemycel/config.toml"), "").unwrap();
    let submodule = root.path().join("vendor/lib");
    std::fs::create_dir_all(submodule.join(".git")).unwrap();
    cli()
        .args(["status", "--repo"])
        .arg(&submodule)
        .assert()
        .code(2)
        .stdout(no_index(root.path()));
}

#[test]
fn queries_on_an_outdated_index_ask_for_init() {
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".codemycel")).unwrap();
    std::fs::write(repo.path().join(".codemycel/index.db"), "not a database").unwrap();
    cli()
        .args(["search", "store", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2)
        .stdout(
            predicate::str::contains("is out of date")
                .and(predicate::str::contains("run codemycel init")),
        );
}

#[test]
fn query_flags_parse_core_enums_and_reject_unknown_values() {
    let repo = tempfile::tempdir().unwrap();
    for args in [
        &[
            "search",
            "store",
            "--kind",
            "struct,type_alias",
            "--limit",
            "5",
        ][..],
        &["impact", "x", "--kinds", "calls", "--depth", "5"],
        &["unresolved", "--reason", "no_definition"],
    ] {
        cli()
            .args(args)
            .arg("--repo")
            .arg(repo.path())
            .assert()
            .code(2)
            .stdout(no_index(repo.path()));
    }
    cli()
        .args(["search", "store", "--kind", "widget"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("widget"));
    cli()
        .args(["impact", "x", "--depth", "6"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("--depth"));
}
