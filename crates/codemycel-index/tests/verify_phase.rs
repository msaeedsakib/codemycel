#![allow(clippy::unwrap_used)]

mod fake;

use std::collections::{BTreeMap, BTreeSet};

use codemycel_core::config::Config;
use codemycel_core::store::{Store, StoreRead};
use codemycel_core::{
    Candidate, Coverage, DegradeReason, EdgeKind, Language, NodeKind, NodeRef, Occurrence,
    RepoPath, Resolver, ResolverSource, SemanticPath, SourceFile, Toolchain, UnresolvedReason,
    Verdict, VerifiedBy,
};
use codemycel_index::{BatchServers, Pipeline, PipelineReport, Registry, Scip, Sources, SyncMode};
use codemycel_lsp::{EnrichOptions, Provisioner};
use fake::{FakeAdapter, Repo};
use serde_json::{Value, json};

const MANIFEST: &str = "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n";

const LIB: &str = "\
mod b;

pub struct S;

pub fn run() -> S {
    helper();
    S
}
";

const B: &str = "\
pub trait T {
    fn go(&self);
}

pub fn helper() {}
";

/// Confirms or rejects candidates by (file, text), and reports fixed implementers.
#[derive(Default)]
struct StubScip {
    covers: BTreeSet<RepoPath>,
    verdicts: BTreeMap<(String, String), Verdict>,
    implementers: BTreeMap<NodeRef, Vec<NodeRef>>,
}

impl Resolver for StubScip {
    fn name(&self) -> &'static str {
        "scip"
    }
    fn covers(&self, file: &RepoPath) -> bool {
        self.covers.contains(file)
    }
    fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
        candidates
            .iter()
            .map(|c| {
                self.verdicts
                    .get(&(c.file.as_str().to_owned(), c.text.clone()))
                    .cloned()
                    .unwrap_or(Verdict::Unresolved {
                        reason: UnresolvedReason::NoDefinition,
                    })
            })
            .collect()
    }
    fn implementers(&self, node: &NodeRef) -> Vec<NodeRef> {
        self.implementers.get(node).cloned().unwrap_or_default()
    }
    fn outgoing_calls(&self, _: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
        None
    }
}

impl ResolverSource for StubScip {
    fn bind<'a>(&'a self, _: &'a [SourceFile<'a>]) -> Box<dyn Resolver + 'a> {
        Box::new(self)
    }
}

fn node(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(path.parse::<SemanticPath>().unwrap(), kind)
}

fn rust_repo() -> Repo {
    Repo::new(&[
        ("Cargo.toml", MANIFEST),
        ("src/lib.rs", LIB),
        ("src/b.rs", B),
    ])
}

struct Run {
    store: Store,
    report: PipelineReport,
    _db: tempfile::TempDir,
}

fn run(
    repo: &Repo,
    adapter: FakeAdapter,
    provisioner: Provisioner,
    scip: Option<&dyn ResolverSource>,
) -> Run {
    let db = tempfile::tempdir().unwrap();
    let mut store = Store::create(&db.path().join("index.db")).unwrap();
    let registry = Registry::default().with_servers(vec![(Language::Rust, Box::new(adapter))]);
    let servers = BatchServers {
        provisioner: &provisioner,
        options: EnrichOptions::default(),
    };
    let sources = Sources {
        servers: &servers,
        scip: scip.map(|source| Scip {
            source,
            hash: "stub",
        }),
        progress: &(),
    };
    let report = Pipeline::sync(
        repo.path(),
        &repo.config(),
        &mut store,
        &registry,
        &sources,
        SyncMode::Full,
    )
    .unwrap();
    Run {
        store,
        report: report.pipeline,
        _db: db,
    }
}

fn provisioner(repo: &Repo, config: &Config) -> Provisioner {
    Provisioner::new(config, repo.home(), false)
}

fn edges_between(run: &Run, src: &NodeRef, kind: EdgeKind, dst: &NodeRef) -> Vec<VerifiedBy> {
    let id = run
        .store
        .node_by_semantic_path(&src.path, Some(src.kind))
        .unwrap()[0]
        .id;
    run.store
        .edges_from(id, &[kind])
        .unwrap()
        .into_iter()
        .filter(|e| &e.edge.dst == dst)
        .map(|e| e.edge.verified_by)
        .collect()
}

#[test]
fn scip_covered_file_skips_lsp() {
    let repo = rust_repo();
    let helper_call = repo.candidate("src/lib.rs", "helper", "run");
    let lsp_answer = repo.respond(
        "textDocument/definition",
        &helper_call,
        json!([repo.location("src/b.rs", "pub fn ", "helper")]),
    );
    let adapter = FakeAdapter::new(&repo.scenario(json!([lsp_answer])));
    let helper = node("src/b.rs::helper", NodeKind::Function);
    let mut scip = StubScip::default();
    scip.covers.insert(RepoPath::new("src/lib.rs").unwrap());
    scip.verdicts.insert(
        ("src/lib.rs".to_owned(), "helper".to_owned()),
        Verdict::Confirmed {
            target: helper.clone(),
        },
    );
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), Some(&scip));
    let caller = node("src/lib.rs::run", NodeKind::Function);
    assert_eq!(
        edges_between(&run, &caller, EdgeKind::Calls, &helper),
        [VerifiedBy::Scip]
    );
    let lib = run
        .store
        .file_by_path(&RepoPath::new("src/lib.rs").unwrap())
        .unwrap()
        .unwrap();
    assert!(lib.record.scip_covered);
    assert_eq!(repo.opened(), [repo.uri("src/b.rs")]);
    let status = &run.report.statuses[0].1;
    assert_eq!(status.coverage, Coverage::ScipAndLsp);
}

#[test]
fn missing_cargo_degrades_root_with_no_cargo() {
    let repo = rust_repo();
    let mut adapter = FakeAdapter::new(&repo.scenario(json!([])));
    adapter.toolchain = Some(Toolchain {
        prerequisite: "cargo",
        locate: &["rustup", "which", "fake"],
        install: &["rustup", "component", "add", "fake"],
    });
    let config = repo.config();
    let stripped = provisioner(&repo, &config).with_search_path(None);
    let run = run(&repo, adapter, stripped, None);
    let (_, status) = &run.report.statuses[0];
    assert_eq!(status.degraded, Some(DegradeReason::NoCargo));
    assert_eq!(status.coverage, Coverage::None);
    let roots = run.store.roots().unwrap();
    assert_eq!(
        roots[0].status.as_ref().unwrap().degraded,
        Some(DegradeReason::NoCargo)
    );
    assert!(repo.log().is_empty(), "the server was started");
}

#[test]
fn edge_discovered_from_other_side_is_written_only_by_owner() {
    let repo = rust_repo();
    let trait_t = node("src/b.rs::T", NodeKind::Trait);
    let struct_s = node("src/lib.rs::S", NodeKind::Struct);
    let b_text_trait = repo.location("src/b.rs", "pub trait ", "T");
    let at = b_text_trait["range"]["start"].clone();
    let implementation = json!({"method": "textDocument/implementation", "uri": "src/b.rs",
        "line": at["line"], "character": at["character"],
        "result": [repo.location("src/lib.rs", "pub struct ", "S")]});
    let adapter = FakeAdapter::new(&repo.scenario(json!([implementation])));
    let mut scip = StubScip::default();
    scip.covers.insert(RepoPath::new("src/lib.rs").unwrap());
    scip.implementers
        .insert(trait_t.clone(), vec![struct_s.clone()]);
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), Some(&scip));
    assert!(repo.log().contains("textDocument/implementation"));
    assert_eq!(
        edges_between(&run, &struct_s, EdgeKind::Implements, &trait_t),
        [VerifiedBy::Scip]
    );
}

#[test]
fn implementer_in_another_root_is_written_by_its_own_root() {
    let repo = Repo::new(&[
        (
            "a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        ),
        ("a/src/lib.rs", "pub trait T {\n    fn go(&self);\n}\n"),
        (
            "b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        ),
        (
            "b/src/lib.rs",
            "pub struct S;\n\nimpl a::T for S {\n    fn go(&self) {}\n}\n",
        ),
    ]);
    let at = repo.location("a/src/lib.rs", "pub trait ", "T")["range"]["start"].clone();
    let implementation = json!({"method": "textDocument/implementation", "uri": "a/src/lib.rs",
        "line": at["line"], "character": at["character"],
        "result": [repo.location("b/src/lib.rs", "pub struct ", "S")]});
    let adapter = FakeAdapter::new(&repo.scenario(json!([implementation])));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), None);
    let trait_t = node("a/src/lib.rs::T", NodeKind::Trait);
    let struct_s = node("b/src/lib.rs::S", NodeKind::Struct);
    let id = run
        .store
        .node_by_semantic_path(&struct_s.path, Some(struct_s.kind))
        .unwrap()[0]
        .id;
    let implements: Vec<_> = run
        .store
        .edges_from(id, &[EdgeKind::Implements])
        .unwrap()
        .into_iter()
        .filter(|e| e.edge.dst == trait_t)
        .collect();
    assert_eq!(implements.len(), 1, "{implements:#?}");
    assert_eq!(implements[0].edge.verified_by, VerifiedBy::Lsp);
    let root_b = run
        .store
        .roots()
        .unwrap()
        .into_iter()
        .find(|r| r.root.path.as_str() == "b")
        .unwrap();
    assert_eq!(implements[0].root_id, Some(root_b.id));
    let launches = repo.launches();
    let (_, received) = launches
        .iter()
        .find(|(root, _)| root.ends_with("/b"))
        .unwrap();
    assert!(received.iter().any(|message| {
        message["method"] == "textDocument/implementation"
            && message["params"]["textDocument"]["uri"] == repo.uri("a/src/lib.rs")
    }));
}

#[test]
fn crash_in_implementer_pass_degrades_the_root() {
    let repo = Repo::new(&[
        (
            "a/Cargo.toml",
            "[package]\nname = \"a\"\nversion = \"0.1.0\"\n",
        ),
        ("a/src/lib.rs", "pub trait T {\n    fn go(&self);\n}\n"),
        (
            "b/Cargo.toml",
            "[package]\nname = \"b\"\nversion = \"0.1.0\"\n",
        ),
        ("b/src/lib.rs", "pub struct S;\n"),
    ]);
    let scenario_path = repo.scenario(json!([]));
    let mut scenario: Value =
        serde_json::from_str(&std::fs::read_to_string(&scenario_path).unwrap()).unwrap();
    scenario["crash_after"] = json!([null, 0, 0]);
    scenario["state_file"] = json!(repo.scratch.path().join("launches"));
    std::fs::write(&scenario_path, scenario.to_string()).unwrap();
    let adapter = FakeAdapter::new(&scenario_path);
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), None);
    let status = |root: &str| {
        run.report
            .statuses
            .iter()
            .find(|(r, _)| r.path.as_str() == root)
            .map(|(_, status)| status.clone())
            .unwrap()
    };
    assert_eq!(status("a").degraded, None);
    assert_eq!(status("b").degraded, Some(DegradeReason::Crashed));
    assert_eq!(status("b").coverage, Coverage::None);
    let launches = repo.launches();
    let b_launches: Vec<_> = launches
        .iter()
        .filter(|(root, _)| root.ends_with("/b"))
        .collect();
    assert_eq!(b_launches.len(), 2, "one start and one restart");
    assert!(b_launches.iter().all(|(_, received)| {
        received.iter().any(|message| {
            message["method"] == "textDocument/implementation"
                && message["params"]["textDocument"]["uri"] == repo.uri("a/src/lib.rs")
        })
    }));
}

#[test]
fn scip_impl_block_yields_implements_and_overrides() {
    let repo = Repo::new(&[
        ("Cargo.toml", MANIFEST),
        (
            "src/lib.rs",
            "mod b;\n\npub struct S;\n\nimpl b::T for S {\n    fn go(&self) {}\n}\n",
        ),
        ("src/b.rs", B),
    ]);
    let trait_t = node("src/b.rs::T", NodeKind::Trait);
    let struct_s = node("src/lib.rs::S", NodeKind::Struct);
    let mut scip = StubScip::default();
    for file in ["src/lib.rs", "src/b.rs"] {
        scip.covers.insert(RepoPath::new(file).unwrap());
    }
    for (text, target) in [("T", &trait_t), ("S", &struct_s)] {
        scip.verdicts.insert(
            ("src/lib.rs".to_owned(), text.to_owned()),
            Verdict::Confirmed {
                target: target.clone(),
            },
        );
    }
    let adapter = FakeAdapter::new(&repo.scenario(json!([])));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), Some(&scip));
    let id = |n: &NodeRef| {
        run.store
            .node_by_semantic_path(&n.path, Some(n.kind))
            .unwrap()[0]
            .id
    };
    let implements = run
        .store
        .edges_from(id(&struct_s), &[EdgeKind::Implements])
        .unwrap();
    assert_eq!(implements.len(), 1, "{implements:#?}");
    assert_eq!(implements[0].edge.dst, trait_t);
    assert_eq!(implements[0].edge.verified_by, VerifiedBy::Scip);
    assert_eq!(
        implements[0].edge.occurrences,
        [Occurrence { line: 5, col: 8 }]
    );
    let overrides = run
        .store
        .edges_to(
            id(&node("src/b.rs::T::go", NodeKind::Method)),
            &[EdgeKind::Overrides],
        )
        .unwrap();
    assert_eq!(overrides.len(), 1, "{overrides:#?}");
    assert_eq!(overrides[0].edge.src.kind, NodeKind::Method);
    assert_eq!(overrides[0].edge.src.path.name(), "go");
    assert_ne!(overrides[0].edge.src, overrides[0].edge.dst);
    assert_eq!(overrides[0].edge.verified_by, VerifiedBy::Scip);
    assert!(overrides[0].edge.occurrences.is_empty());
}

#[test]
fn scip_reference_self_type_implements_through_its_type_identifier() {
    let repo = Repo::new(&[
        ("Cargo.toml", MANIFEST),
        (
            "src/lib.rs",
            "mod b;\n\npub struct S;\n\nimpl b::T for &S {\n    fn go(&self) {}\n}\n",
        ),
        ("src/b.rs", B),
    ]);
    let trait_t = node("src/b.rs::T", NodeKind::Trait);
    let struct_s = node("src/lib.rs::S", NodeKind::Struct);
    let mut scip = StubScip::default();
    for file in ["src/lib.rs", "src/b.rs"] {
        scip.covers.insert(RepoPath::new(file).unwrap());
    }
    for (text, target) in [("T", &trait_t), ("S", &struct_s)] {
        scip.verdicts.insert(
            ("src/lib.rs".to_owned(), text.to_owned()),
            Verdict::Confirmed {
                target: target.clone(),
            },
        );
    }
    let adapter = FakeAdapter::new(&repo.scenario(json!([])));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), Some(&scip));
    assert_eq!(
        edges_between(&run, &struct_s, EdgeKind::Implements, &trait_t),
        [VerifiedBy::Scip]
    );
}

#[test]
fn scip_implementers_are_kept_only_for_implementing_kinds() {
    let repo = Repo::new(&[
        ("Cargo.toml", MANIFEST),
        (
            "src/lib.rs",
            "mod b;\n\npub struct S;\n\npub trait U {\n    fn go(&self);\n}\n",
        ),
        ("src/b.rs", B),
    ]);
    let trait_t = node("src/b.rs::T", NodeKind::Trait);
    let t_go = node("src/b.rs::T::go", NodeKind::Method);
    let struct_s = node("src/lib.rs::S", NodeKind::Struct);
    let trait_u = node("src/lib.rs::U", NodeKind::Trait);
    let u_go = node("src/lib.rs::U::go", NodeKind::Method);
    let helper = node("src/b.rs::helper", NodeKind::Function);
    let mut scip = StubScip::default();
    for file in ["src/lib.rs", "src/b.rs"] {
        scip.covers.insert(RepoPath::new(file).unwrap());
    }
    scip.implementers
        .insert(trait_t.clone(), vec![struct_s.clone(), trait_u.clone()]);
    scip.implementers
        .insert(t_go.clone(), vec![u_go.clone(), helper.clone()]);
    let adapter = FakeAdapter::new(&repo.scenario(json!([])));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), Some(&scip));
    assert_eq!(
        edges_between(&run, &struct_s, EdgeKind::Implements, &trait_t),
        [VerifiedBy::Scip]
    );
    assert!(edges_between(&run, &trait_u, EdgeKind::Implements, &trait_t).is_empty());
    assert_eq!(
        edges_between(&run, &u_go, EdgeKind::Overrides, &t_go),
        [VerifiedBy::Scip]
    );
    assert!(edges_between(&run, &helper, EdgeKind::Overrides, &t_go).is_empty());
}

#[test]
fn definition_in_unparsed_file_is_unindexed_file() {
    let repo = Repo::new(&[
        ("crate/Cargo.toml", MANIFEST),
        ("crate/src/lib.rs", "pub fn run() {\n    generated();\n}\n"),
        ("scripts/gen.rs", "pub fn generated() {}\n"),
    ]);
    let call = repo.candidate("crate/src/lib.rs", "generated", "run");
    let answer = repo.respond(
        "textDocument/definition",
        &call,
        json!([repo.location("scripts/gen.rs", "pub fn ", "generated")]),
    );
    let adapter = FakeAdapter::new(&repo.scenario(json!([answer])));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), None);
    let rows = run
        .store
        .unresolved_for_file(&RepoPath::new("crate/src/lib.rs").unwrap())
        .unwrap();
    let row = rows
        .iter()
        .find(|r| r.unresolved.candidate.text == "generated")
        .unwrap();
    assert_eq!(row.unresolved.reason, UnresolvedReason::UnindexedFile);
    let dumped = run.store.dump_edges().unwrap();
    assert!(!dumped.contains("generated"), "{dumped}");
}

#[test]
fn degraded_root_keeps_nodes_and_structural_edges_only() {
    let repo = rust_repo();
    let adapter = FakeAdapter::new(&repo.scenario(json!([])));
    let mut config = repo.config();
    config.servers.clear();
    let nothing = provisioner(&repo, &config).with_search_path(None);
    let run = run(&repo, adapter, nothing, None);
    assert_eq!(
        run.report.statuses[0].1.degraded,
        Some(DegradeReason::NoServer)
    );
    let db = tempfile::tempdir().unwrap();
    let mut parse_only = Store::create(&db.path().join("index.db")).unwrap();
    Pipeline::parse_only(repo.path(), &config, &mut parse_only, &Registry::default()).unwrap();
    assert_eq!(
        run.store.dump_nodes().unwrap(),
        parse_only.dump_nodes().unwrap()
    );
    assert_eq!(
        run.store.dump_edges().unwrap(),
        parse_only.dump_edges().unwrap()
    );
    assert_eq!(
        run.store.dump_unresolved().unwrap(),
        parse_only.dump_unresolved().unwrap()
    );
}

#[test]
fn node_counts_updated() {
    let repo = rust_repo();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("dep.rs"), "pub struct S;\n").unwrap();
    let external = codemycel_lsp::client::path_to_uri(&outside.path().join("dep.rs"))
        .unwrap()
        .as_str()
        .to_owned();
    let type_use = repo.candidate("src/lib.rs", "S", "run");
    let answers: Vec<Value> = vec![repo.respond(
        "textDocument/definition",
        &type_use,
        json!([{"uri": external, "range": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 12}}}]),
    )];
    let adapter = FakeAdapter::new(&repo.scenario(json!(answers)));
    let config = repo.config();
    let run = run(&repo, adapter, provisioner(&repo, &config), None);
    let caller = node("src/lib.rs::run", NodeKind::Function);
    let stored = &run
        .store
        .node_by_semantic_path(&caller.path, Some(caller.kind))
        .unwrap()[0];
    let rows = run.store.unresolved_for_node(stored.id).unwrap();
    let count = |external: bool| {
        rows.iter()
            .filter(|r| {
                matches!(
                    r.unresolved.reason,
                    UnresolvedReason::ExternalDependency | UnresolvedReason::UnindexedFile
                ) == external
            })
            .count() as u32
    };
    assert!(count(true) >= 1 && count(false) >= 1, "{rows:#?}");
    assert_eq!(stored.external_out, count(true));
    assert_eq!(stored.unresolved_out, count(false));
}
