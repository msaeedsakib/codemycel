use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use axum::extract::{Path as UrlPath, Query, State};
use axum::routing::get;
use axum::{Json, Router};
use codemycel_core::query::Budget;
use codemycel_core::query::symbol::SymbolOptions;
use codemycel_core::store::{StoreRead, meta};
use codemycel_core::{EdgeKind, NodeKind, VerifiedBy};
use codemycel_index::{Lock, LockKind, RepoLocation};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ApiError, Repo};
use crate::graph::{CommunityGraph, CommunityLink, CommunityNode};

/// The relations between symbols that the community map counts across communities.
const RELATIONS: [EdgeKind; 5] = [
    EdgeKind::Calls,
    EdgeKind::References,
    EdgeKind::Extends,
    EdgeKind::Implements,
    EdgeKind::Overrides,
];

type Answer = Result<Json<Value>, ApiError>;

/// The read-only API over `repo_root`'s index. Every route but `/meta` and `/communities`
/// answers with the `JsonFormatter` rendering of the tool of the same name. `max_tokens`
/// overrides a route's budget.
pub fn routes(repo_root: &Path) -> Router {
    let repo = Repo(Arc::new(RepoLocation {
        root: repo_root.to_owned(),
        indexed: true,
    }));
    Router::new()
        .route("/meta", get(meta_route))
        .route("/status", get(status))
        .route("/search", get(search))
        .route("/symbol/{*name}", get(symbol))
        .route("/impact/{*name}", get(impact))
        .route("/overview", get(overview))
        .route("/communities", get(communities))
        .route("/community/{id}", get(community))
        .route("/notes", get(notes))
        .with_state(repo)
}

#[derive(Deserialize)]
struct Tokens {
    max_tokens: Option<u32>,
}

impl Tokens {
    fn budget(&self, default: u32) -> Budget {
        Budget::new(self.max_tokens.unwrap_or(default))
    }
}

/// `repo` and `indexed_at`, which changes on every sync; the front end polls it.
async fn meta_route(State(repo): State<Repo>) -> Answer {
    repo.read(|repo, store| {
        Ok(Json(json!({
            "repo": repo.0.root.display().to_string(),
            "indexed_at": store.meta_get(meta::INDEXED_AT)?,
        })))
    })
    .await
}

async fn status(State(repo): State<Repo>, Query(tokens): Query<Tokens>) -> Answer {
    repo.answer(move |engine| {
        let root = engine.repo_root();
        engine.status(
            Lock::current_owner(root, LockKind::Sync),
            Lock::current_owner(root, LockKind::Session),
            tokens.budget(2000),
        )
    })
    .await
}

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
    /// Comma-separated node kinds.
    kind: Option<String>,
    limit: Option<u32>,
    max_tokens: Option<u32>,
}

async fn search(State(repo): State<Repo>, Query(query): Query<SearchQuery>) -> Answer {
    let kinds: Vec<NodeKind> = query
        .kind
        .as_deref()
        .unwrap_or("")
        .split(',')
        .filter_map(|kind| kind.trim().parse().ok())
        .collect();
    repo.answer(move |engine| {
        engine.search(
            &query.q,
            &kinds,
            query.limit.unwrap_or(20),
            Budget::new(query.max_tokens.unwrap_or(6000)),
        )
    })
    .await
}

#[derive(Deserialize)]
struct SymbolQuery {
    body: Option<bool>,
    limit: Option<u32>,
    max_tokens: Option<u32>,
}

/// The node page: callers and callees on, the body included unless `body=false`.
async fn symbol(
    State(repo): State<Repo>,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<SymbolQuery>,
) -> Answer {
    repo.answer(move |engine| {
        let options = SymbolOptions {
            include_body: query.body.unwrap_or(true),
            callers: true,
            callees: true,
            limit: query.limit.unwrap_or(50),
        };
        engine.symbol(
            &name,
            options,
            Budget::new(query.max_tokens.unwrap_or(12000)),
        )
    })
    .await
}

#[derive(Deserialize)]
struct ImpactQuery {
    depth: Option<u8>,
    max_tokens: Option<u32>,
}

async fn impact(
    State(repo): State<Repo>,
    UrlPath(name): UrlPath<String>,
    Query(query): Query<ImpactQuery>,
) -> Answer {
    repo.answer(move |engine| {
        engine.impact(
            &name,
            query.depth.unwrap_or(2),
            &[],
            Budget::new(query.max_tokens.unwrap_or(12000)),
        )
    })
    .await
}

#[derive(Deserialize)]
struct OverviewQuery {
    scope: Option<String>,
    max_tokens: Option<u32>,
}

async fn overview(State(repo): State<Repo>, Query(query): Query<OverviewQuery>) -> Answer {
    repo.answer(move |engine| {
        engine.overview(
            query.scope.as_deref(),
            Budget::new(query.max_tokens.unwrap_or(6000)),
        )
    })
    .await
}

/// The `overview` of `community:<id>`: its members and boundary edges.
async fn community(
    State(repo): State<Repo>,
    UrlPath(id): UrlPath<String>,
    Query(tokens): Query<Tokens>,
) -> Answer {
    repo.answer(move |engine| {
        engine.overview(Some(&format!("community:{id}")), tokens.budget(12000))
    })
    .await
}

#[derive(Deserialize)]
struct NotesQuery {
    target: Option<String>,
    orphans: Option<bool>,
    max_tokens: Option<u32>,
}

async fn notes(State(repo): State<Repo>, Query(query): Query<NotesQuery>) -> Answer {
    repo.answer(move |engine| {
        engine.notes(
            query.target.as_deref(),
            query.orphans.unwrap_or(false),
            Budget::new(query.max_tokens.unwrap_or(12000)),
        )
    })
    .await
}

async fn communities(State(repo): State<Repo>) -> Result<Json<CommunityGraph>, ApiError> {
    repo.read(|_, store| {
        let communities = store.communities()?;
        let mut owner = HashMap::new();
        for community in &communities {
            for member in &community.members {
                owner.insert(member.clone(), community.stable_id.as_str());
            }
        }
        let mut weights: BTreeMap<(&str, &str), usize> = BTreeMap::new();
        for stored in store.edges_of_kinds(&RELATIONS, VerifiedBy::ALL)? {
            let (Some(&a), Some(&b)) = (owner.get(&stored.edge.src), owner.get(&stored.edge.dst))
            else {
                continue;
            };
            if a != b {
                *weights.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let links = weights
            .into_iter()
            .map(|((source, target), weight)| CommunityLink {
                source: source.to_owned(),
                target: target.to_owned(),
                weight,
            })
            .collect();
        let nodes = communities
            .iter()
            .map(|community| CommunityNode {
                id: community.stable_id.clone(),
                label: community.label.clone(),
                size: community.members.len(),
            })
            .collect();
        Ok(Json(CommunityGraph {
            communities: nodes,
            links,
            derived: true,
        }))
    })
    .await
}
