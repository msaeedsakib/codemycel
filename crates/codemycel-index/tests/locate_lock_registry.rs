#![allow(clippy::unwrap_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use codemycel_index::{Lock, LockKind, RepoLocation, RepoLocator, RepoRegistry};

const CHILD_ENV: &str = "CODEMYCEL_LOCK_TEST_REPO";

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "").unwrap();
}

fn repo_with_codemycel_dir() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::create_dir(temp.path().join(".codemycel")).unwrap();
    temp
}

#[test]
fn locate_stops_at_home_and_ignores_codemycel_home() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    touch(&temp.path().join(".codemycel/index.db"));
    let codemycel_home = home.join("work/.codemycel");
    touch(&codemycel_home.join("config.toml"));
    let start = home.join("work/sub");
    std::fs::create_dir_all(&start).unwrap();

    let found = RepoLocator::locate(&start, Some(&home), Some(&codemycel_home));

    assert_eq!(
        found,
        RepoLocation {
            root: start,
            indexed: false
        }
    );
}

#[test]
fn locate_prefers_index_db_over_git_ancestor() {
    let temp = tempfile::tempdir().unwrap();
    let outer = temp.path().join("outer");
    touch(&outer.join(".codemycel/index.db"));
    std::fs::create_dir_all(outer.join("inner/.git")).unwrap();
    let start = outer.join("inner/src");
    std::fs::create_dir_all(&start).unwrap();

    let found = RepoLocator::locate(&start, None, None);

    assert_eq!(
        found,
        RepoLocation {
            root: outer,
            indexed: true
        }
    );
}

#[test]
fn init_from_subdirectory_targets_repo_root() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("a/b")).unwrap();

    let found = RepoLocator::locate(&repo.join("a/b/../b"), None, None);

    assert_eq!(found.root, repo);
    assert!(!found.indexed);
    assert_eq!(found.index_path(), repo.join(".codemycel/index.db"));
}

#[test]
#[ignore = "the child process of lock_is_exclusive_across_processes"]
fn child_holds_sync_lock() {
    let Some(repo) = std::env::var_os(CHILD_ENV) else {
        return;
    };
    let _guard = Lock::try_acquire(Path::new(&repo), LockKind::Sync)
        .unwrap()
        .unwrap();
    println!("locked");
    std::io::stdout().flush().unwrap();
    std::io::stdin().read_line(&mut String::new()).unwrap();
}

#[test]
fn lock_is_exclusive_across_processes() {
    let repo = repo_with_codemycel_dir();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "child_holds_sync_lock",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert!(
        lines.any(|line| line.unwrap().ends_with("locked")),
        "child never took the lock"
    );

    assert!(
        Lock::try_acquire(repo.path(), LockKind::Sync)
            .unwrap()
            .is_none()
    );
    let owner = Lock::current_owner(repo.path(), LockKind::Sync).unwrap();
    assert_eq!(owner.pid, child.id());
    assert!(
        Lock::try_acquire(repo.path(), LockKind::Session)
            .unwrap()
            .is_some(),
        "the session lock is independent of the sync lock"
    );

    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    assert!(
        Lock::try_acquire(repo.path(), LockKind::Sync)
            .unwrap()
            .is_some()
    );
}

#[test]
fn lock_released_on_drop() {
    let repo = repo_with_codemycel_dir();
    let guard = Lock::try_acquire(repo.path(), LockKind::Session)
        .unwrap()
        .unwrap();
    assert_eq!(guard.owner().pid, std::process::id());
    assert!(
        Lock::try_acquire(repo.path(), LockKind::Session)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        Lock::current_owner(repo.path(), LockKind::Session).as_ref(),
        Some(guard.owner())
    );

    drop(guard);

    assert_eq!(Lock::current_owner(repo.path(), LockKind::Session), None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while Lock::try_acquire(repo.path(), LockKind::Session)
        .unwrap()
        .is_none()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "lock not free a second after drop"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

#[test]
fn registry_round_trips_and_tolerates_missing_file() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    assert!(RepoRegistry::load(&home).entries().is_empty());

    let mut registry = RepoRegistry::load(&home);
    registry.record(Path::new("/repos/b"), Some("abc")).unwrap();
    registry.record(Path::new("/repos/a"), None).unwrap();
    registry.record(Path::new("/repos/b"), Some("def")).unwrap();

    let loaded = RepoRegistry::load(&home);
    let entries: Vec<_> = loaded
        .entries()
        .iter()
        .map(|e| (e.path.to_str().unwrap(), e.commit.as_deref()))
        .collect();
    assert_eq!(entries, [("/repos/a", None), ("/repos/b", Some("def"))]);

    std::fs::write(home.join("registry.json"), "not json").unwrap();
    assert!(RepoRegistry::load(&home).entries().is_empty());
}
