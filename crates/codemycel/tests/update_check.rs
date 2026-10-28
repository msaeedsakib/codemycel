#![allow(clippy::unwrap_used)]

mod support;

use std::io::Write;
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use sha2::{Digest, Sha256};
use support::{Stub, release_json};

fn latest(version: &str) -> Stub {
    let version = version.to_owned();
    Stub::start(move |base| vec![("/latest".to_owned(), release_json(base, &version, &[]))])
}

/// `codemycel <args>` with the version check on, against `stub`.
fn checked(home: &std::path::Path, stub: &Stub) -> assert_cmd::Command {
    let mut command = cargo_bin_cmd!("codemycel");
    command
        .env("CODEMYCEL_HOME", home)
        .env("CODEMYCEL_RELEASES_URL", format!("{}/latest", stub.base))
        .env_remove("CODEMYCEL_NO_UPDATE_CHECK")
        .env_remove("CODEMYCEL_TEST_DENY_NETWORK")
        .timeout(Duration::from_secs(60));
    command
}

#[test]
fn update_check_runs_at_most_daily_and_respects_env() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let stub = latest("99.0.0");
    checked(home.path(), &stub)
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .stderr(predicate::str::contains("codemycel 99.0.0 is available"));
    assert_eq!(stub.hits(), 1);
    checked(home.path(), &stub)
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .stderr(predicate::str::contains("is available").not());
    assert_eq!(stub.hits(), 1, "a second run within a day does not check");

    let state = home.path().join("update-check.json");
    std::fs::write(&state, r#"{"checked_at":"2000-01-01T00:00:00Z"}"#).unwrap();
    checked(home.path(), &stub)
        .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2);
    checked(home.path(), &stub)
        .env("CODEMYCEL_TEST_DENY_NETWORK", "1")
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2);
    assert_eq!(stub.hits(), 1, "disabled checks make no request");
    checked(home.path(), &stub)
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .code(2);
    assert_eq!(stub.hits(), 2, "a day later it checks again");

    let current = latest(env!("CARGO_PKG_VERSION"));
    std::fs::remove_file(&state).unwrap();
    checked(home.path(), &current)
        .args(["status", "--repo"])
        .arg(repo.path())
        .assert()
        .stderr(predicate::str::contains("is available").not());
    assert_eq!(current.hits(), 1);
}

#[test]
fn update_check_is_skipped_for_mcp_and_ui() {
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let stub = latest("99.0.0");
    checked(home.path(), &stub)
        .args(["ui", "--repo"])
        .arg(repo.path())
        .assert()
        .code(1);
    checked(home.path(), &stub)
        .args(["mcp", "--repo"])
        .arg(repo.path())
        .write_stdin("")
        .output()
        .unwrap();
    assert_eq!(stub.hits(), 0);
    assert!(!home.path().join("update-check.json").exists());
}

fn archive(binary: &[u8]) -> (String, Vec<u8>) {
    let target = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-musl",
        ("linux", "aarch64") => "aarch64-unknown-linux-musl",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        _ => "x86_64-pc-windows-msvc",
    };
    if cfg!(windows) {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file::<_, ()>(
            format!("codemycel-{target}/codemycel.exe"),
            zip::write::FileOptions::default(),
        )
        .unwrap();
        zip.write_all(binary).unwrap();
        (
            format!("codemycel-{target}.zip"),
            zip.finish().unwrap().into_inner(),
        )
    } else {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let mut header = tar::Header::new_gnu();
        header.set_size(binary.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, format!("codemycel-{target}/codemycel"), binary)
            .unwrap();
        (
            format!("codemycel-{target}.tar.gz"),
            tar.into_inner().unwrap().finish().unwrap(),
        )
    }
}

fn hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn upgrade_verifies_checksum_and_replaces_binary() {
    let new_binary = b"the next codemycel".to_vec();
    let (name, bytes) = archive(&new_binary);
    for (checksum, replaced) in [(hex(b"something else"), false), (hex(&bytes), true)] {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir
            .path()
            .join(format!("codemycel{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(env!("CARGO_BIN_EXE_codemycel"), &exe).unwrap();
        let original = std::fs::read(&exe).unwrap();
        let (name, bytes, checksum) = (name.clone(), bytes.clone(), checksum.clone());
        let stub = Stub::start(move |base| {
            let sha = format!("{name}.sha256");
            vec![
                (
                    "/latest".to_owned(),
                    release_json(base, "99.0.0", &[&name, &sha]),
                ),
                (
                    format!("/{sha}"),
                    format!("{checksum}  {name}\n").into_bytes(),
                ),
                (format!("/{name}"), bytes),
            ]
        });
        let home = tempfile::tempdir().unwrap();
        let assert = assert_cmd::Command::new(&exe)
            .env("CODEMYCEL_HOME", home.path())
            .env("CODEMYCEL_RELEASES_URL", format!("{}/latest", stub.base))
            .env_remove("CODEMYCEL_TEST_DENY_NETWORK")
            .arg("upgrade")
            .timeout(Duration::from_secs(60))
            .assert();
        if replaced {
            assert
                .success()
                .stdout(predicate::str::contains("upgraded codemycel"))
                .stdout(predicate::str::contains("/releases/v99.0.0"));
            assert_eq!(std::fs::read(&exe).unwrap(), new_binary);
        } else {
            assert
                .code(1)
                .stderr(predicate::str::contains("checksum mismatch"));
            assert_eq!(std::fs::read(&exe).unwrap(), original);
        }
    }
}

#[test]
fn upgrade_refuses_when_network_is_denied() {
    let home = tempfile::tempdir().unwrap();
    let stub = latest("99.0.0");
    checked(home.path(), &stub)
        .env("CODEMYCEL_TEST_DENY_NETWORK", "1")
        .arg("upgrade")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("network access denied"));
    assert_eq!(stub.hits(), 0);
}
