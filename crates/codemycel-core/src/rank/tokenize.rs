use crate::text::name_split;

const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "do", "does", "for", "from", "how", "in",
    "is", "it", "of", "on", "or", "that", "the", "this", "to", "what", "where", "which", "with",
];

/// The words of a search query, split like `search_docs.name_split`, without stop words or
/// repeats, in first-seen order.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for word in name_split(query).split(' ') {
        if !word.is_empty() && !STOP_WORDS.contains(&word) && !terms.iter().any(|t| t == word) {
            terms.push(word.to_owned());
        }
    }
    terms
}

/// An FTS5 query matching any term as a prefix token; `None` without terms.
pub fn fts_query(terms: &[String]) -> Option<String> {
    let parts: Vec<String> = terms.iter().map(|term| format!("\"{term}\"*")).collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}
