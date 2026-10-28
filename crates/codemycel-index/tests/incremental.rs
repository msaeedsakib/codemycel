#![allow(clippy::unwrap_used)]

mod harness;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use codemycel_core::config::Config;
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{Language, RepoPath};
use codemycel_index::{Registry, SyncMode, SyncReport};
use codemycel_lsp::Provisioner;
use codemycel_parse::rust::RustAdapter;
use harness::{fixture, fixture_copy, index_fixture_full, local_provisioner, sync};
use tempfile::TempDir;

type DumpFn = fn(&Store) -> String;

/// A fixture copy indexed into its own store, synced as the test edits the copy.
struct Session {
    _temp: TempDir,
    dir: PathBuf,
    config: Config,
    store: Store,
    registry: Registry,
}

impl Session {
    fn new(dir: &Path, registry: Registry) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        Self {
            config: Config::load_with_home(dir, &home).unwrap(),
            store: Store::create(&temp.path().join("index.db")).unwrap(),
            _temp: temp,
            dir: dir.to_owned(),
            registry,
        }
    }

    fn sync(&mut self, provisioner: &Provisioner, mode: SyncMode) -> SyncReport {
        sync(
            &self.dir,
            &self.config,
            &mut self.store,
            &self.registry,
            provisioner,
            mode,
        )
    }

    fn edit(&self, file: &str, from: &str, to: &str) {
        let path = self.dir.join(file);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(from), "{file} does not contain {from:?}");
        std::fs::write(&path, text.replacen(from, to, 1)).unwrap();
    }

    fn write(&self, file: &str, text: &str) {
        let path = self.dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn remove(&self, file: &str) {
        std::fs::remove_file(self.dir.join(file)).unwrap();
    }

    /// Every dump equals a fresh full index of the tree as it is now. Communities are compared
    /// by membership, since a sync inherits ids a fresh index mints anew.
    fn assert_matches_fresh(&self, provisioner: &Provisioner, registry: Registry) {
        let fresh = index_fixture_full(&self.dir, registry, provisioner);
        let mut failures = String::new();
        let dumps: [(&str, DumpFn); 4] = [
            ("nodes", |s| s.dump_nodes().unwrap()),
            ("edges", |s| s.dump_edges().unwrap()),
            ("unresolved", |s| s.dump_unresolved().unwrap()),
            ("impl_records", |s| s.dump_impl_records().unwrap()),
        ];
        for (name, dump) in dumps {
            let (expected, actual) = (dump(&fresh.store), dump(&self.store));
            if expected != actual {
                let diff = similar::TextDiff::from_lines(&expected, &actual)
                    .unified_diff()
                    .header(&format!("fresh/{name}"), &format!("synced/{name}"))
                    .to_string();
                failures.push_str(&diff);
            }
        }
        assert!(
            failures.is_empty(),
            "sync differs from a fresh index:\n{failures}"
        );
        assert_eq!(partition(&self.store), partition(&fresh.store));
    }
}

fn partition(store: &Store) -> BTreeSet<Vec<String>> {
    store
        .communities()
        .unwrap()
        .into_iter()
        .map(|community| {
            let mut members: Vec<String> =
                community.members.iter().map(|m| m.to_string()).collect();
            members.sort();
            members
        })
        .collect()
}

fn provisioner_for(language: Language) -> Option<Provisioner> {
    local_provisioner(Registry::default().server_for(language).unwrap())
}

#[test]
fn incremental_sync_equals_fresh_init() {
    let Some(provisioner) = provisioner_for(Language::Rust) else {
        return;
    };
    let (_temp, dir) = fixture_copy(&fixture("rust/workspace-basic"));
    let mut session = Session::new(&dir, Registry::default());
    assert!(session.sync(&provisioner, SyncMode::Full).full);

    session.edit(
        "crates/core/src/circle.rs",
        "self.radius * factor",
        "factor * self.radius",
    );
    let body = session.sync(&provisioner, SyncMode::Incremental);
    assert!(!body.full);
    assert_eq!(body.reverified, 1, "a body-only edit re-verifies its file");

    session.edit(
        "crates/core/src/lib.rs",
        "pub fn total_area(shapes: &[Box<dyn Area>]) -> f64 {\n    shapes.iter().map(|shape| shape.area()).sum()",
        "pub fn total_area(shapes: &[Box<dyn Area>], scale: f64) -> f64 {\n    shapes.iter().map(|shape| shape.area() * scale).sum()",
    );
    let signature = session.sync(&provisioner, SyncMode::Incremental);
    assert!(
        signature.reverified > 1,
        "callers of a changed signature are re-verified"
    );

    session.write("README.md", "# Shapes\n");
    session.sync(&provisioner, SyncMode::Incremental);
    session.edit(
        "README.md",
        "# Shapes\n",
        "# Shapes\n\n## Area\n\nSee `total_area` in [the core](crates/core/src/lib.rs).\n",
    );
    session.sync(&provisioner, SyncMode::Incremental);

    session.write(
        "crates/extra/Cargo.toml",
        "[package]\nname = \"fixture-extra\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    session.write(
        "crates/extra/src/lib.rs",
        "pub fn extra() -> u8 {\n    1\n}\n",
    );
    session.edit(
        "crates/cli/Cargo.toml",
        "fixture-core = { path = \"../core\" }",
        "fixture-core = { path = \"../core\" }\nfixture-extra = { path = \"../extra\" }",
    );
    session.sync(&provisioner, SyncMode::Incremental);

    session.edit(
        "crates/core/src/circle.rs",
        "use crate::geometry::{Circle, Point};",
        "use crate::geometry::{Circle, Point};\nuse crate::render::Labelled;\n\nimpl Labelled for Circle {\n    fn label(&self) -> String {\n        String::from(\"round\")\n    }\n}",
    );
    session.sync(&provisioner, SyncMode::Incremental);

    session.remove("crates/cli/src/memory.rs");
    session.edit("crates/cli/src/main.rs", "mod memory;\n\n", "");
    session.sync(&provisioner, SyncMode::Incremental);

    std::fs::rename(
        dir.join("crates/core/src/circle.rs"),
        dir.join("crates/core/src/round.rs"),
    )
    .unwrap();
    session.edit("crates/core/src/lib.rs", "mod circle;", "mod round;");
    let rename = session.sync(&provisioner, SyncMode::Incremental);
    assert_eq!(rename.changes.added.len(), 1);
    assert_eq!(rename.changes.deleted.len(), 1);

    session.assert_matches_fresh(&provisioner, Registry::default());
}

#[test]
fn incremental_typescript_sync_equals_fresh_init() {
    let Some(provisioner) = provisioner_for(Language::TypeScript) else {
        return;
    };
    let (_temp, dir) = fixture_copy(&fixture("typescript/monorepo-basic"));
    let mut session = Session::new(&dir, Registry::default());
    session.sync(&provisioner, SyncMode::Full);

    session.edit(
        "packages/core/src/math.ts",
        "  return total;\n",
        "  return String(total);\n",
    );
    let inferred = session.sync(&provisioner, SyncMode::Incremental);
    assert!(
        inferred.reverified > 1,
        "an inferred return type change re-verifies its users"
    );

    session.edit(
        "packages/core/src/shapes.ts",
        "export enum Kind {",
        "export class Triangle extends Base implements Shape {\n  readonly name = \"triangle\";\n\n  area(): number {\n    return 1;\n  }\n}\n\nexport enum Kind {",
    );
    session.sync(&provisioner, SyncMode::Incremental);

    session.assert_matches_fresh(&provisioner, Registry::default());
}

#[test]
fn delete_file_leaves_no_dangling_rows_and_reverifies_dependents() {
    let Some(provisioner) = provisioner_for(Language::Rust) else {
        return;
    };
    let (_temp, dir) = fixture_copy(&fixture("rust/workspace-basic"));
    let mut session = Session::new(&dir, Registry::default());
    session.sync(&provisioner, SyncMode::Full);
    let memory = RepoPath::new("crates/cli/src/memory.rs").unwrap();
    assert!(!session.store.nodes_in_file(&memory).unwrap().is_empty());

    session.remove(memory.as_str());
    let report = session.sync(&provisioner, SyncMode::Incremental);

    assert_eq!(report.changes.deleted, [memory.clone()].into());
    assert_eq!(
        report.reverified, 1,
        "the file of the trait it imported and implemented is re-verified"
    );
    assert!(session.store.file_by_path(&memory).unwrap().is_none());
    assert!(
        session
            .store
            .unresolved_for_file(&memory)
            .unwrap()
            .is_empty()
    );
    for dump in [
        session.store.dump_nodes().unwrap(),
        session.store.dump_edges().unwrap(),
        session.store.dump_unresolved().unwrap(),
        session.store.dump_impl_records().unwrap(),
    ] {
        assert!(!dump.contains("memory.rs"), "{dump}");
    }
    session.assert_matches_fresh(&provisioner, Registry::default());
}

#[test]
fn large_change_set_is_consistent_after_sync() {
    let Some(provisioner) = provisioner_for(Language::TypeScript) else {
        return;
    };
    let (_temp, dir) = fixture_copy(&fixture("typescript/monorepo-basic"));
    let mut session = Session::new(&dir, Registry::default());
    session.sync(&provisioner, SyncMode::Full);
    let code: Vec<PathBuf> = session
        .store
        .files()
        .unwrap()
        .into_iter()
        .filter(|f| f.record.parsed)
        .map(|f| dir.join(f.record.path.as_str()))
        .collect();
    let touched = code.len() * 6 / 10;
    for path in &code[..touched] {
        let text = std::fs::read_to_string(path).unwrap();
        std::fs::write(path, text.replacen("return ", "return  ", 1) + "\n").unwrap();
    }
    let commits = session.store.commit_count();

    let report = session.sync(&provisioner, SyncMode::Incremental);

    assert_eq!(report.changes.modified.len(), touched);
    assert_eq!(session.store.commit_count(), commits + 1);
    session.assert_matches_fresh(&provisioner, Registry::default());
}

#[test]
fn mention_appears_when_name_becomes_unique_and_disappears_when_duplicated() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("repo");
    let parse_only = || Registry::with_adapters(vec![Box::new(RustAdapter::new())]);
    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let provisioner = Provisioner::new(&Config::default(), home, false);
    let mut session = Session::new(&dir, parse_only());
    session.write(
        "Cargo.toml",
        "[package]\nname = \"m\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    session.write("src/lib.rs", "pub fn helper() {}\n");
    session.write("docs/guide.md", "# Guide\n\nCall `helper()`.\n");
    let mention = "docs/guide.md#guide (heading)\tmentions\tsrc/lib.rs::helper (function)\t1\n";
    let edges = |session: &Session| session.store.dump_edges().unwrap();

    session.sync(&provisioner, SyncMode::Full);
    assert!(edges(&session).contains(mention));

    session.write("src/other.rs", "pub fn helper() {}\n");
    session.sync(&provisioner, SyncMode::Incremental);
    assert!(
        !edges(&session).contains("\tmentions\t"),
        "{}",
        edges(&session)
    );
    session.assert_matches_fresh(&provisioner, parse_only());

    session.remove("src/other.rs");
    session.sync(&provisioner, SyncMode::Incremental);
    assert!(edges(&session).contains(mention));

    session.remove("src/lib.rs");
    session.write("src/moved.rs", "pub fn helper() {}\n");
    session.sync(&provisioner, SyncMode::Incremental);
    assert!(
        edges(&session).contains("\tmentions\tsrc/moved.rs::helper (function)"),
        "a mention follows a unique name to its new file"
    );
    session.assert_matches_fresh(&provisioner, parse_only());
}
