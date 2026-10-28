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
    assert_eq!(
        roots(repo.path(), &files),
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

#[test]
fn path_dependencies_inside_a_workspace_are_members() {
    let repo = tempfile::tempdir().unwrap();
    let package = |deps: &str| format!("{PACKAGE}{deps}");
    let files = [
        write(
            repo.path(),
            "Cargo.toml",
            "[workspace]\nmembers = [\"app\"]\nexclude = [\"libs/vendored\"]\n",
        ),
        write(
            repo.path(),
            "app/Cargo.toml",
            &package(
                "[dependencies]\nshared = { path = \"../shared\" }\nvendored = { path = \"../libs/vendored\" }\nserde = \"1\"\n[dev-dependencies]\ntestkit = { path = \"../libs/testkit\" }\n",
            ),
        ),
        write(
            repo.path(),
            "shared/Cargo.toml",
            &package("[build-dependencies]\ngen = { path = \"../tools/gen\" }\n"),
        ),
        write(repo.path(), "libs/testkit/Cargo.toml", PACKAGE),
        write(repo.path(), "tools/gen/Cargo.toml", PACKAGE),
        write(repo.path(), "libs/vendored/Cargo.toml", PACKAGE),
        write(repo.path(), "lonely/Cargo.toml", PACKAGE),
        write(
            repo.path(),
            "apps/w/Cargo.toml",
            "[workspace]\nmembers = [\"a\"]\n",
        ),
        write(
            repo.path(),
            "apps/w/a/Cargo.toml",
            &package("[dependencies]\next = { path = \"../../../ext\" }\n"),
        ),
        write(repo.path(), "ext/Cargo.toml", PACKAGE),
    ];
    assert_eq!(
        roots(repo.path(), &files),
        ["", "apps/w", "ext", "libs/vendored", "lonely"]
    );
}

fn roots(repo: &Path, files: &[RepoPath]) -> Vec<String> {
    let registry = Registry::with_adapters(vec![Box::new(RustAdapter::new())]);
    RootDetector::detect(repo, files, &Config::default(), &registry)
        .unwrap()
        .roots()
        .iter()
        .inspect(|root| assert_eq!(root.language, Language::Rust))
        .map(|root| root.path.as_str().to_owned())
        .collect()
}
