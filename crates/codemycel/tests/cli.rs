#![allow(clippy::unwrap_used)]

use assert_cmd::cargo::cargo_bin_cmd;
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
