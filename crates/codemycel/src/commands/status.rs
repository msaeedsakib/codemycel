use std::path::PathBuf;

use anyhow::Context as _;
use codemycel_core::config::paths::REPO_DIR_NAME;
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError, StoreRead};

use super::ExitCode;
use crate::cli::{Context, StatusArgs};

const INDEX_FILE_NAME: &str = "index.db";

pub fn run(_args: &StatusArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let repo = resolve_repo(ctx.repo.clone())?;
    let db = repo.join(REPO_DIR_NAME).join(INDEX_FILE_NAME);
    if !db.exists() {
        println!("no index: run codemycel init in {}", repo.display());
        return Ok(ExitCode::Degraded);
    }
    let store = match Store::open(&db, true) {
        Ok(store) => store,
        Err(StoreError::SchemaMismatch { found, .. }) => {
            println!(
                "index schema {found} is out of date (expected {SCHEMA_VERSION}): run codemycel init in {}",
                repo.display()
            );
            return Ok(ExitCode::Degraded);
        }
        Err(error) => return Err(error).with_context(|| format!("opening {}", db.display())),
    };
    let counts = store.counts()?;
    println!("schema_version: {SCHEMA_VERSION}");
    println!("nodes: {}", counts.nodes);
    println!("edges: {}", counts.edges.values().sum::<u64>());
    for (kind, count) in &counts.edges {
        println!("  {kind}: {count}");
    }
    println!("unresolved: {}", counts.unresolved.values().sum::<u64>());
    for (reason, count) in &counts.unresolved {
        println!("  {reason}: {count}");
    }
    Ok(ExitCode::Ok)
}

/// The nearest ancestor below `$HOME` holding `.codemycel/` or `.git`, else the starting
/// directory. The codemycel home never counts. Phase 4 replaces this with spec §8.6.
fn resolve_repo(start: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let start = match start {
        Some(path) => path,
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let start =
        std::path::absolute(&start).with_context(|| format!("resolving {}", start.display()))?;
    let user_home = std::env::home_dir();
    let codemycel_home = codemycel_core::config::home_dir().ok();
    for dir in start.ancestors() {
        if Some(dir) == user_home.as_deref() {
            break;
        }
        let marker = dir.join(REPO_DIR_NAME);
        let indexed = marker.is_dir() && Some(marker.as_path()) != codemycel_home.as_deref();
        if indexed || dir.join(".git").exists() {
            return Ok(dir.to_path_buf());
        }
    }
    Ok(start)
}
