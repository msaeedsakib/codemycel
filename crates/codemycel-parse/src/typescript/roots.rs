use std::collections::BTreeSet;
use std::path::Path;

use codemycel_core::RepoPath;
use serde_json::Value;

const TSCONFIG: &str = "tsconfig.json";
const PACKAGE_JSON: &str = "package.json";

pub(super) struct Roots {
    pub typescript: Vec<RepoPath>,
    pub javascript: Vec<RepoPath>,
}

/// TypeScript: the directory of every readable `tsconfig.json` no other one lists in its
/// `references`. JavaScript: the directory of every readable `package.json` with no TypeScript
/// root at or below it. Unreadable or malformed manifests are not roots.
pub(super) fn detect(repo_root: &Path, files: &[RepoPath]) -> Roots {
    let mut tsconfigs = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for file in files.iter().filter(|file| is_named(file, TSCONFIG)) {
        let Some(config) = read(repo_root, file, true) else {
            continue;
        };
        let references = config.get("references").and_then(Value::as_array);
        for reference in references.into_iter().flatten() {
            let target = reference
                .get("path")
                .and_then(Value::as_str)
                .and_then(|path| resolve_reference(parent(file), path));
            if let Some(target) = target
                && target != *file
            {
                referenced.insert(target);
            }
        }
        tsconfigs.insert(file.clone());
    }
    let typescript: BTreeSet<RepoPath> = tsconfigs
        .difference(&referenced)
        .filter_map(|file| RepoPath::new(parent(file)).ok())
        .collect();
    let javascript: BTreeSet<RepoPath> = files
        .iter()
        .filter(|file| is_named(file, PACKAGE_JSON))
        .filter(|file| read(repo_root, file, false).is_some())
        .filter_map(|file| RepoPath::new(parent(file)).ok())
        .filter(|dir| !typescript.iter().any(|root| at_or_below(root, dir)))
        .collect();
    Roots {
        typescript: typescript.into_iter().collect(),
        javascript: javascript.into_iter().collect(),
    }
}

/// Every `package.json`, `tsconfig*.json`, and `jsconfig*.json`, and the configs they extend by
/// relative path, followed through the chain.
pub(super) fn project_files(repo_root: &Path, files: &[RepoPath]) -> Vec<RepoPath> {
    let name = |file: &RepoPath| {
        file.as_str()
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    let is_config = |name: &str| {
        (name.starts_with("tsconfig") || name.starts_with("jsconfig")) && name.ends_with(".json")
    };
    let mut found: BTreeSet<RepoPath> = files
        .iter()
        .filter(|file| name(file) == PACKAGE_JSON || is_config(&name(file)))
        .cloned()
        .collect();
    let mut pending: Vec<RepoPath> = found
        .iter()
        .filter(|f| is_config(&name(f)))
        .cloned()
        .collect();
    while let Some(config) = pending.pop() {
        let Some(value) = read(repo_root, &config, true) else {
            continue;
        };
        let extends = match value.get("extends") {
            Some(Value::String(one)) => vec![one.as_str()],
            Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        for target in extends.into_iter().filter(|t| t.starts_with('.')) {
            let resolved = resolve_relative(parent(&config), target);
            let with_json = (!target.ends_with(".json"))
                .then(|| resolve_relative(parent(&config), &format!("{target}.json")))
                .flatten();
            for path in resolved.into_iter().chain(with_json) {
                if found.insert(path.clone()) {
                    pending.push(path);
                }
            }
        }
    }
    found.into_iter().collect()
}

fn resolve_relative(dir: &str, relative: &str) -> Option<RepoPath> {
    let mut segments: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    for segment in relative.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            other => segments.push(other),
        }
    }
    RepoPath::new(&segments.join("/")).ok()
}

fn is_named(file: &RepoPath, name: &str) -> bool {
    file.as_str().rsplit('/').next() == Some(name)
}

fn parent(file: &RepoPath) -> &str {
    file.as_str().rsplit_once('/').map_or("", |(dir, _)| dir)
}

fn at_or_below(path: &RepoPath, dir: &RepoPath) -> bool {
    dir.is_root()
        || path
            .as_str()
            .strip_prefix(dir.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// A JSON object read from `file`; `jsonc` admits comments and trailing commas (tsconfig).
fn read(repo_root: &Path, file: &RepoPath, jsonc: bool) -> Option<Value> {
    let bytes = std::fs::read(repo_root.join(file.as_str())).ok()?;
    let text = std::str::from_utf8(&bytes).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let value: Value = if jsonc {
        serde_json::from_str(&strip_jsonc(text)).ok()?
    } else {
        serde_json::from_str(text).ok()?
    };
    value.is_object().then_some(value)
}

/// A `references` entry names a directory holding a `tsconfig.json` or a config file itself,
/// relative to the referencing config's directory.
fn resolve_reference(dir: &str, reference: &str) -> Option<RepoPath> {
    let resolved = resolve_relative(dir, reference).filter(|path| !path.is_root())?;
    if resolved.as_str().ends_with(".json") {
        return Some(resolved);
    }
    resolve_relative(resolved.as_str(), TSCONFIG)
}

/// JSONC to JSON: drops `//` and `/* */` comments and commas before `}` or `]`, leaving strings
/// untouched.
fn strip_jsonc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                while let Some(c) = chars.next() {
                    out.push(c);
                    match c {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push(c);
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for c in chars.by_ref() {
                    if previous == '*' && c == '/' {
                        break;
                    }
                    previous = c;
                }
                out.push(' ');
            }
            '}' | ']' => {
                let kept = out.trim_end().len();
                if out[..kept].ends_with(',') {
                    out.truncate(kept - 1);
                }
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
