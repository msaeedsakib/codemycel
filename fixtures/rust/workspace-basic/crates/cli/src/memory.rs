use std::collections::HashMap;

use async_trait::async_trait;
use fixture_core::store::Store;

#[derive(Default)]
pub struct MemoryStore {
    entries: HashMap<String, String>,
}

#[async_trait]
impl Store for MemoryStore {
    async fn load(&self, key: &str) -> Option<String> {
        self.entries.get(key).cloned()
    }
}
