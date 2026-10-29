use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use codemycel_core::config::Config;
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreRead, meta};
use codemycel_core::{SkipReason, UnresolvedReason};
use codemycel_index::{
    BatchServers, Lock, LockKind, Opened, Pipeline, Registry, RepoLocation, RepoRegistry, Scip,
    Sources, SyncMode, SyncReport, open_or_rebuild, sibling,
};
use codemycel_lsp::{EnrichOptions, Provisioner};

use super::ExitCode;
use crate::cli::{Context, SyncArgs};
use crate::progress::{Printer, display, seconds};

const LOCK_POLL: Duration = Duration::from_millis(500);

pub fn run(args: &SyncArgs, ctx: &Context) -> anyhow::Result<ExitCode> {
    let location = super::locate(ctx)?;
    if !location.index_path().exists() {
        eprintln!(
            "no index: run codemycel init in {}",
            location.root.display()
        );
        return Ok(ExitCode::Error);
    }
    let mode = if args.full {
        SyncMode::Full
    } else {
        SyncMode::Incremental
    };
    index(
        &location,
        mode,
        args.no_download,
        args.scip_commit.as_deref(),
    )
}

/// Syncs the repository under `sync.lock`, waiting for a sync another process runs, then
/// prints the summary and records the repository in the registry. A database of another schema
/// version is deleted and rebuilt in full.
pub(super) fn index(
    location: &RepoLocation,
    mode: SyncMode,
    no_download: bool,
    scip_commit: Option<&str>,
) -> anyhow::Result<ExitCode> {
    let started = Instant::now();
    let repo = &location.root;
    let _lock = match Lock::try_acquire(repo, LockKind::Sync)? {
        Some(lock) => lock,
        None => {
            if let Some(owner) = Lock::current_owner(repo, LockKind::Sync) {
                eprintln!("waiting for sync owned by pid {}", owner.pid);
            }
            Lock::acquire_blocking(repo, LockKind::Sync, LOCK_POLL)?
        }
    };
    let config = Config::load(repo)?;
    let home = codemycel_core::config::home_dir()?;
    let db = location.index_path();
    let (mut store, opened) =
        open_or_rebuild(&db).with_context(|| format!("opening {}", db.display()))?;
    if let Opened::Rebuilt { found } = &opened {
        eprintln!("index schema {found} replaced by {SCHEMA_VERSION}: full build");
    }
    let mode = if opened.needs_full_sync() {
        SyncMode::Full
    } else {
        mode
    };
    let provisioner = Provisioner::new(&config, home.clone(), !(no_download || config.no_download));
    let servers = BatchServers {
        provisioner: &provisioner,
        options: EnrichOptions::default(),
    };
    let scip = Pipeline::load_scip(repo, &config, &store, scip_commit)?;
    let sources = Sources {
        servers: &servers,
        scip: scip.as_ref().map(|index| Scip {
            source: index,
            hash: index.hash(),
        }),
        progress: &Printer,
    };
    let report = Pipeline::sync(
        repo,
        &config,
        &mut store,
        &Registry::default(),
        &sources,
        mode,
    )
    .with_context(|| format!("indexing {}", repo.display()))?;
    let degraded = summary(&store, &report, &db, started.elapsed())?;
    let commit = store.meta_get(meta::INDEXED_COMMIT)?;
    if let Err(error) = RepoRegistry::load(&home).record(repo, commit.as_deref()) {
        tracing::warn!("{error}");
    }
    Ok(if degraded {
        ExitCode::Degraded
    } else {
        ExitCode::Ok
    })
}

/// Prints what the index holds after the sync; `true` when a root is degraded.
fn summary(
    store: &Store,
    report: &SyncReport,
    db: &Path,
    elapsed: Duration,
) -> anyhow::Result<bool> {
    let pipeline = &report.pipeline;
    let changes = &report.changes;
    if report.full {
        println!("full build: {} files", pipeline.files);
    } else {
        println!(
            "changed {} files ({} added, {} modified, {} deleted), re-verified {}",
            changes.len(),
            changes.added.len(),
            changes.modified.len(),
            changes.deleted.len(),
            report.reverified
        );
    }
    let mut skipped: BTreeMap<SkipReason, usize> = BTreeMap::new();
    for (_, reason) in &pipeline.skipped {
        *skipped.entry(*reason).or_default() += 1;
    }
    println!(
        "files indexed {}, skipped {}{}",
        pipeline.parsed + pipeline.docs.docs,
        pipeline.skipped.len(),
        breakdown(&skipped)
    );
    let counts = store.counts()?;
    println!("nodes {}", counts.nodes);
    println!(
        "edges {}{}",
        counts.edges.values().sum::<u64>(),
        breakdown(&counts.edges)
    );
    let (external, unresolved): (BTreeMap<UnresolvedReason, u64>, BTreeMap<_, _>) =
        counts.unresolved.into_iter().partition(|(reason, _)| {
            matches!(
                reason,
                UnresolvedReason::ExternalDependency | UnresolvedReason::UnindexedFile
            )
        });
    println!(
        "unresolved {}{}, external {}{}",
        unresolved.values().sum::<u64>(),
        breakdown(&unresolved),
        external.values().sum::<u64>(),
        breakdown(&external)
    );
    let mut degraded = false;
    for stored in store.roots()? {
        let coverage = match &stored.status {
            Some(status) => match status.degraded {
                Some(reason) => {
                    degraded = true;
                    format!("degraded: {reason}")
                }
                None => format!("verified by {}", status.coverage),
            },
            None => "not verified".to_owned(),
        };
        println!(
            "root {} ({}): {coverage}",
            display(&stored.root),
            stored.root.language
        );
    }
    let size: u64 = [db.to_owned(), sibling(db, "-wal")]
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .sum();
    println!("database {:.1} MiB", size as f64 / f64::from(1 << 20));
    println!("total {}", seconds(elapsed));
    Ok(degraded)
}

/// ` (a 1, b 2)`, or nothing when empty.
fn breakdown<K: std::fmt::Display, V: std::fmt::Display>(counts: &BTreeMap<K, V>) -> String {
    if counts.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = counts.iter().map(|(k, v)| format!("{k} {v}")).collect();
    format!(" ({})", parts.join(", "))
}
