use std::path::{Component, Path, PathBuf};

use anyhow::Context as _;
use codemycel_core::config::paths::{CONFIG_FILE_NAME, REPO_DIR_NAME};
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError, StoreRead};

use super::{ExitCode, not_implemented};
use crate::cli::{Context, StatusArgs};

const INDEX_FILE_NAME: &str = "index.db";

pub fn run(_args: &StatusArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    if ctx.json {
        return not_implemented("status --json");
    }
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

/// Below `$HOME`: the nearest ancestor whose `.codemycel/` holds `index.db` or
/// `config.toml` (never the codemycel home), else the nearest holding `.git`, else the start.
fn resolve_repo(start: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let start = match start {
        Some(path) => path,
        None => std::env::current_dir().context("reading the current directory")?,
    };
    let absolute =
        std::path::absolute(&start).with_context(|| format!("resolving {}", start.display()))?;
    let start = without_parent_segments(&absolute);
    let user_home = std::env::home_dir();
    let codemycel_home = codemycel_core::config::home_dir().ok();
    let ancestors: Vec<&Path> = start
        .ancestors()
        .take_while(|dir| Some(*dir) != user_home.as_deref())
        .collect();
    let indexed = ancestors.iter().find(|dir| {
        let marker = dir.join(REPO_DIR_NAME);
        Some(marker.as_path()) != codemycel_home.as_deref()
            && (marker.join(INDEX_FILE_NAME).is_file() || marker.join(CONFIG_FILE_NAME).is_file())
    });
    let found = indexed.or_else(|| ancestors.iter().find(|dir| dir.join(".git").exists()));
    Ok(found.map_or_else(|| start.clone(), |dir| dir.to_path_buf()))
}

/// Resolves `.` and `..` lexically, so walking ancestors walks real parent directories.
fn without_parent_segments(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
