#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::time::Duration;

use codemycel_core::config::{Config, HomeConfig, RepoConfig, ServerConfig};
use codemycel_core::{
    Arch, Language, Os, ReadinessStrategy, ServerAdapter, ServerSource, Toolchain,
};
use codemycel_lsp::Provisioner;
use codemycel_lsp::provision::cache;
use sha2::{Digest, Sha256};

const VALID: &[u8] = b"valid server 1.2.3";

/// Accepts any file whose content starts with `valid`.
#[derive(Default)]
struct FileAdapter {
    url: Option<String>,
    sha256: Option<&'static str>,
    member: Option<&'static str>,
    toolchain: Option<Toolchain>,
}

impl ServerAdapter for FileAdapter {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn name(&self) -> &'static str {
        "file-server"
    }
    fn pinned_version(&self) -> &'static str {
        "1.2.3"
    }
    fn binary_name(&self) -> &'static str {
        "file-server"
    }
    fn toolchain(&self) -> Option<Toolchain> {
        self.toolchain
    }
    fn download_url(&self, _: Os, _: Arch) -> Option<String> {
        self.url.clone()
    }
    fn pinned_sha256(&self, _: Os, _: Arch) -> Option<&'static str> {
        self.sha256
    }
    fn archive_member(&self, _: Os) -> Option<&'static str> {
        self.member
    }
    fn validate(&self, binary: &Path) -> Result<String, String> {
        match std::fs::read(binary) {
            Ok(bytes) if bytes.starts_with(b"valid") => Ok("1.2.3".to_owned()),
            Ok(_) => Err("not a valid server".to_owned()),
            Err(error) => Err(error.to_string()),
        }
    }
    fn launch_args(&self) -> &'static [&'static str] {
        &[]
    }
    fn language_id(&self, _: &str) -> &'static str {
        "rust"
    }
    fn initialization_options(&self, _: &ServerConfig) -> serde_json::Value {
        serde_json::Value::Null
    }
    fn readiness(&self) -> ReadinessStrategy {
        ReadinessStrategy::ProbeDefinition {
            timeout: Duration::from_secs(1),
        }
    }
    fn request_concurrency(&self) -> usize {
        1
    }
    fn config_file_name(&self) -> Option<&'static str> {
        None
    }
}

fn config(server_path: Option<PathBuf>) -> Config {
    let mut servers = BTreeMap::new();
    servers.insert(
        Language::Rust,
        ServerConfig {
            path: server_path,
            build_scripts: None,
        },
    );
    Config::merge(
        RepoConfig {
            servers,
            ..RepoConfig::default()
        },
        HomeConfig::default(),
    )
}

fn provisioner(config: &Config, home: &Path, path: Option<&Path>, download: bool) -> Provisioner {
    Provisioner::new(config, home.to_owned(), download)
        .with_search_path(path.map(|p| OsString::from(p.as_os_str())))
}

fn write(path: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    path.to_owned()
}

fn on_path(dir: &Path, bytes: &[u8]) -> PathBuf {
    write(
        &dir.join(format!("file-server{}", std::env::consts::EXE_SUFFIX)),
        bytes,
    )
}

/// Serves `body` to every request on a local port; returns the base URL.
fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = Vec::new();
            let mut byte = [0; 1];
            while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
                request.push(byte[0]);
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://{address}")
}

fn sha256(bytes: &[u8]) -> &'static str {
    let digest: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Box::leak(digest.into_boxed_str())
}

#[test]
fn config_path_wins_when_valid() {
    let dir = tempfile::tempdir().unwrap();
    let configured = write(&dir.path().join("custom/server"), VALID);
    let bin = dir.path().join("bin");
    on_path(&bin, VALID);
    let config = config(Some(configured.clone()));
    let located = provisioner(&config, &dir.path().join("home"), Some(&bin), false)
        .locate(&FileAdapter::default())
        .unwrap();
    assert_eq!(located.source, ServerSource::Config);
    assert_eq!(located.path, configured);
    assert_eq!(located.version, "1.2.3");
    assert!(located.rejected.is_empty());
}

#[test]
fn invalid_candidates_are_rejected_with_reason_and_next_step_tried() {
    let dir = tempfile::tempdir().unwrap();
    let configured = write(&dir.path().join("custom/server"), b"Version 5.9.2");
    let bin = dir.path().join("bin");
    let valid = on_path(&bin, VALID);
    let config = config(Some(configured.clone()));
    let located = provisioner(&config, &dir.path().join("home"), Some(&bin), false)
        .locate(&FileAdapter::default())
        .unwrap();
    assert_eq!(located.source, ServerSource::Path);
    assert_eq!(located.path, valid);
    assert_eq!(located.rejected.len(), 1);
    assert_eq!(located.rejected[0].path, configured);
    assert_eq!(located.rejected[0].source, ServerSource::Config);
    assert_eq!(located.rejected[0].reason, "not a valid server");
}

#[test]
fn cache_is_used_before_download() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let adapter = FileAdapter {
        url: Some("http://127.0.0.1:9/unreachable.gz".to_owned()),
        sha256: Some(sha256(b"other")),
        ..FileAdapter::default()
    };
    let cached = write(&cache::cached_binary(&home, &adapter), VALID);
    let located = provisioner(&config(None), &home, None, true)
        .locate(&adapter)
        .unwrap();
    assert_eq!(located.source, ServerSource::Cache);
    assert_eq!(located.path, cached);
}

#[test]
fn download_disabled_returns_no_server() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = FileAdapter {
        url: Some(serve(VALID.to_vec()) + "/server.gz"),
        sha256: Some(sha256(VALID)),
        ..FileAdapter::default()
    };
    let error = provisioner(&config(None), &dir.path().join("home"), None, false)
        .locate(&adapter)
        .unwrap_err();
    assert_eq!(error.download, "downloads are disabled");
    assert!(error.rejected.is_empty());
}

#[test]
fn checksum_mismatch_is_rejected_and_nothing_is_cached() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let adapter = FileAdapter {
        url: Some(serve(VALID.to_vec()) + "/server"),
        sha256: Some(sha256(b"something else")),
        ..FileAdapter::default()
    };
    let error = provisioner(&config(None), &home, None, true)
        .locate(&adapter)
        .unwrap_err();
    assert!(
        error.download.starts_with("checksum mismatch"),
        "{}",
        error.download
    );
    let server_dir = cache::server_dir(&home, &adapter);
    let leftovers: Vec<_> = std::fs::read_dir(&server_dir)
        .map(|entries| entries.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    assert!(leftovers.is_empty(), "left in the cache: {leftovers:?}");
}

#[test]
fn archive_extraction_handles_tgz_gz_zip() {
    let gz = {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(VALID).unwrap();
        encoder.finish().unwrap()
    };
    let tgz = {
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(VALID.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "package/lib/server", VALID)
            .unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    };
    let zip = {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        writer
            .start_file("server.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(VALID).unwrap();
        writer.finish().unwrap().into_inner()
    };
    for (archive, name, member) in [
        (gz, "server.gz", None),
        (tgz, "server.tgz", Some("package/lib/server")),
        (zip, "server.zip", Some("server.exe")),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        let adapter = FileAdapter {
            sha256: Some(sha256(&archive)),
            url: Some(format!("{}/{name}", serve(archive))),
            member,
            ..FileAdapter::default()
        };
        let located = provisioner(&config(None), &home, None, true)
            .locate(&adapter)
            .unwrap();
        assert_eq!(located.source, ServerSource::Download, "{name}");
        assert_eq!(
            located.path,
            cache::cached_binary(&home, &adapter),
            "{name}"
        );
        assert_eq!(std::fs::read(&located.path).unwrap(), VALID, "{name}");
    }
}

#[test]
fn cargo_present_is_false_when_cargo_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let adapter = FileAdapter {
        toolchain: Some(Toolchain {
            prerequisite: "cargo",
            locate: &["rustup", "which", "file-server"],
            install: &["rustup", "component", "add", "file-server"],
        }),
        ..FileAdapter::default()
    };
    let stripped = provisioner(&config(None), dir.path(), None, false);
    assert!(!stripped.prerequisite_present(&adapter));
    let cargo_dir = Path::new(env!("CARGO")).parent().unwrap();
    let with_cargo = provisioner(&config(None), dir.path(), Some(cargo_dir), false);
    assert!(with_cargo.prerequisite_present(&adapter));
    assert!(stripped.prerequisite_present(&FileAdapter::default()));
}
