#![allow(clippy::unwrap_used)]

mod harness;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use codemycel_core::query::{Answer, Budget, JsonFormatter, QueryEngine, TextFormatter};
use codemycel_core::store::{Store, StoreRead, Tx};
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Coverage, DegradeReason, Edge, EdgeKind, FileKind,
    FileRecord, Language, LanguageAdapter, Node, NodeKind, NodeRef, Occurrence, RepoPath, Root,
    RootStatus, SemanticPath, ServerInfo, ServerSource, SkipReason, Span, SyncOwner, Unresolved,
    UnresolvedReason, VerifiedBy,
};
use codemycel_index::Registry;
use harness::{FixtureIndex, fixture, fixture_copy, index_fixture_full, local_provisioner};
use tempfile::TempDir;

const NO_ADAPTERS: &[Box<dyn LanguageAdapter>] = &[];

/// The Rust fixture indexed once with rust-analyzer; `None` when it is not installed.
fn verified() -> Option<&'static Mutex<(TempDir, PathBuf, FixtureIndex)>> {
    static INDEX: OnceLock<Option<Mutex<(TempDir, PathBuf, FixtureIndex)>>> = OnceLock::new();
    INDEX
        .get_or_init(|| {
            let registry = Registry::default();
            let provisioner = local_provisioner(registry.server_for(Language::Rust).unwrap())?;
            let (temp, copy) = fixture_copy(&fixture("rust/workspace-basic"));
            let index = index_fixture_full(&copy, registry, &provisioner);
            Some(Mutex::new((temp, copy, index)))
        })
        .as_ref()
}

/// Runs `query` on the verified Rust fixture and renders its text; `None` without rust-analyzer.
fn on_fixture(query: impl FnOnce(&QueryEngine<'_>) -> Answer) -> Option<String> {
    let index = verified()?.lock().unwrap();
    let (_, copy, index) = &*index;
    let registry = Registry::default();
    let engine = index.engine(copy, registry.adapters());
    Some(TextFormatter::render(&query(&engine)))
}

/// The lines of `section:` in `text`, without the header.
fn section<'a>(text: &'a str, header: &str) -> Vec<&'a str> {
    text.lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect()
}

/// An index written directly through `Tx` at `<repo>/.codemycel/index.db`, with one Rust root.
struct Synthetic {
    dir: TempDir,
}

impl Synthetic {
    fn new(build: impl FnOnce(&mut Tx<'_>, &Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".codemycel")).unwrap();
        let mut store = Store::create(&dir.path().join(".codemycel/index.db")).unwrap();
        store
            .transaction(|tx| {
                tx.set_root(&Root {
                    path: repo_path(""),
                    language: Language::Rust,
                })?;
                build(tx, dir.path());
                Ok::<_, codemycel_core::store::StoreError>(())
            })
            .unwrap();
        Self { dir }
    }

    fn engine(&self) -> QueryEngine<'static> {
        let store = Store::open(&self.dir.path().join(".codemycel/index.db"), true).unwrap();
        QueryEngine::open(self.dir.path(), store, NO_ADAPTERS)
    }
}

fn repo_path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap()
}

fn node_ref(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(path.parse().unwrap(), kind)
}

/// A code file on disk and in the index, with its file node.
fn code_file(tx: &mut Tx<'_>, repo: &Path, path: &str) {
    let text = format!("// {path}\n");
    let on_disk = repo.join(path);
    std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
    std::fs::write(on_disk, &text).unwrap();
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
    let mut file = symbol(path, NodeKind::File, 1);
    file.name = path.rsplit('/').next().unwrap().to_owned();
    tx.insert_nodes(&[file]).unwrap();
}

fn symbol(path: &str, kind: NodeKind, line: u32) -> Node {
    let path: SemanticPath = path.parse().unwrap();
    Node {
        name: path.name().to_owned(),
        file: Some(repo_path(path.file().unwrap())),
        path,
        kind,
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
    }
}

fn edge(src: (&str, NodeKind), dst: (&str, NodeKind), kind: EdgeKind, line: u32) -> Edge {
    Edge {
        src: node_ref(src.0, src.1),
        dst: node_ref(dst.0, dst.1),
        kind,
        verified_by: VerifiedBy::Lsp,
        occurrences: vec![Occurrence { line, col: 4 }],
    }
}

fn candidate(file: &str, enclosing: &str, text: &str, reason: UnresolvedReason) -> Unresolved {
    Unresolved {
        candidate: Candidate {
            file: repo_path(file),
            enclosing: node_ref(enclosing, NodeKind::Function),
            text: text.to_owned(),
            role: CandidateRole::Call,
            at: Occurrence { line: 3, col: 8 },
        },
        reason,
    }
}

const F: NodeKind = NodeKind::Function;
const M: NodeKind = NodeKind::Method;

#[test]
fn impact_groups_by_depth_then_file() {
    let Some(text) = on_fixture(|engine| {
        engine
            .impact(
                "crates/core/src/geometry.rs::Point",
                2,
                &[],
                Budget::new(6000),
            )
            .unwrap()
    }) else {
        return;
    };
    assert_eq!(
        text,
        "crates/core/src/geometry.rs::Point (struct) crates/core/src/geometry.rs:6  pub struct Point
depth 1  3 nodes
depth 2  1 node
total  4 distinct nodes within depth 2
possible additional dependents  0 unresolved candidates named `Point` in 0 files
depth 1:
  ← references  crates/cli/src/main.rs::main (function) L24 [L25]
  ← references  crates/core/src/circle.rs::Circle::new (method) L4 [L4]
  ← references  crates/core/src/geometry.rs::Circle::center (field) L12 [L12]
depth 2:
  ← calls  crates/core/src/circle.rs::Circle::scaled (method) L8 [L10]
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

/// `leaf` overrides `parent`, which overrides `grand`; each has one caller.
fn override_chain() -> Synthetic {
    const FILE: &str = "src/lib.rs";
    Synthetic::new(|tx, repo| {
        code_file(tx, repo, FILE);
        tx.insert_nodes(&[
            symbol("src/lib.rs::grand", M, 1),
            symbol("src/lib.rs::parent", M, 2),
            symbol("src/lib.rs::leaf", M, 3),
            symbol("src/lib.rs::calls_grand", F, 4),
            symbol("src/lib.rs::calls_parent", F, 5),
            symbol("src/lib.rs::calls_leaf", F, 6),
            symbol("src/lib.rs::outer", F, 7),
        ])
        .unwrap();
        tx.insert_edges(&[
            edge(
                ("src/lib.rs::parent", M),
                ("src/lib.rs::grand", M),
                EdgeKind::Overrides,
                2,
            ),
            edge(
                ("src/lib.rs::leaf", M),
                ("src/lib.rs::parent", M),
                EdgeKind::Overrides,
                3,
            ),
            edge(
                ("src/lib.rs::calls_grand", F),
                ("src/lib.rs::grand", M),
                EdgeKind::Calls,
                4,
            ),
            edge(
                ("src/lib.rs::calls_parent", F),
                ("src/lib.rs::parent", M),
                EdgeKind::Calls,
                5,
            ),
            edge(
                ("src/lib.rs::calls_leaf", F),
                ("src/lib.rs::leaf", M),
                EdgeKind::Calls,
                6,
            ),
            edge(
                ("src/lib.rs::outer", F),
                ("src/lib.rs::calls_parent", F),
                EdgeKind::Calls,
                7,
            ),
        ])
        .unwrap();
    })
}

#[test]
fn impact_includes_via_callers_one_step_only() {
    let repo = override_chain();
    let answer = repo
        .engine()
        .impact("src/lib.rs::leaf", 3, &[], Budget::new(6000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    assert_eq!(
        section(&text, "depth 1:"),
        [
            "  ← calls  src/lib.rs::calls_leaf (function) L6 [L6]",
            "  ← calls  src/lib.rs::calls_parent (function) L5 [L5] via src/lib.rs::parent",
        ],
        "{text}"
    );
    assert_eq!(
        section(&text, "depth 2:"),
        ["  ← calls  src/lib.rs::outer (function) L7 [L7]"],
        "{text}"
    );
    assert!(!text.contains("calls_grand"), "{text}");
}

#[test]
fn impact_via_expansion_skips_sibling_implementations() {
    let repo = Synthetic::new(|tx, repo| {
        code_file(tx, repo, "src/lib.rs");
        tx.insert_nodes(&[
            symbol("src/lib.rs::Trait::run", M, 1),
            symbol("src/lib.rs::A::run", M, 2),
            symbol("src/lib.rs::B::run", M, 3),
            symbol("src/lib.rs::caller", F, 4),
        ])
        .unwrap();
        tx.insert_edges(&[
            edge(
                ("src/lib.rs::A::run", M),
                ("src/lib.rs::Trait::run", M),
                EdgeKind::Overrides,
                2,
            ),
            edge(
                ("src/lib.rs::B::run", M),
                ("src/lib.rs::Trait::run", M),
                EdgeKind::Overrides,
                3,
            ),
            edge(
                ("src/lib.rs::caller", F),
                ("src/lib.rs::Trait::run", M),
                EdgeKind::Calls,
                4,
            ),
        ])
        .unwrap();
    });
    let answer = repo
        .engine()
        .impact("src/lib.rs::A::run", 3, &[], Budget::new(6000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    assert_eq!(
        section(&text, "depth 1:"),
        ["  ← calls  src/lib.rs::caller (function) L4 [L4] via src/lib.rs::Trait::run"],
        "{text}"
    );
    assert!(!text.contains("B::run"), "{text}");
    assert!(!text.contains("depth 2:"), "{text}");
}

#[test]
fn impact_on_hub_truncates_with_footer_within_budget() {
    const DEPENDENTS: usize = 5000;
    let repo = Synthetic::new(|tx, repo| {
        code_file(tx, repo, "src/hub.rs");
        let mut nodes = vec![symbol("src/hub.rs::hub", F, 1)];
        let mut edges = Vec::new();
        for i in 0..DEPENDENTS {
            let file = format!("src/callers{}.rs", i % 10);
            if i < 10 {
                code_file(tx, repo, &file);
            }
            let caller = format!("{file}::caller{i:04}");
            nodes.push(symbol(&caller, F, 2 + i as u32));
            edges.push(edge(
                (&caller, F),
                ("src/hub.rs::hub", F),
                EdgeKind::Calls,
                3 + i as u32,
            ));
        }
        tx.insert_nodes(&nodes).unwrap();
        tx.insert_edges(&edges).unwrap();
    });
    let max_tokens = 6000;
    let answer = repo
        .engine()
        .impact("src/hub.rs::hub", 2, &[], Budget::new(max_tokens))
        .unwrap();
    let text = TextFormatter::render(&answer);
    let footer: Vec<&str> = text
        .lines()
        .filter(|line| line.starts_with("truncated: "))
        .collect();
    assert_eq!(footer.len(), 1, "{text}");
    assert!(
        footer[0].starts_with("truncated: showing ")
            && footer[0].ends_with(" of 5000 depth 1 · raise max_tokens or narrow depth"),
        "{}",
        footer[0]
    );
    let without_footer = text.replacen(&format!("{}\n", footer[0]), "", 1);
    assert!(
        Budget::tokens(&without_footer) <= max_tokens as usize,
        "{} tokens",
        Budget::tokens(&without_footer)
    );
    assert!(text.contains("\ndepth 1  5000 nodes\n"), "{text}");
}

#[test]
fn impact_reports_possible_additional_dependents() {
    let repo = Synthetic::new(|tx, repo| {
        for file in ["src/a.rs", "src/b.rs", "src/c.rs", "src/store.rs"] {
            code_file(tx, repo, file);
        }
        tx.insert_nodes(&[
            symbol("src/store.rs::save", F, 1),
            symbol("src/store.rs::caller", F, 2),
            symbol("src/a.rs::one", F, 1),
            symbol("src/a.rs::two", F, 2),
            symbol("src/b.rs::three", F, 1),
            symbol("src/c.rs::external", F, 1),
        ])
        .unwrap();
        tx.insert_edges(&[edge(
            ("src/store.rs::caller", F),
            ("src/store.rs::save", F),
            EdgeKind::Calls,
            2,
        )])
        .unwrap();
        tx.insert_unresolved(&[
            candidate(
                "src/a.rs",
                "src/a.rs::one",
                "save",
                UnresolvedReason::NoDefinition,
            ),
            candidate(
                "src/a.rs",
                "src/a.rs::two",
                "save",
                UnresolvedReason::ServerUnavailable,
            ),
            candidate(
                "src/b.rs",
                "src/b.rs::three",
                "save",
                UnresolvedReason::Unmapped,
            ),
            candidate(
                "src/c.rs",
                "src/c.rs::external",
                "save",
                UnresolvedReason::ExternalDependency,
            ),
            candidate(
                "src/store.rs",
                "src/store.rs::caller",
                "save",
                UnresolvedReason::NoDefinition,
            ),
            candidate(
                "src/b.rs",
                "src/b.rs::three",
                "load",
                UnresolvedReason::NoDefinition,
            ),
        ])
        .unwrap();
    });
    let answer = repo
        .engine()
        .impact("save", 2, &[], Budget::new(6000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    assert!(
        text.contains(
            "\npossible additional dependents  3 unresolved candidates named `save` in 2 files\n"
        ),
        "{text}"
    );
}

#[test]
fn trace_finds_shortest_path_and_reports_nearest_when_none() {
    let Some(found) = on_fixture(|engine| {
        engine
            .trace(
                "crates/cli/src/main.rs::main",
                "crates/core/src/geometry.rs::Area::area",
                &[],
                8,
                Budget::new(2000),
            )
            .unwrap()
    }) else {
        return;
    };
    assert_eq!(
        found,
        "path:
  crates/cli/src/main.rs::main (function) crates/cli/src/main.rs:24  fn main()
  → calls  crates/core/src/geometry.rs::Area::describe (method) L27 [L26]
  → calls  crates/core/src/geometry.rs::Area::area (method) L23 [L28]
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
    let none = on_fixture(|engine| {
        engine
            .trace(
                "crates/cli/src/main.rs::rebinds",
                "crates/cli/src/main.rs::shadowed",
                &[],
                8,
                Budget::new(2000),
            )
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        none,
        "no path within depth 8
nearest:
  crates/cli/src/main.rs::config (function) crates/cli/src/main.rs:6  fn config() -> usize
  crates/core/src/geometry.rs::Shape::Empty (enum_member) crates/core/src/geometry.rs:19  Empty
  crates/core/src/geometry.rs::sides (function) crates/core/src/geometry.rs:54  pub fn sides(shape: &Shape) -> u32
  crates/core/src/geometry.rs::Shape (enum) crates/core/src/geometry.rs:16  pub enum Shape
  crates/core/src/geometry.rs::Shape::Round (enum_member) crates/core/src/geometry.rs:17  Round
  crates/core/src/geometry.rs::Shape::Square (enum_member) crates/core/src/geometry.rs:18  Square
  crates/core/src/lib.rs::MAX_SHAPES (constant) crates/core/src/lib.rs:11  pub const MAX_SHAPES: usize
  crates/core/src/geometry.rs::Circle (struct) crates/core/src/geometry.rs:11  pub struct Circle
  crates/core/src/geometry.rs::Area (trait) crates/core/src/geometry.rs:22  pub trait Area
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

#[test]
fn overview_repo_lists_packages_roots_communities_god_nodes() {
    let Some(text) = on_fixture(|engine| engine.overview(None, Budget::new(4000)).unwrap()) else {
        return;
    };
    assert_eq!(
        text,
        "roots:
  . (rust)  lsp ✓
packages:
  fixture-cli 0.1.0  crates/cli/Cargo.toml
  fixture-core 0.1.0  crates/core/Cargo.toml
  and 1 dependency outside the repository
counts:
  files  11 · 7 parsed · 1 skipped
  nodes  83
  edges  contains 66 · defines 6 · imports 5 · exports 5 · calls 21 · references 25 · extends 2 · implements 2 · overrides 4 · depends_on 3 · member_of 34
  unresolved  external_dependency 51 · unmapped 1
god_nodes:
  crates/core/src/geometry.rs::Area (trait) crates/core/src/geometry.rs:22  pub trait Area
  crates/core/src/geometry.rs::Circle (struct) crates/core/src/geometry.rs:11  pub struct Circle
  crates/core/src/geometry.rs::Area::area (method) crates/core/src/geometry.rs:23  fn area(&self) -> f64
  crates/core/src/geometry.rs::Point (struct) crates/core/src/geometry.rs:6  pub struct Point
  crates/core/src/geometry.rs::Shape::Empty (enum_member) crates/core/src/geometry.rs:19  Empty
  crates/core/src/geometry.rs::sides (function) crates/core/src/geometry.rs:54  pub fn sides(shape: &Shape) -> u32
  crates/cli/src/main.rs::config (function) crates/cli/src/main.rs:6  fn config() -> usize
  crates/core/src/circle.rs::Circle::new (method) crates/core/src/circle.rs:4  pub fn new(center: Point, radius: f64) -> Self
  crates/core/src/geometry.rs::Area::describe (method) crates/core/src/geometry.rs:27  fn describe(&self) -> String
  crates/core/src/geometry.rs::Area::name (method) crates/core/src/geometry.rs:25  fn name(&self) -> String
communities:
  derived:core (community:7406a03c)  7 members · top: Area::area, Area::describe, Area::name
  derived:cluster-88cc1e15 (community:88cc1e15)  6 members · top: sides, config, rebinds
  derived:core (community:074a7a48)  6 members · top: Wrapper, shadowed, Wrapper<T>::get
  derived:core (community:1b29ae35)  4 members · top: Circle, Point, Circle::new
  derived:core (community:ba7f892d)  4 members · top: Area, total_area, Render
  derived:core (community:6a326ce4)  3 members · top: Store::load, <MemoryStore as Store>::load, Store::exists
  derived:cluster-d2f236e4 (community:d2f236e4)  2 members · top: MemoryStore, Store
  derived:core (community:d23a0031)  2 members · top: layout::grid::columns, layout::total_columns
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

#[test]
fn overview_file_lists_symbols_with_counts_imports_importers() {
    let Some(geometry) = on_fixture(|engine| {
        engine
            .overview(Some("crates/core/src/geometry.rs"), Budget::new(4000))
            .unwrap()
    }) else {
        return;
    };
    assert_eq!(
        geometry,
        "file  crates/core/src/geometry.rs
unresolved  0
external  16
symbols:
  crates/core/src/geometry.rs::Point (struct) L6-9 pub
    sig  pub struct Point
    doc  A point on the plane.
    in   derived:core
  crates/core/src/geometry.rs::Point::x (field) L7 pub
    sig  pub x: f64
  crates/core/src/geometry.rs::Point::y (field) L8 pub
    sig  pub y: f64
  crates/core/src/geometry.rs::Circle (struct) L11-14 pub
    sig  pub struct Circle
    in   derived:core
    callers 0 · callees 0 · impls 1 · overrides 0 · unresolved 0 · external 0
  crates/core/src/geometry.rs::Circle::center (field) L12 pub
    sig  pub center: Point
  crates/core/src/geometry.rs::Circle::radius (field) L13 pub(crate)
    sig  pub(crate) radius: f64
  crates/core/src/geometry.rs::Shape (enum) L16-20 pub
    sig  pub enum Shape
    in   derived:cluster-88cc1e15
  crates/core/src/geometry.rs::Shape::Round (enum_member) L17
    sig  Round
  crates/core/src/geometry.rs::Shape::Square (enum_member) L18
    sig  Square
  crates/core/src/geometry.rs::Shape::Square::side (field) L18
    sig  side: f64
  crates/core/src/geometry.rs::Shape::Empty (enum_member) L19
    sig  Empty
  crates/core/src/geometry.rs::Area (trait) L22-30 pub
    sig  pub trait Area
    in   derived:core
    callers 0 · callees 0 · impls 1 · overrides 0 · unresolved 0 · external 0
  crates/core/src/geometry.rs::Area::area (method) L23
    sig  fn area(&self) -> f64
    in   derived:core
    callers 2 · callees 0 · impls 0 · overrides 1 · unresolved 0 · external 0
  crates/core/src/geometry.rs::Area::name (method) L25
    sig  fn name(&self) -> String
    in   derived:core
    callers 1 · callees 0 · impls 0 · overrides 1 · unresolved 0 · external 1
  crates/core/src/geometry.rs::Area::describe (method) L27-29
    sig  fn describe(&self) -> String
    in   derived:core
    callers 2 · callees 2 · impls 0 · overrides 0 · unresolved 0 · external 1
  crates/core/src/geometry.rs::<Circle as fmt::Display>::fmt (method) L33-35
    sig  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    callers 0 · callees 0 · impls 0 · overrides 0 · unresolved 0 · external 2
  crates/core/src/geometry.rs::<Circle as fmt::Debug>::fmt (method) L39-41
    sig  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    callers 0 · callees 0 · impls 0 · overrides 0 · unresolved 0 · external 5
  crates/core/src/geometry.rs::<Circle as Area>::area (method) L45-47
    sig  fn area(&self) -> f64
    in   derived:core
    callers 0 · callees 0 · impls 0 · overrides 1 · unresolved 0 · external 1
  crates/core/src/geometry.rs::<Circle as Area>::name (method) L49-51
    sig  fn name(&self) -> String
    in   derived:core
    callers 0 · callees 0 · impls 0 · overrides 1 · unresolved 0 · external 2
  crates/core/src/geometry.rs::sides (function) L54-60 pub
    sig  pub fn sides(shape: &Shape) -> u32
    in   derived:cluster-88cc1e15
    callers 3 · callees 0 · impls 0 · overrides 0 · unresolved 0 · external 0
  crates/core/src/geometry.rs::Bits (struct) L63-66 pub
    sig  pub union Bits
  crates/core/src/geometry.rs::Bits::int (field) L64 pub
    sig  pub int: u32
  crates/core/src/geometry.rs::Bits::float (field) L65 pub
    sig  pub float: f32
  crates/core/src/geometry.rs::tests (module) L69-76
    sig  mod tests
  crates/core/src/geometry.rs::tests::squares_have_four_sides (function) L73-75
    sig  fn squares_have_four_sides()
    in   derived:cluster-88cc1e15
    callers 0 · callees 1 · impls 0 · overrides 0 · unresolved 0 · external 0
importers:
  ← imports  crates/cli/src/main.rs (file) L1 [L3, L4]
  ← imports  crates/core/src/circle.rs (file) L1 [L1]
  ← imports  crates/core/src/render.rs (file) L1 [L1]
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
    let circle = on_fixture(|engine| {
        engine
            .overview(Some("crates/core/src/circle.rs"), Budget::new(4000))
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        circle,
        "file  crates/core/src/circle.rs
unresolved  1
external  0
symbols:
  crates/core/src/circle.rs::Circle::new (method) L4-6 pub
    sig  pub fn new(center: Point, radius: f64) -> Self
    in   derived:core
    callers 2 · callees 0 · impls 0 · overrides 0 · unresolved 0 · external 0
  crates/core/src/circle.rs::Circle::scaled (method) L8-11 pub
    sig  pub fn scaled(&self, factor: f64) -> Circle
    in   derived:core
    callers 0 · callees 1 · impls 0 · overrides 0 · unresolved 1 · external 0
imports:
  → imports  crates/core/src/geometry.rs (file) L1 [L1]
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

#[test]
fn overview_directory_and_community_scopes() {
    let Some(directory) = on_fixture(|engine| {
        engine
            .overview(Some("crates/cli/"), Budget::new(4000))
            .unwrap()
    }) else {
        return;
    };
    assert_eq!(
        directory,
        "directory  crates/cli
files:
  crates/cli/Cargo.toml  0 symbols · unresolved 0 · external 0
  crates/cli/src/main.rs  6 symbols · unresolved 0 · external 6
  crates/cli/src/memory.rs  3 symbols · unresolved 0 · external 9
symbols:
  crates/cli/src/main.rs::config (function) crates/cli/src/main.rs:6  fn config() -> usize
  crates/cli/src/main.rs::rebinds (function) crates/cli/src/main.rs:19  fn rebinds() -> usize
  crates/cli/src/main.rs::shadowed (function) crates/cli/src/main.rs:10  fn shadowed() -> usize
  crates/cli/src/memory.rs::MemoryStore (struct) crates/cli/src/memory.rs:7  pub struct MemoryStore
  crates/cli/src/memory.rs::<MemoryStore as Store>::load (method) crates/cli/src/memory.rs:13  async fn load(&self, key: &str) -> Option<String>
  crates/cli/src/memory.rs::MemoryStore::entries (field) crates/cli/src/memory.rs:8  entries: HashMap<String, String>
communities:
  derived:cluster-88cc1e15 (community:88cc1e15)  3 members here
  derived:core (community:074a7a48)  2 members here
  derived:cluster-d2f236e4 (community:d2f236e4)  1 member here
  derived:core (community:6a326ce4)  1 member here
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
    let package = on_fixture(|engine| {
        engine
            .overview(Some("fixture-cli"), Budget::new(4000))
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        package,
        directory
            .replace(
                "directory  crates/cli",
                "package  fixture-cli (crates/cli/Cargo.toml)"
            )
            .replace(
                "  crates/cli/Cargo.toml  0 symbols · unresolved 0 · external 0\n",
                ""
            )
    );
    let community = on_fixture(|engine| {
        engine
            .overview(Some("community:1b29ae35"), Budget::new(4000))
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        community,
        "community  derived:core (community:1b29ae35)
size  4 members
internal  4 edges
inbound:
  no community  4 edges
  derived:core (community:074a7a48)  2 edges
outbound:
  derived:core (community:ba7f892d)  1 edge
members:
  crates/core/src/circle.rs::Circle::new (method) crates/core/src/circle.rs:4  pub fn new(center: Point, radius: f64) -> Self
  crates/core/src/circle.rs::Circle::scaled (method) crates/core/src/circle.rs:8  pub fn scaled(&self, factor: f64) -> Circle
  crates/core/src/geometry.rs::Point (struct) crates/core/src/geometry.rs:6  pub struct Point
  crates/core/src/geometry.rs::Circle (struct) crates/core/src/geometry.rs:11  pub struct Circle
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

#[test]
fn overview_json_marks_every_community_derived() {
    let Some(index) = verified() else {
        return;
    };
    let index = index.lock().unwrap();
    let (_, copy, index) = &*index;
    let registry = Registry::default();
    let engine = index.engine(copy, registry.adapters());
    let render = |scope: Option<&str>| {
        JsonFormatter::render(&engine.overview(scope, Budget::new(4000)).unwrap())
    };
    let communities = |json: &serde_json::Value, section: &str| -> Vec<serde_json::Value> {
        json[section]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["type"] == "community")
            .cloned()
            .collect()
    };
    let repo = render(None);
    let directory = render(Some("crates/cli/"));
    let community = render(Some("community:1b29ae35"));
    for (json, section, count) in [
        (&repo, "communities", 8),
        (&directory, "communities", 4),
        (&community, "overview", 1),
        (&community, "inbound", 1),
        (&community, "outbound", 1),
    ] {
        let items = communities(json, section);
        assert_eq!(items.len(), count, "{section}: {json}");
        for item in items {
            assert_eq!(item["derived"], true, "{item}");
            assert!(
                item["label"].is_string() && item["path"].is_string(),
                "{item}"
            );
        }
    }
    assert_eq!(repo["communities"].as_array().unwrap().len(), 8);
}

#[test]
fn unresolved_repo_summary_by_reason_and_root() {
    let Some(summary) =
        on_fixture(|engine| engine.unresolved(None, None, Budget::new(3000)).unwrap())
    else {
        return;
    };
    assert_eq!(
        summary,
        "reasons:
  external_dependency  51
  unmapped  1
roots:
  . (rust)  52 · external_dependency 51 · unmapped 1
files:
  crates/core/src/geometry.rs  16
  crates/core/src/lib.rs  13
  crates/cli/src/memory.rs  9
  crates/cli/src/main.rs  6
  crates/core/src/store.rs  4
  crates/core/src/render.rs  3
  crates/core/src/circle.rs  1
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
    let in_file = on_fixture(|engine| {
        engine
            .unresolved(Some("crates/cli/src/main.rs"), None, Budget::new(3000))
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        in_file,
        "unresolved:
  L27:16 type  Vec  (external_dependency)
  L27:20 type  Box  (external_dependency)
  L27:47 call  new  (external_dependency)
  L30:20 call  iter  (external_dependency)
  L30:27 call  fold  (external_dependency)
  L33:28 call  parse  (external_dependency)
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
    let unmapped = on_fixture(|engine| {
        engine
            .unresolved(None, Some(UnresolvedReason::Unmapped), Budget::new(3000))
            .unwrap()
    })
    .unwrap();
    assert_eq!(
        unmapped,
        "reasons:
  unmapped  1
roots:
  . (rust)  1 · unmapped 1
files:
  crates/core/src/circle.rs  1
index: no git (0 files changed since) · coverage: rust lsp ✓
"
    );
}

#[test]
fn notes_snapshot_on_rust_fixture() {
    let Some(text) = on_fixture(|engine| engine.notes(None, false, Budget::new(2000)).unwrap())
    else {
        return;
    };
    assert_eq!(
        text,
        "no notes\nindex: no git (0 files changed since) · coverage: rust lsp ✓\n"
    );
}

#[test]
fn status_snapshot_on_rust_fixture() {
    let Some(text) = on_fixture(|engine| engine.status(None, None, Budget::new(1000)).unwrap())
    else {
        return;
    };
    let masked: Vec<String> = text
        .lines()
        .map(|line| {
            if let Some(state) = line.strip_prefix("  . (rust)  ") {
                return format!("  . (rust)  …{}", &state[state.rfind(" · ").unwrap()..]);
            }
            match line.split_once("  ") {
                Some((key @ ("indexed" | "database"), _)) => format!("{key}  …"),
                _ => line.to_owned(),
            }
        })
        .collect();
    assert_eq!(
        masked.join("\n"),
        "commit  no git
indexed  …
changed  0 files since indexing
session  no session is watching
sync  idle
database  …
roots:
  . (rust)  … · lsp ✓
counts:
  files  11 · 7 parsed · 1 skipped
  nodes  83
  edges  contains 66 · defines 6 · imports 5 · exports 5 · calls 21 · references 25 · extends 2 · implements 2 · overrides 4 · depends_on 3 · member_of 34
  unresolved  external_dependency 51 · unmapped 1
skipped:
  index.scip  binary
index: no git (0 files changed since) · coverage: rust lsp ✓"
    );
}

/// Two roots, one degraded with a configuration diagnostic, a partially enriched file, and
/// three skipped files of which two are binary or oversize.
fn status_repo() -> Synthetic {
    Synthetic::new(|tx, repo| {
        let rust = tx.roots().unwrap()[0].id;
        tx.set_root_status(
            rust,
            &RootStatus {
                server: Some(ServerInfo {
                    name: "rust-analyzer".to_owned(),
                    version: "1.91.0".to_owned(),
                    source: ServerSource::Toolchain,
                }),
                coverage: Coverage::Lsp,
                degraded: None,
                config_diagnostics: Vec::new(),
            },
        )
        .unwrap();
        let web = tx
            .set_root(&Root {
                path: repo_path("web"),
                language: Language::TypeScript,
            })
            .unwrap();
        tx.set_root_status(
            web,
            &RootStatus {
                server: None,
                coverage: Coverage::None,
                degraded: Some(DegradeReason::NoServer),
                config_diagnostics: vec!["tsconfig.json: unknown option `strct`".to_owned()],
            },
        )
        .unwrap();
        tx.meta_set("indexed_commit", "3f2a9c1e0b").unwrap();
        tx.meta_set("indexed_at", "2026-10-28T09:00:00Z").unwrap();
        code_file(tx, repo, "src/lib.rs");
        code_file(tx, repo, "src/main.rs");
        tx.set_partially_enriched(&repo_path("src/main.rs"))
            .unwrap();
        for (path, reason) in [
            ("assets/logo.png", SkipReason::Binary),
            ("data/huge.json", SkipReason::Oversize),
            ("vendor/skip.rs", SkipReason::Ignored),
        ] {
            let on_disk = repo.join(path);
            std::fs::create_dir_all(on_disk.parent().unwrap()).unwrap();
            std::fs::write(on_disk, path).unwrap();
            tx.upsert_file(&FileRecord {
                path: repo_path(path),
                root_id: None,
                hash: blake3::hash(path.as_bytes()),
                size: 1,
                language: None,
                kind: FileKind::Other,
                parsed: false,
                skip_reason: Some(reason),
                scip_covered: false,
            })
            .unwrap();
        }
    })
}

#[test]
fn status_fields_match_spec() {
    let repo = status_repo();
    std::fs::write(repo.dir.path().join("src/lib.rs"), "// edited\n").unwrap();
    let other = |pid| SyncOwner {
        pid,
        since: SystemTime::UNIX_EPOCH,
    };
    let answer = repo
        .engine()
        .status(Some(other(4242)), Some(other(4343)), Budget::new(1000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "commit  3f2a9c1e0b", "{text}");
    assert_eq!(lines[1], "indexed  2026-10-28T09:00:00Z", "{text}");
    assert_eq!(lines[2], "changed  1 file since indexing", "{text}");
    assert_eq!(
        lines[3], "session  another session is watching (pid 4343)",
        "{text}"
    );
    assert_eq!(
        lines[4], "sync  another process owns sync (pid 4242)",
        "{text}"
    );
    assert!(
        lines[5].starts_with("database  ") && lines[5].ends_with("KiB"),
        "{text}"
    );
    assert_eq!(
        section(&text, "roots:"),
        [
            "  . (rust)  rust-analyzer 1.91.0 (toolchain) · lsp ✓",
            "  web (typescript)  degraded: no_server",
        ],
        "{text}"
    );
    assert_eq!(
        section(&text, "diagnostics:"),
        ["  web (typescript)  tsconfig.json: unknown option `strct`"],
        "{text}"
    );
    assert_eq!(
        section(&text, "counts:"),
        [
            "  files  5 · 2 parsed · 3 skipped",
            "  nodes  2",
            "  edges  none",
            "  unresolved  none",
        ],
        "{text}"
    );
    assert_eq!(
        section(&text, "partially_enriched:"),
        ["  src/main.rs"],
        "{text}"
    );
    let me = std::process::id();
    let own = repo
        .engine()
        .status(None, Some(other(me)), Budget::new(1000))
        .unwrap();
    let own = TextFormatter::render(&own);
    assert!(
        own.contains(&format!(
            "\nsession  this session is watching (pid {me})\nsync  idle\n"
        )),
        "{own}"
    );
}

#[test]
fn status_lists_skipped_files() {
    let answer = status_repo()
        .engine()
        .status(None, None, Budget::new(1000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    assert_eq!(
        section(&text, "skipped:"),
        ["  assets/logo.png  binary", "  data/huge.json  oversize"],
        "{text}"
    );
}
