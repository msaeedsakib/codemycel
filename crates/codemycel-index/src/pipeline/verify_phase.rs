use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Instant;

use codemycel_core::config::Config;
use codemycel_core::store::Tx;
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Coverage, DegradeReason, Edge, EdgeKind, NodeKind,
    NodeRef, RepoPath, Resolver, ResolverSource, Root, RootStatus, SourceFile, Unresolved,
    UnresolvedReason, Verdict, VerifiedBy,
};
use codemycel_lsp::{FileView, IndexView};

use super::ownership::{Owner, Ownership};
use super::{Progress, Staged};
use crate::servers::{RootJob, RootOutcome, Servers};
use crate::{IndexError, Registry};

/// An imported SCIP index, bound to the parsed files once they exist, and the hash of the file
/// it was read from.
#[derive(Clone, Copy)]
pub struct Scip<'a> {
    pub source: &'a dyn ResolverSource,
    pub hash: &'a str,
}

/// The verified sources a run consults, and where it reports progress.
pub struct Sources<'a> {
    pub servers: &'a dyn Servers,
    /// Without one every file goes to its root's server.
    pub scip: Option<Scip<'a>>,
    pub progress: &'a dyn Progress,
}

/// The files a verify run rebuilds. Every file is resolved against the whole index, but only
/// edges whose source lives in `files`, and only `files`' unresolved rows, are kept.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    pub files: BTreeSet<RepoPath>,
    /// Files whose traits and interfaces are asked for implementers; a superset of `files`.
    pub implementer_sources: BTreeSet<RepoPath>,
    /// Files added, modified, or deleted since the last sync.
    pub changed: Vec<RepoPath>,
}

impl Scope {
    pub fn everything(staged: &Staged, changed: Vec<RepoPath>) -> Self {
        let files: BTreeSet<RepoPath> = staged.code_files().into_iter().collect();
        Self {
            implementer_sources: files.clone(),
            files,
            changed,
        }
    }

    fn keeps(&self, edge: &Edge) -> bool {
        edge.src
            .path
            .file()
            .and_then(|file| RepoPath::new(file).ok())
            .is_some_and(|file| self.files.contains(&file))
    }
}

/// Verdicts, edges, and per-root status of a verify run, ready to write.
#[derive(Debug, Default)]
pub struct Verified {
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
    pub partially_enriched: BTreeSet<RepoPath>,
    /// Every parsed file SCIP covers, in scope or not.
    pub scip_covered: BTreeSet<RepoPath>,
    /// Roots whose status this run decided; a root with no file in scope keeps its status.
    pub statuses: Vec<(Root, RootStatus)>,
}

/// Every parsed file as the resolvers see it, and the files of each root.
struct Prepared<'s> {
    view: IndexView,
    per_root: BTreeMap<&'s Root, Vec<RepoPath>>,
}

impl<'s> Prepared<'s> {
    fn new(staged: &'s Staged) -> Result<Self, IndexError> {
        let mut view = IndexView::default();
        let mut per_root: BTreeMap<&Root, Vec<RepoPath>> = BTreeMap::new();
        for code in &staged.parsed {
            let path = &staged.files[code.file].path;
            view.insert(
                path.clone(),
                FileView::new(path, code.text.clone(), code.parsed.clone())?,
            );
            if let Some(root) = &staged.assignment[code.file] {
                per_root.entry(root).or_default().push(path.clone());
            }
        }
        Ok(Self { view, per_root })
    }

    fn source_files(&self) -> Vec<SourceFile<'_>> {
        self.per_root
            .values()
            .flatten()
            .filter_map(|path| {
                let file = self.view.file(path)?;
                Some(SourceFile {
                    path,
                    text: file.index.text(),
                    nodes: &file.nodes,
                })
            })
            .collect()
    }

    fn covered(&self, scip: Option<&dyn Resolver>) -> BTreeSet<RepoPath> {
        match scip {
            Some(scip) => self
                .per_root
                .values()
                .flatten()
                .filter(|file| scip.covers(file))
                .cloned()
                .collect(),
            None => BTreeSet::new(),
        }
    }
}

/// The parsed files SCIP would answer for.
pub fn scip_covered(
    staged: &Staged,
    scip: Option<Scip<'_>>,
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let prepared = Prepared::new(staged)?;
    let files = prepared.source_files();
    let bound = scip.map(|scip| scip.source.bind(&files));
    Ok(prepared.covered(bound.as_deref()))
}

/// Resolves the candidates of the files in `scope` with the one resolver that owns each file:
/// SCIP when it covers the file, else the server of the file's root. Edges are kept only from
/// the resolver that owns their source node's file.
pub fn run(
    staged: &Staged,
    repo_root: &Path,
    config: &Config,
    registry: &Registry,
    sources: &Sources<'_>,
    scope: &Scope,
) -> Result<Verified, IndexError> {
    let prepared = Prepared::new(staged)?;
    let (view, per_root) = (&prepared.view, &prepared.per_root);
    let source_files = prepared.source_files();
    let bound = sources.scip.map(|scip| scip.source.bind(&source_files));
    let scip = bound.as_deref();
    let scip_covered = prepared.covered(scip);
    let mut ownership = Ownership::default();
    for (root, files) in per_root {
        for file in files {
            let owner = if scip_covered.contains(file) {
                Owner::Scip
            } else {
                Owner::Lsp(root)
            };
            ownership.assign(file.clone(), owner);
        }
    }
    let mut verified = Verified::default();
    if let Some(scip) = scip {
        let mut edges = Vec::new();
        let mut unresolved = Vec::new();
        for file in &scip_covered {
            if let Some(file) = view.file(file) {
                scip_file(scip, file, view, &mut edges, &mut unresolved);
            }
        }
        let every_file = per_root.values().flatten().filter_map(|f| view.file(f));
        for file in every_file {
            edges.extend(implementer_edges(scip, file, VerifiedBy::Scip));
        }
        let edges = ownership.retain(Owner::Scip, edges);
        verified
            .edges
            .extend(edges.into_iter().filter(|edge| scope.keeps(edge)));
        verified.unresolved.extend(
            unresolved
                .into_iter()
                .filter(|u| scope.files.contains(&u.candidate.file)),
        );
    }
    for root in staged.roots.roots() {
        let files = per_root.get(root).map_or(&[][..], Vec::as_slice);
        let has_scip = files.iter().any(|file| scip_covered.contains(file));
        let lsp_all: Vec<&RepoPath> = files
            .iter()
            .filter(|file| !scip_covered.contains(*file))
            .collect();
        let lsp_files: Vec<RepoPath> = lsp_all
            .iter()
            .filter(|file| scope.files.contains(**file))
            .map(|file| (*file).clone())
            .collect();
        let status = if lsp_all.is_empty() {
            RootStatus {
                server: None,
                coverage: if has_scip {
                    Coverage::Scip
                } else {
                    Coverage::None
                },
                degraded: None,
                config_diagnostics: Vec::new(),
            }
        } else if lsp_files.is_empty() {
            continue;
        } else {
            let started = Instant::now();
            let server = |root: &Root| registry.server_for(root.language).map(|s| s.name());
            let enriched: BTreeSet<&RepoPath> = lsp_files.iter().collect();
            let others: Vec<RepoPath> = per_root
                .iter()
                .filter(|(other, _)| server(other).is_some() && server(other) == server(root))
                .flat_map(|(_, files)| files)
                .filter(|file| {
                    scope.implementer_sources.contains(*file) && !enriched.contains(file)
                })
                .cloned()
                .collect();
            let lsp = match (
                registry.server_for(root.language),
                registry.for_language(root.language),
            ) {
                (Some(server), Some(language)) => sources.servers.enrich(&RootJob {
                    repo_root,
                    config,
                    root,
                    server,
                    language,
                    view,
                    files: &lsp_files,
                    others: &others,
                    changed: &scope.changed,
                }),
                _ => RootOutcome::degraded(view, &lsp_files, DegradeReason::NoServer, None),
            };
            let coverage = match (lsp.degraded.is_none(), has_scip) {
                (true, true) => Coverage::ScipAndLsp,
                (true, false) => Coverage::Lsp,
                (false, true) => Coverage::Scip,
                (false, false) => Coverage::None,
            };
            let edges = ownership.retain(Owner::Lsp(root), lsp.edges);
            verified
                .edges
                .extend(edges.into_iter().filter(|edge| scope.keeps(edge)));
            verified.unresolved.extend(lsp.unresolved);
            verified.partially_enriched.extend(
                lsp.partially_enriched
                    .into_iter()
                    .filter(|file| scope.files.contains(file)),
            );
            let status = RootStatus {
                server: lsp.server,
                coverage,
                degraded: lsp.degraded,
                config_diagnostics: lsp.config_diagnostics,
            };
            sources.progress.root(root, &status, started.elapsed());
            status
        };
        verified.statuses.push((root.clone(), status));
    }
    verified.scip_covered = scip_covered;
    Ok(verified)
}

/// Writes edges, unresolved rows, partial-enrichment marks, and root status, then refreshes the
/// per-node unresolved counts. Every node the edges name must already be written.
pub fn write(tx: &mut Tx<'_>, verified: &Verified) -> Result<(), IndexError> {
    tx.insert_edges(&verified.edges)?;
    tx.insert_unresolved(&verified.unresolved)?;
    for file in &verified.partially_enriched {
        tx.set_partially_enriched(file)?;
    }
    for (root, status) in &verified.statuses {
        let id = tx.set_root(root)?;
        tx.set_root_status(id, status)?;
    }
    tx.refresh_unresolved_counts()?;
    Ok(())
}

/// `implements` edges for every trait and interface of `file`, and `overrides` edges for their
/// methods, as `resolver` reports them. Only structs, enums, classes and type aliases implement,
/// and only methods override.
fn implementer_edges(
    resolver: &dyn Resolver,
    file: &FileView,
    verified_by: VerifiedBy,
) -> Vec<Edge> {
    let edge = |src, dst, kind| Edge {
        src,
        dst,
        kind,
        verified_by,
        occurrences: Vec::new(),
    };
    let mut edges = Vec::new();
    for node in &file.nodes {
        if !matches!(node.kind, NodeKind::Trait | NodeKind::Interface) {
            continue;
        }
        let parent = node.node_ref();
        for implementer in resolver.implementers(&parent) {
            if is_implementer(implementer.kind) {
                edges.push(edge(implementer, parent.clone(), EdgeKind::Implements));
            }
        }
        for method in file.members(&parent) {
            if method.kind == NodeKind::Method {
                for overrider in resolver.implementers(method) {
                    if overrider.kind == NodeKind::Method {
                        edges.push(edge(overrider, method.clone(), EdgeKind::Overrides));
                    }
                }
            }
        }
    }
    edges
}

/// Edges of the impl blocks in `file` whose trait clause has a confirmed verdict: `implements`
/// from the self type to the trait, sited at the clause, when the self type is confirmed as a
/// type that implements, and `overrides` from each method in the block to the trait's method of
/// the same name.
fn impl_block_edges(view: &IndexView, file: &FileView, verdicts: &[Verdict]) -> Vec<Edge> {
    let placed: Vec<(&Candidate, u32, Option<&NodeRef>)> = file
        .candidates
        .iter()
        .zip(verdicts)
        .filter_map(|(candidate, verdict)| {
            let position = file.index.to_lsp(candidate.at)?;
            let target = match verdict {
                Verdict::Confirmed { target } => Some(target),
                Verdict::Unresolved { .. } => None,
            };
            Some((candidate, file.index.offset(position)?, target))
        })
        .collect();
    let inside = |range: ByteRange, at: u32| range.start <= at && at < range.end;
    let mut edges = Vec::new();
    for record in &file.impl_records {
        let clause = placed.iter().find_map(|(candidate, at, target)| {
            let is_clause = candidate.role == CandidateRole::Implements
                && !is_implementer(candidate.enclosing.kind)
                && inside(record.header, *at);
            is_clause.then_some((*candidate, (*target)?))
        });
        let Some((clause, parent)) = clause else {
            continue;
        };
        let self_type = placed
            .iter()
            .filter(|(_, at, _)| inside(record.self_type_selection, *at))
            .min_by_key(|(_, at, _)| *at)
            .and_then(|(_, _, target)| *target);
        if let Some(implementer) = self_type
            && is_implementer(implementer.kind)
        {
            edges.push(Edge {
                src: (*implementer).clone(),
                dst: (*parent).clone(),
                kind: EdgeKind::Implements,
                verified_by: VerifiedBy::Scip,
                occurrences: vec![clause.at],
            });
        }
        let parent_methods: Vec<&NodeRef> = parent
            .path
            .file()
            .and_then(|path| RepoPath::new(path).ok())
            .and_then(|path| view.file(&path))
            .map(|parent_file| {
                parent_file
                    .members(parent)
                    .iter()
                    .filter(|m| m.kind == NodeKind::Method)
                    .collect()
            })
            .unwrap_or_default();
        let block_methods = file.nodes.iter().filter(|n| {
            n.kind == NodeKind::Method
                && n.selection
                    .is_some_and(|s| record.body.start <= s.start && s.end <= record.body.end)
        });
        for method in block_methods {
            for parent_method in parent_methods
                .iter()
                .filter(|m| m.path.name() == method.name)
            {
                edges.push(Edge {
                    src: method.node_ref(),
                    dst: (*parent_method).clone(),
                    kind: EdgeKind::Overrides,
                    verified_by: VerifiedBy::Scip,
                    occurrences: Vec::new(),
                });
            }
        }
    }
    edges
}

fn is_implementer(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Class | NodeKind::TypeAlias
    )
}

/// Candidates through `resolve`, then the file's impl blocks, then `outgoing_calls` for
/// functions and methods.
fn scip_file(
    scip: &dyn Resolver,
    file: &FileView,
    view: &IndexView,
    edges: &mut Vec<Edge>,
    unresolved: &mut Vec<Unresolved>,
) {
    let verdicts = scip.resolve(&file.candidates);
    edges.extend(impl_block_edges(view, file, &verdicts));
    for (index, candidate) in file.candidates.iter().enumerate() {
        match verdicts.get(index) {
            Some(Verdict::Confirmed { target }) => {
                edges.extend(Edge::for_candidate(candidate, target, VerifiedBy::Scip));
            }
            Some(Verdict::Unresolved { reason }) => unresolved.push(Unresolved {
                candidate: candidate.clone(),
                reason: *reason,
            }),
            None => unresolved.push(Unresolved {
                candidate: candidate.clone(),
                reason: UnresolvedReason::ServerUnavailable,
            }),
        }
    }
    for node in &file.nodes {
        if matches!(node.kind, NodeKind::Function | NodeKind::Method) {
            let caller = node.node_ref();
            for (callee, occurrences) in scip.outgoing_calls(&caller).unwrap_or_default() {
                edges.push(Edge {
                    src: caller.clone(),
                    dst: callee,
                    kind: EdgeKind::Calls,
                    verified_by: VerifiedBy::Scip,
                    occurrences,
                });
            }
        }
    }
}
