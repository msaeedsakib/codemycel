#![allow(clippy::unwrap_used)]

use std::path::Path;

use codemycel_core::config::{Config, RootOverride};
use codemycel_core::{AdapterError, Language, LanguageAdapter, ParsedFile, RepoPath, Root};
use codemycel_index::{Registry, RootDetector};

struct Fixed {
    language: Language,
    roots: &'static [&'static str],
}

impl LanguageAdapter for Fixed {
    fn language(&self) -> Language {
        self.language
    }
    fn extensions(&self) -> &'static [&'static str] {
        &[]
    }
    fn detect_roots(&self, _: &Path, _: &[RepoPath]) -> Result<Vec<Root>, AdapterError> {
        Ok(self
            .roots
            .iter()
            .map(|path| root(path, self.language))
            .collect())
    }
    fn parse(&self, _: &RepoPath, _: &[u8]) -> Result<ParsedFile, AdapterError> {
        Ok(ParsedFile::default())
    }
    fn external_path_predicate(&self, _: &Path, _: &Path) -> bool {
        false
    }
}

fn repo(path: &str) -> RepoPath {
    RepoPath::new(path).unwrap()
}

fn root(path: &str, language: Language) -> Root {
    Root {
        path: repo(path),
        language,
    }
}

fn detect(adapters: Vec<Fixed>, overrides: Vec<RootOverride>) -> RootDetector {
    let registry = Registry::with_adapters(
        adapters
            .into_iter()
            .map(|a| Box::new(a) as Box<dyn LanguageAdapter>)
            .collect(),
    );
    let config = Config {
        roots: overrides,
        ..Config::default()
    };
    RootDetector::detect(Path::new("."), &[], &config, &registry).unwrap()
}

fn entry(path: &str, language: Language, disabled: bool) -> RootOverride {
    RootOverride {
        path: repo(path),
        language,
        disabled,
    }
}

#[test]
fn overrides_add_retype_and_disable_roots() {
    let detector = detect(
        vec![
            Fixed {
                language: Language::JavaScript,
                roots: &["apps/web", "tools"],
            },
            Fixed {
                language: Language::Rust,
                roots: &["crates/legacy"],
            },
        ],
        vec![
            entry("apps/web", Language::JavaScript, true),
            entry("apps/web", Language::TypeScript, false),
            entry("crates/legacy", Language::Rust, true),
            entry("extra", Language::Rust, false),
        ],
    );
    assert_eq!(
        detector.roots(),
        [
            root("apps/web", Language::TypeScript),
            root("extra", Language::Rust),
            root("tools", Language::JavaScript),
        ]
    );
}

#[test]
fn file_belongs_to_deepest_root_of_its_own_language() {
    let detector = detect(
        vec![Fixed {
            language: Language::Rust,
            roots: &["", "crates/a", "crates/ab"],
        }],
        Vec::new(),
    );
    let assigned = |file: &str| {
        detector
            .assign(&repo(file), Language::Rust)
            .map(|root| root.path.as_str().to_owned())
    };
    assert_eq!(assigned("crates/a/src/lib.rs").as_deref(), Some("crates/a"));
    assert_eq!(
        assigned("crates/ab/src/lib.rs").as_deref(),
        Some("crates/ab")
    );
    assert_eq!(assigned("crates/abc/src/lib.rs").as_deref(), Some(""));
    assert_eq!(assigned("build.rs").as_deref(), Some(""));
    assert_eq!(
        detector.assign(&repo("crates/a/x.ts"), Language::TypeScript),
        None
    );
}

#[test]
fn roots_of_different_languages_do_not_suppress_each_other() {
    let detector = detect(
        vec![
            Fixed {
                language: Language::Rust,
                roots: &["apps/desktop/src-tauri"],
            },
            Fixed {
                language: Language::TypeScript,
                roots: &["apps/desktop"],
            },
        ],
        Vec::new(),
    );
    assert_eq!(detector.roots().len(), 2);
    let file = repo("apps/desktop/src-tauri/src/main.rs");
    assert_eq!(
        detector
            .assign(&file, Language::Rust)
            .unwrap()
            .path
            .as_str(),
        "apps/desktop/src-tauri"
    );
    let file = repo("apps/desktop/src-tauri/bindings.ts");
    assert_eq!(
        detector
            .assign(&file, Language::TypeScript)
            .unwrap()
            .path
            .as_str(),
        "apps/desktop"
    );
}
