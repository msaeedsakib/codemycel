use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::store::StoreRead;
use codemycel_core::{CandidateRole, EdgeKind, NodeKind, NodeRef, RepoPath, SemanticPath};

use super::changes::ChangeSet;
use super::symbols_diff::ChangedSymbols;
use crate::IndexError;

const VERIFIED: [EdgeKind; 7] = [
    EdgeKind::Imports,
    EdgeKind::Exports,
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];

/// What a file's new parse says, as far as planning needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedFacts {
    /// The names of its symbols.
    pub symbols: BTreeSet<String>,
    /// The names of the classes, traits, and interfaces it declares.
    pub parents: BTreeSet<String>,
    /// The texts naming the traits, interfaces, and classes it implements or extends.
    pub supertypes: BTreeSet<String>,
    /// The texts of its import candidates outside function and method bodies.
    pub imports: BTreeSet<String>,
    /// The texts of its export candidates outside function and method bodies.
    pub exports: BTreeSet<String>,
}

/// What an incremental sync rewrites and re-verifies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncPlan {
    /// Changed code files that still exist, parsed or not; their code rows are rewritten.
    pub rewrite: BTreeSet<RepoPath>,
    /// Parsed code files whose verified edges and unresolved rows are rebuilt; the verify run
    /// adds every file of a root it has to rebuild whole.
    pub reverify: BTreeSet<RepoPath>,
    /// Files whose traits and interfaces are asked for implementers: `reverify`, plus the
    /// files of the types the re-verified files implement, extend, or override, as stored and
    /// as their new parse names them, followed up the hierarchy.
    pub implementer_sources: BTreeSet<RepoPath>,
}

/// The blast radius as it is gathered: nodes whose stored edges reach files to re-verify, and
/// names whose unresolved candidates do.
#[derive(Default)]
struct Reach {
    files: BTreeSet<RepoPath>,
    nodes: BTreeSet<NodeRef>,
    names: BTreeSet<String>,
    /// Texts naming modules whose symbols, and what they export, became reachable.
    modules: BTreeSet<String>,
}

impl SyncPlan {
    /// `symbols` holds every changed code file, parsed before or now; `parsed` every file
    /// parsed now. Re-verified, among the parsed files: each file in `forced`; each changed
    /// file; every file with a verified edge to or from a removed or changed symbol, or to or
    /// from the file itself once any of its symbols or its import and export statements
    /// changed; every file with an unresolved candidate named like an added or changed symbol;
    /// and for a trait impl added or removed, every file declaring a type or trait of those
    /// names. When a file's import and export statements change, so do the files reaching what
    /// it exposed, as stored, and those with unresolved candidates naming what it exposes now,
    /// including every symbol of the modules its statements name. The same holds for a module
    /// added, removed, or changed. An added or deleted file re-verifies the files whose
    /// unresolved import and export candidates could name it, and those linked to a file it
    /// shadows or unshadows. The index is read before anything is written.
    pub fn build(
        changes: &ChangeSet,
        symbols: &BTreeMap<RepoPath, ChangedSymbols>,
        forced: &BTreeSet<RepoPath>,
        parsed: &BTreeMap<RepoPath, ParsedFacts>,
        store: &impl StoreRead,
    ) -> Result<Self, IndexError> {
        let mut reach = Reach {
            files: forced.clone(),
            ..Reach::default()
        };
        let stored_code: Vec<RepoPath> = store
            .files()?
            .into_iter()
            .filter(|stored| stored.record.parsed)
            .map(|stored| stored.record.path)
            .collect();
        let mut specifiers: Option<Vec<(RepoPath, String)>> = None;
        for (file, changed) in symbols {
            reach.files.insert(file.clone());
            reach
                .nodes
                .extend(changed.removed.iter().chain(&changed.changed).cloned());
            if !changed.is_empty() || changed.surface || !parsed.contains_key(file) {
                reach.nodes.insert(file_node(file)?);
            }
            reach.names.extend(
                changed.added.iter().map(|node| node.name.clone()).chain(
                    changed
                        .changed
                        .iter()
                        .map(|node| node.path.name().to_owned()),
                ),
            );
            for found in changed.impls_added.iter().chain(&changed.impls_removed) {
                reach
                    .files
                    .extend(declaring_files(store, &found.self_type, is_implementer)?);
                reach
                    .files
                    .extend(declaring_files(store, &found.trait_name, is_parent)?);
            }
            let modules = changed
                .added
                .iter()
                .filter(|node| node.kind == NodeKind::Module)
                .map(|node| node.name.as_str())
                .chain(
                    changed
                        .removed
                        .iter()
                        .chain(&changed.changed)
                        .filter(|node| node.kind == NodeKind::Module)
                        .map(|node| node.path.name()),
                );
            for module in modules {
                let keys = declared_module_keys(file, module);
                for other in stored_code.iter().filter(|f| keys.contains(&module_key(f))) {
                    reach.exposed_by_file(store, other)?;
                }
                for (other, facts) in parsed {
                    if keys.contains(&module_key(other)) {
                        reach.names.extend(facts.symbols.iter().cloned());
                        reach.modules.extend(facts.exports.iter().cloned());
                    }
                }
            }
            if changed.surface {
                if let Some(facts) = parsed.get(file) {
                    let texts = facts.imports.iter().chain(&facts.exports);
                    reach.names.extend(texts.clone().cloned());
                    reach.modules.extend(texts.cloned());
                }
                reach.exposed_by(store, file)?;
                reach.files.extend(imported_below(store, file)?);
            }
            if changes.added.contains(file) || changes.deleted.contains(file) {
                let names = module_names(file);
                let specifiers = match &mut specifiers {
                    Some(specifiers) => specifiers,
                    None => specifiers.insert(
                        store.unresolved_texts(&[CandidateRole::Import, CandidateRole::Export])?,
                    ),
                };
                reach.files.extend(
                    specifiers
                        .iter()
                        .filter(|(_, text)| names.contains(&module_name(text)))
                        .map(|(path, _)| path.clone()),
                );
                for other in &stored_code {
                    if other != file && module_key(other) == module_key(file) {
                        reach.nodes.insert(file_node(other)?);
                    }
                }
            }
        }
        reach.expand_modules(parsed);
        let mut reverify = reach.files;
        for node in &reach.nodes {
            reverify.extend(linked_files(store, node, &VERIFIED)?);
        }
        for name in &reach.names {
            reverify.extend(store.files_with_unresolved_text(name)?);
        }
        reverify.retain(|file| parsed.contains_key(file));
        let implementer_sources = implementer_sources(&reverify, parsed, store)?;
        let rewrite = symbols
            .keys()
            .filter(|file| !changes.deleted.contains(*file))
            .cloned()
            .collect();
        Ok(Self {
            rewrite,
            reverify,
            implementer_sources,
        })
    }
}

impl Reach {
    /// Every node `file` exposed through its stored `exports` edges, following exported files
    /// and modules into everything they hold and export in turn.
    fn exposed_by(&mut self, store: &impl StoreRead, file: &RepoPath) -> Result<(), IndexError> {
        let mut pending = vec![file.clone()];
        let mut seen = BTreeSet::from([file.clone()]);
        while let Some(current) = pending.pop() {
            for stored in store.node_by_semantic_path(&SemanticPath::for_file(&current)?, None)? {
                for edge in store.edges_from(stored.id, &[EdgeKind::Exports])? {
                    let target = edge.edge.dst;
                    if matches!(target.kind, NodeKind::File | NodeKind::Module) {
                        if let Some(next) = file_of(&target)
                            && seen.insert(next.clone())
                        {
                            self.nodes.extend(nodes_of(store, &next)?);
                            pending.push(next);
                        }
                    } else {
                        self.nodes.insert(target);
                    }
                }
            }
        }
        Ok(())
    }

    /// Every stored node of `file` and everything it exposes.
    fn exposed_by_file(
        &mut self,
        store: &impl StoreRead,
        file: &RepoPath,
    ) -> Result<(), IndexError> {
        self.nodes.extend(nodes_of(store, file)?);
        self.exposed_by(store, file)
    }

    /// Adds the symbol names of every parsed file a module text could name, following what
    /// those files export in turn.
    fn expand_modules(&mut self, parsed: &BTreeMap<RepoPath, ParsedFacts>) {
        if self.modules.is_empty() {
            return;
        }
        let mut by_name: BTreeMap<String, Vec<&ParsedFacts>> = BTreeMap::new();
        for (file, facts) in parsed {
            for name in module_names(file) {
                by_name.entry(name).or_default().push(facts);
            }
        }
        let mut pending: Vec<String> = self.modules.iter().cloned().collect();
        let mut seen: BTreeSet<String> = pending.iter().map(|text| module_name(text)).collect();
        while let Some(text) = pending.pop() {
            for facts in by_name.get(&module_name(&text)).into_iter().flatten() {
                self.names.extend(facts.symbols.iter().cloned());
                for export in &facts.exports {
                    self.names.insert(export.clone());
                    if seen.insert(module_name(export)) {
                        pending.push(export.clone());
                    }
                }
            }
        }
    }
}

/// Files of the modules below `file` with a verified edge to any symbol of a file it imports
/// from, since they may reach those symbols through its imports. A crate root, `mod.rs`, or
/// `index` file has its modules in its own directory, any other file in the directory named
/// after it.
fn imported_below(
    store: &impl StoreRead,
    file: &RepoPath,
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let (parent, name) = file
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", file.as_str()));
    let stem = module_name(name);
    let dir = match (parent, stem.as_str()) {
        (_, "lib" | "main" | "mod" | "index") => parent.to_owned(),
        ("", _) => stem,
        _ => format!("{parent}/{stem}"),
    };
    let mut found = BTreeSet::new();
    for stored in store.node_by_semantic_path(&SemanticPath::for_file(file)?, None)? {
        for edge in store.edges_from(stored.id, &[EdgeKind::Imports])? {
            let Some(imported) = file_of(&edge.edge.dst) else {
                continue;
            };
            for node in nodes_of(store, &imported)? {
                found.extend(
                    linked_files(store, &node, &VERIFIED)?
                        .into_iter()
                        .filter(|linked| within(linked.as_str(), &dir)),
                );
            }
        }
    }
    Ok(found)
}

/// `reverify` and the files declaring what their types implement, extend, or override, as
/// stored edges and the new parse name it, followed to the top of each hierarchy.
fn implementer_sources(
    reverify: &BTreeSet<RepoPath>,
    parsed: &BTreeMap<RepoPath, ParsedFacts>,
    store: &impl StoreRead,
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let mut declaring: BTreeMap<&str, Vec<&RepoPath>> = BTreeMap::new();
    for (file, facts) in parsed {
        for name in &facts.parents {
            declaring.entry(name.as_str()).or_default().push(file);
        }
    }
    let mut sources = reverify.clone();
    let mut pending: Vec<RepoPath> = reverify.iter().cloned().collect();
    while let Some(file) = pending.pop() {
        let mut parents = BTreeSet::new();
        for node in store.nodes_in_file(&file)? {
            let kinds = [EdgeKind::Implements, EdgeKind::Overrides, EdgeKind::Extends];
            for edge in store.edges_from(node.id, &kinds)? {
                parents.extend(file_of(&edge.edge.dst));
            }
        }
        for text in parsed.get(&file).into_iter().flat_map(|f| &f.supertypes) {
            let files = declaring.get(base_name(text)).into_iter().flatten();
            parents.extend(files.map(|file| (*file).clone()));
        }
        for parent in parents {
            if sources.insert(parent.clone()) {
                pending.push(parent);
            }
        }
    }
    Ok(sources)
}

/// The files at the other end of `node`'s stored edges of `kinds`, in either direction.
fn linked_files(
    store: &impl StoreRead,
    node: &NodeRef,
    kinds: &[EdgeKind],
) -> Result<BTreeSet<RepoPath>, IndexError> {
    let mut files = BTreeSet::new();
    for stored in store.node_by_semantic_path(&node.path, Some(node.kind))? {
        for edge in store.edges_to(stored.id, kinds)? {
            files.extend(file_of(&edge.edge.src));
        }
        for edge in store.edges_from(stored.id, kinds)? {
            files.extend(file_of(&edge.edge.dst));
        }
    }
    Ok(files)
}

fn nodes_of(store: &impl StoreRead, file: &RepoPath) -> Result<Vec<NodeRef>, IndexError> {
    Ok(store
        .nodes_in_file(file)?
        .iter()
        .map(|stored| stored.node.node_ref())
        .collect())
}

/// Files declaring a node of a matching kind named like the last path segment of `text`, with
/// generic arguments dropped: `crate::a::Foo<T>` looks for `Foo`.
fn declaring_files(
    store: &impl StoreRead,
    text: &str,
    kind: fn(NodeKind) -> bool,
) -> Result<BTreeSet<RepoPath>, IndexError> {
    Ok(store
        .nodes_named(base_name(text))?
        .iter()
        .filter(|stored| kind(stored.node.kind))
        .filter_map(|stored| stored.node.file.clone())
        .collect())
}

/// The last path segment of a type's text, generic arguments and qualifiers dropped.
fn base_name(text: &str) -> &str {
    let base = text.split('<').next().unwrap_or(text);
    let name = base.rsplit("::").next().unwrap_or(base).trim();
    let name = name.rsplit('.').next().unwrap_or(name);
    name.rsplit(' ').next().unwrap_or(name)
}

/// The module an import or export text names: its last path segment up to the first dot, so
/// `./later.js`, `../later`, `@scope/later`, and `later` all name `later`. A text naming a
/// directory itself, `.` or `..`, names `index`.
fn module_name(text: &str) -> String {
    let segment = text.rsplit(['/', '\\']).next().unwrap_or(text);
    match segment {
        "" | "." | ".." => "index".to_owned(),
        _ => segment.split('.').next().unwrap_or(segment).to_owned(),
    }
}

/// Every module name an import could reach `file` by: its stem and each directory above it,
/// for directory imports and package names.
fn module_names(file: &RepoPath) -> BTreeSet<String> {
    file.as_str()
        .split('/')
        .map(module_name)
        .filter(|name| !name.is_empty())
        .collect()
}

/// Where the file of a module named `module`, declared in `file`, may live: beside `file` when
/// it is a crate root or `mod.rs`, else in the directory named after `file`; both are kept.
fn declared_module_keys(file: &RepoPath, module: &str) -> [(String, String); 2] {
    let (dir, name) = file
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", file.as_str()));
    let nested = match dir {
        "" => module_name(name),
        _ => format!("{dir}/{}", module_name(name)),
    };
    [
        (dir.to_owned(), module.to_owned()),
        (nested, module.to_owned()),
    ]
}

/// The directory and name a module resolves by: `a/later.ts`, `a/later.d.ts`, and
/// `a/later/index.ts` share one, as do `a/later.rs` and `a/later/mod.rs`.
fn module_key(file: &RepoPath) -> (String, String) {
    let (dir, name) = file
        .as_str()
        .rsplit_once('/')
        .unwrap_or(("", file.as_str()));
    let stem = module_name(name);
    if matches!(stem.as_str(), "index" | "mod") {
        let (parent, own) = dir.rsplit_once('/').unwrap_or(("", dir));
        return (parent.to_owned(), own.to_owned());
    }
    (dir.to_owned(), stem)
}

fn within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

fn file_node(file: &RepoPath) -> Result<NodeRef, IndexError> {
    Ok(NodeRef::new(SemanticPath::for_file(file)?, NodeKind::File))
}

fn file_of(node: &NodeRef) -> Option<RepoPath> {
    RepoPath::new(node.path.file()?).ok()
}

fn is_implementer(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Struct | NodeKind::Enum | NodeKind::Class | NodeKind::TypeAlias
    )
}

fn is_parent(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Trait | NodeKind::Interface)
}
