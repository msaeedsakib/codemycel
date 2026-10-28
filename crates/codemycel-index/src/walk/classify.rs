use codemycel_core::{FileKind, Language, RepoPath, SemanticPath, SkipReason};

use crate::Registry;

const MANIFESTS: [&str; 2] = ["Cargo.toml", "package.json"];
const DOC_EXTENSIONS: [&str; 2] = ["md", "mdx"];
const BINARY_SNIFF: usize = 8000;

pub fn kind(path: &RepoPath, registry: &Registry) -> (FileKind, Option<Language>) {
    let name = path.as_str().rsplit('/').next().unwrap_or_default();
    if MANIFESTS.contains(&name) {
        return (FileKind::Manifest, None);
    }
    let Some((_, extension)) = name.rsplit_once('.') else {
        return (FileKind::Other, None);
    };
    if DOC_EXTENSIONS
        .iter()
        .any(|doc| extension.eq_ignore_ascii_case(doc))
    {
        return (FileKind::Doc, None);
    }
    match registry.for_extension(extension) {
        Some(adapter) => (FileKind::Code, Some(adapter.language())),
        None => (FileKind::Other, None),
    }
}

/// Why a file of `kind` within the size cap is listed but not parsed; `None` when it is parsed.
pub fn skip_reason(path: &RepoPath, kind: FileKind, bytes: &[u8]) -> Option<SkipReason> {
    if bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0) {
        return Some(SkipReason::Binary);
    }
    if kind == FileKind::Other {
        return Some(SkipReason::Unsupported);
    }
    if std::str::from_utf8(bytes).is_err() {
        return Some(SkipReason::InvalidUtf8);
    }
    if SemanticPath::for_file(path).is_err() {
        return Some(SkipReason::UnaddressablePath);
    }
    None
}
