use std::collections::BTreeMap;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

pub use crate::graph::CommunityGraph;

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Meta {
    pub repo: String,
    pub indexed_at: Option<String>,
}

/// A tool answer as the JSON formatter renders it: one list of lines per section.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Answer {
    pub outcome: String,
    pub truncated: Option<Truncation>,
    pub freshness: Freshness,
    #[serde(flatten)]
    pub sections: BTreeMap<String, Vec<Line>>,
}

impl Answer {
    pub fn section(&self, name: &str) -> &[Line] {
        self.sections.get(name).map_or(&[], Vec::as_slice)
    }

    pub fn edges(&self, name: &str) -> Vec<EdgeLine> {
        self.section(name)
            .iter()
            .filter_map(|line| match line {
                Line::Edge(edge) => Some(edge.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn cards(&self, name: &str) -> Vec<Card> {
        self.section(name)
            .iter()
            .filter_map(|line| match line {
                Line::Card(card) => Some(card.clone()),
                _ => None,
            })
            .collect()
    }

    pub fn fields(&self, name: &str) -> Vec<(String, String)> {
        self.section(name)
            .iter()
            .filter_map(|line| match line {
                Line::Field { key, value } => Some((key.clone(), value.clone())),
                Line::Text { text } => Some((String::new(), text.clone())),
                _ => None,
            })
            .collect()
    }

    pub fn texts(&self, name: &str) -> Vec<String> {
        self.section(name)
            .iter()
            .filter_map(|line| match line {
                Line::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// How many entries of `name` the budget or a limit left out.
    pub fn hidden(&self, name: &str) -> usize {
        self.truncated
            .iter()
            .flat_map(|truncation| &truncation.sections)
            .find(|count| count.name == name)
            .map_or(0, |count| count.total.saturating_sub(count.shown))
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Line {
    Card(Card),
    Edge(EdgeLine),
    Node(NodeLine),
    Community(CommunityLine),
    Unresolved(UnresolvedLine),
    Note(NoteLine),
    Source(SourceLine),
    Field {
        key: String,
        value: String,
    },
    Text {
        text: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NodeLine {
    pub path: String,
    pub kind: String,
    #[serde(default)]
    pub derived: bool,
    pub file: Option<String>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub signature: Option<String>,
}

impl NodeLine {
    /// `L42-78`, `L42`, or nothing.
    pub fn lines(&self) -> String {
        match (self.line_start, self.line_end) {
            (Some(start), Some(end)) if end != start => format!("L{start}-{end}"),
            (Some(start), _) => format!("L{start}"),
            _ => String::new(),
        }
    }

    /// The last segment of the semantic path, which is what a reader recognises.
    pub fn short_name(&self) -> &str {
        let path = self.path.as_str();
        let tail = path.rsplit_once("::").map_or(path, |(_, tail)| tail);
        tail.rsplit_once('/').map_or(tail, |(_, tail)| tail)
    }

    /// The semantic path without the trailing name.
    pub fn container(&self) -> &str {
        self.path
            .rsplit_once("::")
            .map_or("", |(container, _)| container)
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Card {
    #[serde(flatten)]
    pub node: NodeLine,
    pub visibility: Option<String>,
    pub doc: Option<String>,
    pub community: Option<CommunityRef>,
    pub counts: CardCounts,
    #[serde(default)]
    pub notes: Vec<CardNote>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CommunityRef {
    pub id: String,
    pub label: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct CardCounts {
    pub callers: u64,
    pub callees: u64,
    pub impls: u64,
    pub overrides: u64,
    pub unresolved: u32,
    pub external: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CardNote {
    pub kind: String,
    pub first_line: String,
    pub updated: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct EdgeLine {
    pub direction: String,
    pub kind: String,
    pub node: NodeLine,
    #[serde(default)]
    pub sites: Vec<u32>,
    pub verified_by: String,
    pub via: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CommunityLine {
    pub key: Option<String>,
    pub path: String,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct UnresolvedLine {
    pub file: String,
    pub line: u32,
    pub text: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct NoteLine {
    pub uuid: String,
    pub kind: String,
    pub target: String,
    #[serde(default)]
    pub derived: bool,
    pub orphaned: bool,
    pub updated: String,
    pub body: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SourceLine {
    pub line: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Truncation {
    pub sections: Vec<SectionCount>,
    pub hint: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct SectionCount {
    pub name: String,
    pub shown: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Freshness {
    pub indexed_commit: Option<String>,
    pub indexed_at: Option<String>,
    pub changed_files: u64,
    #[serde(default)]
    pub coverage: Vec<LanguageCoverage>,
    #[serde(default)]
    pub stale: Vec<String>,
}

impl Freshness {
    /// `index: <commit> (<n> files changed since) · coverage: <language> <states>`.
    pub fn line(&self) -> String {
        let commit = self
            .indexed_commit
            .as_deref()
            .map_or("no git", |commit| commit.get(..7).unwrap_or(commit));
        let mut out = format!(
            "index: {commit} ({} {} changed since)",
            self.changed_files,
            if self.changed_files == 1 {
                "file"
            } else {
                "files"
            }
        );
        if !self.coverage.is_empty() {
            let languages: Vec<String> = self
                .coverage
                .iter()
                .map(|language| {
                    let states: Vec<String> =
                        language.states.iter().map(CoverageState::text).collect();
                    format!("{} {}", language.language, states.join(", "))
                })
                .collect();
            out.push_str(" · coverage: ");
            out.push_str(&languages.join(" · "));
        }
        out
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct LanguageCoverage {
    pub language: String,
    pub states: Vec<CoverageState>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct CoverageState {
    pub state: String,
    pub by: Option<String>,
    pub reason: Option<String>,
}

impl CoverageState {
    pub fn text(&self) -> String {
        match (
            self.state.as_str(),
            self.by.as_deref(),
            self.reason.as_deref(),
        ) {
            ("verified", Some("scip+lsp"), _) => "scip ✓ lsp ✓".to_owned(),
            ("verified", Some("none"), _) => "none".to_owned(),
            ("verified", Some(by), _) => format!("{by} ✓"),
            ("degraded", _, Some(reason)) => format!("degraded: {}", reason.replace('_', " ")),
            _ => "not verified".to_owned(),
        }
    }

    pub fn is_verified(&self) -> bool {
        self.state == "verified" && self.by.as_deref() != Some("none")
    }
}

#[derive(Deserialize)]
struct Failure {
    message: String,
}

pub fn encode(text: &str) -> String {
    String::from(js_sys::encode_uri_component(text))
}

/// GETs `path` and decodes its JSON; a failure carries the API's message.
pub async fn get<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(path))
        .await
        .map_err(|_| "the codemycel server is not reachable".to_owned())?
        .dyn_into()
        .map_err(|_| "unexpected fetch result".to_owned())?;
    let text = JsFuture::from(
        response
            .text()
            .map_err(|_| "unreadable response".to_owned())?,
    )
    .await
    .map_err(|_| "unreadable response".to_owned())?
    .as_string()
    .unwrap_or_default();
    if !response.ok() {
        return Err(serde_json::from_str::<Failure>(&text)
            .map(|failure| failure.message)
            .unwrap_or_else(|_| format!("request failed ({})", response.status())));
    }
    serde_json::from_str(&text).map_err(|error| format!("unexpected answer: {error}"))
}

pub async fn meta() -> Result<Meta, String> {
    get("/api/meta").await
}

pub async fn status() -> Result<Answer, String> {
    get("/api/status").await
}

pub async fn communities() -> Result<CommunityGraph, String> {
    get("/api/communities").await
}

pub async fn community(id: &str) -> Result<Answer, String> {
    get(&format!("/api/community/{}", encode(id))).await
}

pub async fn search(query: &str, kind: &str) -> Result<Answer, String> {
    get(&format!(
        "/api/search?q={}&kind={}&limit=40",
        encode(query),
        encode(kind)
    ))
    .await
}

pub async fn symbol(path: &str) -> Result<Answer, String> {
    get(&format!("/api/symbol/{}", encode(path))).await
}

pub async fn impact(path: &str, depth: u8) -> Result<Answer, String> {
    get(&format!("/api/impact/{}?depth={depth}", encode(path))).await
}

pub async fn notes() -> Result<Answer, String> {
    get("/api/notes").await
}
