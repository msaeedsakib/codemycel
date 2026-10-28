//! Builds an index of a fixture repository and diffs its dumps against `<fixture>.expected/`.
//! `CODEMYCEL_UPDATE_EXPECTED=1` rewrites the expected files; review every line before committing.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use codemycel_core::ServerAdapter;
use codemycel_core::config::Config;
use codemycel_core::store::{Store, StoreRead};
use codemycel_index::{Pipeline, PipelineReport, Registry, Sources};
use codemycel_lsp::{EnrichOptions, Provisioner};
use tempfile::TempDir;

#[derive(Clone, Copy, Debug)]
pub enum Dump {
    Nodes,
    Edges,
    Unresolved,
    ImplRecords,
}

const EXPECTED: [(Dump, &str); 3] = [
    (Dump::Nodes, "nodes.txt"),
    (Dump::Edges, "edges.txt"),
    (Dump::Unresolved, "unresolved.txt"),
];

pub struct FixtureIndex {
    _dir: TempDir,
    pub store: Store,
    pub report: PipelineReport,
}

/// `fixtures/<relative>` at the workspace root.
pub fn fixture(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative)
}

pub fn index_fixture(dir: &Path) -> FixtureIndex {
    index_fixture_with(dir, Registry::default())
}

pub fn index_fixture_with(dir: &Path, registry: Registry) -> FixtureIndex {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let config = Config::load_with_home(dir, &home).unwrap();
    let mut store = Store::create(&temp.path().join("index.db")).unwrap();
    let report = Pipeline::parse_only(dir, &config, &mut store, &registry).unwrap();
    FixtureIndex {
        _dir: temp,
        store,
        report,
    }
}

/// A copy of a fixture in a temporary directory, for runs whose servers write into the tree
/// (rust-analyzer's `Cargo.lock` and `target/`). Diff its dumps against the original's
/// expected files.
pub fn fixture_copy(source: &Path) -> (TempDir, PathBuf) {
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let dest = to.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            if kind.is_symlink() {
                let target = std::fs::read_link(entry.path()).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(target, dest).unwrap();
                #[cfg(windows)]
                std::os::windows::fs::symlink_dir(target, dest).unwrap();
            } else if kind.is_dir() {
                copy_dir(&entry.path(), &dest);
            } else {
                std::fs::copy(entry.path(), dest).unwrap();
            }
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join(source.file_name().unwrap());
    copy_dir(source, &copy);
    (temp, copy)
}

/// `Pipeline::run` with `registry`'s servers, found by `provisioner`, and no SCIP index.
pub fn index_fixture_full(
    dir: &Path,
    registry: Registry,
    provisioner: &Provisioner,
) -> FixtureIndex {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let config = Config::load_with_home(dir, &home).unwrap();
    let mut store = Store::create(&temp.path().join("index.db")).unwrap();
    let sources = Sources {
        provisioner,
        scip: None,
        options: EnrichOptions::default(),
    };
    let report = Pipeline::run(dir, &config, &mut store, &registry, &sources).unwrap();
    FixtureIndex {
        _dir: temp,
        store,
        report,
    }
}

/// A provisioner over the machine's real servers (config, toolchain, `PATH`, and the
/// codemycel cache; never a download), or `None` with a printed notice when `server` is not
/// installed. With `CODEMYCEL_REQUIRE_SERVERS=1`, as CI sets it, a missing server fails instead.
pub fn local_provisioner(server: &dyn ServerAdapter) -> Option<Provisioner> {
    let home = codemycel_core::config::home_dir().unwrap();
    let provisioner = Provisioner::new(&Config::default(), home, false);
    match provisioner.locate(server) {
        Ok(_) => Some(provisioner),
        Err(error) if std::env::var_os("CODEMYCEL_REQUIRE_SERVERS").is_some_and(|v| v == "1") => {
            panic!("{} is required: {error}", server.name())
        }
        Err(error) => {
            eprintln!("skipping: no local {}: {error}", server.name());
            None
        }
    }
}

pub fn assert_matches_expected(fixture_dir: &Path) {
    index_fixture(fixture_dir).assert_matches(fixture_dir);
}

impl FixtureIndex {
    pub fn dump(&self, dump: Dump) -> String {
        match dump {
            Dump::Nodes => self.store.dump_nodes(),
            Dump::Edges => self.store.dump_edges(),
            Dump::Unresolved => self.store.dump_unresolved(),
            Dump::ImplRecords => self.store.dump_impl_records(),
        }
        .unwrap()
    }

    pub fn assert_matches(&self, fixture_dir: &Path) {
        let expected_dir = expected_dir(fixture_dir);
        let update = std::env::var_os("CODEMYCEL_UPDATE_EXPECTED").is_some_and(|v| v == "1");
        let mut failures = String::new();
        for (dump, file) in EXPECTED {
            let actual = self.dump(dump);
            let path = expected_dir.join(file);
            if update {
                std::fs::create_dir_all(&expected_dir).unwrap();
                std::fs::write(&path, &actual).unwrap();
                continue;
            }
            let expected = std::fs::read_to_string(&path).unwrap_or_default();
            if expected != actual {
                let diff = similar::TextDiff::from_lines(&expected, &actual)
                    .unified_diff()
                    .header(&format!("expected/{file}"), &format!("actual/{file}"))
                    .to_string();
                failures.push_str(&diff);
            }
        }
        assert!(
            failures.is_empty(),
            "dumps differ from {}:\n{failures}",
            expected_dir.display()
        );
    }
}

fn expected_dir(fixture_dir: &Path) -> PathBuf {
    let name = fixture_dir.file_name().unwrap().to_string_lossy();
    fixture_dir.with_file_name(format!("{name}.expected"))
}
