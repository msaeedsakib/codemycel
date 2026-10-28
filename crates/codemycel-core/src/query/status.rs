use super::{Answer, Budget, QueryEngine, Result};
use crate::SyncOwner;

impl QueryEngine<'_> {
    pub fn status(
        &self,
        sync_owner: Option<SyncOwner>,
        session_owner: Option<SyncOwner>,
        budget: Budget,
    ) -> Result<Answer> {
        let _ = (sync_owner, session_owner);
        self.not_found("not implemented: status", budget)
    }
}
