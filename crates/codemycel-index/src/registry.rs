use codemycel_core::{Language, LanguageAdapter, ServerAdapter};
use codemycel_lsp::adapters::rust_analyzer::RustAnalyzer;
use codemycel_lsp::adapters::typescript7::TypeScript7;
use codemycel_parse::rust::RustAdapter;
use codemycel_parse::typescript::{JavaScriptAdapter, TypeScriptAdapter};

/// The language adapters, in the fixed order Rust, TypeScript, JavaScript, each paired by
/// language with the server that verifies its candidates.
pub struct Registry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
    servers: Vec<(Language, Box<dyn ServerAdapter>)>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_adapters(vec![
            Box::new(RustAdapter::new()),
            Box::new(TypeScriptAdapter::new()),
            Box::new(JavaScriptAdapter::new()),
        ])
        .with_servers(vec![
            (Language::Rust, Box::new(RustAnalyzer::new())),
            (Language::TypeScript, Box::new(TypeScript7::new())),
            (Language::JavaScript, Box::new(TypeScript7::new())),
        ])
    }
}

impl Registry {
    /// A caller-built registry, for tests that run an adapter before it is registered.
    pub fn with_adapters(adapters: Vec<Box<dyn LanguageAdapter>>) -> Self {
        Self {
            adapters,
            servers: Vec::new(),
        }
    }

    /// Replaces the server pairings. One server may verify several languages.
    pub fn with_servers(mut self, servers: Vec<(Language, Box<dyn ServerAdapter>)>) -> Self {
        self.servers = servers;
        self
    }

    pub fn adapters(&self) -> &[Box<dyn LanguageAdapter>] {
        &self.adapters
    }

    /// The adapter claiming `extension` (without the dot).
    pub fn for_extension(&self, extension: &str) -> Option<&dyn LanguageAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.extensions().contains(&extension))
            .map(AsRef::as_ref)
    }

    pub fn for_language(&self, language: Language) -> Option<&dyn LanguageAdapter> {
        self.adapters
            .iter()
            .find(|adapter| adapter.language() == language)
            .map(AsRef::as_ref)
    }

    pub fn server_for(&self, language: Language) -> Option<&dyn ServerAdapter> {
        self.servers
            .iter()
            .find(|(paired, _)| *paired == language)
            .map(|(_, server)| server.as_ref())
    }
}
