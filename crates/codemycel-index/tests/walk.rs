#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::{
    AdapterError, FileKind, FileRecord, Language, LanguageAdapter, ParsedFile, RepoPath, Root,
    SkipReason,
};
use codemycel_index::{Registry, Walker};

struct RustLike;

impl LanguageAdapter for RustLike {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn detect_roots(&self, _: &Path, _: &[RepoPath]) -> Result<Vec<Root>, AdapterError> {
        Ok(Vec::new())
    }
    fn parse(&self, _: &RepoPath, _: &[u8]) -> Result<ParsedFile, AdapterError> {
        Ok(ParsedFile::default())
    }
    fn external_path_predicate(&self, _: &Path, _: &Path) -> bool {
        false
    }
}

fn write(root: &Path, relative: &str, contents: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

fn walk(root: &Path, config: &Config) -> Vec<FileRecord> {
    let registry = Registry::with_adapters(vec![Box::new(RustLike)]);
    Walker::new(root, config, &registry).walk().unwrap()
}

fn listed(records: &[FileRecord]) -> Vec<&str> {
    records.iter().map(|record| record.path.as_str()).collect()
}

#[test]
fn gitignore_and_config_globs_are_honoured() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    write(root, ".gitignore", b"*.log\n/gen/\n");
    write(root, "sub/.gitignore", b"local.txt\n");
    write(root, ".git/info/exclude", b"secret.txt\n");
    for file in [
        "keep.rs",
        "debug.log",
        "gen/out.rs",
        "sub/gen/kept.rs",
        "sub/local.txt",
        "local.txt",
        "secret.txt",
        "fixtures/a/x.rs",
    ] {
        write(root, file, b"x\n");
    }
    let config = Config {
        ignore: vec!["fixtures/**".to_owned()],
        ..Config::default()
    };
    assert_eq!(
        listed(&walk(root, &config)),
        [
            ".gitignore",
            "keep.rs",
            "local.txt",
            "sub/.gitignore",
            "sub/gen/kept.rs"
        ]
    );
}

#[test]
fn always_ignored_directories_are_skipped() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    for dir in [
        ".git",
        ".codemycel",
        "node_modules",
        "target",
        "dist",
        "build",
        ".next",
        "coverage",
        "src/target",
        "packages/a/node_modules/@scope/pkg",
    ] {
        write(root, &format!("{dir}/file.rs"), b"x\n");
    }
    write(root, "build.rs", b"x\n");
    write(root, "src/dist.rs", b"x\n");
    assert_eq!(
        listed(&walk(root, &Config::default())),
        ["build.rs", "src/dist.rs"]
    );
}

#[test]
fn invalid_utf8_and_oversize_files_are_skipped_with_reason() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    write(root, "bad.rs", b"fn f() { \xff }\n");
    write(root, "big.rs", &[b'/'; 64]);
    write(root, "blob.rs", b"fn\0f\n");
    write(root, "notes.txt", b"plain\n");
    write(root, "x.md#y.md", b"# y\n");
    write(root, "ok.rs", b"fn ok() {}\n");
    let config = Config {
        max_file_size: 32,
        ..Config::default()
    };
    let records = walk(root, &config);
    let reasons: Vec<(&str, Option<SkipReason>)> = records
        .iter()
        .map(|record| (record.path.as_str(), record.skip_reason))
        .collect();
    assert_eq!(
        reasons,
        [
            ("bad.rs", Some(SkipReason::InvalidUtf8)),
            ("big.rs", Some(SkipReason::Oversize)),
            ("blob.rs", Some(SkipReason::Binary)),
            ("notes.txt", Some(SkipReason::Unsupported)),
            ("ok.rs", None),
            ("x.md#y.md", Some(SkipReason::UnaddressablePath)),
        ]
    );
    let big = records
        .iter()
        .find(|record| record.path.as_str() == "big.rs")
        .unwrap();
    assert_eq!(big.size, 64);
    assert_eq!(big.hash, blake3::hash(&[b'/'; 64]));
}

#[test]
fn classification_by_extension_and_name() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    for file in [
        "Cargo.toml",
        "web/package.json",
        "README.md",
        "docs/guide.MDX",
        "src/lib.rs",
        "notes.txt",
        "Makefile",
    ] {
        write(root, file, b"x\n");
    }
    let records = walk(root, &Config::default());
    let kinds: Vec<(&str, FileKind, Option<Language>)> = records
        .iter()
        .map(|record| (record.path.as_str(), record.kind, record.language))
        .collect();
    assert_eq!(
        kinds,
        [
            ("Cargo.toml", FileKind::Manifest, None),
            ("Makefile", FileKind::Other, None),
            ("README.md", FileKind::Doc, None),
            ("docs/guide.MDX", FileKind::Doc, None),
            ("notes.txt", FileKind::Other, None),
            ("src/lib.rs", FileKind::Code, Some(Language::Rust)),
            ("web/package.json", FileKind::Manifest, None),
        ]
    );
}

#[test]
fn walk_order_is_sorted_and_stable() {
    let repo = tempfile::tempdir().unwrap();
    let root = repo.path();
    for file in ["z.rs", "a/b.rs", "a.rs", "Q.rs", "a/a.rs", "m/z/z.rs"] {
        write(root, file, b"x\n");
    }
    let first = walk(root, &Config::default());
    assert_eq!(
        listed(&first),
        ["Q.rs", "a.rs", "a/a.rs", "a/b.rs", "m/z/z.rs", "z.rs"]
    );
    assert_eq!(first, walk(root, &Config::default()));
}
