#![allow(clippy::unwrap_used)]

mod harness;

use std::path::Path;

use harness::{Dump, FixtureIndex, fixture, index_fixture};

const ALL: [Dump; 4] = [
    Dump::Nodes,
    Dump::Edges,
    Dump::Unresolved,
    Dump::ImplRecords,
];

/// Copies a fixture's regular files, transforming each one's bytes; symlinks are left out.
fn copy_tree(from: &Path, to: &Path, transform: &dyn Fn(&Path, Vec<u8>) -> Vec<u8>) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let file_type = entry.file_type().unwrap();
        let target = to.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target, transform);
        } else if file_type.is_file() {
            let bytes = std::fs::read(entry.path()).unwrap();
            std::fs::write(&target, transform(&entry.path(), bytes)).unwrap();
        }
    }
}

fn assert_same(a: &FixtureIndex, b: &FixtureIndex, dumps: &[Dump]) {
    for &dump in dumps {
        assert_eq!(a.dump(dump), b.dump(dump), "{dump:?} differs");
    }
}

#[test]
fn dumps_identical_from_two_absolute_paths() {
    for name in [
        "rust/workspace-basic",
        "typescript/monorepo-basic",
        "docs/mixed",
    ] {
        let original = fixture(name);
        let temp = tempfile::tempdir().unwrap();
        let copy = temp.path().join("with space").join("тест").join("repo");
        copy_tree(&original, &copy, &|_, bytes| bytes);
        let a = index_fixture(&original);
        assert!(!a.dump(Dump::Nodes).is_empty(), "{name} indexed nothing");
        assert_same(&a, &index_fixture(&copy), &ALL);
    }
}

#[test]
fn dumps_identical_across_two_runs() {
    for name in [
        "rust/workspace-basic",
        "typescript/monorepo-basic",
        "docs/mixed",
    ] {
        let dir = fixture(name);
        assert_same(&index_fixture(&dir), &index_fixture(&dir), &ALL);
    }
}

#[test]
fn crlf_and_bom_do_not_change_the_graph() {
    let original = fixture("typescript/monorepo-basic");
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("repo");
    copy_tree(&original, &copy, &|_, bytes| {
        let text = String::from_utf8(bytes).unwrap();
        let crlf = text.replace("\r\n", "\n").replace('\n', "\r\n");
        format!("\u{feff}{crlf}").into_bytes()
    });
    assert_same(
        &index_fixture(&original),
        &index_fixture(&copy),
        &[Dump::Nodes, Dump::Edges, Dump::Unresolved],
    );
}
