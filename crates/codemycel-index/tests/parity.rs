#![allow(clippy::unwrap_used)]

//! Indexes each fixture through its committed SCIP index and through the language server, and
//! fails on any difference in edges or unresolved rows that `<fixture>.parity-allowlist.txt`
//! does not list. Allowlist lines are diff lines as printed below; `#` lines explain them.

mod harness;

use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::config::Config;
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{
    Candidate, Language, NodeRef, Occurrence, RepoPath, Resolver, ResolverSource, SourceFile,
    Verdict,
};
use codemycel_index::{Pipeline, Registry, Sources};
use codemycel_lsp::{EnrichOptions, Provisioner};
use codemycel_scip::{ScipImporter, ScipIndex};
use harness::{fixture, fixture_copy, local_provisioner};

/// Covers every file the index has a document for, whatever git says.
struct Forced(ScipIndex);

struct ForcedResolver<'a> {
    inner: Box<dyn Resolver + 'a>,
    documents: BTreeSet<&'a RepoPath>,
}

impl ResolverSource for Forced {
    fn bind<'a>(&'a self, files: &'a [SourceFile<'a>]) -> Box<dyn Resolver + 'a> {
        Box::new(ForcedResolver {
            inner: self.0.bind(files),
            documents: self.0.documents().collect(),
        })
    }
}

impl Resolver for ForcedResolver<'_> {
    fn name(&self) -> &'static str {
        "scip"
    }
    fn covers(&self, file: &RepoPath) -> bool {
        self.documents.contains(file)
    }
    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        self.inner.resolve(candidates)
    }
    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        self.inner.implementers(node)
    }
    fn outgoing_calls(&self, node: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        self.inner.outgoing_calls(node)
    }
}

fn dumps(dir: &Path, provisioner: &Provisioner, scip: Option<&dyn ResolverSource>) -> String {
    let temp = tempfile::tempdir().unwrap();
    let config = Config::load_with_home(dir, &temp.path().join("home")).unwrap();
    let mut store = Store::create(&temp.path().join("index.db")).unwrap();
    let sources = Sources {
        provisioner,
        scip,
        options: EnrichOptions::default(),
    };
    Pipeline::run(dir, &config, &mut store, &Registry::default(), &sources).unwrap();
    let edges = store.dump_edges().unwrap();
    let unresolved = store.dump_unresolved().unwrap();
    edges
        .lines()
        .map(|l| format!("edge\t{l}"))
        .chain(unresolved.lines().map(|l| format!("unresolved\t{l}")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_parity(relative: &str, language: Language) {
    let source = fixture(relative);
    let (_temp, dir) = fixture_copy(&source);
    let registry = Registry::default();
    let server = registry
        .server_for(language)
        .unwrap_or_else(|| panic!("no server registered for {language}"));
    let Some(provisioner) = local_provisioner(server) else {
        return;
    };
    let index = ScipImporter::load(&dir.join("index.scip")).unwrap();
    let forced = Forced(index);
    let lsp = dumps(&dir, &provisioner, None);
    let scip = dumps(&dir, &provisioner, Some(&forced));
    let lsp: BTreeSet<&str> = lsp.lines().collect();
    let scip: BTreeSet<&str> = scip.lines().collect();
    let diff: BTreeSet<String> = lsp
        .difference(&scip)
        .map(|l| format!("lsp-only\t{l}"))
        .chain(scip.difference(&lsp).map(|l| format!("scip-only\t{l}")))
        .collect();
    let allowlist_path = source.with_file_name(format!(
        "{}.parity-allowlist.txt",
        source.file_name().unwrap().to_string_lossy()
    ));
    let allowlist_text = std::fs::read_to_string(&allowlist_path).unwrap_or_default();
    let allowed: BTreeSet<String> = allowlist_text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(ToOwned::to_owned)
        .collect();
    let unexplained: Vec<&String> = diff.difference(&allowed).collect();
    let stale: Vec<&String> = allowed.difference(&diff).collect();
    let report: Vec<&str> = diff.iter().map(String::as_str).collect();
    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "{relative}: SCIP and LSP differ\n\nunexplained:\n{}\n\nallowlisted but no longer differing:\n{}\n\nfull diff:\n{}",
        join(&unexplained),
        join(&stale),
        report.join("\n")
    );
}

fn join(lines: &[&String]) -> String {
    lines
        .iter()
        .map(|l| l.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rust_scip_and_lsp_edge_sets_differ_only_by_allowlist() {
    assert_parity("rust/workspace-basic", Language::Rust);
}

#[test]
fn typescript_scip_and_lsp_edge_sets_differ_only_by_allowlist() {
    assert_parity("typescript/monorepo-basic", Language::TypeScript);
}
