use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{ModelError, RepoPath};

/// The human-readable identity of a node (spec §6.4). The node's full identity is this path
/// together with its `NodeKind`; see `NodeRef`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SemanticPath(String);

const COMMUNITY: &str = "community:";
const NOTE: &str = "note:";

enum Form<'a> {
    Symbol {
        file: &'a str,
        containers: Vec<&'a str>,
        name: &'a str,
        disambiguator: Option<u32>,
    },
    Heading {
        doc: &'a str,
        slug: &'a str,
        disambiguator: Option<u32>,
    },
    Path(&'a str),
    Community(&'a str),
    Note(&'a str),
}

impl SemanticPath {
    /// A code symbol. `containers` are the enclosing named nodes outermost first, with Rust impl
    /// containers given as their literal text (`Foo`, `<Foo as Display>`).
    pub fn symbol<S: AsRef<str>>(
        file: &RepoPath,
        containers: &[S],
        name: &str,
        disambiguator: Option<u32>,
    ) -> Result<Self, ModelError> {
        let mut text = file.as_str().to_owned();
        for container in containers {
            let container = container.as_ref();
            check_segment(container, &text)?;
            text.push_str("::");
            text.push_str(container);
        }
        check_segment(name, &text)?;
        if split_suffix(name).1.is_some() {
            return Err(invalid(name, "name carries a disambiguator suffix"));
        }
        text.push_str("::");
        text.push_str(name);
        push_suffix(&mut text, disambiguator);
        Ok(Self(text))
    }

    pub fn heading(doc: &RepoPath, slug: &str, disambiguator: Option<u32>) -> Result<Self, ModelError> {
        if slug.is_empty() || slug.contains('#') || slug.contains("::") {
            return Err(invalid(slug, "invalid heading slug"));
        }
        let mut text = format!("{doc}#{slug}");
        push_suffix(&mut text, disambiguator);
        Ok(Self(text))
    }

    pub fn for_file(path: &RepoPath) -> Self {
        Self(path.as_str().to_owned())
    }

    pub fn for_package(path: &RepoPath) -> Self {
        Self(path.as_str().to_owned())
    }

    pub fn community(id: &str) -> Result<Self, ModelError> {
        check_tag(id)?;
        Ok(Self(format!("{COMMUNITY}{id}")))
    }

    pub fn note(uuid: &str) -> Result<Self, ModelError> {
        check_tag(uuid)?;
        Ok(Self(format!("{NOTE}{uuid}")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The file the node lives in (for headings, the document); `None` for communities and notes.
    pub fn file(&self) -> Option<&str> {
        match self.form() {
            Form::Symbol { file, .. } => Some(file),
            Form::Heading { doc, .. } => Some(doc),
            Form::Path(path) => Some(path),
            Form::Community(_) | Form::Note(_) => None,
        }
    }

    pub fn containers(&self) -> Vec<&str> {
        match self.form() {
            Form::Symbol { containers, .. } => containers,
            _ => Vec::new(),
        }
    }

    /// The last segment without its `[n]` suffix: the symbol name, heading slug, file or package
    /// path, community id, or note uuid.
    pub fn name(&self) -> &str {
        match self.form() {
            Form::Symbol { name, .. } => name,
            Form::Heading { slug, .. } => slug,
            Form::Path(path) => path,
            Form::Community(id) | Form::Note(id) => id,
        }
    }

    pub fn disambiguator(&self) -> Option<u32> {
        match self.form() {
            Form::Symbol { disambiguator, .. } | Form::Heading { disambiguator, .. } => disambiguator,
            _ => None,
        }
    }

    fn form(&self) -> Form<'_> {
        let text = self.0.as_str();
        if let Some(id) = text.strip_prefix(COMMUNITY) {
            return Form::Community(id);
        }
        if let Some(uuid) = text.strip_prefix(NOTE) {
            return Form::Note(uuid);
        }
        let mut segments = split_top_level(text);
        if segments.len() == 1 {
            return match text.rsplit_once('#') {
                Some((doc, rest)) => {
                    let (slug, disambiguator) = split_suffix(rest);
                    Form::Heading {
                        doc,
                        slug,
                        disambiguator,
                    }
                }
                None => Form::Path(text),
            };
        }
        let last = segments.pop().unwrap_or_default();
        let (name, disambiguator) = split_suffix(last);
        let file = segments.remove(0);
        Form::Symbol {
            file,
            containers: segments,
            name,
            disambiguator,
        }
    }
}

fn invalid(text: &str, reason: &'static str) -> ModelError {
    ModelError::InvalidSemanticPath {
        text: text.to_owned(),
        reason,
    }
}

fn check_segment(segment: &str, prefix: &str) -> Result<(), ModelError> {
    if segment.is_empty() {
        return Err(invalid(prefix, "empty segment"));
    }
    if split_top_level(segment).len() > 1 {
        return Err(invalid(segment, "segment contains a top-level `::`"));
    }
    Ok(())
}

fn check_tag(tag: &str) -> Result<(), ModelError> {
    if tag.is_empty() || tag.contains("::") || tag.contains('#') {
        return Err(invalid(tag, "invalid identifier"));
    }
    Ok(())
}

fn push_suffix(text: &mut String, disambiguator: Option<u32>) {
    if let Some(n) = disambiguator {
        text.push_str(&format!("[{n}]"));
    }
}

/// Splits on `::` outside angle, round, square, and curly brackets. The `>` of `->` is not a
/// closing bracket.
fn split_top_level(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'<' | b'(' | b'[' | b'{' => depth += 1,
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' | b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && bytes.get(i + 1) == Some(&b':') => {
                segments.push(&text[start..i]);
                i += 2;
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    segments.push(&text[start..]);
    segments
}

fn split_suffix(segment: &str) -> (&str, Option<u32>) {
    let Some(body) = segment.strip_suffix(']') else {
        return (segment, None);
    };
    let Some((name, digits)) = body.rsplit_once('[') else {
        return (segment, None);
    };
    if name.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return (segment, None);
    }
    match digits.parse() {
        Ok(n) => (name, Some(n)),
        Err(_) => (segment, None),
    }
}

impl fmt::Display for SemanticPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SemanticPath {
    type Err = ModelError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if text.is_empty() {
            return Err(invalid(text, "empty"));
        }
        for tag in [COMMUNITY, NOTE] {
            if let Some(rest) = text.strip_prefix(tag) {
                check_tag(rest)?;
                return Ok(Self(text.to_owned()));
            }
        }
        let segments = split_top_level(text);
        if segments.iter().any(|segment| segment.is_empty()) {
            return Err(invalid(text, "empty segment"));
        }
        if segments.len() == 1
            && let Some((doc, slug)) = text.rsplit_once('#')
            && (doc.is_empty() || split_suffix(slug).0.is_empty())
        {
            return Err(invalid(text, "invalid heading"));
        }
        Ok(Self(text.to_owned()))
    }
}

impl TryFrom<String> for SemanticPath {
    type Error = ModelError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<SemanticPath> for String {
    fn from(value: SemanticPath) -> Self {
        value.0
    }
}
