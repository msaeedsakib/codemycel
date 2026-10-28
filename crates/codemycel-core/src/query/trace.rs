use super::{Answer, Budget, QueryEngine, Result};
use crate::EdgeKind;

impl QueryEngine<'_> {
    pub fn trace(
        &self,
        from: &str,
        to: &str,
        kinds: &[EdgeKind],
        max_depth: u32,
        budget: Budget,
    ) -> Result<Answer> {
        let _ = (from, to, kinds, max_depth);
        self.not_found("not implemented: trace", budget)
    }
}
