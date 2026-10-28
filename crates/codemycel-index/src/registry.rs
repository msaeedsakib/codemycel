use codemycel_core::{Language, LanguageAdapter};

/// The language adapters, in the fixed order Rust, TypeScript, JavaScript (spec §7.1).
pub struct Registry {
    adapters: Vec<Box<dyn LanguageAdapter>>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::with_adapters(Vec::new())
    }
}

impl Registry {
    /// A caller-built registry, for tests that run an adapter before it is registered.
    pub fn with_adapters(adapters: Vec<Box<dyn LanguageAdapter>>) -> Self {
        Self { adapters }
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
}
