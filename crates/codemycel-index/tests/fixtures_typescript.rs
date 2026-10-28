#![allow(clippy::unwrap_used)]

mod harness;

use std::path::PathBuf;

use codemycel_core::Language;
use codemycel_index::Registry;
use codemycel_lsp::adapters::typescript7::TypeScript7;
use harness::{fixture, index_fixture_full, local_provisioner};

#[test]
fn typescript_fixture_matches_expected_after_lsp() {
    let Some(provisioner) = local_provisioner(&TypeScript7::new()) else {
        return;
    };
    let registry = Registry::default().with_servers(vec![
        (Language::TypeScript, Box::new(TypeScript7::new())),
        (Language::JavaScript, Box::new(TypeScript7::new())),
    ]);
    let source = fixture("typescript/monorepo-basic");
    let (_copy, dir) = indexable(source.clone());
    index_fixture_full(&dir, registry, &provisioner).assert_matches(&source);
}

#[cfg(not(windows))]
fn indexable(fixture: PathBuf) -> (Option<tempfile::TempDir>, PathBuf) {
    (None, fixture)
}

/// A copy of the fixture whose `node_modules/@fixture/core` link is a directory junction when
/// the checkout, lacking symlink support, holds the link target as a plain file.
#[cfg(windows)]
fn indexable(fixture: PathBuf) -> (Option<tempfile::TempDir>, PathBuf) {
    let (temp, copy) = harness::fixture_copy(&fixture);
    let link = copy.join("packages/app/node_modules/@fixture/core");
    if link.symlink_metadata().unwrap().is_file() {
        let target = std::fs::read_to_string(&link).unwrap();
        std::fs::remove_file(&link).unwrap();
        let mut resolved = link.parent().unwrap().to_owned();
        for part in target.trim().split(['/', '\\']) {
            match part {
                "" | "." => {}
                ".." => {
                    resolved.pop();
                }
                name => resolved.push(name),
            }
        }
        let status = std::process::Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&resolved)
            .status()
            .unwrap();
        assert!(status.success(), "mklink /J failed for {}", link.display());
    }
    (Some(temp), copy)
}
