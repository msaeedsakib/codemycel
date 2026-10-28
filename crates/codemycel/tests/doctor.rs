#![allow(clippy::unwrap_used)]

mod support;

use predicates::prelude::*;
use support::{codemycel, fixture_copy};

#[test]
fn doctor_exit_0_without_degraded_roots() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::write(repo.path().join("README.md"), "# Notes\n").unwrap();
    codemycel(home.path())
        .args(["init", "--no-download", "--repo"])
        .arg(repo.path())
        .assert()
        .code(0);
    codemycel(home.path())
        .args(["doctor", "--repo"])
        .arg(repo.path())
        .assert()
        .code(0)
        .stdout(predicate::str::contains("index schema 3"))
        .stdout(predicate::str::contains("orphaned notes 0"));
}

/// A Rust fixture indexed with only a broken `rust-analyzer` on `PATH` and no cargo.
#[cfg(unix)]
#[test]
fn doctor_reports_servers_rejections_scip_schema_and_degradations() {
    use std::os::unix::fs::PermissionsExt;

    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let fake = bin.path().join("rust-analyzer");
    std::fs::write(&fake, "#!/bin/sh\nexit 1\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (_temp, repo) = fixture_copy("rust/workspace-basic");
    codemycel(home.path())
        .env("PATH", bin.path())
        .args(["init", "--no-download", "--repo"])
        .arg(&repo)
        .assert()
        .code(2);
    codemycel(home.path())
        .env("PATH", bin.path())
        .args(["doctor", "--repo"])
        .arg(&repo)
        .assert()
        .code(2)
        .stdout(predicate::str::contains(format!(
            "codemycel {}",
            env!("CARGO_PKG_VERSION")
        )))
        .stdout(predicate::str::contains("index schema 3"))
        .stdout(predicate::str::contains("scip index.scip"))
        .stdout(predicate::str::contains(". (rust)"))
        .stdout(predicate::str::contains("server rust-analyzer not found"))
        .stdout(predicate::str::contains(format!(
            "rejected {} (path)",
            fake.display()
        )))
        .stdout(predicate::str::contains("cargo missing"))
        .stdout(predicate::str::contains("degraded: "))
        .stdout(predicate::str::contains("; fix: "))
        .stdout(predicate::str::contains("session lock free"));
}

#[test]
fn doctor_exit_2_when_index_is_out_of_date() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    std::fs::create_dir(repo.path().join(".codemycel")).unwrap();
    std::fs::write(repo.path().join(".codemycel/index.db"), "not a database").unwrap();
    codemycel(home.path())
        .args(["doctor", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2)
        .stdout(predicate::str::contains("is out of date"));
}
