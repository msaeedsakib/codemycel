#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used)]

mod support;

use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use support::Stub;

const INSTALL_TIMEOUT: Duration = Duration::from_secs(60);

/// `codemycel-<target>.tar.gz` holding this build's binary under a top-level directory, as
/// cargo-dist lays it out.
fn release_archive(target: &str) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut archive = tar::Builder::new(encoder);
    archive
        .append_path_with_name(
            env!("CARGO_BIN_EXE_codemycel"),
            format!("codemycel-{target}/codemycel"),
        )
        .unwrap();
    archive.into_inner().unwrap().finish().unwrap()
}

fn sha256_line(bytes: &[u8], name: &str) -> Vec<u8> {
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{digest}  {name}\n").into_bytes()
}

#[test]
fn installer_handles_missing_path_and_no_tty() {
    let target = format!("{}-unknown-linux-musl", std::env::consts::ARCH);
    let asset = format!("codemycel-{target}.tar.gz");
    let archive = release_archive(&target);
    let checksum = sha256_line(&archive, &asset);
    let stub = Stub::start(|_| {
        vec![
            (format!("/{asset}"), archive),
            (format!("/{asset}.sha256"), checksum),
        ]
    });
    let home = tempfile::tempdir().unwrap();
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../install.sh");

    let mut child = Command::new("setsid")
        .arg("--wait")
        .arg("sh")
        .arg(&script)
        .env_clear()
        .env("HOME", home.path())
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("CODEMYCEL_INSTALL_BASE_URL", &stub.base)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + INSTALL_TIMEOUT;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the installer did not finish within {INSTALL_TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .unwrap();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(status.success(), "{status}\n{stdout}\n{stderr}");
    let bin_dir = home.path().join(".local/bin");
    assert!(
        stdout.contains(&format!("{} is not on your PATH", bin_dir.display())),
        "{stdout}"
    );
    assert!(
        stdout.contains("run codemycel setup to configure your agents"),
        "{stdout}"
    );
    assert_eq!(stub.hits(), 2);
    let installed = Command::new(bin_dir.join("codemycel"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(installed.status.success());
    assert!(
        String::from_utf8(installed.stdout)
            .unwrap()
            .contains(env!("CARGO_PKG_VERSION"))
    );
}
