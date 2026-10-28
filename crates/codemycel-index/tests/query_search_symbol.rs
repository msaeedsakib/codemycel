#![allow(clippy::unwrap_used)]

mod harness;

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::query::{Answer, Budget, Line, TextFormatter};
use codemycel_core::{Language, NodeKind};
use codemycel_index::Registry;
use harness::{
    FixtureIndex, fixture, fixture_copy, index_fixture, index_fixture_full, local_provisioner,
};
use tempfile::TempDir;

fn options() -> SymbolOptions {
    SymbolOptions {
        include_body: false,
        callers: true,
        callees: true,
        limit: 25,
    }
}

/// A parse-only index of a copy of the Rust fixture, with `edit` applied to the copy first.
fn parsed(edit: impl FnOnce(&Path)) -> (TempDir, PathBuf, FixtureIndex) {
    let (temp, copy) = fixture_copy(&fixture("rust/workspace-basic"));
    edit(&copy);
    let index = index_fixture(&copy);
    (temp, copy, index)
}

/// The Rust fixture indexed once with rust-analyzer, shared by the tests that need verified
/// edges; `None` when rust-analyzer is not installed.
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

fn card_paths(answer: &Answer) -> Vec<String> {
    answer.sections[0]
        .lines
        .iter()
        .filter_map(|line| match line {
            Line::Card(card) => Some(card.node.path.to_string()),
            _ => None,
        })
        .collect()
}

fn position(paths: &[String], path: &str) -> usize {
    paths
        .iter()
        .position(|p| p == path)
        .unwrap_or_else(|| panic!("{path} not in {paths:#?}"))
}

#[test]
fn search_splits_camel_and_snake_case() {
    let (_temp, copy, index) = parsed(|_| {});
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let snake = card_paths(
        &engine
            .search("total area", &[], 20, Budget::new(3000))
            .unwrap(),
    );
    assert_eq!(
        position(&snake, "crates/core/src/lib.rs::total_area"),
        0,
        "{snake:#?}"
    );
    let camel = card_paths(
        &engine
            .search("memory store", &[], 20, Budget::new(3000))
            .unwrap(),
    );
    assert_eq!(
        position(&camel, "crates/cli/src/memory.rs::MemoryStore"),
        0,
        "{camel:#?}"
    );
    let typed = card_paths(
        &engine
            .search("MemoryStore", &[], 20, Budget::new(3000))
            .unwrap(),
    );
    assert_eq!(
        position(&typed, "crates/cli/src/memory.rs::MemoryStore"),
        0,
        "{typed:#?}"
    );
}

#[test]
fn exact_name_outranks_doc_hit() {
    let (_temp, copy, index) = parsed(|_| {});
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let paths = card_paths(&engine.search("shape", &[], 20, Budget::new(3000)).unwrap());
    assert!(
        position(&paths, "crates/core/src/geometry.rs::Shape")
            < position(&paths, "crates/core/src/lib.rs::total_area"),
        "{paths:#?}"
    );
}

#[test]
fn test_files_are_penalised() {
    let (_temp, copy, index) = parsed(|copy| {
        let tests = copy.join("crates/core/tests");
        std::fs::create_dir_all(&tests).unwrap();
        std::fs::write(
            tests.join("geometry.rs"),
            "pub fn sides() -> u32 {\n    4\n}\n",
        )
        .unwrap();
    });
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let paths = card_paths(&engine.search("sides", &[], 20, Budget::new(3000)).unwrap());
    assert!(
        position(&paths, "crates/core/src/geometry.rs::sides")
            < position(&paths, "crates/core/tests/geometry.rs::sides"),
        "{paths:#?}"
    );
}

#[test]
fn kind_filter_restricts_results() {
    let (_temp, copy, index) = parsed(|_| {});
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let answer = engine
        .search("area", &[NodeKind::Trait], 20, Budget::new(3000))
        .unwrap();
    assert_eq!(
        card_paths(&answer),
        [
            "crates/core/src/geometry.rs::Area",
            "crates/core/src/render.rs::Labelled",
        ]
    );
    let all = engine.search("area", &[], 20, Budget::new(3000)).unwrap();
    assert!(card_paths(&all).len() > 2);
}

#[test]
fn symbol_lists_dispatch_expanded_callers_with_via() {
    let Some(index) = verified() else {
        return;
    };
    let index = index.lock().unwrap();
    let (_, copy, index) = &*index;
    let registry = Registry::default();
    let engine = index.engine(copy, registry.adapters());
    let answer = engine
        .symbol(
            "crates/core/src/geometry.rs::<Circle as Area>::area",
            options(),
            Budget::new(4000),
        )
        .unwrap();
    let text = TextFormatter::render(&answer);
    let callers: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "callers:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect();
    assert_eq!(
        callers,
        [
            "  ← calls  crates/core/src/geometry.rs::Area::describe (method) L27 [L28] via crates/core/src/geometry.rs::Area::area",
            "  ← calls  crates/core/src/lib.rs::total_area (function) L16 [L17] via crates/core/src/geometry.rs::Area::area",
        ],
        "{text}"
    );
}

#[test]
fn symbol_shows_unresolved_with_reasons() {
    let Some(index) = verified() else {
        return;
    };
    let index = index.lock().unwrap();
    let (_, copy, index) = &*index;
    let registry = Registry::default();
    let engine = index.engine(copy, registry.adapters());
    let answer = engine
        .symbol("crates/cli/src/main.rs::main", options(), Budget::new(4000))
        .unwrap();
    let text = TextFormatter::render(&answer);
    assert!(
        text.contains("unresolved:\n  L27:16 type  Vec  (external_dependency)\n"),
        "{text}"
    );
}

#[test]
fn body_read_after_edit_is_marked_stale() {
    let (_temp, copy, index) = parsed(|_| {});
    let geometry = copy.join("crates/core/src/geometry.rs");
    let source = std::fs::read_to_string(&geometry).unwrap();
    let edited = source
        .replace("pub fn sides(", "// moved down\n\npub fn sides(")
        .replace("Shape::Empty => 0,", "Shape::Empty => 1,");
    std::fs::write(&geometry, edited).unwrap();
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let answer = engine
        .symbol(
            "crates/core/src/geometry.rs::sides",
            SymbolOptions {
                include_body: true,
                ..options()
            },
            Budget::new(4000),
        )
        .unwrap();
    let text = TextFormatter::render(&answer);
    let body: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "body:")
        .skip(1)
        .take_while(|line| line.starts_with("  "))
        .collect();
    assert_eq!(
        body,
        [
            "  stale  the file changed since indexing; showing its current text",
            "    56  pub fn sides(shape: &Shape) -> u32 {",
            "    57      match shape {",
            "    58          Shape::Round(_) => 0,",
            "    59          Shape::Square { .. } => 4,",
            "    60          Shape::Empty => 1,",
            "    61      }",
            "    62  }",
        ],
        "{text}"
    );
    assert!(
        text.lines()
            .last()
            .unwrap()
            .ends_with("stale in this answer: crates/core/src/geometry.rs"),
        "{text}"
    );
}

#[test]
fn search_snapshot_on_rust_fixture() {
    let (_temp, copy, index) = parsed(|_| {});
    let registry = Registry::default();
    let engine = index.engine(&copy, registry.adapters());
    let answer = engine.search("circle", &[], 5, Budget::new(3000)).unwrap();
    assert_eq!(
        TextFormatter::render(&answer),
        "crates/core/src/lib.rs::circle (module) L6
  sig  mod circle
crates/core/src/geometry.rs::Circle (struct) L11-14 pub
  sig  pub struct Circle
crates/core/src/circle.rs::Circle::scaled (method) L8-11 pub
  sig  pub fn scaled(&self, factor: f64) -> Circle
crates/core/src/circle.rs (file) L1-12
crates/core/src/circle.rs::Circle::new (method) L4-6 pub
  sig  pub fn new(center: Point, radius: f64) -> Self
index: no git (0 files changed since) · coverage: rust not verified
"
    );
}
