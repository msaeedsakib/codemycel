#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use assert_cmd::cargo::cargo_bin_cmd;
use codemycel_core::UnresolvedReason;
use codemycel_core::config::TEMPLATE;
use codemycel_core::store::{Store, StoreRead};
use predicates::prelude::*;
use tempfile::TempDir;

/// A repository and a codemycel home, both temporary.
struct Sandbox {
    _temp: TempDir,
    repo: PathBuf,
    home: PathBuf,
}

impl Sandbox {
    fn new(files: &[(&str, &str)]) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path().join("repo");
        let home = temp.path().join("home");
        for (path, text) in files {
            write(&repo.join(path), text);
        }
        std::fs::create_dir_all(&repo).unwrap();
        Self {
            _temp: temp,
            repo,
            home,
        }
    }

    /// `codemycel <args>` in the repository, with servers hidden: an empty `PATH`, so no
    /// toolchain or server can be found, and no downloads.
    fn hidden(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        let empty = self.home.join("empty-path");
        std::fs::create_dir_all(&empty).unwrap();
        cargo_bin_cmd!("codemycel")
            .args(args)
            .arg("--no-download")
            .current_dir(&self.repo)
            .env("CODEMYCEL_HOME", &self.home)
            .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
            .env("PATH", &empty)
            .assert()
    }

    fn store(&self) -> Store {
        Store::open(&self.repo.join(".codemycel/index.db"), true).unwrap()
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn docs_only() -> Sandbox {
    Sandbox::new(&[("README.md", "# Readme\n")])
}

fn tiny_crate() -> Sandbox {
    Sandbox::new(&[
        (
            "Cargo.toml",
            "[package]\nname = \"tiny\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        ),
        ("src/lib.rs", "mod b;\n\npub fn a() -> u8 {\n    1\n}\n"),
        ("src/b.rs", "pub fn b() -> u8 {\n    crate::a()\n}\n"),
    ])
}

#[test]
fn init_creates_layout_and_template_config() {
    let sandbox = docs_only();

    sandbox.hidden(&["init"]).code(0);

    let dir = sandbox.repo.join(".codemycel");
    assert_eq!(
        std::fs::read_to_string(dir.join("config.toml")).unwrap(),
        TEMPLATE
    );
    assert!(dir.join("notes").is_dir());
    assert!(dir.join("index.db").is_file());
    let registry = std::fs::read_to_string(sandbox.home.join("registry.json")).unwrap();
    assert!(registry.contains("repo"), "{registry}");
}

#[test]
fn init_summary_lists_counts_and_coverage() {
    let available = Command::new("rustup")
        .args(["which", "rust-analyzer"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        assert!(
            std::env::var_os("CODEMYCEL_REQUIRE_SERVERS").is_none_or(|v| v != "1"),
            "rust-analyzer is required"
        );
        eprintln!("skipping: no rust-analyzer");
        return;
    }
    let sandbox = Sandbox::new(&[]);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/rust/workspace-basic");
    copy_dir(&fixture, &sandbox.repo);

    cargo_bin_cmd!("codemycel")
        .args(["init", "--no-download"])
        .current_dir(&sandbox.repo)
        .env("CODEMYCEL_HOME", &sandbox.home)
        .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
        .assert()
        .code(0)
        .stdout(
            predicate::str::starts_with("full build:")
                .and(predicate::str::contains("\nnodes "))
                .and(predicate::str::contains("\nedges "))
                .and(predicate::str::contains("calls "))
                .and(predicate::str::contains("(rust): verified by lsp"))
                .and(predicate::str::contains("\ndatabase ")),
        )
        .stderr(predicate::str::contains("lsp . (rust): rust-analyzer"));
}

#[test]
fn a_missing_pinned_toolchain_degrades_the_root() {
    let available = Command::new("rustup")
        .args(["which", "rust-analyzer"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !available {
        assert!(
            std::env::var_os("CODEMYCEL_REQUIRE_SERVERS").is_none_or(|v| v != "1"),
            "rust-analyzer is required"
        );
        eprintln!("skipping: no rust-analyzer");
        return;
    }
    let sandbox = tiny_crate();
    write(
        &sandbox.repo.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.60.0\"\n",
    );
    std::fs::create_dir_all(&sandbox.home).unwrap();
    let run = |args: &[&str]| {
        cargo_bin_cmd!("codemycel")
            .args(args)
            .arg("--repo")
            .arg(&sandbox.repo)
            .current_dir(&sandbox.home)
            .env("CODEMYCEL_HOME", &sandbox.home)
            .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env_remove("RUSTUP_TOOLCHAIN")
            .assert()
    };

    run(&["init", "--no-download"]).code(2).stdout(
        predicate::str::contains("root . (rust): degraded: toolchain_missing")
            .and(predicate::str::contains("verified by lsp").not()),
    );
    run(&["doctor"]).code(2).stdout(
        predicate::str::contains("rustup toolchain install")
            .and(predicate::str::contains("toolchain '1.60.0")),
    );

    let counts = sandbox.store().unresolved_counts_by_root(None).unwrap();
    assert!(
        counts
            .iter()
            .any(|(_, reason, _)| *reason == UnresolvedReason::ServerUnavailable),
        "{counts:?}"
    );
    assert!(
        counts
            .iter()
            .all(|(_, reason, _)| *reason != UnresolvedReason::NoDefinition),
        "{counts:?}"
    );
}

#[test]
fn init_exit_code_2_when_root_degraded() {
    let sandbox = tiny_crate();

    sandbox
        .hidden(&["init"])
        .code(2)
        .stdout(predicate::str::contains(
            "root . (rust): degraded: no_cargo",
        ));
}

#[test]
fn sync_after_body_edit_in_degraded_root_reverifies_the_whole_root() {
    let sandbox = tiny_crate();
    sandbox.hidden(&["init"]).code(2);

    write(
        &sandbox.repo.join("src/b.rs"),
        "pub fn b() -> u8 {\n    crate::a() + 1\n}\n",
    );

    sandbox
        .hidden(&["sync"])
        .code(2)
        .stdout(predicate::str::starts_with(
            "changed 1 files (0 added, 1 modified, 0 deleted), re-verified 2\n",
        ));
}

#[test]
fn init_on_indexed_repo_is_full_sync() {
    let sandbox = docs_only();
    sandbox.hidden(&["init"]).code(0);

    sandbox
        .hidden(&["init"])
        .code(0)
        .stdout(predicate::str::starts_with("full build:"));
    sandbox
        .hidden(&["sync"])
        .code(0)
        .stdout(predicate::str::starts_with("changed 0 files"));
}

#[test]
fn schema_mismatch_rebuilds() {
    let sandbox = docs_only();
    sandbox.hidden(&["init"]).code(0);
    let db = sandbox.repo.join(".codemycel/index.db");
    Store::open(&db, false)
        .unwrap()
        .transaction(|tx| tx.meta_set("schema_version", "0"))
        .unwrap();

    sandbox
        .hidden(&["sync"])
        .code(0)
        .stdout(predicate::str::starts_with("full build:"))
        .stderr(predicate::str::contains("index schema 0 replaced"));

    assert_eq!(sandbox.store().counts().unwrap().nodes, 2);
}

#[test]
fn sync_without_index_points_at_init() {
    let sandbox = docs_only();

    sandbox
        .hidden(&["sync"])
        .code(1)
        .stderr(predicate::str::contains("no index: run codemycel init in"));
    assert!(!sandbox.repo.join(".codemycel").exists());
}

#[test]
fn scip_commit_flag_overrides_head() {
    let sandbox = docs_only();
    let scip = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/rust/workspace-basic/index.scip");
    std::fs::copy(scip, sandbox.repo.join("index.scip")).unwrap();

    sandbox
        .hidden(&["init", "--scip-commit", "deadbeef"])
        .code(0);

    let config = std::fs::read_to_string(sandbox.repo.join(".codemycel/config.toml")).unwrap();
    assert!(config.contains("\ncommit = \"deadbeef\"\n"), "{config}");
    assert!(
        sandbox
            .store()
            .meta_get("scip_hash")
            .unwrap()
            .is_some_and(|hash| !hash.is_empty())
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let dest = to.join(entry.file_name());
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            copy_dir(&entry.path(), &dest);
        } else if kind.is_file() {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}
