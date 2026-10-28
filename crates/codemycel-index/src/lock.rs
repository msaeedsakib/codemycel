use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use codemycel_core::SyncOwner;
use codemycel_core::config::paths::REPO_DIR_NAME;

use crate::IndexError;

/// `session.lock` is held for a session's lifetime and decides who watches; `sync.lock` is held
/// while a sync runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockKind {
    Session,
    Sync,
}

impl LockKind {
    fn stem(self) -> &'static str {
        match self {
            LockKind::Session => "session",
            LockKind::Sync => "sync",
        }
    }
}

/// An advisory lock on `.codemycel/<kind>.lock`, released on drop and when the process dies.
/// The owner is written beside it in `<kind>.owner`, because a locked file cannot be read on
/// Windows.
#[derive(Debug)]
pub struct LockGuard {
    _file: File,
    owner_path: PathBuf,
    owner: SyncOwner,
}

impl LockGuard {
    pub fn owner(&self) -> &SyncOwner {
        &self.owner
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.owner_path);
    }
}

pub struct Lock;

impl Lock {
    /// `None` when another process, or another handle in this one, holds the lock. The
    /// repository's `.codemycel/` must exist.
    pub fn try_acquire(repo_root: &Path, kind: LockKind) -> Result<Option<LockGuard>, IndexError> {
        let (lock_path, owner_path) = paths(repo_root, kind);
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io(&lock_path))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Ok(None),
            Err(TryLockError::Error(source)) => return Err(io(&lock_path)(source)),
        }
        let owner = SyncOwner {
            pid: std::process::id(),
            since: SystemTime::now(),
        };
        let text = serde_json::to_string(&owner).map_err(|e| io(&owner_path)(e.into()))?;
        std::fs::write(&owner_path, text).map_err(io(&owner_path))?;
        Ok(Some(LockGuard {
            _file: file,
            owner_path,
            owner,
        }))
    }

    /// `try_acquire` every `poll` until the lock is free.
    pub fn acquire_blocking(
        repo_root: &Path,
        kind: LockKind,
        poll: Duration,
    ) -> Result<LockGuard, IndexError> {
        loop {
            if let Some(guard) = Self::try_acquire(repo_root, kind)? {
                return Ok(guard);
            }
            std::thread::sleep(poll);
        }
    }

    /// The holder of the lock, without taking it. An owner file left by a process that died is
    /// ignored, since its lock is free.
    pub fn current_owner(repo_root: &Path, kind: LockKind) -> Option<SyncOwner> {
        let (lock_path, owner_path) = paths(repo_root, kind);
        let owner: SyncOwner = serde_json::from_slice(&std::fs::read(&owner_path).ok()?).ok()?;
        let file = File::open(&lock_path).ok()?;
        match file.try_lock_shared() {
            Err(TryLockError::WouldBlock) => Some(owner),
            _ => None,
        }
    }
}

fn paths(repo_root: &Path, kind: LockKind) -> (PathBuf, PathBuf) {
    let dir = repo_root.join(REPO_DIR_NAME);
    (
        dir.join(format!("{}.lock", kind.stem())),
        dir.join(format!("{}.owner", kind.stem())),
    )
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> IndexError + '_ {
    move |source| IndexError::Io {
        path: path.to_owned(),
        source,
    }
}
