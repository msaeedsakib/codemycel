use super::{Answer, Budget, Line, Outcome, QueryEngine, Result, Section, SymbolCard};
use crate::rank::{Bm25Ranker, fts_query, query_terms};
use crate::store::StoreRead;
use crate::{NodeKind, Ranker};

impl QueryEngine<'_> {
    /// Symbol cards for `query`, ranked, at most `limit`, only of `kinds` unless empty.
    pub fn search(
        &self,
        query: &str,
        kinds: &[NodeKind],
        limit: u32,
        budget: Budget,
    ) -> Result<Answer> {
        let hits = match fts_query(&query_terms(query)) {
            Some(fts) => self.store().search_fts(&fts, kinds)?,
            None => Vec::new(),
        };
        let ranked = Bm25Ranker.rank(query, hits);
        let mut cards = Vec::new();
        for scored in ranked.iter().take(limit as usize) {
            if let Some(node) = self.store().node_by_id(scored.hit.node_id)? {
                cards.push(Line::Card(SymbolCard::for_node(self.store(), &node)?));
            }
        }
        if ranked.is_empty() {
            cards.push(Line::text(format!("no matches for `{}`", query.trim())));
        }
        let section = Section {
            total: ranked.len().max(cards.len()),
            ..Section::new("results", 0, cards).without_header()
        };
        self.finish(
            Outcome::Ok,
            vec![section],
            budget,
            "raise max_tokens or limit, or narrow the query",
        )
    }
}
