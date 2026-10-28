use super::{Answer, Budget, QueryEngine, Result};
use crate::EdgeKind;

impl QueryEngine<'_> {
    pub fn impact(
        &self,
        name: &str,
        depth: u8,
        kinds: &[EdgeKind],
        budget: Budget,
    ) -> Result<Answer> {
        let _ = (name, depth, kinds);
        self.not_found("not implemented: impact", budget)
    }
}
