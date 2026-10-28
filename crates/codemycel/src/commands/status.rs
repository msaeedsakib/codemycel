use anyhow::Context as _;
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError, StoreRead};

use super::{ExitCode, not_implemented};
use crate::cli::{Context, StatusArgs};

pub fn run(_args: &StatusArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    if ctx.json {
        return not_implemented("status --json");
    }
    let location = super::locate(ctx)?;
    let repo = &location.root;
    let db = location.index_path();
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
