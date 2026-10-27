mod language_adapter;
mod ranker;
mod resolver;
mod server_adapter;

pub use language_adapter::{AdapterError, LanguageAdapter};
pub use ranker::{Ranker, ScoredHit, SearchHit};
pub use resolver::Resolver;
pub use server_adapter::{Arch, Os, ReadinessStrategy, ServerAdapter, Toolchain};

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Duration;

    use super::*;
    use crate::config::ServerConfig;
    use crate::{Candidate, Language, NodeRef, Occurrence, ParsedFile, RepoPath, Root, Verdict};

    struct Dummy;

    impl LanguageAdapter for Dummy {
        fn language(&self) -> Language {
            Language::Rust
        }
        fn extensions(&self) -> &'static [&'static str] {
            &["rs"]
        }
        fn detect_roots(&self, _: &Path, _: &[RepoPath]) -> Result<Vec<Root>, AdapterError> {
            Ok(Vec::new())
        }
        fn parse(&self, _: &RepoPath, _: &[u8]) -> Result<ParsedFile, AdapterError> {
            Ok(ParsedFile::default())
        }
        fn external_path_predicate(&self, _: &Path, _: &Path) -> bool {
            false
        }
    }

    impl Resolver for Dummy {
        fn name(&self) -> &'static str {
            "lsp"
        }
        fn covers(&self, _: &RepoPath) -> bool {
            true
        }
        fn resolve(&self, candidates: &[Candidate]) -> Vec<Verdict> {
            Vec::with_capacity(candidates.len())
        }
        fn implementers(&self, _: &NodeRef) -> Vec<NodeRef> {
            Vec::new()
        }
        fn outgoing_calls(&self, _: &NodeRef) -> Option<Vec<(NodeRef, Vec<Occurrence>)>> {
            None
        }
    }

    impl Ranker for Dummy {
        fn rank(&self, _: &str, hits: Vec<SearchHit>) -> Vec<ScoredHit> {
            hits.into_iter()
                .map(|hit| ScoredHit { hit, score: 0.0 })
                .collect()
        }
    }

    impl ServerAdapter for Dummy {
        fn language(&self) -> Language {
            Language::Rust
        }
        fn name(&self) -> &'static str {
            "dummy"
        }
        fn pinned_version(&self) -> &'static str {
            "0"
        }
        fn toolchain(&self) -> Option<Toolchain> {
            None
        }
        fn download_url(&self, _: Os, _: Arch) -> Option<String> {
            None
        }
        fn pinned_sha256(&self, _: Os, _: Arch) -> Option<&'static str> {
            None
        }
        fn archive_member(&self, _: Os) -> Option<&'static str> {
            None
        }
        fn validate(&self, _: &Path) -> Result<String, String> {
            Err("dummy".into())
        }
        fn launch_args(&self) -> &'static [&'static str] {
            &[]
        }
        fn initialization_options(&self, _: &ServerConfig) -> serde_json::Value {
            serde_json::Value::Null
        }
        fn readiness(&self) -> ReadinessStrategy {
            ReadinessStrategy::ProbeDefinition {
                timeout: Duration::from_secs(60),
            }
        }
        fn request_concurrency(&self) -> usize {
            8
        }
        fn config_file_name(&self) -> Option<&'static str> {
            None
        }
    }

    #[test]
    fn traits_are_object_safe() {
        let _: Vec<Box<dyn LanguageAdapter>> = vec![Box::new(Dummy)];
        let _: Vec<Box<dyn Resolver>> = vec![Box::new(Dummy)];
        let _: Box<dyn Ranker> = Box::new(Dummy);
        let _: Vec<Box<dyn ServerAdapter>> = vec![Box::new(Dummy)];
    }
}
