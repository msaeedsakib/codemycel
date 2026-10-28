#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::{Language, RepoPath};
use codemycel_index::{Registry, RootDetector};
use codemycel_parse::rust::RustAdapter;

const WORKSPACE: &str =
    "[workspace]\nmembers = [\"crates/*\", \"tools/gen\"]\nexclude = [\"crates/legacy\"]\n";
const PACKAGE: &str = "[package]\nname = \"p\"\nversion = \"0.1.0\"\n";

fn write(repo: &Path, path: &str, text: &str) -> RepoPath {
    let absolute = repo.join(path);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(absolute, text).unwrap();
    RepoPath::new(path).unwrap()
}

#[test]
fn workspace_and_standalone_roots_detected() {
    let repo = tempfile::tempdir().unwrap();
    let files = [
        write(repo.path(), "Cargo.toml", WORKSPACE),
        write(repo.path(), "crates/a/Cargo.toml", PACKAGE),
        write(repo.path(), "crates/a/src/lib.rs", "pub fn a() {}\n"),
        write(repo.path(), "crates/legacy/Cargo.toml", PACKAGE),
        write(repo.path(), "crates/legacy/nested/Cargo.toml", PACKAGE),
        write(repo.path(), "crates/deep/inner/Cargo.toml", PACKAGE),
        write(repo.path(), "tools/gen/Cargo.toml", PACKAGE),
        write(repo.path(), "tools/other/Cargo.toml", PACKAGE),
        write(
            repo.path(),
            "apps/desktop/Cargo.toml",
            "[workspace]\n\n[package]\nname = \"d\"\n",
        ),
        write(repo.path(), "apps/desktop/plugin/Cargo.toml", PACKAGE),
        write(repo.path(), "broken/Cargo.toml", "[package\nname = "),
        write(repo.path(), "plain/Cargo.toml", "[dependencies]\n"),
    ];
    let registry = Registry::with_adapters(vec![Box::new(RustAdapter::new())]);
    let detector =
        RootDetector::detect(repo.path(), &files, &Config::default(), &registry).unwrap();
    let roots: Vec<&str> = detector
        .roots()
        .iter()
        .inspect(|root| assert_eq!(root.language, Language::Rust))
        .map(|root| root.path.as_str())
        .collect();
    assert_eq!(
        roots,
        [
            "",
            "apps/desktop",
            "apps/desktop/plugin",
            "crates/deep/inner",
            "crates/legacy",
            "crates/legacy/nested",
            "tools/other",
        ]
    );
}
