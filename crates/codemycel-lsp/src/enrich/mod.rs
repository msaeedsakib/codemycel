mod mapping;
mod resolver;
mod session;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use codemycel_core::{
    Candidate, CandidateRole, DegradeReason, Edge, EdgeKind, ImplRecord, LanguageAdapter,
    ModelError, Node, NodeKind, NodeRef, Occurrence, ParsedFile, RepoPath, SemanticPath,
    Unresolved, UnresolvedReason, Verdict, VerifiedBy,
};
use futures::future::join_all;
use lsp_types::{Location, Range, Uri};

pub use mapping::{Mapper, Target};
pub use resolver::LspResolver;
pub use session::{Probe, Readiness, ServerLaunch, Session};

use crate::client::{ClientError, LineIndex, LspClient, path_to_uri};

/// One indexed file as the enricher sees it: its text, nodes, impl records, and candidates.
#[derive(Clone, Debug)]
pub struct FileView {
    pub index: LineIndex,
    pub file_node: NodeRef,
    pub nodes: Vec<Node>,
    pub impl_records: Vec<ImplRecord>,
    pub candidates: Vec<Candidate>,
    members: BTreeMap<NodeRef, Vec<NodeRef>>,
}

impl FileView {
    pub fn new(path: &RepoPath, text: String, parsed: ParsedFile) -> Result<Self, ModelError> {
        let mut members: BTreeMap<NodeRef, Vec<NodeRef>> = BTreeMap::new();
        for edge in parsed.structural_edges {
            if edge.kind == EdgeKind::Contains {
                members.entry(edge.src).or_default().push(edge.dst);
            }
        }
        Ok(Self {
            index: LineIndex::new(text),
            file_node: NodeRef::new(SemanticPath::for_file(path)?, NodeKind::File),
            nodes: parsed.nodes,
            impl_records: parsed.impl_records,
            candidates: parsed.candidates,
            members,
        })
    }

    /// The nodes `node` contains directly.
    pub fn members(&self, node: &NodeRef) -> &[NodeRef] {
        self.members.get(node).map_or(&[], Vec::as_slice)
    }

    fn node(&self, node: &NodeRef) -> Option<&Node> {
        self.nodes
            .iter()
            .find(|n| n.path == node.path && n.kind == node.kind)
    }
}

/// Every parsed file a server's answers can land in, keyed by path. Files absent from the view
/// are unindexed.
#[derive(Clone, Debug, Default)]
pub struct IndexView {
    files: BTreeMap<RepoPath, FileView>,
}

impl IndexView {
    pub fn insert(&mut self, path: RepoPath, file: FileView) {
        self.files.insert(path, file);
    }

    pub fn file(&self, path: &RepoPath) -> Option<&FileView> {
        self.files.get(path)
    }
}

/// What enriching a root's files produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EnrichReport {
    pub edges: Vec<Edge>,
    pub unresolved: Vec<Unresolved>,
    /// Files with at least one request that failed twice.
    pub partially_enriched: BTreeSet<RepoPath>,
    /// Set when the server crashed and its one restart failed or crashed too; the report then
    /// holds no edges and every candidate as `server_unavailable`.
    pub degraded: Option<DegradeReason>,
    pub config_diagnostics: Vec<String>,
}

impl EnrichReport {
    fn sort(&mut self) {
        self.edges
            .sort_by(|a, b| (&a.src, &a.dst, a.kind).cmp(&(&b.src, &b.dst, b.kind)));
        for edge in &mut self.edges {
            edge.occurrences.sort();
        }
        self.unresolved.sort_by(|a, b| {
            let key = |u: &Unresolved| {
                (
                    u.candidate.file.clone(),
                    u.candidate.at,
                    u.candidate.text.clone(),
                    u.candidate.role,
                )
            };
            key(a).cmp(&key(b))
        });
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EnrichOptions {
    /// Per request; a request that times out twice counts as failed.
    pub request_timeout: Duration,
}

impl Default for EnrichOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(30),
        }
    }
}

enum Failure {
    Crashed,
    Failed,
}

#[derive(Default)]
struct FileOutcome {
    edges: Vec<Edge>,
    unresolved: Vec<Unresolved>,
    partial: bool,
}

/// Runs the enrichment algorithm for one root over a started, ready server.
pub struct Enricher<'a> {
    session: Session<'a>,
    mapper: Mapper<'a>,
    view: &'a IndexView,
    repo_root: PathBuf,
    probe: Option<Probe>,
    readiness: Readiness,
    options: EnrichOptions,
    restarted: AtomicBool,
}

impl<'a> Enricher<'a> {
    /// Starts the root's server and waits until it is ready. `files` are the root's files this
    /// server will enrich; the first is the readiness probe's file.
    pub async fn start(
        launch: ServerLaunch<'a>,
        language: &'a dyn LanguageAdapter,
        view: &'a IndexView,
        repo_root: &Path,
        root: &RepoPath,
        files: &[RepoPath],
        options: EnrichOptions,
    ) -> Result<Self, DegradeReason> {
        let adapter = launch.adapter;
        let session = Session::start(launch, &absolute(repo_root, root)).await?;
        let mut sorted = files.to_vec();
        sorted.sort();
        let probe = sorted.first().and_then(|file| {
            probe_for(view, repo_root, file, adapter.language_id(extension(file)))
        });
        let readiness = session.wait_ready(probe.as_ref()).await;
        if readiness == Readiness::NotReady {
            session.shutdown().await;
            return Err(DegradeReason::NotReady);
        }
        Ok(Self {
            session,
            mapper: Mapper::new(view, language, repo_root),
            view,
            repo_root: repo_root.to_owned(),
            probe,
            readiness,
            options,
            restarted: AtomicBool::new(false),
        })
    }

    pub fn readiness(&self) -> Readiness {
        self.readiness
    }

    /// Enriches `files` in sorted order, restarting a crashed server once.
    pub async fn enrich_files(&self, files: &[RepoPath]) -> EnrichReport {
        let mut sorted = files.to_vec();
        sorted.sort();
        let mut report = EnrichReport::default();
        let mut next = 0;
        while let Some(file) = sorted.get(next) {
            match self.enrich_file(file).await {
                Ok(outcome) => {
                    report.edges.extend(outcome.edges);
                    report.unresolved.extend(outcome.unresolved);
                    if outcome.partial {
                        report.partially_enriched.insert(file.clone());
                    }
                    next += 1;
                }
                Err(Failure::Failed) => {
                    report.partially_enriched.insert(file.clone());
                    report
                        .unresolved
                        .extend(self.unavailable(std::slice::from_ref(file)));
                    next += 1;
                }
                Err(Failure::Crashed) => {
                    let restart_allowed = !self.restarted.swap(true, Ordering::SeqCst);
                    if !restart_allowed || self.session.restart(self.probe.as_ref()).await.is_err()
                    {
                        return self.degraded(&sorted, DegradeReason::Crashed);
                    }
                }
            }
        }
        report.config_diagnostics = self.session.config_diagnostics();
        report.sort();
        report
    }

    fn degraded(&self, files: &[RepoPath], reason: DegradeReason) -> EnrichReport {
        let mut report = EnrichReport {
            unresolved: self.unavailable(files),
            degraded: Some(reason),
            ..EnrichReport::default()
        };
        report.sort();
        report
    }

    fn unavailable(&self, files: &[RepoPath]) -> Vec<Unresolved> {
        files
            .iter()
            .filter_map(|file| self.view.file(file))
            .flat_map(|view| &view.candidates)
            .map(|candidate| Unresolved {
                candidate: candidate.clone(),
                reason: UnresolvedReason::ServerUnavailable,
            })
            .collect()
    }

    async fn enrich_file(&self, file: &RepoPath) -> Result<FileOutcome, Failure> {
        let Some(view) = self.view.file(file) else {
            return Ok(FileOutcome::default());
        };
        let client = self.session.client().ok_or(Failure::Crashed)?;
        let uri = self.uri(file).map_err(|_| Failure::Failed)?;
        let language_id = self.session.adapter().language_id(extension(file));
        client
            .did_open(&uri, language_id, view.index.text())
            .map_err(|_| Failure::Crashed)?;
        let outcome = self.enrich_open_file(&client, &uri, view).await;
        let _ = client.did_close(&uri);
        outcome
    }

    async fn enrich_open_file(
        &self,
        client: &LspClient,
        uri: &Uri,
        view: &FileView,
    ) -> Result<FileOutcome, Failure> {
        let mut outcome = FileOutcome::default();
        let callables: Vec<&Node> = view
            .nodes
            .iter()
            .filter(|n| is_callable(n.kind) && n.selection.is_some())
            .collect();
        let calls = join_all(
            callables
                .iter()
                .map(|node| self.outgoing(client, uri, view, node)),
        )
        .await;
        let mut confirmed = HashSet::new();
        for (node, calls) in callables.iter().zip(calls) {
            let calls = match calls {
                Ok(calls) => calls,
                Err(Failure::Crashed) => return Err(Failure::Crashed),
                Err(Failure::Failed) => {
                    outcome.partial = true;
                    continue;
                }
            };
            let caller = node.node_ref();
            for (callee, ranges) in calls {
                for (index, candidate) in view.candidates.iter().enumerate() {
                    let covered = candidate.role == CandidateRole::Call
                        && candidate.enclosing == caller
                        && view
                            .index
                            .to_lsp(candidate.at)
                            .is_some_and(|at| ranges.iter().any(|r| r.start <= at && at < r.end));
                    if covered {
                        confirmed.insert(index);
                    }
                }
                outcome.edges.push(Edge {
                    src: caller.clone(),
                    dst: callee,
                    kind: EdgeKind::Calls,
                    verified_by: VerifiedBy::Lsp,
                    occurrences: ranges
                        .iter()
                        .filter_map(|r| view.index.from_lsp(r.start))
                        .collect(),
                });
            }
        }
        let pending: Vec<&Candidate> = view
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| !confirmed.contains(index))
            .map(|(_, candidate)| candidate)
            .collect();
        let verdicts = join_all(
            pending
                .iter()
                .map(|candidate| self.define(client, uri, view, candidate)),
        )
        .await;
        for (candidate, verdict) in pending.into_iter().zip(verdicts) {
            let reason = match verdict {
                Ok(Verdict::Confirmed { target }) => {
                    outcome
                        .edges
                        .extend(Edge::for_candidate(candidate, &target, VerifiedBy::Lsp));
                    continue;
                }
                Ok(Verdict::Unresolved { reason }) => reason,
                Err(Failure::Crashed) => return Err(Failure::Crashed),
                Err(Failure::Failed) => {
                    outcome.partial = true;
                    UnresolvedReason::ServerUnavailable
                }
            };
            outcome.unresolved.push(Unresolved {
                candidate: candidate.clone(),
                reason,
            });
        }
        let parents: Vec<(&Node, Option<&Node>)> = view
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Trait | NodeKind::Interface))
            .flat_map(|parent| {
                let methods = view
                    .members(&parent.node_ref())
                    .iter()
                    .filter(|m| m.kind == NodeKind::Method)
                    .filter_map(|m| view.node(m))
                    .map(move |m| (parent, Some(m)));
                std::iter::once((parent, None)).chain(methods)
            })
            .collect();
        let found = join_all(
            parents
                .iter()
                .map(|(parent, method)| self.implementations(client, uri, view, parent, *method)),
        )
        .await;
        for edges in found {
            match edges {
                Ok(edges) => outcome.edges.extend(edges),
                Err(Failure::Crashed) => return Err(Failure::Crashed),
                Err(Failure::Failed) => outcome.partial = true,
            }
        }
        Ok(outcome)
    }

    /// Callees of `node` that map to functions or methods, with the call sites in its file.
    async fn outgoing(
        &self,
        client: &LspClient,
        uri: &Uri,
        view: &FileView,
        node: &Node,
    ) -> Result<Vec<(NodeRef, Vec<Range>)>, Failure> {
        let Some(position) = node.selection.and_then(|s| view.index.position(s.start)) else {
            return Ok(Vec::new());
        };
        let timeout = self.options.request_timeout;
        let items = twice(|| client.prepare_call_hierarchy(uri, position, timeout)).await?;
        let Some(item) = items.first() else {
            return Ok(Vec::new());
        };
        let calls = twice(|| client.outgoing_calls(item, timeout)).await?;
        let mut callees: BTreeMap<NodeRef, Vec<Range>> = BTreeMap::new();
        for call in calls {
            if let Target::Node(callee) = self.mapper.locate(&call.to.uri, call.to.selection_range)
                && is_callable(callee.kind)
            {
                callees.entry(callee).or_default().extend(call.from_ranges);
            }
        }
        Ok(callees.into_iter().collect())
    }

    async fn define(
        &self,
        client: &LspClient,
        uri: &Uri,
        view: &FileView,
        candidate: &Candidate,
    ) -> Result<Verdict, Failure> {
        let Some(position) = view.index.to_lsp(candidate.at) else {
            return Ok(Verdict::Unresolved {
                reason: UnresolvedReason::NoDefinition,
            });
        };
        let timeout = self.options.request_timeout;
        let locations = twice(|| client.definition(uri, position, timeout)).await?;
        let verdict = self.verdict(&locations);
        if matches!(verdict, Verdict::Confirmed { .. }) {
            return Ok(verdict);
        }
        for location in &locations {
            if let Target::ImplHeader { file, index } =
                self.mapper.locate(&location.uri, location.range)
                && let Some(target) = self.self_type(client, &file, index).await?
            {
                return Ok(Verdict::Confirmed { target });
            }
        }
        Ok(verdict)
    }

    /// The node an impl record's self type is defined as, through `definition` on the type's
    /// identifier in the header.
    async fn self_type(
        &self,
        client: &LspClient,
        file: &RepoPath,
        index: usize,
    ) -> Result<Option<NodeRef>, Failure> {
        let Some(view) = self.view.file(file) else {
            return Ok(None);
        };
        let Some(record) = view.impl_records.get(index) else {
            return Ok(None);
        };
        let Some(position) = view.index.position(record.self_type_selection.start) else {
            return Ok(None);
        };
        let uri = self.uri(file).map_err(|_| Failure::Failed)?;
        let timeout = self.options.request_timeout;
        let definitions = twice(|| client.definition(&uri, position, timeout)).await?;
        Ok(definitions
            .iter()
            .find_map(|l| match self.mapper.locate(&l.uri, l.range) {
                Target::Node(node) => Some(node),
                _ => None,
            }))
    }

    /// The first location that maps to a node confirms; otherwise the first location's reason.
    fn verdict(&self, locations: &[Location]) -> Verdict {
        let mut reason = None;
        for location in locations {
            match self.mapper.locate(&location.uri, location.range) {
                Target::Node(target) => return Verdict::Confirmed { target },
                other => {
                    reason = reason.or(other.reason());
                }
            }
        }
        Verdict::Unresolved {
            reason: reason.unwrap_or(UnresolvedReason::NoDefinition),
        }
    }

    /// `implements` and `overrides` edges for a trait or interface, or for one of its methods.
    async fn implementations(
        &self,
        client: &LspClient,
        uri: &Uri,
        view: &FileView,
        parent: &Node,
        method: Option<&Node>,
    ) -> Result<Vec<Edge>, Failure> {
        let asked = method.unwrap_or(parent);
        let Some(position) = asked.selection.and_then(|s| view.index.position(s.start)) else {
            return Ok(Vec::new());
        };
        let timeout = self.options.request_timeout;
        let locations = twice(|| client.implementation(uri, position, timeout)).await?;
        let mut edges = Vec::new();
        for location in locations {
            let at = self.occurrence(&location);
            match self.mapper.locate(&location.uri, location.range) {
                Target::Node(found) => match method {
                    None if is_implementer(found.kind) => {
                        edges.push(lsp_edge(found, parent.node_ref(), EdgeKind::Implements, at));
                    }
                    Some(method)
                        if found.kind == NodeKind::Method && found != method.node_ref() =>
                    {
                        edges.push(lsp_edge(found, method.node_ref(), EdgeKind::Overrides, at));
                    }
                    _ => {}
                },
                Target::ImplHeader { file, index } => {
                    edges.extend(
                        self.impl_block_edges(client, &file, index, view, parent, method, at)
                            .await?,
                    );
                }
                _ => {}
            }
        }
        Ok(edges)
    }

    /// Converts an `implementation` result that landed on an impl header: the self type's
    /// definition is the implementer, and the block's methods override the parent's methods of
    /// the same name.
    #[allow(clippy::too_many_arguments)]
    async fn impl_block_edges(
        &self,
        client: &LspClient,
        file: &RepoPath,
        index: usize,
        parent_view: &FileView,
        parent: &Node,
        method: Option<&Node>,
        at: Option<Occurrence>,
    ) -> Result<Vec<Edge>, Failure> {
        let Some(block_view) = self.view.file(file) else {
            return Ok(Vec::new());
        };
        let Some(record) = block_view.impl_records.get(index) else {
            return Ok(Vec::new());
        };
        let implementer = self.self_type(client, file, index).await?;
        let Some(implementer) = implementer.filter(|node| is_implementer(node.kind)) else {
            return Ok(Vec::new());
        };
        let block_methods: Vec<&Node> = block_view
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Method)
            .filter(|n| {
                n.selection
                    .is_some_and(|s| record.body.start <= s.start && s.end <= record.body.end)
            })
            .collect();
        let site = |node: &Node| {
            node.selection
                .and_then(|s| block_view.index.position(s.start))
                .and_then(|p| block_view.index.from_lsp(p))
        };
        let mut edges = Vec::new();
        let parent_methods: Vec<NodeRef> = match method {
            Some(method) => vec![method.node_ref()],
            None => {
                edges.push(lsp_edge(
                    implementer,
                    parent.node_ref(),
                    EdgeKind::Implements,
                    at,
                ));
                parent_view
                    .members(&parent.node_ref())
                    .iter()
                    .filter(|m| m.kind == NodeKind::Method)
                    .cloned()
                    .collect()
            }
        };
        for parent_method in &parent_methods {
            for block_method in &block_methods {
                if block_method.name == parent_method.path.name() {
                    edges.push(lsp_edge(
                        block_method.node_ref(),
                        parent_method.clone(),
                        EdgeKind::Overrides,
                        site(block_method),
                    ));
                }
            }
        }
        Ok(edges)
    }

    fn occurrence(&self, location: &Location) -> Option<Occurrence> {
        let file = self.mapper.repo_path(&location.uri).ok()?;
        self.view.file(&file)?.index.from_lsp(location.range.start)
    }

    fn uri(&self, file: &RepoPath) -> Result<Uri, ClientError> {
        path_to_uri(&absolute(&self.repo_root, file))
    }

    /// Verdicts for candidates of any files of the root, opening each file around its requests.
    pub async fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        let mut verdicts = Vec::with_capacity(candidates.len());
        for candidate in candidates {
            let unavailable = Verdict::Unresolved {
                reason: UnresolvedReason::ServerUnavailable,
            };
            let verdict = match self.open(&candidate.file).await {
                Some((client, uri, view)) => {
                    let verdict = self.define(&client, &uri, view, candidate).await;
                    let _ = client.did_close(&uri);
                    verdict.unwrap_or(unavailable)
                }
                None => unavailable,
            };
            verdicts.push(verdict);
        }
        verdicts
    }

    /// Implementers of a trait or interface, or overriders of one of its methods.
    pub async fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        let Some(file) = node.path.file().and_then(|f| RepoPath::new(f).ok()) else {
            return Vec::new();
        };
        let Some((client, uri, view)) = self.open(&file).await else {
            return Vec::new();
        };
        let (parent, method) = match view.node(node) {
            Some(n) if matches!(n.kind, NodeKind::Trait | NodeKind::Interface) => (Some(n), None),
            Some(n) => (
                view.nodes
                    .iter()
                    .find(|p| view.members(&p.node_ref()).contains(node)),
                Some(n),
            ),
            None => (None, None),
        };
        let edges = match parent {
            Some(parent) => self
                .implementations(&client, &uri, view, parent, method)
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let _ = client.did_close(&uri);
        let mut found: Vec<NodeRef> = edges
            .into_iter()
            .filter(|e| &e.dst == node)
            .map(|e| e.src)
            .collect();
        found.sort();
        found.dedup();
        found
    }

    /// Callees of a function or method with their call sites.
    pub async fn outgoing_calls(&self, node: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        let file = RepoPath::new(node.path.file()?).ok()?;
        let (client, uri, view) = self.open(&file).await?;
        let calls = match view.node(node) {
            Some(n) => self.outgoing(&client, &uri, view, n).await.ok(),
            None => None,
        };
        let _ = client.did_close(&uri);
        Some(
            calls?
                .into_iter()
                .map(|(callee, ranges)| {
                    let sites = ranges
                        .iter()
                        .filter_map(|r| view.index.from_lsp(r.start))
                        .collect();
                    (callee, sites)
                })
                .collect(),
        )
    }

    async fn open(
        &self,
        file: &RepoPath,
    ) -> Option<(std::sync::Arc<LspClient>, Uri, &'a FileView)> {
        let view = self.view.file(file)?;
        let client = self.session.client()?;
        let uri = self.uri(file).ok()?;
        client
            .did_open(
                &uri,
                self.session.adapter().language_id(extension(file)),
                view.index.text(),
            )
            .ok()?;
        Some((client, uri, view))
    }

    pub async fn shutdown(self) {
        self.session.shutdown().await;
    }
}

async fn twice<T, F, Fut>(mut call: F) -> Result<T, Failure>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, ClientError>>,
{
    match call().await {
        Ok(value) => return Ok(value),
        Err(ClientError::Closed) => return Err(Failure::Crashed),
        Err(_) => {}
    }
    match call().await {
        Ok(value) => Ok(value),
        Err(ClientError::Closed) => Err(Failure::Crashed),
        Err(_) => Err(Failure::Failed),
    }
}

fn lsp_edge(src: NodeRef, dst: NodeRef, kind: EdgeKind, at: Option<Occurrence>) -> Edge {
    Edge {
        src,
        dst,
        kind,
        verified_by: VerifiedBy::Lsp,
        occurrences: at.into_iter().collect(),
    }
}

fn is_callable(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Function | NodeKind::Method)
}

fn is_implementer(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Class | NodeKind::TypeAlias
    )
}

fn probe_for(
    view: &IndexView,
    repo_root: &Path,
    file: &RepoPath,
    language_id: &'static str,
) -> Option<Probe> {
    let view = view.file(file)?;
    let candidate = view
        .candidates
        .iter()
        .find(|c| c.role == CandidateRole::Import)
        .or_else(|| view.candidates.first())?;
    Some(Probe {
        uri: path_to_uri(&absolute(repo_root, file)).ok()?,
        language_id,
        text: view.index.text().to_owned(),
        position: view.index.to_lsp(candidate.at)?,
    })
}

/// The file's extension without the dot; empty when it has none.
pub fn extension(file: &RepoPath) -> &str {
    let name = file.as_str().rsplit('/').next().unwrap_or_default();
    name.rsplit_once('.').map_or("", |(_, ext)| ext)
}

/// `repo_root` joined with a repository path, one component at a time.
pub fn absolute(repo_root: &Path, path: &RepoPath) -> PathBuf {
    if path.is_root() {
        return repo_root.to_owned();
    }
    path.as_str()
        .split('/')
        .fold(repo_root.to_owned(), |dir, part| dir.join(part))
}
