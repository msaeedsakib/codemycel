#![allow(clippy::unwrap_used)]

use std::time::Duration;

use codemycel_core::config::{Config, ServerConfig};
use codemycel_core::{ReadinessStrategy, ServerAdapter, ServerSource};
use codemycel_lsp::Provisioner;
use codemycel_lsp::adapters::rust_analyzer::RustAnalyzer;
use serde_json::{Value, json};

fn build_flags(options: &Value) -> (Value, Value) {
    (
        options.pointer("/cargo/buildScripts/enable").cloned().unwrap(),
        options.pointer("/procMacro/enable").cloned().unwrap(),
    )
}

#[test]
fn initialization_options_follow_config() {
    let adapter = RustAnalyzer::new();
    let defaults = adapter.initialization_options(&ServerConfig::default());
    assert_eq!(defaults["checkOnSave"], json!(false));
    assert_eq!(defaults["diagnostics"]["enable"], json!(false));
    assert_eq!(defaults["cachePriming"]["enable"], json!(true));
    assert_eq!(build_flags(&defaults), (json!(true), json!(true)));

    let disabled = adapter.initialization_options(&ServerConfig {
        build_scripts: Some(false),
        ..ServerConfig::default()
    });
    assert_eq!(build_flags(&disabled), (json!(false), json!(false)));
}

#[test]
fn readiness_is_quiescent_notification() {
    assert_eq!(
        RustAnalyzer::new().readiness(),
        ReadinessStrategy::WaitForQuiescent {
            timeout: Duration::from_secs(600)
        }
    );
}

#[cfg(unix)]
#[test]
fn failing_version_command_is_rejected() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let proxy = dir.path().join("rust-analyzer");
    std::fs::write(
        &proxy,
        "#!/bin/sh\necho \"error: Unknown binary 'rust-analyzer' in official toolchain\" >&2\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&proxy, std::fs::Permissions::from_mode(0o755)).unwrap();

    let reason = RustAnalyzer::new().validate(&proxy).unwrap_err();
    assert!(reason.contains("Unknown binary"), "{reason}");
}

#[test]
#[ignore = "downloads the pinned rust-analyzer release"]
fn rust_analyzer_downloads_and_validates() {
    let home = tempfile::tempdir().unwrap();
    let adapter = RustAnalyzer::new();
    let located = Provisioner::new(&Config::default(), home.path().to_owned(), true)
        .with_search_path(None)
        .locate(&adapter)
        .unwrap();
    assert_eq!(located.source, ServerSource::Download);
    assert!(located.path.starts_with(home.path()), "{located:?}");
    assert!(!located.version.is_empty());
}
