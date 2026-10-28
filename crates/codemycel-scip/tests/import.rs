#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::process::Command;

use codemycel_core::config::Config;
use codemycel_core::{
    Candidate, LanguageAdapter, NodeKind, NodeRef, ParsedFile, RepoPath, ResolverSource,
    SemanticPath, SourceFile, UnresolvedReason, Verdict,
};
use codemycel_parse::rust::RustAdapter;
use codemycel_scip::{ScipImporter, ScipIndex};
use protobuf::{EnumOrUnknown, Message};
use scip::types;

const A: &str = "\
pub struct S;

impl T for S {
    fn go(&self) {}
}

pub fn run() {
    helper();
    other();
}

// helper
";

const B: &str = "\
pub trait T {
    fn go(&self);
}

pub fn helper() {}
";

const T: &str = "cargo demo 0.1.0 b/T#";
const T_GO: &str = "cargo demo 0.1.0 b/T#go().";
const HELPER: &str = "cargo demo 0.1.0 b/helper().";
const S: &str = "cargo demo 0.1.0 a/S#";
const S_GO: &str = "cargo demo 0.1.0 a/impl#[S][T]go().";

/// An occurrence of the first `needle` after `anchor`, as a single-line UTF-8 range.
fn occ(
    text: &str,
    anchor: &str,
    needle: &str,
    symbol: &str,
    definition: bool,
) -> types::Occurrence {
    let start =
        text.find(anchor).unwrap() + text[text.find(anchor).unwrap()..].find(needle).unwrap();
    let line = text[..start].matches('\n').count();
    let column = start - text[..start].rfind('\n').map_or(0, |i| i + 1);
    let as_i32 = |n: usize| i32::try_from(n).unwrap();
    types::Occurrence {
        range: vec![as_i32(line), as_i32(column), as_i32(column + needle.len())],
        symbol: symbol.to_owned(),
        symbol_roles: if definition { 1 } else { 0 },
        ..Default::default()
    }
}

fn implements(symbol: &str, parent: &str) -> types::SymbolInformation {
    types::SymbolInformation {
        symbol: symbol.to_owned(),
        relationships: vec![types::Relationship {
            symbol: parent.to_owned(),
            is_implementation: true,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn document(
    path: &str,
    occurrences: Vec<types::Occurrence>,
    symbols: Vec<types::SymbolInformation>,
) -> types::Document {
    types::Document {
        relative_path: path.to_owned(),
        occurrences,
        symbols,
        position_encoding: EnumOrUnknown::new(
            types::PositionEncoding::UTF8CodeUnitOffsetFromLineStart,
        ),
        ..Default::default()
    }
}

fn index() -> types::Index {
    let a = document(
        "src/a.rs",
        vec![
            occ(A, "pub struct ", "S", S, true),
            occ(A, "impl ", "T", T, false),
            occ(A, "fn ", "go", S_GO, true),
            occ(A, "pub fn run", "helper", HELPER, false),
            occ(A, "// ", "helper", HELPER, false),
            occ(A, "pub fn run", "run", "local 3", true),
        ],
        vec![implements(S, T), implements(S_GO, T_GO)],
    );
    let b = document(
        "src/b.rs",
        vec![
            occ(B, "pub trait ", "T", T, true),
            occ(B, "fn ", "go", T_GO, true),
            occ(B, "pub fn ", "helper", HELPER, true),
        ],
        Vec::new(),
    );
    types::Index {
        documents: vec![a, b],
        ..Default::default()
    }
}

fn write_index(dir: &Path, index: &types::Index) -> std::path::PathBuf {
    let path = dir.join("index.scip");
    std::fs::write(&path, index.write_to_bytes().unwrap()).unwrap();
    path
}

struct Parsed {
    a: (RepoPath, ParsedFile),
    b: (RepoPath, ParsedFile),
}

fn parsed() -> Parsed {
    let adapter = RustAdapter::new();
    let parse = |path: &str, text: &str| {
        let path = RepoPath::new(path).unwrap();
        let parsed = adapter.parse(&path, text.as_bytes()).unwrap();
        (path, parsed)
    };
    Parsed {
        a: parse("src/a.rs", A),
        b: parse("src/b.rs", B),
    }
}

fn candidate<'a>(parsed: &'a ParsedFile, text: &str) -> &'a Candidate {
    parsed
        .candidates
        .iter()
        .find(|c| c.text == text)
        .unwrap_or_else(|| panic!("no `{text}` among {:#?}", parsed.candidates))
}

fn node(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(path.parse::<SemanticPath>().unwrap(), kind)
}

fn loaded() -> (tempfile::TempDir, ScipIndex) {
    let dir = tempfile::tempdir().unwrap();
    let path = write_index(dir.path(), &index());
    let index = ScipImporter::load(&path).unwrap();
    (dir, index)
}

fn sources(parsed: &Parsed) -> Vec<SourceFile<'_>> {
    vec![
        SourceFile {
            path: &parsed.a.0,
            text: A,
            nodes: &parsed.a.1.nodes,
        },
        SourceFile {
            path: &parsed.b.0,
            text: B,
            nodes: &parsed.b.1.nodes,
        },
    ]
}

#[test]
fn definition_occurrences_map_to_nodes() {
    let (_dir, index) = loaded();
    let parsed = parsed();
    let files = sources(&parsed);
    let resolver = index.bind(&files);
    let call = candidate(&parsed.a.1, "helper");
    assert_eq!(
        resolver.resolve(std::slice::from_ref(call)),
        [Verdict::Confirmed {
            target: node("src/b.rs::helper", NodeKind::Function)
        }]
    );
}

#[test]
fn reference_without_candidate_is_ignored() {
    let (_dir, index) = loaded();
    let parsed = parsed();
    let files = sources(&parsed);
    let resolver = index.bind(&files);
    let verdicts = resolver.resolve(&parsed.a.1.candidates);
    assert_eq!(verdicts.len(), parsed.a.1.candidates.len());
    let confirmed_helper = verdicts
        .iter()
        .filter(|v| {
            **v == Verdict::Confirmed {
                target: node("src/b.rs::helper", NodeKind::Function),
            }
        })
        .count();
    assert_eq!(
        confirmed_helper, 1,
        "the comment's occurrence has no candidate"
    );
}

#[test]
fn candidate_without_occurrence_is_no_definition() {
    let (_dir, index) = loaded();
    let parsed = parsed();
    let files = sources(&parsed);
    let resolver = index.bind(&files);
    let other = candidate(&parsed.a.1, "other");
    assert_eq!(
        resolver.resolve(std::slice::from_ref(other)),
        [Verdict::Unresolved {
            reason: UnresolvedReason::NoDefinition
        }]
    );
}

#[test]
fn undefined_symbol_is_unmapped_inside_a_defined_package_and_external_outside() {
    let a = document(
        "src/a.rs",
        vec![
            occ(
                A,
                "pub struct ",
                "S",
                "rust-analyzer cargo demo 0.1.0 a/S#",
                true,
            ),
            occ(
                A,
                "impl T for ",
                "S",
                "rust-analyzer cargo demo 0.1.0 a/impl#[S]",
                false,
            ),
            occ(A, "impl ", "T", "rust-analyzer cargo other 1.0.0 T#", false),
        ],
        Vec::new(),
    );
    let raw = types::Index {
        documents: vec![a],
        ..Default::default()
    };
    let dir = tempfile::tempdir().unwrap();
    let index = ScipImporter::load(&write_index(dir.path(), &raw)).unwrap();
    let parsed = parsed();
    let files = sources(&parsed);
    let resolver = index.bind(&files);
    let self_type = candidate(&parsed.a.1, "S");
    let foreign = candidate(&parsed.a.1, "T");
    assert_eq!(
        resolver.resolve(&[self_type.clone(), foreign.clone()]),
        [
            Verdict::Unresolved {
                reason: UnresolvedReason::Unmapped
            },
            Verdict::Unresolved {
                reason: UnresolvedReason::ExternalDependency
            },
        ]
    );
}

#[test]
fn implementation_relationship_yields_implements_and_overrides() {
    let (_dir, index) = loaded();
    let parsed = parsed();
    let files = sources(&parsed);
    let resolver = index.bind(&files);
    assert_eq!(
        resolver.implementers(&node("src/b.rs::T", NodeKind::Trait)),
        [node("src/a.rs::S", NodeKind::Struct)]
    );
    assert_eq!(
        resolver.implementers(&node("src/b.rs::T::go", NodeKind::Method)),
        [node("src/a.rs::<S as T>::go", NodeKind::Method)]
    );
    assert_eq!(
        resolver.outgoing_calls(&node("src/a.rs::run", NodeKind::Function)),
        None
    );
}

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    std::fs::create_dir(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.rs"), A).unwrap();
    std::fs::write(dir.path().join("src/b.rs"), B).unwrap();
    std::fs::write(dir.path().join("src/c.rs"), "pub fn c() {}\n").unwrap();
    std::fs::write(dir.path().join(".gitignore"), "index.scip\n.codemycel/\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-q", "--no-gpg-sign", "-m", "one"]);
    dir
}

#[test]
fn coverage_is_transitive_over_changed_targets() {
    let dir = repo();
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let mut raw = index();
    raw.documents
        .push(document("src/c.rs", Vec::new(), Vec::new()));
    raw.documents
        .push(document("src/new.rs", Vec::new(), Vec::new()));
    let mut index = ScipImporter::load(&write_index(dir.path(), &raw)).unwrap();
    index.compute_coverage(dir.path(), &head);
    let covers = |path: &str| index.covers(&RepoPath::new(path).unwrap());
    assert!(covers("src/a.rs") && covers("src/b.rs") && covers("src/c.rs"));

    std::fs::write(
        dir.path().join("src/b.rs"),
        format!("{B}\npub fn later() {{}}\n"),
    )
    .unwrap();
    std::fs::write(dir.path().join("src/new.rs"), "pub fn new() {}\n").unwrap();
    index.compute_coverage(dir.path(), &head);
    let covers = |path: &str| index.covers(&RepoPath::new(path).unwrap());
    assert!(!covers("src/b.rs"), "changed itself");
    assert!(!covers("src/a.rs"), "points into a changed file");
    assert!(covers("src/c.rs"));
    assert!(!covers("src/new.rs"), "untracked since the commit");

    index.compute_coverage(dir.path(), "0000000000000000000000000000000000000000");
    assert!(!index.covers(&RepoPath::new("src/c.rs").unwrap()));
}

#[test]
fn commit_is_recorded_once_and_refreshed_only_when_index_hash_changes() {
    let dir = repo();
    let load_config = || {
        let home = dir.path().join(".home");
        Config::load_with_home(dir.path(), &home).unwrap()
    };
    let first_head = git(dir.path(), &["rev-parse", "HEAD"]);
    let path = write_index(dir.path(), &index());
    let first = ScipImporter::load(&path).unwrap();
    let recorded = first
        .commit_handling(dir.path(), &load_config(), None, None)
        .unwrap();
    assert_eq!(recorded, first_head);
    assert_eq!(
        load_config().scip_commit.as_deref(),
        Some(first_head.as_str())
    );

    std::fs::write(dir.path().join("src/c.rs"), "pub fn c2() {}\n").unwrap();
    git(dir.path(), &["commit", "-qam", "two", "--no-gpg-sign"]);
    let second_head = git(dir.path(), &["rev-parse", "HEAD"]);
    let again = first
        .commit_handling(dir.path(), &load_config(), Some(first.hash()), None)
        .unwrap();
    assert_eq!(
        again, first_head,
        "same index file, so the recorded commit stays"
    );

    let mut changed = index();
    changed.documents.pop();
    let second = ScipImporter::load(&write_index(dir.path(), &changed)).unwrap();
    assert_ne!(second.hash(), first.hash());
    let refreshed = second
        .commit_handling(dir.path(), &load_config(), Some(first.hash()), None)
        .unwrap();
    assert_eq!(refreshed, second_head);
    assert_eq!(
        load_config().scip_commit.as_deref(),
        Some(second_head.as_str())
    );

    let ignored = second
        .commit_handling(
            dir.path(),
            &load_config(),
            Some(second.hash()),
            Some("feedface"),
        )
        .unwrap();
    assert_eq!(
        ignored, second_head,
        "same index file, so the override is not recorded"
    );
    assert_eq!(
        load_config().scip_commit.as_deref(),
        Some(second_head.as_str())
    );

    let overridden = first
        .commit_handling(
            dir.path(),
            &load_config(),
            Some(second.hash()),
            Some("feedface"),
        )
        .unwrap();
    assert_eq!(overridden, "feedface");
    assert_eq!(load_config().scip_commit.as_deref(), Some("feedface"));
}

#[test]
fn discovery_uses_the_configured_path() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config::default();
    assert_eq!(ScipImporter::discover(dir.path(), &config), None);
    let path = write_index(dir.path(), &index());
    assert_eq!(ScipImporter::discover(dir.path(), &config), Some(path));
}
