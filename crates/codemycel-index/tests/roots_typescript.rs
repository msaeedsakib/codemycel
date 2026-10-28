#![allow(clippy::unwrap_used)]

use codemycel_core::config::Config;
use codemycel_core::{LanguageAdapter, RepoPath};
use codemycel_index::{Registry, RootDetector};
use codemycel_parse::typescript::{JavaScriptAdapter, TypeScriptAdapter};

/// Writes `files` into a fresh repository and returns its detected roots as `language path`.
fn roots(files: &[(&str, &str)]) -> Vec<String> {
    let repo = tempfile::tempdir().unwrap();
    for (path, content) in files {
        let absolute = repo.path().join(path);
        std::fs::create_dir_all(absolute.parent().unwrap()).unwrap();
        std::fs::write(absolute, content).unwrap();
    }
    let listed: Vec<RepoPath> = files
        .iter()
        .map(|(path, _)| RepoPath::new(path).unwrap())
        .collect();
    let registry = Registry::with_adapters(vec![
        Box::new(TypeScriptAdapter::new()) as Box<dyn LanguageAdapter>,
        Box::new(JavaScriptAdapter::new()),
    ]);
    let detector =
        RootDetector::detect(repo.path(), &listed, &Config::default(), &registry).unwrap();
    detector
        .roots()
        .iter()
        .map(|root| format!("{} {}", root.language, display(&root.path)))
        .collect()
}

fn display(path: &RepoPath) -> &str {
    if path.is_root() { "." } else { path.as_str() }
}

#[test]
fn workspace_root_is_not_a_js_root() {
    let found = roots(&[
        (
            "package.json",
            r#"{ "private": true, "workspaces": ["packages/*"] }"#,
        ),
        ("packages/a/package.json", r#"{ "name": "a" }"#),
        ("packages/a/tsconfig.json", "{}"),
        ("packages/b/tsconfig.json", "{}"),
        ("packages/js/package.json", r#"{ "name": "js" }"#),
        ("packages/broken/package.json", "{ not json"),
    ]);
    assert_eq!(
        found,
        [
            "typescript packages/a",
            "typescript packages/b",
            "javascript packages/js",
        ]
    );
}

#[test]
fn nested_tsconfig_roots_are_detected() {
    let found = roots(&[
        ("tsconfig.json", "{}"),
        (
            "apps/web/tsconfig.json",
            "// web\n{ \"compilerOptions\": {}, }",
        ),
        ("apps/web/e2e/tsconfig.json", "{}"),
        ("tools/tsconfig.json", "{ broken"),
    ]);
    assert_eq!(
        found,
        [
            "typescript .",
            "typescript apps/web",
            "typescript apps/web/e2e",
        ]
    );
}

#[test]
fn referenced_tsconfig_is_not_a_root() {
    let found = roots(&[
        (
            "tsconfig.json",
            r#"{
  "$schema": "https://json.schemastore.org/tsconfig", // a URL is not a comment
  /* solution-style config */
  "files": [],
  "references": [
    { "path": "./packages/core" },
    { "path": "packages/app/tsconfig.json", },
  ],
}"#,
        ),
        ("packages/core/tsconfig.json", "{}"),
        (
            "packages/app/tsconfig.json",
            r#"{ "references": [{ "path": "../core" }] }"#,
        ),
        ("packages/extra/tsconfig.json", "{}"),
        ("packages/extra/package.json", "{}"),
    ]);
    assert_eq!(found, ["typescript .", "typescript packages/extra"]);
}
