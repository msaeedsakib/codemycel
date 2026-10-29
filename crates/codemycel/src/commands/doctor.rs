use std::collections::BTreeMap;

use codemycel_core::config::Config;
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError, StoreRead, meta};
use codemycel_core::{DegradeReason, Language, SyncOwner};
use codemycel_index::{Lock, LockKind, Registry, RepoLocation, RepoRegistry, Watcher, sibling};
use codemycel_lsp::Provisioner;
use codemycel_lsp::enrich::absolute;
use jiff::Timestamp;

use super::ExitCode;
use crate::cli::{Context, DoctorArgs};
use crate::progress::display;

/// Prints the install, the registry, and for the current repository its roots, servers, SCIP
/// index, schema, degradations with a fix each, config diagnostics, orphaned notes, and lock
/// owners. Exits 2 when a root is degraded or the index is out of date.
pub fn run(_args: &DoctorArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let home = codemycel_core::config::home_dir()?;
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "unknown path".to_owned());
    println!("codemycel {} at {exe}", env!("CARGO_PKG_VERSION"));
    println!("home {}", home.display());
    println!("watcher {}", Watcher::backend());
    let registry = RepoRegistry::load(&home);
    println!("registry: {} repositories", registry.entries().len());
    for entry in registry.entries() {
        println!(
            "  {} (commit {}, last seen {})",
            entry.path.display(),
            entry.commit.as_deref().unwrap_or("none"),
            entry.last_seen
        );
    }
    let location = super::locate(ctx)?;
    println!("repository {}", location.root.display());
    let db = location.index_path();
    if !db.exists() {
        println!(
            "  no index: run codemycel init in {}",
            location.root.display()
        );
        return Ok(ExitCode::Ok);
    }
    let store = match Store::open(&db, true) {
        Ok(store) => store,
        Err(StoreError::SchemaMismatch { found, .. }) => {
            println!(
                "  index schema {found} is out of date (expected {SCHEMA_VERSION}): run codemycel init"
            );
            return Ok(ExitCode::Degraded);
        }
        Err(error) => return Err(error.into()),
    };
    let degraded = repository(&location, &store, &home)?;
    Ok(if degraded {
        ExitCode::Degraded
    } else {
        ExitCode::Ok
    })
}

/// Prints the repository's part of the report; `true` when a root is degraded.
fn repository(
    location: &RepoLocation,
    store: &Store,
    home: &std::path::Path,
) -> anyhow::Result<bool> {
    let repo = &location.root;
    let config = Config::load_with_home(repo, home)?;
    let db = location.index_path();
    let size: u64 = [db.clone(), sibling(&db, "-wal")]
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum();
    println!(
        "  index schema {SCHEMA_VERSION}, {:.1} MiB, indexed {} at commit {}",
        size as f64 / f64::from(1 << 20),
        store
            .meta_get(meta::INDEXED_AT)?
            .as_deref()
            .unwrap_or("never"),
        store
            .meta_get(meta::INDEXED_COMMIT)?
            .as_deref()
            .unwrap_or("none (no git)")
    );
    let scip = repo.join(&config.scip_path);
    if scip.is_file() {
        println!(
            "  scip {} (commit {})",
            config.scip_path.display(),
            config.scip_commit.as_deref().unwrap_or("not recorded yet")
        );
    } else {
        println!("  scip none");
    }
    let registry = Registry::default();
    let provisioner = Provisioner::new(&config, home.to_owned(), false);
    let mut servers: BTreeMap<Language, Vec<String>> = BTreeMap::new();
    let mut degraded = false;
    println!("  roots:");
    for stored in store.roots()? {
        let root = &stored.root;
        println!("    {} ({})", display(root), root.language);
        let Some(adapter) = registry.server_for(root.language) else {
            continue;
        };
        let lines = servers.entry(root.language).or_insert_with(|| {
            let mut lines = Vec::new();
            match provisioner.locate(adapter) {
                Ok(located) => {
                    lines.push(format!(
                        "server {} {} from {} at {}",
                        adapter.name(),
                        located.version,
                        located.source,
                        located.path.display()
                    ));
                    lines.extend(located.rejected.iter().map(|r| {
                        format!("rejected {} ({}): {}", r.path.display(), r.source, r.reason)
                    }));
                }
                Err(missing) => {
                    lines.push(format!(
                        "server {} not found: {}",
                        adapter.name(),
                        missing.download
                    ));
                    lines.extend(missing.rejected.iter().map(|r| {
                        format!("rejected {} ({}): {}", r.path.display(), r.source, r.reason)
                    }));
                }
            }
            if let Some(toolchain) = adapter.toolchain() {
                let present = provisioner.prerequisite_present(adapter);
                lines.push(format!(
                    "{} {}",
                    toolchain.prerequisite,
                    if present { "present" } else { "missing" }
                ));
            }
            lines
        });
        for line in lines.iter() {
            println!("      {line}");
        }
        match &stored.status {
            None => println!("      not verified yet: run codemycel sync"),
            Some(status) => {
                match status.degraded {
                    Some(reason) => {
                        degraded = true;
                        println!(
                            "      degraded: {reason}; fix: {}",
                            fix(reason, root.language)
                        );
                        if reason == DegradeReason::ToolchainMissing
                            && let Some(error) =
                                provisioner.toolchain_missing(adapter, &absolute(repo, &root.path))
                        {
                            println!("      {error}");
                        }
                    }
                    None => println!("      verified by {}", status.coverage),
                }
                for diagnostic in &status.config_diagnostics {
                    println!("      config diagnostic: {diagnostic}");
                }
            }
        }
    }
    let orphaned = store.notes()?.iter().filter(|note| note.orphaned).count();
    println!("  orphaned notes {orphaned}");
    println!(
        "  session lock {}",
        owner(Lock::current_owner(repo, LockKind::Session))
    );
    println!(
        "  sync lock {}",
        owner(Lock::current_owner(repo, LockKind::Sync))
    );
    Ok(degraded)
}

fn owner(owner: Option<SyncOwner>) -> String {
    match owner {
        Some(owner) => {
            let since = Timestamp::try_from(owner.since)
                .map(|t| t.to_string())
                .unwrap_or_default();
            format!("held by pid {} since {since}", owner.pid)
        }
        None => "free".to_owned(),
    }
}

fn fix(reason: DegradeReason, language: Language) -> String {
    match reason {
        DegradeReason::NoServer => format!(
            "install the language server, or set [servers.{language}] path in .codemycel/config.toml, then run codemycel sync --full"
        ),
        DegradeReason::NoCargo => {
            "install a Rust toolchain with cargo (rustup.rs), then run codemycel sync --full".to_owned()
        }
        DegradeReason::StartFailed => {
            "run the server above with --version to see why it does not start".to_owned()
        }
        DegradeReason::NotReady => {
            "the server never became ready; check that the project builds, then run codemycel sync --full".to_owned()
        }
        DegradeReason::ToolchainMissing => {
            "the root pins a toolchain (rust-toolchain.toml) that is not installed; run rustup toolchain install in the root's directory, then run codemycel sync --full".to_owned()
        }
        DegradeReason::Crashed => format!(
            "the server crashed; run codemycel sync --full, or set [servers.{language}] build_scripts = false for untrusted code"
        ),
    }
}
