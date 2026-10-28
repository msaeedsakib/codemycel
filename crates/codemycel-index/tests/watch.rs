#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use codemycel_core::RepoPath;
use codemycel_index::{ChangeSet, Watcher, WatcherHandle};
use tempfile::TempDir;

const DEBOUNCE: Duration = Duration::from_millis(200);
const WAIT: Duration = Duration::from_secs(30);

fn repo(files: &[(&str, &str)]) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        write(dir.path(), path, text);
    }
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let absolute = root.join(path);
    std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
    std::fs::write(absolute, text).unwrap();
}

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap()
}

fn watch(root: &Path, ignore: &[String]) -> (WatcherHandle, Receiver<ChangeSet>) {
    let (tx, rx) = mpsc::channel();
    let handle = Watcher::start(root, ignore, DEBOUNCE, move |changes| {
        let _ = tx.send(changes);
    })
    .unwrap();
    (handle, rx)
}

/// The union of the change sets received until `done` holds for it.
fn collect_until(rx: &Receiver<ChangeSet>, done: impl Fn(&ChangeSet) -> bool) -> ChangeSet {
    let deadline = Instant::now() + WAIT;
    let mut all = ChangeSet::default();
    while !done(&all) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let changes = rx
            .recv_timeout(remaining)
            .unwrap_or_else(|_| panic!("no change set completing {all:?}"));
        all.added.extend(changes.added);
        all.modified.extend(changes.modified);
        all.deleted.extend(changes.deleted);
    }
    all
}

#[test]
fn events_are_debounced_into_one_change_set() {
    let repo = repo(&[("a.rs", "fn a() {}\n"), ("src/lib.rs", "")]);
    let (_watcher, rx) = watch(repo.path(), &[]);
    write(repo.path(), "b.rs", "fn b() {}\n");
    write(repo.path(), "src/c.rs", "fn c() {}\n");
    write(repo.path(), "a.rs", "fn a() { 1; }\n");
    let first = rx.recv_timeout(WAIT).unwrap();
    assert!(first.added.contains(&path("b.rs")), "{first:?}");
    assert!(first.added.contains(&path("src/c.rs")), "{first:?}");
    assert!(
        first.modified.contains(&path("a.rs")) || first.added.contains(&path("a.rs")),
        "{first:?}"
    );
    assert!(first.deleted.is_empty(), "{first:?}");
}

#[test]
fn ignored_paths_and_codemycel_dir_do_not_emit() {
    let repo = repo(&[
        (".gitignore", "ignored/\n*.log\n"),
        ("sub/.gitignore", "local.rs\n"),
        ("sub/kept.rs", ""),
        (".codemycel/config.toml", ""),
        ("target/.keep", ""),
        ("node_modules/.keep", ""),
        ("ignored/.keep", ""),
        ("skip/.keep", ""),
    ]);
    let (_watcher, rx) = watch(repo.path(), &["skip/".to_owned()]);
    for file in [
        ".codemycel/index.db",
        ".codemycel/notes/n.md",
        "target/debug/x.rs",
        "node_modules/pkg/index.js",
        "ignored/a.rs",
        "build.log",
        "skip/c.rs",
        "sub/local.rs",
    ] {
        write(repo.path(), file, "x\n");
    }
    write(repo.path(), "marker.rs", "fn m() {}\n");
    let all = collect_until(&rx, |all| all.added.contains(&path("marker.rs")));
    assert_eq!(all.paths(), vec![path("marker.rs")]);
}

#[test]
fn rename_is_delete_plus_create() {
    let repo = repo(&[("old.rs", "fn f() {}\n")]);
    let (_watcher, rx) = watch(repo.path(), &[]);
    std::fs::rename(repo.path().join("old.rs"), repo.path().join("new.rs")).unwrap();
    let all = collect_until(&rx, |all| {
        all.deleted.contains(&path("old.rs")) && all.added.contains(&path("new.rs"))
    });
    assert!(!all.added.contains(&path("old.rs")), "{all:?}");
    assert!(!all.deleted.contains(&path("new.rs")), "{all:?}");
    assert!(all.modified.is_empty(), "{all:?}");
}

#[test]
fn events_during_a_pause_are_emitted_after_resume() {
    let repo = repo(&[("a.rs", "fn a() {}\n")]);
    let (watcher, rx) = watch(repo.path(), &[]);
    watcher.pause();
    write(repo.path(), "b.rs", "fn b() {}\n");
    assert!(rx.recv_timeout(DEBOUNCE * 5).is_err());
    watcher.resume();
    collect_until(&rx, |all| all.added.contains(&path("b.rs")));
}
