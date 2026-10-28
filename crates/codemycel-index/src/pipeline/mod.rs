mod docs_phase;
mod ownership;
mod parse_phase;
pub(crate) mod verify_phase;
pub(crate) mod write_phase;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use codemycel_core::config::Config;
use codemycel_core::config::paths::notes_dir;
use codemycel_core::store::{Store, StoreRead, Tx, meta};
use codemycel_core::{Community, FileRecord, Node, RepoPath, Root, RootStatus, SkipReason};
use codemycel_scip::{ScipError, ScipImporter, ScipIndex};

pub use docs_phase::DocsCounts;
pub(crate) use parse_phase::ParsedCode;
pub use verify_phase::{Scip, Scope, Sources, Verified};

use crate::communities::CommunityDetector;
use crate::notes::NotesReconciler;
use crate::sync::{ChangeSet, SymbolsDiff, SyncPlan, apply};
use crate::{IndexError, Registry, RootDetector, Walker};

/// What a pipeline run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineReport {
    pub files: usize,
    pub parsed: usize,
    pub skipped: Vec<(RepoPath, SkipReason)>,
    pub roots: usize,
    /// File and symbol nodes of parsed code files.
    pub nodes: usize,
    pub structural_edges: usize,
    pub candidates: usize,
    pub docs: DocsCounts,
    /// Per root whose files were verified, what the verify phase recorded.
    pub statuses: Vec<(Root, RootStatus)>,
}

/// What a sync did on top of the pipeline counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncReport {
    /// Everything was rebuilt: asked for, a first sync, or forced by a change of roots, SCIP
    /// index, server configuration, or codemycel version.
    pub full: bool,
    pub changes: ChangeSet,
    /// Code files whose verified edges were rebuilt.
    pub reverified: usize,
    pub pipeline: PipelineReport,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncMode {
    Incremental,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Walk,
    Parse,
    Docs,
    Communities,
    Write,
}

/// Where a run reports how it is going; every method does nothing by default.
pub trait Progress {
    fn roots(&self, _roots: &[Root]) {}
    fn phase(&self, _phase: Phase, _elapsed: Duration) {}
    /// After a root's files were enriched by its server.
    fn root(&self, _root: &Root, _status: &RootStatus, _elapsed: Duration) {}
}

impl Progress for () {}

/// The walk, the roots, and the parsed code and documents of the whole repository, before
/// anything is written. Every sync stages the same state a first build does.
pub struct Staged {
    pub(crate) files: Vec<FileRecord>,
    pub(crate) roots: RootDetector,
    pub(crate) assignment: Vec<Option<Root>>,
    pub(crate) parsed: Vec<ParsedCode>,
    docs: docs_phase::Prepared,
}

impl Staged {
    fn new(
        repo_root: &Path,
        config: &Config,
        registry: &Registry,
        progress: &dyn Progress,
    ) -> Result<Self, IndexError> {
        let started = Instant::now();
        let mut files = Walker::new(repo_root, config, registry).walk()?;
        progress.phase(Phase::Walk, started.elapsed());
        let listed: Vec<RepoPath> = files.iter().map(|file| file.path.clone()).collect();
        let roots = RootDetector::detect(repo_root, &listed, config, registry)?;
        progress.roots(roots.roots());
        let started = Instant::now();
        let (assignment, parsed) = parse_phase::run(repo_root, &mut files, &roots, registry)?;
        progress.phase(Phase::Parse, started.elapsed());
        let started = Instant::now();
        let docs = docs_phase::prepare(repo_root, &files)?;
        progress.phase(Phase::Docs, started.elapsed());
        Ok(Self {
            files,
            roots,
            assignment,
            parsed,
            docs,
        })
    }

    pub(crate) fn code_files(&self) -> Vec<RepoPath> {
        self.parsed
            .iter()
            .map(|code| self.files[code.file].path.clone())
            .collect()
    }

    fn report(&self, docs: DocsCounts, statuses: Vec<(Root, RootStatus)>) -> PipelineReport {
        PipelineReport {
            files: self.files.len(),
            parsed: self.parsed.len(),
            skipped: self
                .files
                .iter()
                .filter_map(|file| Some((file.path.clone(), file.skip_reason?)))
                .collect(),
            roots: self.roots.roots().len(),
            nodes: self
                .parsed
                .iter()
                .map(|code| 1 + code.parsed.nodes.len())
                .sum(),
            structural_edges: self
                .parsed
                .iter()
                .map(|code| code.parsed.structural_edges.len())
                .sum(),
            candidates: self
                .parsed
                .iter()
                .map(|code| code.parsed.candidates.len())
                .sum(),
            docs,
            statuses,
        }
    }
}

pub struct Pipeline;

impl Pipeline {
    /// Walks, detects roots, parses every rooted code file, then the documents and manifests,
    /// and writes the syntactic graph into an empty store in one transaction. No resolver runs,
    /// so every candidate is stored as `server_unavailable`.
    pub fn parse_only(
        repo_root: &Path,
        config: &Config,
        store: &mut Store,
        registry: &Registry,
    ) -> Result<PipelineReport, IndexError> {
        let staged = Staged::new(repo_root, config, registry, &())?;
        let code_files = staged.code_files();
        let docs = store.transaction(|tx| {
            write_phase::run(
                tx,
                &staged.files,
                staged.roots.roots(),
                &staged.assignment,
                &staged.parsed,
            )?;
            write_phase::placeholders(tx, &staged.parsed)?;
            tx.recompute_name_counts()?;
            docs_phase::write(tx, &staged.docs, &code_files)
        })?;
        Ok(staged.report(docs, Vec::new()))
    }

    /// Brings the store up to date with the working tree in one transaction. Every sync stages
    /// the whole repository as a first build would. A full sync then verifies and rewrites
    /// everything; an incremental one rewrites the changed code files and re-verifies their
    /// blast radius, leaving the rest as stored. Servers run before the transaction opens.
    pub fn sync(
        repo_root: &Path,
        config: &Config,
        store: &mut Store,
        registry: &Registry,
        sources: &Sources<'_>,
        mode: SyncMode,
    ) -> Result<SyncReport, IndexError> {
        let mut staged = Staged::new(repo_root, config, registry, sources.progress)?;
        let changes = ChangeSet::between(&store.files()?, &staged.files);
        let full = mode == SyncMode::Full || needs_full(store, &staged, config, sources)?;
        let (scope, plan) = if full {
            (Scope::everything(&staged), None)
        } else {
            let plan = plan(store, &staged, &changes, sources)?;
            let scope = Scope {
                files: plan.reverify.clone(),
                implementer_sources: plan.implementer_sources.clone(),
                statuses: store
                    .roots()?
                    .into_iter()
                    .filter_map(|stored| Some((stored.root, stored.status?)))
                    .collect(),
            };
            (scope, Some(plan))
        };
        if !changes.is_empty() {
            sources.servers.files_changed(repo_root, &changes.paths());
        }
        let verified = verify_phase::run(&staged, repo_root, config, registry, sources, &scope)?;
        for file in &mut staged.files {
            file.scip_covered = verified.scip_covered.contains(&file.path);
        }
        let previous = store.communities()?;
        let started = Instant::now();
        let docs = store.transaction(|tx| {
            match &plan {
                None => {
                    tx.clear_index()?;
                    write_phase::run(
                        tx,
                        &staged.files,
                        staged.roots.roots(),
                        &staged.assignment,
                        &staged.parsed,
                    )?;
                    verify_phase::write(tx, &verified)?;
                }
                Some(plan) => apply::run(tx, &staged, &changes, plan, &verified)?,
            }
            finish(tx, &staged, &previous, repo_root, config, sources, &changes)
        })?;
        sources.progress.phase(Phase::Write, started.elapsed());
        Ok(SyncReport {
            full,
            reverified: verified.files.len(),
            pipeline: staged.report(docs, verified.statuses),
            changes,
        })
    }

    /// The SCIP index at the configured path, or `index.scip`, with its commit recorded or kept
    /// and its coverage computed against the working tree. `None` without an index, or when no
    /// commit can be recorded for it (no git HEAD and no `override_sha`).
    pub fn load_scip(
        repo_root: &Path,
        config: &Config,
        store: &impl StoreRead,
        override_sha: Option<&str>,
    ) -> Result<Option<ScipIndex>, IndexError> {
        let Some(path) = ScipImporter::discover(repo_root, config) else {
            return Ok(None);
        };
        let mut index = ScipImporter::load(&path)?;
        let previous = store.meta_get(meta::SCIP_HASH)?;
        let commit =
            match index.commit_handling(repo_root, config, previous.as_deref(), override_sha) {
                Ok(commit) => commit,
                Err(ScipError::NoHead) => {
                    tracing::warn!("{}: {}", path.display(), ScipError::NoHead);
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            };
        index.compute_coverage(repo_root, &commit);
        Ok(Some(index))
    }
}

/// A first sync, or one after a change that invalidates every stored verdict: the roots, the
/// SCIP index, the server configuration, or the codemycel version.
fn needs_full(
    store: &Store,
    staged: &Staged,
    config: &Config,
    sources: &Sources<'_>,
) -> Result<bool, IndexError> {
    if store.meta_get(meta::INDEXED_AT)?.is_none() {
        return Ok(true);
    }
    let stored: BTreeSet<Root> = store.roots()?.into_iter().map(|r| r.root).collect();
    let roots: BTreeSet<Root> = staged.roots.roots().iter().cloned().collect();
    Ok(stored != roots
        || store.meta_get(meta::SCIP_HASH)?.as_deref() != sources.scip.map(|s| s.hash)
        || store.meta_get(meta::SERVERS_CONFIG)? != Some(servers_config(config)?)
        || store.meta_get(meta::CODEMYCEL_VERSION)?.as_deref() != Some(env!("CARGO_PKG_VERSION")))
}

/// Diffs the symbols of every changed code file, parsed before or now, and plans the blast
/// radius against the store as it stands.
fn plan(
    store: &Store,
    staged: &Staged,
    changes: &ChangeSet,
    sources: &Sources<'_>,
) -> Result<SyncPlan, IndexError> {
    let stored: BTreeMap<RepoPath, FileRecord> = store
        .files()?
        .into_iter()
        .map(|f| (f.record.path.clone(), f.record))
        .collect();
    let parsed: BTreeMap<&RepoPath, &ParsedCode> = staged
        .parsed
        .iter()
        .map(|code| (&staged.files[code.file].path, code))
        .collect();
    let mut symbols = BTreeMap::new();
    for path in changes.paths() {
        let was_parsed = stored.get(&path).is_some_and(|f| f.parsed);
        let now = parsed.get(&path);
        if !was_parsed && now.is_none() {
            continue;
        }
        let (old_nodes, old_impls): (Vec<Node>, _) = if was_parsed {
            (
                store
                    .nodes_in_file(&path)?
                    .into_iter()
                    .map(|s| s.node)
                    .collect(),
                store.impl_records_in_file(&path)?,
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let (new_nodes, new_impls) = now.map_or((&[][..], &[][..]), |code| {
            (&code.parsed.nodes[..], &code.parsed.impl_records[..])
        });
        let diff = SymbolsDiff::compute(&old_nodes, &old_impls, new_nodes, new_impls);
        symbols.insert(path, diff);
    }
    let covered = verify_phase::scip_covered(staged, sources.scip)?;
    let flips: BTreeSet<RepoPath> = parsed
        .keys()
        .filter(|path| {
            stored
                .get(**path)
                .is_some_and(|f| f.scip_covered != covered.contains(**path))
        })
        .map(|path| (*path).clone())
        .collect();
    let parsed_now: BTreeSet<RepoPath> = parsed.keys().map(|path| (*path).clone()).collect();
    SyncPlan::build(changes, &symbols, &flips, &parsed_now, store)
}

/// The stages every sync ends with, after the code layer is written: name counts, the whole
/// docs and manifests layer, notes and communities, and meta.
fn finish(
    tx: &mut Tx<'_>,
    staged: &Staged,
    previous: &[Community],
    repo_root: &Path,
    config: &Config,
    sources: &Sources<'_>,
    changes: &ChangeSet,
) -> Result<DocsCounts, IndexError> {
    tx.recompute_name_counts()?;
    tx.clear_docs_layer()?;
    let docs = docs_phase::write(tx, &staged.docs, &staged.code_files())?;
    NotesReconciler::load(tx, &notes_dir(repo_root))?;
    let started = Instant::now();
    CommunityDetector::run(tx, previous)?;
    sources
        .progress
        .phase(Phase::Communities, started.elapsed());
    NotesReconciler::attach(tx)?;
    match head_commit(repo_root) {
        Some(commit) => tx.meta_set(meta::INDEXED_COMMIT, &commit)?,
        None => tx.meta_delete(meta::INDEXED_COMMIT)?,
    }
    match sources.scip {
        Some(scip) => tx.meta_set(meta::SCIP_HASH, scip.hash)?,
        None => tx.meta_delete(meta::SCIP_HASH)?,
    }
    tx.meta_set(meta::INDEXED_AT, &jiff::Timestamp::now().to_string())?;
    tx.meta_set(meta::LAST_SYNC_CHANGED_FILES, &changes.len().to_string())?;
    tx.meta_set(meta::SERVERS_CONFIG, &servers_config(config)?)?;
    tx.meta_set(meta::CODEMYCEL_VERSION, env!("CARGO_PKG_VERSION"))?;
    Ok(docs)
}

fn servers_config(config: &Config) -> Result<String, IndexError> {
    serde_json::to_string(&config.servers).map_err(|e| IndexError::Store(e.into()))
}

/// Git HEAD of the repository, when it holds a `.git` of its own.
fn head_commit(repo_root: &Path) -> Option<String> {
    if !repo_root.join(".git").exists() {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["rev-parse", "HEAD"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let head = String::from_utf8(output.stdout).ok()?.trim().to_owned();
    (output.status.success() && !head.is_empty()).then_some(head)
}
