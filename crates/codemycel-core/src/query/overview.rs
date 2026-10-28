use std::collections::BTreeMap;

use super::impact::plural;
use super::status::{root_label, root_state};
use super::{
    Answer, Budget, Direction, Line, NodeLine, Outcome, QueryEngine, Result, Section, SymbolCard,
};
use crate::store::{StoreRead, StoredEdge};
use crate::{EdgeKind, NodeId, NodeKind, RepoPath, SemanticPath, StoredNode};

/// The relations between symbols that rank god nodes and top symbols and that cross community
/// boundaries.
const RELATIONS: [EdgeKind; 5] = [
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];
const GOD_NODES: u32 = 10;
const TOP_MEMBERS: u32 = 3;
const TOP_SYMBOLS: u32 = 3;
const COMMUNITY_PREFIX: &str = "community:";

impl QueryEngine<'_> {
    /// An orientation answer. `scope` is resolved in this order: omitted, empty or `.` is the
    /// repository; a `community:<id>` prefix names a community; then an indexed file path; then
    /// a directory holding indexed files; then a package name.
    pub fn overview(&self, scope: Option<&str>, budget: Budget) -> Result<Answer> {
        let scope = scope
            .map(|scope| scope.trim().trim_end_matches('/'))
            .filter(|scope| !scope.is_empty() && *scope != ".");
        let Some(scope) = scope else {
            return self.overview_repository(budget);
        };
        if scope.starts_with(COMMUNITY_PREFIX) {
            return self.overview_community(scope, budget);
        }
        let store = self.store();
        if let Ok(path) = RepoPath::new(scope) {
            if store.file_by_path(&path)?.is_some() {
                return self.overview_file(&path, budget);
            }
            let prefix = format!("{}/", path.as_str());
            let files: Vec<RepoPath> = store
                .files()?
                .into_iter()
                .map(|file| file.record.path)
                .filter(|file| file.as_str().starts_with(&prefix))
                .collect();
            if !files.is_empty() {
                let heading = Line::field("directory", path.as_str());
                return self.overview_area(heading, files, budget);
            }
        }
        let packages: Vec<StoredNode> = store
            .nodes_named(scope)?
            .into_iter()
            .filter(|node| node.node.kind == NodeKind::Package)
            .collect();
        match packages.as_slice() {
            [] => self.not_found(
                format!("no file, directory, package, or community `{scope}`"),
                budget,
            ),
            [package] => self.overview_package(package, budget),
            _ => {
                let lines = packages
                    .iter()
                    .map(|n| Line::Node(NodeLine::of(n)))
                    .collect();
                let section = Section::new("candidates", 0, lines);
                self.finish(
                    Outcome::Ambiguous,
                    vec![section],
                    budget,
                    "pass a semantic path",
                )
            }
        }
    }

    fn overview_repository(&self, budget: Budget) -> Result<Answer> {
        let store = self.store();
        let packages = store.packages()?;
        let mut package_lines: Vec<Line> = packages
            .iter()
            .filter(|package| !package.external)
            .map(|package| {
                let name = match &package.version {
                    Some(version) => format!("{} {version}", package.name),
                    None => package.name.clone(),
                };
                Line::field(name, package.node.path.as_str())
            })
            .collect();
        let external = packages.iter().filter(|package| package.external).count();
        if external > 0 {
            package_lines.push(Line::text(format!(
                "and {} outside the repository",
                plural(external, "dependency")
            )));
        }
        let roots = store
            .roots()?
            .iter()
            .map(|root| Line::field(root_label(root), root_state(root, false)))
            .collect();
        let mut communities = Vec::new();
        for community in store.communities()? {
            let Ok(path) = SemanticPath::community(&community.stable_id) else {
                continue;
            };
            let Some(node) = store
                .node_by_semantic_path(&path, Some(NodeKind::Community))?
                .into_iter()
                .next()
            else {
                continue;
            };
            let top = self.ranked(Some(node.id), None, TOP_MEMBERS)?;
            let names: Vec<&str> = top.iter().map(in_file).collect();
            communities.push((
                community.members.len(),
                format!(
                    "derived:{} ({path})  {} · top: {}",
                    community.label,
                    plural(community.members.len(), "member"),
                    names.join(", ")
                ),
            ));
        }
        communities.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let communities = communities
            .into_iter()
            .map(|(_, text)| Line::text(text))
            .collect();
        let gods = store
            .nodes_by_in_degree(&RELATIONS, None, None, GOD_NODES)?
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(id, _)| Ok(Line::Node(NodeLine::of(&self.node(id)?))))
            .collect::<Result<Vec<_>>>()?;
        let sections = vec![
            Section::new("roots", 0, roots),
            Section::new("packages", 1, package_lines),
            Section::new("counts", 1, self.count_lines()?),
            Section::new("god_nodes", 2, gods),
            Section::new("communities", 3, communities),
        ];
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "raise max_tokens or pass a scope",
        )
    }

    fn overview_package(&self, package: &StoredNode, budget: Budget) -> Result<Answer> {
        let files: Vec<RepoPath> = self
            .store()
            .edges_from(package.id, &[EdgeKind::Contains])?
            .into_iter()
            .filter(|edge| edge.edge.dst.kind == NodeKind::File)
            .filter_map(|edge| {
                edge.edge
                    .dst
                    .path
                    .file()
                    .and_then(|f| RepoPath::new(f).ok())
            })
            .collect();
        let heading = Line::field(
            "package",
            format!("{} ({})", package.node.name, package.node.path),
        );
        if files.is_empty() {
            let section = Section::new(
                "overview",
                0,
                vec![heading, Line::text("no files in the repository")],
            )
            .without_header();
            return self.finish(Outcome::Ok, vec![section], budget, "");
        }
        self.overview_area(heading, files, budget)
    }

    /// A directory's or package's files with their symbol and unresolved counts, each file's top
    /// symbols, and the communities their symbols belong to.
    fn overview_area(
        &self,
        heading: Line,
        mut files: Vec<RepoPath>,
        budget: Budget,
    ) -> Result<Answer> {
        files.sort();
        let store = self.store();
        let mut file_lines = Vec::new();
        let mut symbols = Vec::new();
        let mut present: BTreeMap<NodeId, usize> = BTreeMap::new();
        for file in &files {
            let nodes = store.nodes_in_file(file)?;
            let mut count = 0;
            let (mut unresolved, mut external) = (0, 0);
            for node in &nodes {
                unresolved += node.unresolved_out;
                external += node.external_out;
                if !matches!(node.node.kind, NodeKind::File | NodeKind::Package) {
                    count += 1;
                }
                if let Some(community) = node.community_id {
                    *present.entry(community).or_default() += 1;
                }
            }
            file_lines.push(Line::field(
                file.as_str(),
                format!(
                    "{} · unresolved {unresolved} · external {external}",
                    plural(count, "symbol")
                ),
            ));
            for node in self.ranked(None, Some(file), TOP_SYMBOLS)? {
                symbols.push(Line::Node(NodeLine::of(&node)));
            }
        }
        let mut communities = Vec::new();
        for (id, here) in present {
            let community = self.node(id)?;
            communities.push((
                here,
                format!(
                    "derived:{} ({})  {} here",
                    community.node.name,
                    community.node.path,
                    plural(here, "member")
                ),
            ));
        }
        communities.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let communities = communities
            .into_iter()
            .map(|(_, text)| Line::text(text))
            .collect();
        let sections = vec![
            Section::new("overview", 0, vec![heading]).without_header(),
            Section::new("files", 1, file_lines),
            Section::new("symbols", 2, symbols),
            Section::new("communities", 3, communities),
        ];
        self.finish(
            Outcome::Ok,
            sections,
            budget,
            "raise max_tokens or pass a narrower scope",
        )
    }

    /// A file's symbols in source order as cards with their one-hop counts, and the files it
    /// imports and that import it.
    fn overview_file(&self, path: &RepoPath, budget: Budget) -> Result<Answer> {
        let store = self.store();
        let mut nodes = store.nodes_in_file(path)?;
        nodes.sort_by(|a, b| {
            let line = |n: &StoredNode| n.node.span.map(|span| span.line_start);
            (line(a), &a.node.path, a.node.kind).cmp(&(line(b), &b.node.path, b.node.kind))
        });
        let (mut unresolved, mut external) = (0, 0);
        let mut cards = Vec::new();
        let mut file_node = None;
        for node in &nodes {
            unresolved += node.unresolved_out;
            external += node.external_out;
            if node.node.kind == NodeKind::File {
                file_node = Some(node.id);
            } else {
                cards.push(Line::Card(SymbolCard::for_node(store, node)?));
            }
        }
        let (imports, importers) = match file_node {
            Some(id) => (
                self.edge_lines(&store.edges_from(id, &[EdgeKind::Imports])?, Direction::Out)?,
                self.edge_lines(&store.edges_to(id, &[EdgeKind::Imports])?, Direction::In)?,
            ),
            None => (Vec::new(), Vec::new()),
        };
        let summary = vec![
            Line::field("file", path.as_str()),
            Line::field("unresolved", unresolved),
            Line::field("external", external),
        ];
        let sections = vec![
            Section::new("overview", 0, summary).without_header(),
            Section::new("symbols", 1, cards),
            Section::new("imports", 2, imports),
            Section::new("importers", 2, importers),
        ];
        self.finish(Outcome::Ok, sections, budget, "raise max_tokens")
    }

    /// A community's members by file, with the edges crossing its boundary counted by the
    /// community at the other end.
    fn overview_community(&self, scope: &str, budget: Budget) -> Result<Answer> {
        let store = self.store();
        let not_found = || format!("no community `{scope}`");
        let Ok(path) = scope.parse::<SemanticPath>() else {
            return self.not_found(not_found(), budget);
        };
        let Some(community) = store
            .node_by_semantic_path(&path, Some(NodeKind::Community))?
            .into_iter()
            .next()
        else {
            return self.not_found(not_found(), budget);
        };
        let mut members: Vec<StoredNode> = store
            .edges_to(community.id, &[EdgeKind::MemberOf])?
            .iter()
            .map(|edge| self.node(edge.src_id))
            .collect::<Result<_>>()?;
        members.sort_by(|a, b| {
            let key = |n: &StoredNode| {
                (
                    n.node.file.clone(),
                    n.node.span.map(|span| span.line_start),
                    n.node.path.clone(),
                    n.node.kind,
                )
            };
            key(a).cmp(&key(b))
        });
        let mut internal = 0;
        let mut inbound: BTreeMap<String, usize> = BTreeMap::new();
        let mut outbound: BTreeMap<String, usize> = BTreeMap::new();
        for member in &members {
            for edge in store.edges_to(member.id, &RELATIONS)? {
                match self.community_of(edge.src_id, community.id)? {
                    None => internal += 1,
                    Some(label) => *inbound.entry(label).or_default() += 1,
                }
            }
            for edge in store.edges_from(member.id, &RELATIONS)? {
                if let Some(label) = self.community_of(edge.dst_id, community.id)? {
                    *outbound.entry(label).or_default() += 1;
                }
            }
        }
        let summary = vec![
            Line::field(
                "community",
                format!("derived:{} ({})", community.node.name, path),
            ),
            Line::field("size", plural(members.len(), "member")),
            Line::field("internal", plural(internal, "edge")),
        ];
        let member_lines = members
            .iter()
            .map(|m| Line::Node(NodeLine::of(m)))
            .collect();
        let sections = vec![
            Section::new("overview", 0, summary).without_header(),
            Section::new("inbound", 1, boundary(inbound)),
            Section::new("outbound", 1, boundary(outbound)),
            Section::new("members", 2, member_lines),
        ];
        self.finish(Outcome::Ok, sections, budget, "raise max_tokens")
    }

    /// `None` when `other` is in `community`; else the label of the community it is in.
    fn community_of(&self, other: NodeId, community: NodeId) -> Result<Option<String>> {
        let node = self.node(other)?;
        match node.community_id {
            Some(id) if id == community => Ok(None),
            Some(id) => {
                let other = self.node(id)?;
                Ok(Some(format!(
                    "derived:{} ({})",
                    other.node.name, other.node.path
                )))
            }
            None => Ok(Some("no community".to_owned())),
        }
    }

    /// The top `limit` code symbols by incoming relations, in a community or a file.
    fn ranked(
        &self,
        community: Option<NodeId>,
        file: Option<&RepoPath>,
        limit: u32,
    ) -> Result<Vec<StoredNode>> {
        self.store()
            .nodes_by_in_degree(&RELATIONS, community, file, limit)?
            .into_iter()
            .map(|(id, _)| self.node(id))
            .collect()
    }

    fn edge_lines(&self, edges: &[StoredEdge], direction: Direction) -> Result<Vec<Line>> {
        edges
            .iter()
            .map(|edge| Ok(Line::Edge(self.edge_line(edge, direction, None)?)))
            .collect()
    }
}

/// A symbol's semantic path without its file: `Store::load`, `<MemoryStore as Store>::load`.
fn in_file(node: &StoredNode) -> &str {
    let path = node.node.path.as_str();
    path.split_once("::").map_or(path, |(_, rest)| rest)
}

/// `<community>  N edges` lines, most edges first.
fn boundary(counts: BTreeMap<String, usize>) -> Vec<Line> {
    let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    counts
        .into_iter()
        .map(|(label, count)| Line::field(label, plural(count, "edge")))
        .collect()
}
