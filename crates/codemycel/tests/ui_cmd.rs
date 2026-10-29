#![allow(clippy::unwrap_used)]

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

use support::{codemycel, fixture_copy};

#[test]
fn ui_command_prints_url() {
    let (_temp, repo) = fixture_copy("rust/workspace-basic");
    let home = tempfile::tempdir().unwrap();
    let init = codemycel(home.path())
        .args(["init", "--no-download", "--repo"])
        .arg(&repo)
        .output()
        .unwrap();
    assert!(matches!(init.status.code(), Some(0 | 2)));

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("codemycel"))
        .env("CODEMYCEL_HOME", home.path())
        .env("CODEMYCEL_NO_UPDATE_CHECK", "1")
        .args(["ui", "--port", "0", "--repo"])
        .arg(&repo)
        .env("CODEMYCEL_TEST_NO_BROWSER", "1")
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = line
        .trim()
        .strip_prefix("http://127.0.0.1:")
        .and_then(|rest| rest.strip_suffix('/'))
        .map(|port| format!("127.0.0.1:{port}"));

    let response = addr.as_ref().map(|addr| {
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(
            stream,
            "GET /api/meta HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        response
    });
    child.kill().unwrap();
    child.wait().unwrap();

    assert!(addr.is_some(), "not a loopback URL: {line:?}");
    let response = response.unwrap();
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    assert!(response.contains("\"indexed_at\""), "{response}");
}
