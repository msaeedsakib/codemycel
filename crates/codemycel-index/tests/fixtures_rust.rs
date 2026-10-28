#![allow(clippy::unwrap_used)]

mod harness;

use std::path::Path;

use codemycel_core::Language;
use codemycel_index::Registry;
use codemycel_lsp::adapters::rust_analyzer::RustAnalyzer;
use harness::{fixture, index_fixture_full, local_provisioner};

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

/// Indexes a copy, so the `Cargo.lock` and `target/` rust-analyzer writes stay out of the
/// fixture.
#[test]
fn rust_fixture_matches_expected_after_lsp() {
    let Some(provisioner) = local_provisioner(&RustAnalyzer::new()) else {
        return;
    };
    let source = fixture("rust/workspace-basic");
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("workspace-basic");
    copy_dir(&source, &copy);
    let registry =
        Registry::default().with_servers(vec![(Language::Rust, Box::new(RustAnalyzer::new()))]);
    index_fixture_full(&copy, registry, &provisioner).assert_matches(&source);
}
