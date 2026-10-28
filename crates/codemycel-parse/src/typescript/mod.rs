//! The TypeScript and JavaScript language adapters.

mod hooks;
mod roots;

use std::path::{Component, Path};
use std::sync::OnceLock;

use codemycel_core::{AdapterError, Language, LanguageAdapter, ParsedFile, RepoPath, Root};

use crate::driver::{Driver, DriverError, QuerySet};

/// Directories the walker never enters; the adapter's view of an ignored directory.
const IGNORED_DIRECTORIES: [&str; 8] = [
    ".git",
    ".codemycel",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    "coverage",
];

const TYPESCRIPT: QuerySet = QuerySet {
    symbols: concat!(
        include_str!("queries/symbols.scm"),
        include_str!("queries/symbols_ts.scm")
    ),
    candidates: concat!(
        include_str!("queries/candidates.scm"),
        include_str!("queries/candidates_ts.scm")
    ),
    locals: concat!(
        include_str!("queries/locals.scm"),
        include_str!("queries/locals_ts.scm")
    ),
};

const TSX: QuerySet = QuerySet {
    candidates: concat!(
        include_str!("queries/candidates.scm"),
        include_str!("queries/candidates_ts.scm"),
        include_str!("queries/candidates_jsx.scm")
    ),
    ..TYPESCRIPT
};

const JAVASCRIPT: QuerySet = QuerySet {
    symbols: concat!(
        include_str!("queries/symbols.scm"),
        include_str!("queries/symbols_js.scm")
    ),
    candidates: concat!(
        include_str!("queries/candidates.scm"),
        include_str!("queries/candidates_js.scm"),
        include_str!("queries/candidates_jsx.scm")
    ),
    locals: include_str!("queries/locals.scm"),
};

/// `.ts`, `.tsx`, `.mts`, `.cts`; `.tsx` files use the TSX grammar.
pub struct TypeScriptAdapter {
    typescript: LazyDriver,
    tsx: LazyDriver,
}

/// `.js`, `.jsx`, `.mjs`, `.cjs`, all through the JavaScript grammar, which parses JSX.
pub struct JavaScriptAdapter {
    javascript: LazyDriver,
}

impl TypeScriptAdapter {
    pub fn new() -> Self {
        Self {
            typescript: LazyDriver::new(Dialect::TypeScript),
            tsx: LazyDriver::new(Dialect::Tsx),
        }
    }
}

impl Default for TypeScriptAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptAdapter {
    pub fn new() -> Self {
        Self {
            javascript: LazyDriver::new(Dialect::JavaScript),
        }
    }
}

impl Default for JavaScriptAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl LanguageAdapter for TypeScriptAdapter {
    fn language(&self) -> Language {
        Language::TypeScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["ts", "tsx", "mts", "cts"]
    }

    fn detect_roots(
        &self,
        repo_root: &Path,
        files: &[RepoPath],
    ) -> Result<Vec<Root>, AdapterError> {
        Ok(as_roots(
            roots::detect(repo_root, files).typescript,
            Language::TypeScript,
        ))
    }

    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        if file.as_str().ends_with(".tsx") {
            self.tsx.parse(file, bytes)
        } else {
            self.typescript.parse(file, bytes)
        }
    }

    fn external_path_predicate(&self, canonical_path: &Path, repo_root: &Path) -> bool {
        is_external(canonical_path, repo_root)
    }
}

impl LanguageAdapter for JavaScriptAdapter {
    fn language(&self) -> Language {
        Language::JavaScript
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["js", "jsx", "mjs", "cjs"]
    }

    fn detect_roots(
        &self,
        repo_root: &Path,
        files: &[RepoPath],
    ) -> Result<Vec<Root>, AdapterError> {
        Ok(as_roots(
            roots::detect(repo_root, files).javascript,
            Language::JavaScript,
        ))
    }

    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        self.javascript.parse(file, bytes)
    }

    fn external_path_predicate(&self, canonical_path: &Path, repo_root: &Path) -> bool {
        is_external(canonical_path, repo_root)
    }
}

#[derive(Clone, Copy)]
enum Dialect {
    TypeScript,
    Tsx,
    JavaScript,
}

impl Dialect {
    fn grammar(self) -> tree_sitter::Language {
        match self {
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }

    fn queries(self) -> QuerySet {
        match self {
            Self::TypeScript => TYPESCRIPT,
            Self::Tsx => TSX,
            Self::JavaScript => JAVASCRIPT,
        }
    }
}

struct LazyDriver {
    dialect: Dialect,
    driver: OnceLock<Result<Driver, DriverError>>,
}

impl LazyDriver {
    fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            driver: OnceLock::new(),
        }
    }

    fn parse(&self, file: &RepoPath, bytes: &[u8]) -> Result<ParsedFile, AdapterError> {
        let driver = self.driver.get_or_init(|| {
            Driver::new(
                self.dialect.grammar(),
                self.dialect.queries(),
                Box::new(hooks::Hooks),
            )
        });
        match driver {
            Ok(driver) => driver.parse(file, bytes),
            Err(error) => Err(AdapterError::Parse {
                path: file.clone(),
                message: error.to_string(),
            }),
        }
    }
}

fn as_roots(paths: Vec<RepoPath>, language: Language) -> Vec<Root> {
    paths
        .into_iter()
        .map(|path| Root { path, language })
        .collect()
}

/// A path under `node_modules` that canonicalises outside the repository, or a `.d.ts` inside
/// it under an always-ignored directory.
fn is_external(canonical_path: &Path, repo_root: &Path) -> bool {
    let canonical_root =
        std::fs::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let inside = canonical_path
        .strip_prefix(&canonical_root)
        .or_else(|_| canonical_path.strip_prefix(repo_root));
    match inside {
        Ok(relative) => {
            let declaration = relative
                .file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".d.ts"));
            declaration
                && relative.parent().is_some_and(|dir| {
                    dir.components()
                        .any(|c| is_named(c, |name| IGNORED_DIRECTORIES.contains(&name)))
                })
        }
        Err(_) => canonical_path
            .components()
            .any(|c| is_named(c, |name| name == "node_modules")),
    }
}

fn is_named(component: Component<'_>, test: impl Fn(&str) -> bool) -> bool {
    matches!(component, Component::Normal(name) if test(&name.to_string_lossy()))
}
