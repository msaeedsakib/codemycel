#![allow(clippy::unwrap_used)]

mod harness;

use std::path::Path;

use codemycel_core::{
    AdapterError, ByteRange, Edge, EdgeKind, Language, LanguageAdapter, Node, NodeKind, NodeRef,
    ParsedFile, RepoPath, Root, SemanticPath, Span, VerifiedBy,
};
use codemycel_index::Registry;
use harness::{Dump, assert_matches_expected, fixture, index_fixture_with};

/// Claims `.names`, roots at the repository root, and reads each line as a function name.
struct Names;

impl LanguageAdapter for Names {
    fn language(&self) -> Language {
        Language::Rust
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["names"]
    }
    fn detect_roots(&self, _: &Path, _: &[RepoPath]) -> Result<Vec<Root>, AdapterError> {
        Ok(vec![Root {
            path: RepoPath::new("").unwrap(),
            language: Language::Rust,
        }])
    }
    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        let file_ref = NodeRef::new(SemanticPath::for_file(file).unwrap(), NodeKind::File);
        let mut parsed = ParsedFile::default();
        for (row, name) in std::str::from_utf8(bytes).unwrap().lines().enumerate() {
            let line = u32::try_from(row).unwrap() + 1;
            let path = SemanticPath::symbol(file, &[] as &[&str], name, None).unwrap();
            parsed.nodes.push(Node {
                path: path.clone(),
                kind: NodeKind::Function,
                name: name.to_owned(),
                file: Some(file.clone()),
                span: Some(Span {
                    bytes: ByteRange { start: 0, end: 1 },
                    line_start: line,
                    line_end: line,
                }),
                selection: None,
                signature: None,
                has_explicit_return_type: false,
                body_hash: None,
                doc_comment: None,
                visibility: None,
            });
            parsed.structural_edges.push(Edge {
                src: file_ref.clone(),
                dst: NodeRef::new(path, NodeKind::Function),
                kind: EdgeKind::Contains,
                verified_by: VerifiedBy::Syntactic,
                occurrences: Vec::new(),
            });
        }
        Ok(parsed)
    }
    fn external_path_predicate(&self, _: &Path, _: &Path) -> bool {
        false
    }
}

fn mentions(repo: &Path) -> Vec<String> {
    let index = index_fixture_with(repo, Registry::with_adapters(vec![Box::new(Names)]));
    index
        .dump(Dump::Edges)
        .lines()
        .filter(|line| line.contains("\tmentions\t"))
        .map(str::to_owned)
        .collect()
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn mention_requires_exactly_one_matching_name() {
    let repo = tempfile::tempdir().unwrap();
    write(repo.path(), "a.names", "unique\ntwice\n");
    write(repo.path(), "b.names", "twice\n");
    write(
        repo.path(),
        "README.md",
        "# Twice and once\n\n`unique` and `twice` and `Unique` and `absent`\n\n# Unique\n",
    );
    assert_eq!(
        mentions(repo.path()),
        ["README.md#twice-and-once (heading)\tmentions\ta.names::unique (function)\t1"]
    );
}

#[test]
fn mention_strips_trailing_parens_and_leading_dot() {
    let repo = tempfile::tempdir().unwrap();
    write(repo.path(), "a.names", "open\nclose\n");
    write(
        repo.path(),
        "notes.md",
        "`open()` then `.close` and `.open()` and `open`\n",
    );
    assert_eq!(
        mentions(repo.path()),
        [
            "notes.md (doc)\tmentions\ta.names::close (function)\t1",
            "notes.md (doc)\tmentions\ta.names::open (function)\t3",
        ]
    );
}

#[test]
fn relative_link_to_existing_file_is_a_mention() {
    let repo = tempfile::tempdir().unwrap();
    write(repo.path(), "src/a.names", "f\n");
    write(repo.path(), "src/notes.txt", "plain\n");
    write(
        repo.path(),
        "docs/guide.md",
        "# Code\n\n[a](../src/a.names) [again](../src/a.names#L1) [txt](../src/notes.txt) \
         [gone](../src/b.names)\n",
    );
    assert_eq!(
        mentions(repo.path()),
        ["docs/guide.md#code (heading)\tmentions\tsrc/a.names (file)\t2"]
    );
}

#[test]
fn links_to_documents_mention_them_in_any_order() {
    let repo = tempfile::tempdir().unwrap();
    write(repo.path(), "a.md", "See [z](z.md) and [the end][end].\n\n[END]: ./z.md\n");
    write(repo.path(), "z.md", "# End\n\nBack to [a](a.md#top).\n");
    assert_eq!(
        mentions(repo.path()),
        [
            "a.md (doc)\tmentions\tz.md (doc)\t2",
            "z.md#end (heading)\tmentions\ta.md (doc)\t1",
        ]
    );
}

#[test]
fn docs_fixture_matches_expected() {
    assert_matches_expected(&fixture("docs/mixed"));
}
