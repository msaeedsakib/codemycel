#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// `codemycel` with the version check off and the home in `home`.
pub fn codemycel(home: &Path) -> Command {
    let mut command = cargo_bin_cmd!("codemycel");
    command
        .env("CODEMYCEL_HOME", home)
        .env("CODEMYCEL_NO_UPDATE_CHECK", "1");
    command
}

/// A copy of `fixtures/<relative>` in a temporary directory.
pub fn fixture_copy(relative: &str) -> (TempDir, PathBuf) {
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let dest = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), dest).unwrap();
            }
        }
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative);
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("repo");
    copy_dir(&source, &copy);
    (temp, copy)
}

/// A local HTTP server answering `GET <path>` with fixed bodies and 404 otherwise, counting
/// requests.
pub struct Stub {
    pub base: String,
    hits: Arc<AtomicUsize>,
}

impl Stub {
    /// `routes` receives the server's base URL (`http://127.0.0.1:<port>`) and returns the
    /// paths and bodies to serve.
    pub fn start(routes: impl FnOnce(&str) -> Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = routes(&base);
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let _ = reader.read_line(&mut request);
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let (status, body) = match routes.iter().find(|(route, _)| *route == path) {
                    Some((_, body)) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        Self { base, hits }
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }
}

/// A release named `v<version>` whose assets are served at `<base>/<name>`.
pub fn release_json(base: &str, version: &str, assets: &[&str]) -> Vec<u8> {
    let assets: Vec<serde_json::Value> = assets
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "browser_download_url": format!("{base}/{name}"),
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "tag_name": format!("v{version}"),
        "html_url": format!("{base}/releases/v{version}"),
        "assets": assets,
    }))
    .unwrap()
}
