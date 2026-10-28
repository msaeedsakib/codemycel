use std::collections::BTreeMap;
use std::path::Path;

use codemycel_core::store::{StoreRead, Tx};
use codemycel_core::{
    Edge, EdgeKind, FileKind, FileRecord, Node, NodeKind, NodeRef, Package, RepoPath, SemanticPath,
    VerifiedBy,
};
use codemycel_parse::manifests::{self, Ecosystem, PackageParse};
use codemycel_parse::markdown::{self, DocParse};
use rayon::prelude::*;

use super::write_phase::search_doc;
use crate::IndexError;

/// Documents and manifests parsed ahead of the write transaction.
pub struct Prepared {
    manifests: Vec<PackageParse>,
    docs: Vec<DocParse>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocsCounts {
    pub docs: usize,
    pub headings: usize,
    pub packages: usize,
    pub mentions: usize,
}

/// Parses every listed document and manifest, repository-wide and independent of roots. A
/// manifest that does not parse declares no package.
pub fn prepare(repo_root: &Path, files: &[FileRecord]) -> Result<Prepared, IndexError> {
    let read = |file: &FileRecord| {
        let absolute = repo_root.join(file.path.as_str());
        std::fs::read(&absolute).map_err(|source| IndexError::Io {
            path: absolute,
            source,
        })
    };
    let listed = |kind: FileKind| {
        files
            .iter()
            .filter(move |f| f.kind == kind && f.skip_reason.is_none())
            .collect::<Vec<_>>()
    };
    let manifests = listed(FileKind::Manifest)
        .par_iter()
        .map(|file| {
            let bytes = read(file)?;
            let parsed = if file.path.as_str().ends_with("Cargo.toml") {
                manifests::parse_cargo(&file.path, &bytes)
            } else {
                manifests::parse_npm(&file.path, &bytes)
            };
            Ok(parsed.ok())
        })
        .collect::<Result<Vec<_>, IndexError>>()?
        .into_iter()
        .flatten()
        .collect();
    let docs = listed(FileKind::Doc)
        .par_iter()
        .map(|file| Ok(markdown::parse(&file.path, &read(file)?)?))
        .collect::<Result<Vec<_>, IndexError>>()?;
    Ok(Prepared { manifests, docs })
}

/// Writes package, document, and heading nodes with their `contains`, `depends_on`, and
/// `mentions` edges. Runs after the code nodes and `name_counts` are written, in the same
/// transaction, so mentions see the whole repository's names.
pub fn write(
    tx: &mut Tx<'_>,
    prepared: &Prepared,
    code_files: &[RepoPath],
) -> Result<DocsCounts, IndexError> {
    let mut counts = DocsCounts::default();
    let packages = Packages::new(&prepared.manifests)?;
    counts.packages = packages.write(tx, code_files)?;
    for doc in &prepared.docs {
        let mut nodes = vec![doc.doc.clone()];
        nodes.extend(doc.headings.iter().cloned());
        tx.insert_nodes(&nodes)?;
        tx.insert_edges(&doc.contains)?;
        tx.insert_search_docs(&nodes.iter().map(search_doc).collect::<Vec<_>>())?;
        let mut mentions = Vec::new();
        for mention in &doc.mentions {
            if let Some(target) = sole_code_node(tx, &mention.text)? {
                mentions.push(syntactic(
                    mention.from.clone(),
                    target,
                    EdgeKind::Mentions,
                    mention.at,
                ));
            }
        }
        for link in &doc.links {
            let Some(target) = RepoPath::new(&link.text)
                .ok()
                .and_then(|path| SemanticPath::for_file(&path).ok())
            else {
                continue;
            };
            if !tx
                .node_by_semantic_path(&target, Some(NodeKind::File))?
                .is_empty()
            {
                let file = NodeRef::new(target, NodeKind::File);
                mentions.push(syntactic(
                    link.from.clone(),
                    file,
                    EdgeKind::Mentions,
                    link.at,
                ));
            }
        }
        tx.insert_edges(&mentions)?;
        counts.docs += 1;
        counts.headings += doc.headings.len();
        counts.mentions += mentions.len();
    }
    Ok(counts)
}

/// The code node a backtick span names, when exactly one code node in the repository has that
/// name. `name_counts` covers code nodes only.
fn sole_code_node(tx: &Tx<'_>, name: &str) -> Result<Option<NodeRef>, IndexError> {
    if tx.name_count(name)? != 1 {
        return Ok(None);
    }
    let mut code = tx
        .nodes_named(name)?
        .into_iter()
        .map(|stored| stored.node.node_ref())
        .filter(|node| !is_non_code(node.kind));
    Ok(match (code.next(), code.next()) {
        (Some(node), None) => Some(node),
        _ => None,
    })
}

fn is_non_code(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::File
            | NodeKind::Package
            | NodeKind::Doc
            | NodeKind::Heading
            | NodeKind::Community
            | NodeKind::Note
    )
}

fn syntactic(src: NodeRef, dst: NodeRef, kind: EdgeKind, at: codemycel_core::Occurrence) -> Edge {
    Edge {
        src,
        dst,
        kind,
        verified_by: VerifiedBy::Syntactic,
        occurrences: vec![at],
    }
}

struct Packages<'a> {
    /// Manifests that declare a package, with their node identities.
    workspace: Vec<(&'a PackageParse, NodeRef)>,
    roots: Vec<&'a PackageParse>,
}

impl<'a> Packages<'a> {
    fn new(manifests: &'a [PackageParse]) -> Result<Self, IndexError> {
        let mut workspace = Vec::new();
        for manifest in manifests.iter().filter(|m| m.name.is_some()) {
            let path = SemanticPath::for_package(&manifest.manifest)?;
            workspace.push((manifest, NodeRef::new(path, NodeKind::Package)));
        }
        let roots = manifests
            .iter()
            .filter(|m| !m.workspace_members.is_empty())
            .collect();
        Ok(Self { workspace, roots })
    }

    /// Package nodes, `depends_on` edges, and `contains` from each package to the code files under
    /// it that no nested package claims. Returns the number of package nodes.
    fn write(&self, tx: &mut Tx<'_>, code_files: &[RepoPath]) -> Result<usize, IndexError> {
        let mut nodes = Vec::new();
        let mut rows = Vec::new();
        for (manifest, node) in &self.workspace {
            let name = manifest.name.clone().unwrap_or_default();
            nodes.push(package_node(node, &name, Some(&manifest.manifest)));
            rows.push(Package {
                node: node.clone(),
                name,
                version: manifest.version.clone(),
                external: false,
                manifest: Some(manifest.manifest.clone()),
            });
        }
        let mut external: BTreeMap<(Ecosystem, String), NodeRef> = BTreeMap::new();
        let mut edges = Vec::new();
        for (manifest, node) in &self.workspace {
            for dependency in &manifest.dependencies {
                let target = match self.internal(
                    manifest,
                    &dependency.name,
                    dependency.workspace_protocol,
                ) {
                    Some(target) => target.clone(),
                    None => {
                        let key = (manifest.ecosystem, dependency.name.clone());
                        match external.get(&key) {
                            Some(target) => target.clone(),
                            None => {
                                let path = SemanticPath::for_external_package(
                                    manifest.ecosystem.as_str(),
                                    &dependency.name,
                                )?;
                                let target = NodeRef::new(path, NodeKind::Package);
                                external.insert(key, target.clone());
                                target
                            }
                        }
                    }
                };
                edges.push(Edge {
                    src: node.clone(),
                    dst: target,
                    kind: EdgeKind::DependsOn,
                    verified_by: VerifiedBy::Syntactic,
                    occurrences: Vec::new(),
                });
            }
        }
        for ((_, name), node) in &external {
            nodes.push(package_node(node, name, None));
            rows.push(Package {
                node: node.clone(),
                name: name.clone(),
                version: None,
                external: true,
                manifest: None,
            });
        }
        for file in code_files {
            if let Some(owner) = self.owner(file) {
                edges.push(Edge {
                    src: owner.clone(),
                    dst: NodeRef::new(SemanticPath::for_file(file)?, NodeKind::File),
                    kind: EdgeKind::Contains,
                    verified_by: VerifiedBy::Syntactic,
                    occurrences: Vec::new(),
                });
            }
        }
        tx.insert_nodes(&nodes)?;
        tx.insert_packages(&rows)?;
        tx.insert_edges(&edges)?;
        tx.insert_search_docs(&nodes.iter().map(search_doc).collect::<Vec<_>>())?;
        Ok(nodes.len())
    }

    /// The workspace package a dependency names: the one package of that name in the same
    /// ecosystem, and for npm only when it is a workspace member or the version uses the
    /// `workspace:` protocol.
    fn internal(
        &self,
        from: &PackageParse,
        name: &str,
        workspace_protocol: bool,
    ) -> Option<&NodeRef> {
        let mut named = self
            .workspace
            .iter()
            .filter(|(m, _)| m.ecosystem == from.ecosystem && m.name.as_deref() == Some(name));
        let (target, node) = named.next()?;
        if named.next().is_some() || std::ptr::eq(*target, from) {
            return None;
        }
        let linked = match target.ecosystem {
            Ecosystem::Cargo => true,
            Ecosystem::Npm => {
                workspace_protocol
                    || self.roots.iter().any(|root| {
                        root.ecosystem == Ecosystem::Npm && root.has_member(&target.directory)
                    })
            }
        };
        linked.then_some(node)
    }

    /// The deepest package whose directory holds `file`, of any ecosystem.
    fn owner(&self, file: &RepoPath) -> Option<&NodeRef> {
        self.workspace
            .iter()
            .filter(|(m, _)| {
                m.directory.is_root()
                    || file
                        .as_str()
                        .strip_prefix(m.directory.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|(m, _)| m.directory.as_str().len())
            .map(|(_, node)| node)
    }
}

fn package_node(node: &NodeRef, name: &str, manifest: Option<&RepoPath>) -> Node {
    Node {
        path: node.path.clone(),
        kind: NodeKind::Package,
        name: name.to_owned(),
        file: manifest.cloned(),
        span: None,
        selection: None,
        signature: None,
        has_explicit_return_type: false,
        body_hash: None,
        doc_comment: None,
        visibility: None,
    }
}
