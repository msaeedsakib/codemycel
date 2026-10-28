use super::{Answer, Budget, QueryEngine, Result};
use crate::UnresolvedReason;

impl QueryEngine<'_> {
    pub fn unresolved(
        &self,
        scope: Option<&str>,
        reason: Option<UnresolvedReason>,
        budget: Budget,
    ) -> Result<Answer> {
        let _ = (scope, reason);
        self.not_found("not implemented: unresolved", budget)
    }
}
