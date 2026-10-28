use std::cmp::Ordering;

use super::tokenize::query_terms;
use crate::text::name_split;
use crate::{NodeKind, Ranker, ScoredHit, SearchHit};

/// FTS5's column-weighted BM25, boosted for exact and prefix name matches, weighted by kind, and
/// halved for test files.
pub struct Bm25Ranker;

impl Ranker for Bm25Ranker {
    fn rank(&self, query: &str, hits: Vec<SearchHit>) -> Vec<ScoredHit> {
        let query = query.trim();
        let words = query_terms(query).join(" ");
        let mut scored: Vec<ScoredHit> = hits
            .into_iter()
            .map(|hit| {
                let score = -hit.bm25
                    * name_boost(query, &words, &hit.name)
                    * kind_weight(hit.kind)
                    * hit.file.as_ref().map_or(1.0, |f| test_penalty(f.as_str()));
                ScoredHit { hit, score }
            })
            .collect();
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(Ordering::Equal)
                .then(a.hit.node_id.cmp(&b.hit.node_id))
        });
        scored
    }
}

/// ×3 when the name is the query, ×1.5 when it starts with it, compared as typed and as words.
fn name_boost(query: &str, words: &str, name: &str) -> f64 {
    let split = name_split(name);
    if name == query || (!words.is_empty() && split == words) {
        3.0
    } else if (!query.is_empty() && name.starts_with(query))
        || (!words.is_empty() && split.starts_with(words))
    {
        1.5
    } else {
        1.0
    }
}

fn kind_weight(kind: NodeKind) -> f64 {
    match kind {
        NodeKind::Class
        | NodeKind::Struct
        | NodeKind::Enum
        | NodeKind::Trait
        | NodeKind::Interface
        | NodeKind::Module
        | NodeKind::Function
        | NodeKind::Method => 1.0,
        NodeKind::TypeAlias
        | NodeKind::Constant
        | NodeKind::Variable
        | NodeKind::Field
        | NodeKind::EnumMember => 0.7,
        NodeKind::Heading | NodeKind::Doc | NodeKind::Note => 0.6,
        NodeKind::File | NodeKind::Package | NodeKind::Community => 0.5,
    }
}

/// ×0.5 for paths matching `*test*`, `*spec*`, `__tests__`, or `tests/`.
fn test_penalty(path: &str) -> f64 {
    let path = path.to_lowercase();
    if path.contains("test") || path.contains("spec") {
        0.5
    } else {
        1.0
    }
}
