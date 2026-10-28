use async_trait::async_trait;

#[async_trait]
pub trait Store {
    async fn load(&self, key: &str) -> Option<String>;

    async fn exists(&self, key: &str) -> bool {
        self.load(key).await.is_some()
    }
}
