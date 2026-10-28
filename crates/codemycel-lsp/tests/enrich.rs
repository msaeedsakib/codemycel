#![allow(clippy::unwrap_used)]

mod support;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use codemycel_core::config::ServerConfig;
use codemycel_core::{
    Candidate, CandidateRole, DegradeReason, EdgeKind, LanguageAdapter, NodeKind, NodeRef,
    ReadinessStrategy, RepoPath, SemanticPath, UnresolvedReason, VerifiedBy,
};
use codemycel_lsp::client::LineIndex;
use codemycel_lsp::{
    EnrichOptions, EnrichReport, Enricher, FileView, IndexView, Readiness, ServerLaunch,
};
use codemycel_parse::rust::RustAdapter;
use lsp_types::Position;
use serde_json::{Value, json};
use support::{FakeAdapter, fake_lsp};
use tempfile::TempDir;

const LIB: &str = "\
use other::Thing;

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle;

impl Shape for Circle {
    fn area(&self) -> f64 {
        helper()
    }
}

pub fn helper() -> f64 {
    1.0
}

pub fn run() -> Circle {
    helper();
    Circle
}
";

const FILE: &str = "src/lib.rs";

struct Fixture {
    dir: TempDir,
    view: IndexView,
    files: Vec<RepoPath>,
    language: RustAdapter,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let language = RustAdapter::new();
        let path = RepoPath::new(FILE).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join(FILE), LIB).unwrap();
        std::fs::write(dir.path().join("tsconfig.json"), "{}").unwrap();
        let parsed = language.parse(&path, LIB.as_bytes()).unwrap();
        let mut view = IndexView::default();
        view.insert(
            path.clone(),
            FileView::new(&path, LIB.to_owned(), parsed).unwrap(),
        );
        Self {
            dir,
            view,
            files: vec![path],
            language,
        }
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn candidates(&self) -> &[Candidate] {
        &self.view.file(&self.files[0]).unwrap().candidates
    }

    /// The candidate with `text` whose enclosing node is named `enclosing`.
    fn candidate(&self, text: &str, enclosing: &str) -> Candidate {
        self.candidates()
            .iter()
            .find(|c| c.text == text && c.enclosing.path.name() == enclosing)
            .cloned()
            .unwrap_or_else(|| {
                panic!(
                    "no `{text}` in `{enclosing}` among {:#?}",
                    self.candidates()
                )
            })
    }

    fn position(&self, candidate: &Candidate) -> Position {
        LineIndex::new(LIB).to_lsp(candidate.at).unwrap()
    }

    /// The range of the first `needle` after the first `anchor`.
    fn range(&self, anchor: &str, needle: &str) -> Value {
        let start =
            LIB.find(anchor).unwrap() + LIB[LIB.find(anchor).unwrap()..].find(needle).unwrap();
        let index = LineIndex::new(LIB);
        let at = |offset: usize| index.position(u32::try_from(offset).unwrap()).unwrap();
        json!({"start": at(start), "end": at(start + needle.len())})
    }

    fn location(&self, anchor: &str, needle: &str) -> Value {
        json!({"uri": format!("$ROOT/{FILE}"), "range": self.range(anchor, needle)})
    }

    fn scenario(&self, mut scenario: Value) -> PathBuf {
        if scenario.get("after_initialized").is_none() {
            scenario["after_initialized"] = json!([quiescent()]);
        }
        let path = self.root().join("scenario.json");
        std::fs::write(&path, scenario.to_string()).unwrap();
        path
    }

    async fn start<'a>(
        &'a self,
        adapter: &'a FakeAdapter,
        binary: PathBuf,
        options: EnrichOptions,
    ) -> Result<Enricher<'a>, DegradeReason> {
        let launch = ServerLaunch {
            adapter,
            binary,
            config: ServerConfig::default(),
            search_path: None,
        };
        Enricher::start(
            launch,
            &self.language,
            &self.view,
            self.root(),
            &RepoPath::new("").unwrap(),
            &self.files,
            options,
        )
        .await
    }

    async fn enrich(&self, scenario: Value) -> EnrichReport {
        let adapter = FakeAdapter::new(&self.scenario(scenario));
        let enricher = self
            .start(&adapter, fake_lsp(), EnrichOptions::default())
            .await
            .unwrap();
        let report = enricher.enrich_files(&self.files).await;
        enricher.shutdown().await;
        report
    }
}

fn quiescent() -> Value {
    json!({"method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": true}})
}

fn respond(method: &str, position: Position, result: Value) -> Value {
    json!({"method": method, "uri": FILE, "line": position.line, "character": position.character, "result": result})
}

fn node(path: &str, kind: NodeKind) -> NodeRef {
    NodeRef::new(path.parse::<SemanticPath>().unwrap(), kind)
}

fn reason_of(report: &EnrichReport, candidate: &Candidate) -> Option<UnresolvedReason> {
    report
        .unresolved
        .iter()
        .find(|u| &u.candidate == candidate)
        .map(|u| u.reason)
}

fn has_edge(report: &EnrichReport, src: &NodeRef, kind: EdgeKind, dst: &NodeRef) -> bool {
    report.edges.iter().any(|e| {
        &e.src == src && e.kind == kind && &e.dst == dst && e.verified_by == VerifiedBy::Lsp
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn hanging_server_degrades_root_within_timeout() {
    let fixture = Fixture::new();
    let adapter = FakeAdapter::new(&fixture.scenario(json!({"after_initialized": []}))).readiness(
        ReadinessStrategy::WaitForQuiescent {
            timeout: Duration::from_secs(1),
        },
    );
    let started = Instant::now();
    let result = fixture
        .start(&adapter, fake_lsp(), EnrichOptions::default())
        .await;
    assert!(matches!(result, Err(DegradeReason::NotReady)));
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_timeout_proceeds_without_degrading() {
    let fixture = Fixture::new();
    let scenario = json!({"responses": [
        {"method": "textDocument/definition", "silent_times": 1000000}
    ]});
    let adapter = FakeAdapter::new(&fixture.scenario(scenario)).readiness(
        ReadinessStrategy::ProbeDefinition {
            timeout: Duration::from_secs(1),
        },
    );
    let options = EnrichOptions {
        request_timeout: Duration::from_millis(200),
    };
    let enricher = fixture.start(&adapter, fake_lsp(), options).await.unwrap();
    assert_eq!(enricher.readiness(), Readiness::ReadyAfterProbeTimeout);
    let report = enricher.enrich_files(&fixture.files).await;
    enricher.shutdown().await;
    assert_eq!(report.degraded, None);
    assert_eq!(report.unresolved.len(), fixture.candidates().len());
    assert!(
        report
            .unresolved
            .iter()
            .all(|u| u.reason == UnresolvedReason::ServerUnavailable)
    );
    assert!(report.partially_enriched.contains(&fixture.files[0]));
}

#[tokio::test(flavor = "multi_thread")]
async fn probe_skips_files_without_candidates() {
    let fixture = Fixture::new();
    let empty = RepoPath::new("src/a.rs").unwrap();
    let empty_text = "pub struct A;\n";
    std::fs::write(fixture.root().join(empty.as_str()), empty_text).unwrap();
    let parsed = fixture
        .language
        .parse(&empty, empty_text.as_bytes())
        .unwrap();
    assert!(parsed.candidates.is_empty());
    let mut view = fixture.view.clone();
    view.insert(
        empty.clone(),
        FileView::new(&empty, empty_text.to_owned(), parsed).unwrap(),
    );
    let import = fixture
        .candidates()
        .iter()
        .find(|c| c.role == CandidateRole::Import)
        .cloned()
        .unwrap();
    let log = fixture.root().join("messages.log");
    let scenario = json!({
        "log_file": log,
        "responses": [respond(
            "textDocument/definition",
            fixture.position(&import),
            json!([fixture.location("pub struct ", "Circle")])
        )],
    });
    let adapter = FakeAdapter::new(&fixture.scenario(scenario)).readiness(
        ReadinessStrategy::ProbeDefinition {
            timeout: Duration::from_secs(10),
        },
    );
    let launch = ServerLaunch {
        adapter: &adapter,
        binary: fake_lsp(),
        config: ServerConfig::default(),
        search_path: None,
    };
    let files = [empty, fixture.files[0].clone()];
    let enricher = Enricher::start(
        launch,
        &fixture.language,
        &view,
        fixture.root(),
        &RepoPath::new("").unwrap(),
        &files,
        EnrichOptions::default(),
    )
    .await
    .unwrap();
    assert_eq!(enricher.readiness(), Readiness::Ready);
    enricher.shutdown().await;
    let probe = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|message| message["method"] == "textDocument/definition")
        .unwrap();
    let at = fixture.position(&import);
    assert!(
        probe["params"]["textDocument"]["uri"]
            .as_str()
            .unwrap()
            .ends_with(FILE)
    );
    assert_eq!(probe["params"]["position"], json!(at));
}

#[tokio::test(flavor = "multi_thread")]
async fn launch_failure_is_start_failed() {
    let fixture = Fixture::new();
    let adapter = FakeAdapter::new(&fixture.scenario(json!({})));
    let missing = fixture.root().join("no-such-server");
    let result = fixture
        .start(&adapter, missing, EnrichOptions::default())
        .await;
    assert!(matches!(result, Err(DegradeReason::StartFailed)));
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_restarts_once_then_marks_unavailable() {
    let fixture = Fixture::new();
    let circle = fixture.candidate("Circle", "run");
    let responses = json!([respond(
        "textDocument/definition",
        fixture.position(&circle),
        json!([fixture.location("pub struct ", "Circle")])
    )]);
    let expected = fixture.enrich(json!({ "responses": responses })).await;
    assert!(has_edge(
        &expected,
        &node("src/lib.rs::run", NodeKind::Function),
        EdgeKind::References,
        &node("src/lib.rs::Circle", NodeKind::Struct)
    ));

    let state = fixture.root().join("launches-once");
    let once = fixture
        .enrich(json!({"responses": responses, "crash_after": [1, null], "state_file": state}))
        .await;
    assert_eq!(std::fs::read_to_string(&state).unwrap(), "2");
    assert_eq!(once, expected);

    let state = fixture.root().join("launches-twice");
    let twice = fixture
        .enrich(json!({"responses": responses, "crash_after": [1, 1], "state_file": state}))
        .await;
    assert_eq!(twice.degraded, Some(DegradeReason::Crashed));
    assert!(twice.edges.is_empty());
    assert_eq!(twice.unresolved.len(), fixture.candidates().len());
    assert!(
        twice
            .unresolved
            .iter()
            .all(|u| u.reason == UnresolvedReason::ServerUnavailable)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn config_diagnostics_are_collected_from_publish_diagnostics() {
    let fixture = Fixture::new();
    let diagnostic = |file: &str, message: &str| {
        json!({"method": "textDocument/publishDiagnostics", "params": {
            "uri": format!("$ROOT/{file}"),
            "diagnostics": [{"range": {"start": {"line": 2, "character": 4}, "end": {"line": 2, "character": 13}}, "message": message}]
        }})
    };
    let scenario = fixture.scenario(json!({"after_initialized": [
        diagnostic("tsconfig.json", "Option 'baseUrl' has been removed."),
        diagnostic(FILE, "unrelated"),
        quiescent(),
    ]}));
    let mut adapter = FakeAdapter::new(&scenario);
    adapter.config_file = Some("tsconfig.json");
    let enricher = fixture
        .start(&adapter, fake_lsp(), EnrichOptions::default())
        .await
        .unwrap();
    let report = enricher.enrich_files(&fixture.files).await;
    enricher.shutdown().await;
    assert_eq!(report.degraded, None);
    assert_eq!(
        report.config_diagnostics,
        ["tsconfig.json:3: Option 'baseUrl' has been removed."]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn outgoing_calls_produce_calls_edges_with_from_ranges() {
    let fixture = Fixture::new();
    let run_selection = fixture.range("pub fn ", "run");
    let run_item = json!({"name": "run", "kind": 12, "uri": format!("$ROOT/{FILE}"),
        "range": run_selection, "selectionRange": run_selection});
    let helper_selection = fixture.range("pub fn ", "helper");
    let helper_item = json!({"name": "helper", "kind": 12, "uri": format!("$ROOT/{FILE}"),
        "range": helper_selection, "selectionRange": helper_selection});
    let run_at: Position = serde_json::from_value(run_selection["start"].clone()).unwrap();
    let scenario = json!({"responses": [
        respond("textDocument/prepareCallHierarchy", run_at, json!([run_item])),
        respond("callHierarchy/outgoingCalls", run_at, json!([
            {"to": helper_item, "fromRanges": [fixture.range("pub fn run", "helper")]}
        ])),
    ]});
    let report = fixture.enrich(scenario).await;
    let run = node("src/lib.rs::run", NodeKind::Function);
    let helper = node("src/lib.rs::helper", NodeKind::Function);
    let calls: Vec<_> = report
        .edges
        .iter()
        .filter(|e| e.src == run && e.dst == helper && e.kind == EdgeKind::Calls)
        .collect();
    assert_eq!(calls.len(), 1, "{:#?}", report.edges);
    let call = fixture.candidate("helper", "run");
    assert_eq!(calls[0].occurrences, [call.at]);
    assert_eq!(
        reason_of(&report, &call),
        None,
        "covered by the call hierarchy"
    );
    let other = fixture.candidate("helper", "area");
    assert_eq!(
        reason_of(&report, &other),
        Some(UnresolvedReason::NoDefinition)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn definition_maps_by_selection_range_containment() {
    let fixture = Fixture::new();
    let circle = fixture.candidate("Circle", "run");
    let inside_name = fixture.range("pub struct C", "ircle");
    let scenario = json!({"responses": [respond(
        "textDocument/definition",
        fixture.position(&circle),
        json!([{"uri": format!("$ROOT/{FILE}"), "range": inside_name}])
    )]});
    let report = fixture.enrich(scenario).await;
    assert!(has_edge(
        &report,
        &node("src/lib.rs::run", NodeKind::Function),
        EdgeKind::References,
        &node("src/lib.rs::Circle", NodeKind::Struct)
    ));
    assert_eq!(reason_of(&report, &circle), None);
}

#[tokio::test(flavor = "multi_thread")]
async fn whole_file_and_other_declaration_ranges_map_to_their_nodes() {
    let fixture = Fixture::new();
    let circle = fixture.candidate("Circle", "run");
    let call = fixture.candidate("helper", "area");
    let lines = u32::try_from(LIB.lines().count()).unwrap();
    let whole_file = json!([{"uri": format!("$ROOT/{FILE}"), "range":
        {"start": {"line": 0, "character": 0}, "end": {"line": lines, "character": 0}}}]);
    let scenario = json!({"responses": [
        respond("textDocument/definition", fixture.position(&circle), whole_file),
        respond("textDocument/definition", fixture.position(&call),
            json!([fixture.location("pub fn run", "helper")])),
    ]});
    let report = fixture.enrich(scenario).await;
    assert!(has_edge(
        &report,
        &node("src/lib.rs::run", NodeKind::Function),
        EdgeKind::References,
        &node("src/lib.rs", NodeKind::File)
    ));
    assert!(
        has_edge(
            &report,
            &node("src/lib.rs::<Circle as Shape>::area", NodeKind::Method),
            EdgeKind::Calls,
            &node("src/lib.rs::helper", NodeKind::Function)
        ),
        "{:#?}",
        report.edges
    );
}

#[test]
fn server_paths_are_normalised() {
    let root = Path::new("/work/crates/index/../../fixtures/./demo");
    assert_eq!(
        codemycel_lsp::enrich::absolute(root, &RepoPath::new("src/lib.rs").unwrap()),
        Path::new("/work/fixtures/demo/src/lib.rs")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn definition_landing_in_impl_header_maps_to_self_type() {
    let fixture = Fixture::new();
    let call = fixture.candidate("helper", "area");
    let self_type_at: Position =
        serde_json::from_value(fixture.range("impl Shape for ", "Circle")["start"].clone())
            .unwrap();
    let scenario = json!({"responses": [
        respond("textDocument/definition", fixture.position(&call),
            json!([fixture.location("impl Shape for ", "Circle")])),
        respond("textDocument/definition", self_type_at,
            json!([fixture.location("pub struct ", "Circle")])),
    ]});
    let report = fixture.enrich(scenario).await;
    assert_eq!(reason_of(&report, &call), None, "{:#?}", report.unresolved);
    assert!(has_edge(
        &report,
        &node("src/lib.rs::<Circle as Shape>::area", NodeKind::Method),
        EdgeKind::References,
        &node("src/lib.rs::Circle", NodeKind::Struct)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn implementation_on_trait_landing_in_impl_block_maps_to_self_type() {
    let fixture = Fixture::new();
    let shape_at: Position =
        serde_json::from_value(fixture.range("pub trait ", "Shape")["start"].clone()).unwrap();
    let self_type_at: Position =
        serde_json::from_value(fixture.range("impl Shape for ", "Circle")["start"].clone())
            .unwrap();
    let scenario = json!({"responses": [
        respond("textDocument/implementation", shape_at,
            json!([fixture.location("impl Shape for ", "Circle")])),
        respond("textDocument/definition", self_type_at,
            json!([fixture.location("pub struct ", "Circle")])),
    ]});
    let report = fixture.enrich(scenario).await;
    assert!(
        has_edge(
            &report,
            &node("src/lib.rs::Circle", NodeKind::Struct),
            EdgeKind::Implements,
            &node("src/lib.rs::Shape", NodeKind::Trait)
        ),
        "{:#?}",
        report.edges
    );
    assert!(has_edge(
        &report,
        &node("src/lib.rs::<Circle as Shape>::area", NodeKind::Method),
        EdgeKind::Overrides,
        &node("src/lib.rs::Shape::area", NodeKind::Method)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn external_definition_is_external_dependency() {
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let dependency = outside.path().join("thing.rs");
    std::fs::write(&dependency, "pub struct Thing;\n").unwrap();
    let import = fixture
        .candidates()
        .iter()
        .find(|c| c.role == CandidateRole::Import && c.text.ends_with("Thing"))
        .cloned()
        .unwrap_or_else(|| panic!("no import of Thing among {:#?}", fixture.candidates()));
    let uri = url::Url::from_file_path(&dependency).unwrap().to_string();
    let scenario = json!({"responses": [respond(
        "textDocument/definition",
        fixture.position(&import),
        json!([{"uri": uri, "range": {"start": {"line": 0, "character": 11}, "end": {"line": 0, "character": 16}}}])
    )]});
    let report = fixture.enrich(scenario).await;
    assert_eq!(
        reason_of(&report, &import),
        Some(UnresolvedReason::ExternalDependency)
    );
    assert!(report.edges.iter().all(|e| e.kind != EdgeKind::Imports));
}

#[tokio::test(flavor = "multi_thread")]
async fn unmapped_position_inside_indexed_file_is_unmapped() {
    let fixture = Fixture::new();
    let call = fixture.candidate("helper", "area");
    let scenario = json!({"responses": [respond(
        "textDocument/definition",
        fixture.position(&call),
        json!([fixture.location("-> f64 {\n    ", "1.0")])
    )]});
    let report = fixture.enrich(scenario).await;
    assert_eq!(reason_of(&report, &call), Some(UnresolvedReason::Unmapped));
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_enrichment_flag_set_after_second_failure() {
    let fixture = Fixture::new();
    let failing = fixture.candidate("helper", "area");
    let retried = fixture.candidate("Circle", "run");
    let mut fail_twice = respond(
        "textDocument/definition",
        fixture.position(&failing),
        json!([fixture.location("pub fn ", "helper")]),
    );
    fail_twice["fail_times"] = json!(2);
    let mut fail_once = respond(
        "textDocument/definition",
        fixture.position(&retried),
        json!([fixture.location("pub struct ", "Circle")]),
    );
    fail_once["fail_times"] = json!(1);
    let report = fixture
        .enrich(json!({"responses": [fail_twice, fail_once]}))
        .await;
    assert_eq!(
        reason_of(&report, &failing),
        Some(UnresolvedReason::ServerUnavailable)
    );
    assert_eq!(reason_of(&report, &retried), None, "the retry succeeded");
    assert!(report.partially_enriched.contains(&fixture.files[0]));
}
