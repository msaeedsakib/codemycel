use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::store::Tx;
use codemycel_core::{
    ByteRange, Candidate, CandidateRole, Coverage, DegradeReason, Edge, EdgeKind, NodeKind,
    NodeRef, RepoPath, Resolver, ResolverSource, Root, RootStatus, ServerInfo, SourceFile,
    Unresolved, UnresolvedReason, Verdict, VerifiedBy,
};
use codemycel_lsp::{EnrichOptions, FileView, IndexView, LspResolver, Provisioner, ServerLaunch};

use super::ownership::{Owner, Ownership};
use super::parse_phase::ParsedCode;
use crate::{IndexError, Registry};

/// The verified sources a full run consults.
pub struct Sources<'a> {
    pub provisioner: &'a Provisioner,
    /// An imported SCIP index, bound to the parsed files once they exist; without one every
    /// file goes to its root's server.
    pub scip: Option<&'a dyn ResolverSource>,
    pub options: EnrichOptions,
}

/// Verdicts, edges, and per-root status of a verify run, ready to write.
#[derive(Debug, Default)]
pub struct Verified {
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
    pub partially_enriched: BTreeSet<RepoPath>,
    pub scip_covered: BTreeSet<RepoPath>,
    pub statuses: Vec<(Root, RootStatus)>,
}

/// Resolves every parsed file's candidates with the one resolver that owns it: SCIP when it
/// covers the file, else the server of the file's root. Edges are kept only from the resolver
/// that owns their source node's file.
#[allow(clippy::too_many_arguments)]
pub fn run(
    repo_root: &Path,
    config: &Config,
    roots: &[Root],
    paths: &[RepoPath],
    assignment: &[Option<Root>],
    parsed: &[ParsedCode],
    registry: &Registry,
    sources: &Sources<'_>,
) -> Result<Verified, IndexError> {
    let mut view = IndexView::default();
    let mut per_root: BTreeMap<&Root, Vec<RepoPath>> = BTreeMap::new();
    for code in parsed {
        let path = &paths[code.file];
        view.insert(
            path.clone(),
            FileView::new(path, code.text.clone(), code.parsed.clone())?,
        );
        if let Some(root) = &assignment[code.file] {
            per_root.entry(root).or_default().push(path.clone());
        }
    }
    let source_files: Vec<SourceFile<'_>> = per_root
        .values()
        .flatten()
        .filter_map(|path| {
            let file = view.file(path)?;
            Some(SourceFile {
                path,
                text: file.index.text(),
                nodes: &file.nodes,
            })
        })
        .collect();
    let bound = sources.scip.map(|source| source.bind(&source_files));
    let scip = bound.as_deref();
    let scip_covered: BTreeSet<RepoPath> = match scip {
        Some(scip) => per_root
            .values()
            .flatten()
            .filter(|file| scip.covers(file))
            .cloned()
            .collect(),
        None => BTreeSet::new(),
    };
    let mut ownership = Ownership::default();
    for (root, files) in &per_root {
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
        for file in &scip_covered {
            if let Some(file) = view.file(file) {
                scip_file(scip, file, &view, &mut edges, &mut verified.unresolved);
            }
        }
        let every_file = per_root.values().flatten().filter_map(|f| view.file(f));
        for file in every_file {
            edges.extend(implementer_edges(scip, file, VerifiedBy::Scip));
        }
        verified.edges.extend(ownership.retain(Owner::Scip, edges));
    }
    for root in roots {
        let files = per_root.get(root).map_or(&[][..], Vec::as_slice);
        let has_scip = files.iter().any(|file| scip_covered.contains(file));
        let lsp_files: Vec<RepoPath> = files
            .iter()
            .filter(|file| !scip_covered.contains(file))
            .cloned()
            .collect();
        let status = if lsp_files.is_empty() {
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
        } else {
            let server = |root: &Root| registry.server_for(root.language).map(|s| s.name());
            let scip_covered = &scip_covered;
            let others: Vec<RepoPath> = per_root
                .iter()
                .filter(|(other, _)| server(other).is_some() && server(other) == server(root))
                .flat_map(|(other, files)| {
                    files
                        .iter()
                        .filter(move |file| *other != root || scip_covered.contains(*file))
                })
                .cloned()
                .collect();
            let lsp = enrich_root(
                repo_root, config, root, &lsp_files, &others, &view, registry, sources,
            );
            let coverage = match (lsp.degraded.is_none(), has_scip) {
                (true, true) => Coverage::ScipAndLsp,
                (true, false) => Coverage::Lsp,
                (false, true) => Coverage::Scip,
                (false, false) => Coverage::None,
            };
            verified
                .edges
                .extend(ownership.retain(Owner::Lsp(root), lsp.edges));
            verified.unresolved.extend(lsp.unresolved);
            verified.partially_enriched.extend(lsp.partially_enriched);
            RootStatus {
                server: lsp.server,
                coverage,
                degraded: lsp.degraded,
                config_diagnostics: lsp.config_diagnostics,
            }
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

#[derive(Default)]
struct RootOutcome {
    server: Option<ServerInfo>,
    degraded: Option<DegradeReason>,
    edges: Vec<Edge>,
    unresolved: Vec<Unresolved>,
    partially_enriched: BTreeSet<RepoPath>,
    config_diagnostics: Vec<String>,
}

/// Enriches the root's LSP files. Traits and interfaces in `others`, the files this server did
/// not enrich but can answer for, are asked for their implementers too, since an implementer in
/// one of the root's LSP files is this server's to write.
#[allow(clippy::too_many_arguments)]
fn enrich_root(
    repo_root: &Path,
    config: &Config,
    root: &Root,
    files: &[RepoPath],
    others: &[RepoPath],
    view: &IndexView,
    registry: &Registry,
    sources: &Sources<'_>,
) -> RootOutcome {
    let degraded = |reason, server| RootOutcome {
        server,
        degraded: Some(reason),
        unresolved: files
            .iter()
            .filter_map(|file| view.file(file))
            .flat_map(|file| &file.candidates)
            .map(|candidate| Unresolved {
                candidate: candidate.clone(),
                reason: UnresolvedReason::ServerUnavailable,
            })
            .collect(),
        ..RootOutcome::default()
    };
    let (Some(server), Some(language)) = (
        registry.server_for(root.language),
        registry.for_language(root.language),
    ) else {
        return degraded(DegradeReason::NoServer, None);
    };
    if !sources.provisioner.prerequisite_present(server) {
        return degraded(DegradeReason::NoCargo, None);
    }
    let located = match sources.provisioner.locate(server) {
        Ok(located) => located,
        Err(error) => {
            tracing::warn!(root = %root.path, "{error}");
            return degraded(DegradeReason::NoServer, None);
        }
    };
    let info = ServerInfo {
        name: server.name().to_owned(),
        version: located.version.clone(),
        source: located.source,
    };
    let launch = ServerLaunch {
        adapter: server,
        binary: located.path,
        config: config.server(server.language()),
        search_path: sources.provisioner.search_path().map(ToOwned::to_owned),
    };
    let resolver = match LspResolver::start(
        launch,
        language,
        view,
        repo_root,
        &root.path,
        files,
        sources.options,
    ) {
        Ok(resolver) => resolver,
        Err(reason) => return degraded(reason, Some(info)),
    };
    let mut report = resolver.enrich_files(files);
    if report.degraded.is_none() {
        let found = resolver.enrich_implementers(others);
        report.degraded = found.degraded;
        report.edges.extend(found.edges);
        report.partially_enriched.extend(found.partially_enriched);
    }
    resolver.shutdown();
    if let Some(reason) = report.degraded {
        return degraded(reason, Some(info));
    }
    RootOutcome {
        server: Some(info),
        degraded: None,
        edges: report.edges,
        unresolved: report.unresolved,
        partially_enriched: report.partially_enriched,
        config_diagnostics: report.config_diagnostics,
    }
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
    let confirmed: Vec<(&Candidate, u32, &NodeRef)> = file
        .candidates
        .iter()
        .zip(verdicts)
        .filter_map(|(candidate, verdict)| match verdict {
            Verdict::Confirmed { target } => {
                let position = file.index.to_lsp(candidate.at)?;
                Some((candidate, file.index.offset(position)?, target))
            }
            Verdict::Unresolved { .. } => None,
        })
        .collect();
    let inside = |range: ByteRange, at: u32| range.start <= at && at < range.end;
    let mut edges = Vec::new();
    for record in &file.impl_records {
        let clause = confirmed.iter().find(|(candidate, at, _)| {
            candidate.role == CandidateRole::Implements
                && !is_implementer(candidate.enclosing.kind)
                && inside(record.header, *at)
        });
        let Some((clause, _, parent)) = clause else {
            continue;
        };
        let self_type = confirmed
            .iter()
            .find(|(_, at, _)| inside(record.self_type_selection, *at));
        if let Some((_, _, implementer)) = self_type
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
