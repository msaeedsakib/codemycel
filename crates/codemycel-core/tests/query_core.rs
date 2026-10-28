#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::query::{
    Answer, Budget, Freshness, JsonFormatter, Line, Outcome, QueryEngine, Section, SymbolCard,
    TextFormatter,
};
use codemycel_core::query::{Lookup, Resolved};
use codemycel_core::store::{NoteTarget, Store, StoreRead, Tx};
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Community, Coverage, Edge, EdgeKind, FileKind, FileRecord,
    LabelSource, Language, LanguageAdapter, Node, NodeKind, NodeRef, Note, NoteId, NoteKind,
    Occurrence, RepoPath, Root, RootStatus, SemanticPath, Span, Unresolved, UnresolvedReason,
    VerifiedBy,
};
use tempfile::TempDir;

const STORE_RS: &str = "crates/core/src/store.rs";
const MAIN_RS: &str = "crates/cli/src/main.rs";
const OTHER_RS: &str = "crates/cli/src/other.rs";
const OPEN: &str = "crates/core/src/store.rs::Store::open";

struct Repo {
    dir: TempDir,
}

impl Repo {
    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn db(&self) -> std::path::PathBuf {
        self.root().join(".codemycel/index.db")
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.root().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn engine<'a>(&self, adapters: &'a [Box<dyn LanguageAdapter>]) -> QueryEngine<'a> {
        QueryEngine::open(
            self.root(),
            Store::open(&self.db(), true).unwrap(),
            adapters,
        )
    }
}

fn sem(text: &str) -> SemanticPath {
    text.parse().unwrap()
}

fn repo_path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap()
}

fn node_ref(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(sem(path), kind)
}

fn node(path: &str, kind: NodeKind, lines: (u32, u32)) -> Node {
    let path = sem(path);
    Node {
        name: path.name().to_owned(),
        file: Some(repo_path(path.file().unwrap())),
        path,
        kind,
        span: Some(Span {
            bytes: ByteRange { start: 0, end: 1 },
            line_start: lines.0,
            line_end: lines.1,
        }),
        selection: None,
        signature: None,
        has_explicit_return_type: false,
        body_hash: None,
        doc_comment: None,
        visibility: None,
    }
}

fn calls(src: &str, dst: &str, line: u32) -> Edge {
    Edge {
        src: node_ref(src, NodeKind::Function),
        dst: if dst == OPEN {
            node_ref(dst, NodeKind::Method)
        } else {
            node_ref(dst, NodeKind::Function)
        },
        kind: EdgeKind::Calls,
        verified_by: VerifiedBy::Lsp,
        occurrences: vec![Occurrence { line, col: 4 }],
    }
}

fn file(tx: &mut Tx<'_>, repo: &Repo, path: &str, text: &str) {
    repo.write(path, text);
    let root = tx.roots().unwrap()[0].id;
    tx.upsert_file(&FileRecord {
        path: repo_path(path),
        root_id: Some(root),
        hash: blake3::hash(text.as_bytes()),
        size: text.len() as u64,
        language: Some(Language::Rust),
        kind: FileKind::Code,
        parsed: true,
        skip_reason: None,
        scip_covered: false,
    })
    .unwrap();
    let mut file_node = node(path, NodeKind::File, (1, 100));
    file_node.name = path.rsplit('/').next().unwrap().to_owned();
    tx.insert_nodes(&[file_node]).unwrap();
}

/// `Store::open` as in the card example: seven callers in `main.rs`, four callees in
/// `store.rs`, one unresolved and three external candidates, a community, and a note.
fn seeded() -> Repo {
    let repo = Repo {
        dir: tempfile::tempdir().unwrap(),
    };
    std::fs::create_dir_all(repo.root().join(".codemycel")).unwrap();
    let mut store = Store::create(&repo.db()).unwrap();
    store
        .transaction(|tx| {
            let root = tx
                .set_root(&Root {
                    path: repo_path(""),
                    language: Language::Rust,
                })
                .unwrap();
            tx.set_root_status(
                root,
                &RootStatus {
                    server: None,
                    coverage: Coverage::Lsp,
                    degraded: None,
                    config_diagnostics: Vec::new(),
                },
            )
            .unwrap();
            tx.meta_set("indexed_commit", "3f2a9c1e0b").unwrap();
            file(tx, &repo, STORE_RS, "pub struct Store;\n");
            file(tx, &repo, MAIN_RS, "fn main() {}\n");
            file(tx, &repo, OTHER_RS, "fn other() {}\n");
            let mut open = node(OPEN, NodeKind::Method, (42, 78));
            open.signature = Some("pub fn open(path: &Path) -> Result<Store>".to_owned());
            open.doc_comment =
                Some("/// Opens or creates the index database.\n/// More.".to_owned());
            open.visibility = Some("pub".to_owned());
            let mut nodes = vec![
                open,
                node("crates/core/src/store.rs::Store", NodeKind::Struct, (1, 3)),
                node("crates/cli/src/other.rs::open", NodeKind::Function, (1, 2)),
            ];
            for i in 0..7 {
                nodes.push(node(
                    &format!("{MAIN_RS}::caller{i}"),
                    NodeKind::Function,
                    (10 + i, 10 + i),
                ));
            }
            for i in 0..4 {
                nodes.push(node(
                    &format!("{STORE_RS}::helper{i}"),
                    NodeKind::Function,
                    (90 + i, 90 + i),
                ));
            }
            tx.insert_nodes(&nodes).unwrap();
            let mut edges: Vec<Edge> = (0..7)
                .map(|i| calls(&format!("{MAIN_RS}::caller{i}"), OPEN, 10 + i))
                .collect();
            for i in 0..4 {
                let mut edge = calls(OPEN, &format!("{STORE_RS}::helper{i}"), 50 + i);
                edge.src = node_ref(OPEN, NodeKind::Method);
                edges.push(edge);
            }
            tx.insert_edges(&edges).unwrap();
            let candidate = |text: &str, line| Candidate {
                file: repo_path(STORE_RS),
                enclosing: node_ref(OPEN, NodeKind::Method),
                text: text.to_owned(),
                role: CandidateRole::Call,
                at: Occurrence { line, col: 8 },
            };
            tx.insert_unresolved(&[
                Unresolved {
                    candidate: candidate("save", 60),
                    reason: UnresolvedReason::NoDefinition,
                },
                Unresolved {
                    candidate: candidate("Vec", 61),
                    reason: UnresolvedReason::ExternalDependency,
                },
                Unresolved {
                    candidate: candidate("Box", 62),
                    reason: UnresolvedReason::ExternalDependency,
                },
                Unresolved {
                    candidate: candidate("Arc", 63),
                    reason: UnresolvedReason::ExternalDependency,
                },
            ])
            .unwrap();
            tx.refresh_unresolved_counts().unwrap();
            tx.replace_communities(&[Community {
                stable_id: "a1b2c3d4".to_owned(),
                label: "storage".to_owned(),
                label_source: LabelSource::Folder,
                members: vec![node_ref(OPEN, NodeKind::Method)],
            }])
            .unwrap();
            let uuid = NoteId("0b6f2c1e-4a8d-4f3e-9a51-2c7d8e9f0a1b".to_owned());
            tx.upsert_note(&Note {
                uuid: uuid.clone(),
                target_semantic_path: OPEN.to_owned(),
                kind: NoteKind::Note,
                body: "Slow on NFS; see #212\nMeasured on a 2 GiB index.".to_owned(),
                created: "2026-10-19T09:00:00Z".to_owned(),
                updated: "2026-10-20T10:00:00Z".to_owned(),
            })
            .unwrap();
            tx.set_note_target(&uuid, NoteTarget::Node(&node_ref(OPEN, NodeKind::Method)))
                .unwrap();
            Ok::<_, codemycel_core::store::StoreError>(())
        })
        .unwrap();
    repo
}

fn no_adapters() -> Vec<Box<dyn LanguageAdapter>> {
    Vec::new()
}

/// The text of an answer without its last line, the freshness footer.
fn without_footer(text: &str) -> String {
    let mut lines: Vec<&str> = text.lines().collect();
    lines.pop();
    lines.iter().map(|line| format!("{line}\n")).collect()
}

fn card_answer(engine: &QueryEngine<'_>, path: &str, kind: NodeKind) -> Answer {
    let stored = engine
        .store()
        .node_by_semantic_path(&sem(path), Some(kind))
        .unwrap()
        .remove(0);
    let card = SymbolCard::for_node(engine.store(), &stored).unwrap();
    Answer {
        outcome: Outcome::Ok,
        sections: vec![Section::new("symbol", 0, vec![Line::Card(card)]).without_header()],
        truncated: None,
        freshness: Freshness::default(),
    }
}

fn options() -> SymbolOptions {
    SymbolOptions {
        include_body: false,
        callers: true,
        callees: true,
        limit: 25,
    }
}

#[test]
fn text_card_matches_spec_layout_exactly() {
    let repo = seeded();
    let adapters = no_adapters();
    let engine = repo.engine(&adapters);
    let answer = card_answer(&engine, OPEN, NodeKind::Method);
    assert_eq!(
        without_footer(&TextFormatter::render(&answer)),
        "crates/core/src/store.rs::Store::open (method) L42-78 pub
  sig  pub fn open(path: &Path) -> Result<Store>
  doc  Opens or creates the index database.
  in   derived:storage
  callers 7 · callees 4 · impls 0 · overrides 0 · unresolved 1 · external 3
  note \"Slow on NFS; see #212\" (2026-10-20)
"
    );
}

#[test]
fn empty_card_lines_are_omitted() {
    let repo = seeded();
    let adapters = no_adapters();
    let engine = repo.engine(&adapters);
    let answer = card_answer(&engine, "crates/core/src/store.rs::Store", NodeKind::Struct);
    assert_eq!(
        without_footer(&TextFormatter::render(&answer)),
        "crates/core/src/store.rs::Store (struct) L1-3\n"
    );
}

fn text_lines(prefix: &str, count: usize) -> Vec<Line> {
    (0..count)
        .map(|i| Line::text(format!("{prefix} line {i:02} with some padding text")))
        .collect()
}

fn two_section_answer() -> Answer {
    Answer {
        outcome: Outcome::Ok,
        sections: vec![
            Section::new("primary", 0, text_lines("primary", 10)),
            Section::new("secondary", 1, text_lines("secondary", 10)),
        ],
        truncated: None,
        freshness: Freshness::default(),
    }
}

#[test]
fn budget_cuts_secondary_before_primary_and_reports_counts() {
    let mut answer = two_section_answer();
    Budget::new(150).apply(&mut answer, "raise max_tokens");
    assert_eq!(answer.sections[0].lines.len(), 10, "primary kept whole");
    let shown = answer.sections[1].lines.len();
    assert!(shown < 10, "secondary cut");
    let truncated = answer.truncated.as_ref().unwrap();
    assert_eq!(truncated.sections.len(), 1);
    assert_eq!(truncated.sections[0].name, "secondary");
    assert_eq!(truncated.sections[0].shown, shown);
    assert_eq!(truncated.sections[0].total, 10);
    let text = TextFormatter::render(&answer);
    assert!(
        text.contains(&format!(
            "truncated: showing {shown} of 10 secondary · raise max_tokens"
        )),
        "{text}"
    );
}

#[test]
fn budget_never_exceeds_limit_by_more_than_footer() {
    for max_tokens in (40..=400).step_by(7) {
        let mut answer = two_section_answer();
        Budget::new(max_tokens).apply(&mut answer, "raise max_tokens");
        let text = TextFormatter::render(&answer);
        let kept: String = text
            .lines()
            .filter(|line| !line.starts_with("truncated: "))
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(
            Budget::tokens(&kept) <= max_tokens as usize,
            "{max_tokens}: {} tokens\n{text}",
            Budget::tokens(&kept)
        );
    }
}

#[test]
fn freshness_lists_only_changed_files_from_answer() {
    let repo = seeded();
    repo.write(MAIN_RS, "fn main() { edited(); }\n");
    repo.write(OTHER_RS, "fn other() { edited(); }\n");
    let adapters = no_adapters();
    let engine = repo.engine(&adapters);
    let answer = engine.symbol(OPEN, options(), Budget::new(4000)).unwrap();
    assert_eq!(answer.freshness.stale, vec![repo_path(MAIN_RS)]);
    assert_eq!(answer.freshness.changed_files, 1);
    let footer = TextFormatter::render(&answer)
        .lines()
        .last()
        .unwrap()
        .to_owned();
    assert_eq!(
        footer,
        format!(
            "index: 3f2a9c1 (1 files changed since) · coverage: rust lsp ✓ · stale in this answer: {MAIN_RS}"
        )
    );
}

#[test]
fn ambiguous_bare_name_returns_candidates_only() {
    let repo = seeded();
    let adapters = no_adapters();
    let engine = repo.engine(&adapters);
    let answer = engine.symbol("open", options(), Budget::new(4000)).unwrap();
    assert_eq!(answer.outcome, Outcome::Ambiguous);
    assert_eq!(answer.sections.len(), 1);
    assert_eq!(answer.sections[0].name, "candidates");
    assert_eq!(
        without_footer(&TextFormatter::render(&answer)),
        format!(
            "candidates:
  crates/cli/src/other.rs::open (function) {OTHER_RS}:1
  {OPEN} (method) {STORE_RS}:42  pub fn open(path: &Path) -> Result<Store>
"
        )
    );
}

#[test]
fn semantic_path_lookup_is_exact() {
    let repo = seeded();
    let store = Store::open(&repo.db(), true).unwrap();
    let one = |name: &str| match Lookup::resolve(&store, name).unwrap() {
        Resolved::One(node) => Some(node.node.path.to_string()),
        _ => None,
    };
    assert_eq!(one(OPEN).as_deref(), Some(OPEN));
    assert_eq!(one(&format!("{OPEN} (method)")).as_deref(), Some(OPEN));
    assert_eq!(one(&format!("{OPEN} (function)")), None);
    assert_eq!(one("crates/core/src/store.rs::Store::ope"), None);
    assert_eq!(
        one("Store").as_deref(),
        Some("crates/core/src/store.rs::Store")
    );
    assert_eq!(one("store"), None, "case-sensitive");
    assert_eq!(one("storage"), None, "communities are not bare names");
}

#[test]
fn json_and_text_carry_same_data() {
    let repo = seeded();
    let adapters = no_adapters();
    let engine = repo.engine(&adapters);
    let answer = engine.symbol(OPEN, options(), Budget::new(4000)).unwrap();
    let text = TextFormatter::render(&answer);
    let json = JsonFormatter::render(&answer);
    let card = &json["symbol"][0];
    assert_eq!(card["type"], "card");
    assert_eq!(card["path"], OPEN);
    assert_eq!(card["counts"]["callers"], 7);
    assert_eq!(card["community"]["label"], "storage");
    assert_eq!(card["community"]["derived"], true);
    let callers = json["callers"].as_array().unwrap();
    assert_eq!(callers.len(), text.matches("← calls").count());
    for caller in callers {
        assert_eq!(caller["kind"], "calls");
        assert!(text.contains(caller["node"]["path"].as_str().unwrap()));
    }
    assert_eq!(
        json["callees"].as_array().unwrap().len(),
        text.matches("→ calls").count()
    );
    assert_eq!(json["unresolved"].as_array().unwrap().len(), 4);
    assert_eq!(
        json["notes"][0]["body"],
        "Slow on NFS; see #212\nMeasured on a 2 GiB index."
    );
    assert_eq!(json["freshness"]["indexed_commit"], "3f2a9c1e0b");
    assert_eq!(json["outcome"], "ok");
    assert!(json["truncated"].is_null());
}
