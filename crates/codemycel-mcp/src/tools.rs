use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::query::{Answer, Budget, QueryEngine, QueryError};
use codemycel_core::{EdgeKind, NodeKind, NoteKind, UnresolvedReason};
use codemycel_index::NoteRequest;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::Deserialize;
use serde_json::json;

use crate::server::Codemycel;
use crate::session::Session;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Free text; identifiers are split into words
    pub query: String,
    /// Only return nodes of these kinds
    #[serde(default)]
    #[schemars(schema_with = "node_kinds")]
    pub kind: Vec<NodeKind>,
    #[serde(default = "limit_20")]
    pub limit: u32,
    #[serde(default = "tokens_3000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolParams {
    /// Semantic path or bare name
    pub name: String,
    /// Include the body as on disk, with line numbers
    #[serde(default)]
    pub include_body: bool,
    #[serde(default = "yes")]
    pub callers: bool,
    #[serde(default = "yes")]
    pub callees: bool,
    /// Maximum entries per list
    #[serde(default = "limit_25")]
    pub limit: u32,
    #[serde(default = "tokens_4000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ImpactParams {
    /// Semantic path or bare name
    pub name: String,
    /// Steps to walk, 1 to 5
    #[serde(default = "depth_2")]
    #[schemars(range(min = 1, max = 5))]
    pub depth: u8,
    /// Edge kinds to follow
    #[serde(default = "impact_kinds")]
    #[schemars(schema_with = "edge_kinds")]
    pub kinds: Vec<EdgeKind>,
    #[serde(default = "tokens_6000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TraceParams {
    /// Semantic path or bare name to start from
    pub from: String,
    /// Semantic path or bare name to reach
    pub to: String,
    /// Edge kinds to follow; all verified kinds when omitted
    #[serde(default)]
    #[schemars(schema_with = "edge_kinds")]
    pub kinds: Vec<EdgeKind>,
    #[serde(default = "depth_8")]
    pub max_depth: u32,
    #[serde(default = "tokens_2000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct OverviewParams {
    /// A directory, file, `community:<id>`, or package name; the repository when omitted
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default = "tokens_4000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UnresolvedParams {
    /// A file path or symbol name; a repository summary when omitted
    #[serde(default)]
    pub scope: Option<String>,
    /// Only candidates with this reason
    #[serde(default)]
    #[schemars(schema_with = "unresolved_reason")]
    pub reason: Option<UnresolvedReason>,
    #[serde(default = "tokens_3000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatusParams {
    #[serde(default = "tokens_1000")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NoteParams {
    /// A name, file path, `community:<id>`, package name, or `repo`
    pub target: String,
    /// Markdown body
    pub body: String,
    #[serde(default = "note_kind")]
    #[schemars(schema_with = "note_kinds")]
    pub kind: NoteKind,
    /// Uuid of a note to replace
    #[serde(default)]
    pub replace: Option<String>,
    #[serde(default = "tokens_500")]
    pub max_tokens: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NotesParams {
    /// A name, file path, `community:<id>`, package name, or `repo`; every note when omitted
    #[serde(default)]
    pub target: Option<String>,
    /// Only notes whose target no longer exists
    #[serde(default)]
    pub orphans: bool,
    #[serde(default = "tokens_2000")]
    pub max_tokens: u32,
}

#[tool_router(vis = "pub(crate)")]
impl Codemycel {
    #[tool(
        description = "Find symbols by words. Identifiers in the query are split like camelCase and snake_case, and matches are ranked by name, signature, doc comment and path. Returns symbol cards; pass a card's path to `symbol`.",
        annotations(read_only_hint = true)
    )]
    async fn search(&self, Parameters(params): Parameters<SearchParams>) -> String {
        self.query(move |engine| {
            engine.search(
                &params.query,
                &params.kind,
                params.limit,
                Budget::new(params.max_tokens),
            )
        })
        .await
    }

    #[tool(
        description = "One symbol by semantic path or bare name: its card, implementers, implements, overrides, callers (callers through a trait or interface marked `via`), callees, unresolved candidates with reasons, notes, and optionally its body as on disk. A bare name matching several symbols returns only the candidates.",
        annotations(read_only_hint = true)
    )]
    async fn symbol(&self, Parameters(params): Parameters<SymbolParams>) -> String {
        self.query(move |engine| {
            let options = SymbolOptions {
                include_body: params.include_body,
                callers: params.callers,
                callees: params.callees,
                limit: params.limit,
            };
            engine.symbol(&params.name, options, Budget::new(params.max_tokens))
        })
        .await
    }

    #[tool(
        description = "What depends on a symbol: a reverse walk over the given edge kinds up to `depth`, grouped by depth then file, with per-depth counts, the total, and a count of unresolved candidates that may be further dependents. Run it before changing a symbol.",
        annotations(read_only_hint = true)
    )]
    async fn impact(&self, Parameters(params): Parameters<ImpactParams>) -> String {
        if !(1..=5).contains(&params.depth) {
            return "depth must be between 1 and 5".to_owned();
        }
        self.query(move |engine| {
            engine.impact(
                &params.name,
                params.depth,
                &params.kinds,
                Budget::new(params.max_tokens),
            )
        })
        .await
    }

    #[tool(
        description = "The shortest directed path from one symbol to another, as a chain of edge lines, or the nearest symbols reached when no path exists within `max_depth`.",
        annotations(read_only_hint = true)
    )]
    async fn trace(&self, Parameters(params): Parameters<TraceParams>) -> String {
        self.query(move |engine| {
            engine.trace(
                &params.from,
                &params.to,
                &params.kinds,
                params.max_depth,
                Budget::new(params.max_tokens),
            )
        })
        .await
    }

    #[tool(
        description = "Orient in an area. No scope: packages, roots and coverage, communities, most depended-on symbols, counts. A directory or package: files, top symbols, communities. A file: symbols with counts, imports, importers. `community:<id>`: members by file, links in and out.",
        annotations(read_only_hint = true)
    )]
    async fn overview(&self, Parameters(params): Parameters<OverviewParams>) -> String {
        self.query(move |engine| {
            engine.overview(params.scope.as_deref(), Budget::new(params.max_tokens))
        })
        .await
    }

    #[tool(
        description = "Candidates no language server or SCIP index could verify. No scope: counts by reason and by root, and the top files. A file or symbol: each candidate with its line, column, role, text and reason.",
        annotations(read_only_hint = true)
    )]
    async fn unresolved(&self, Parameters(params): Parameters<UnresolvedParams>) -> String {
        self.query(move |engine| {
            engine.unresolved(
                params.scope.as_deref(),
                params.reason,
                Budget::new(params.max_tokens),
            )
        })
        .await
    }

    #[tool(
        description = "Index freshness and health: indexed commit and time, files changed since, each root's language server, version, source and coverage, degraded reasons, config diagnostics, which process owns sync, database size, and counts. Use it when an answer looks stale.",
        annotations(read_only_hint = true)
    )]
    async fn status(&self, Parameters(params): Parameters<StatusParams>) -> String {
        self.blocking(move |session| session.status(Budget::new(params.max_tokens)))
            .await
    }

    #[tool(
        description = "Store a durable Markdown note on a symbol, file, `community:<id>`, package, or `repo`; later sessions see it on the target's card. Kind `label` names a community. Pass `replace` with a note's uuid to rewrite it. Returns the note's uuid and target.",
        annotations(read_only_hint = false)
    )]
    async fn note(&self, Parameters(params): Parameters<NoteParams>) -> String {
        self.blocking(move |session| {
            let request = NoteRequest {
                target: &params.target,
                body: &params.body,
                kind: params.kind,
                replace: params.replace.as_deref(),
            };
            session.note(&request, Budget::new(params.max_tokens))
        })
        .await
    }

    #[tool(
        description = "List notes: every note, or those on one target. With `orphans`, only notes whose target no longer exists.",
        annotations(read_only_hint = true)
    )]
    async fn notes(&self, Parameters(params): Parameters<NotesParams>) -> String {
        self.query(move |engine| {
            engine.notes(
                params.target.as_deref(),
                params.orphans,
                Budget::new(params.max_tokens),
            )
        })
        .await
    }
}

impl Codemycel {
    async fn query(
        &self,
        query: impl FnOnce(&QueryEngine<'_>) -> Result<Answer, QueryError> + Send + 'static,
    ) -> String {
        self.blocking(move |session| session.answer(query)).await
    }

    /// Runs `call` on a blocking thread: every answer reads SQLite and source files.
    async fn blocking(&self, call: impl FnOnce(&Session) -> String + Send + 'static) -> String {
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || call(&session))
            .await
            .unwrap_or_else(|error| format!("error: {error}"))
    }
}

fn string_list(values: impl Iterator<Item = &'static str>) -> Schema {
    let values: Vec<&str> = values.collect();
    schema(json!({ "type": "array", "items": { "type": "string", "enum": values } }))
}

fn one_of(values: impl Iterator<Item = &'static str>) -> Schema {
    let values: Vec<&str> = values.collect();
    schema(json!({ "type": "string", "enum": values }))
}

fn schema(value: serde_json::Value) -> Schema {
    match value {
        serde_json::Value::Object(map) => Schema::from(map),
        _ => Schema::default(),
    }
}

fn node_kinds(_: &mut SchemaGenerator) -> Schema {
    string_list(NodeKind::ALL.iter().map(|kind| kind.as_str()))
}

fn edge_kinds(_: &mut SchemaGenerator) -> Schema {
    string_list(EdgeKind::ALL.iter().map(|kind| kind.as_str()))
}

fn note_kinds(_: &mut SchemaGenerator) -> Schema {
    one_of(NoteKind::ALL.iter().map(|kind| kind.as_str()))
}

fn unresolved_reason(_: &mut SchemaGenerator) -> Schema {
    one_of(UnresolvedReason::ALL.iter().map(|reason| reason.as_str()))
}

fn yes() -> bool {
    true
}

fn limit_20() -> u32 {
    20
}

fn limit_25() -> u32 {
    25
}

fn depth_2() -> u8 {
    2
}

fn depth_8() -> u32 {
    8
}

fn impact_kinds() -> Vec<EdgeKind> {
    vec![
        EdgeKind::Calls,
        EdgeKind::References,
        EdgeKind::Extends,
        EdgeKind::Implements,
        EdgeKind::Overrides,
        EdgeKind::Imports,
    ]
}

fn note_kind() -> NoteKind {
    NoteKind::Note
}

fn tokens_500() -> u32 {
    500
}

fn tokens_1000() -> u32 {
    1000
}

fn tokens_2000() -> u32 {
    2000
}

fn tokens_3000() -> u32 {
    3000
}

fn tokens_4000() -> u32 {
    4000
}

fn tokens_6000() -> u32 {
    6000
}
