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
const DOC_EXTENSIONS: [&str; 2] = [".md", ".mdx"];

#[derive(Debug, PartialEq, Eq)]
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
    /// containers given as their literal text (`Foo`, `<Foo as Display>`). A self type that is
    /// itself a path is written in Rust's qualified form, `<super::Foo>`, because a bare
    /// `super::Foo` would read as two containers.
    pub fn symbol<S: AsRef<str>>(
        file: &RepoPath,
        containers: &[S],
        name: &str,
        disambiguator: Option<u32>,
    ) -> Result<Self, ModelError> {
        for segment in containers.iter().map(AsRef::as_ref).chain([name]) {
            if scan(segment).segments.len() > 1 {
                return Err(invalid(
                    segment,
                    "contains a top-level `::`; write a qualified self type as `<path>`",
                ));
            }
        }
        let mut text = file.as_str().to_owned();
        for container in containers {
            text.push_str("::");
            text.push_str(container.as_ref());
        }
        text.push_str("::");
        text.push_str(name);
        push_suffix(&mut text, disambiguator);
        Self::checked(
            text,
            Form::Symbol {
                file: file.as_str(),
                containers: containers.iter().map(AsRef::as_ref).collect(),
                name,
                disambiguator,
            },
        )
    }

    pub fn heading(
        doc: &RepoPath,
        slug: &str,
        disambiguator: Option<u32>,
    ) -> Result<Self, ModelError> {
        let mut text = format!("{doc}#{slug}");
        push_suffix(&mut text, disambiguator);
        Self::checked(
            text,
            Form::Heading {
                doc: doc.as_str(),
                slug,
                disambiguator,
            },
        )
    }

    pub fn for_file(path: &RepoPath) -> Result<Self, ModelError> {
        Self::checked(path.as_str().to_owned(), Form::Path(path.as_str()))
    }

    /// A package: its manifest's path for a workspace package, its name for an external one.
    pub fn for_package(manifest_or_name: &str) -> Result<Self, ModelError> {
        Self::checked(manifest_or_name.to_owned(), Form::Path(manifest_or_name))
    }

    pub fn community(id: &str) -> Result<Self, ModelError> {
        Self::checked(format!("{COMMUNITY}{id}"), Form::Community(id))
    }

    pub fn note(uuid: &str) -> Result<Self, ModelError> {
        Self::checked(format!("{NOTE}{uuid}"), Form::Note(uuid))
    }

    /// Accepts `text` only if it parses back to exactly the parts it was built from, so every
    /// constructed path round-trips through `FromStr` with the same accessors.
    fn checked(text: String, expected: Form<'_>) -> Result<Self, ModelError> {
        let path: Self = text.parse()?;
        if path.form() != expected {
            return Err(invalid(&text, "parts do not survive a round trip"));
        }
        Ok(path)
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
            Form::Symbol { disambiguator, .. } | Form::Heading { disambiguator, .. } => {
                disambiguator
            }
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
        let mut segments = scan(text).segments;
        if segments.len() == 1 {
            return match text.rsplit_once('#') {
                Some((doc, rest)) if is_doc(doc) => {
                    let (slug, disambiguator) = split_suffix(rest);
                    Form::Heading {
                        doc,
                        slug,
                        disambiguator,
                    }
                }
                _ => Form::Path(text),
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

fn is_doc(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    DOC_EXTENSIONS.iter().any(|ext| lower.ends_with(ext))
}

fn push_suffix(text: &mut String, disambiguator: Option<u32>) {
    if let Some(n) = disambiguator {
        text.push_str(&format!("[{n}]"));
    }
}

struct Scan<'a> {
    segments: Vec<&'a str>,
    /// Every segment closes the brackets it opens.
    balanced: bool,
}

/// Splits on `::` outside brackets. Angle brackets count only outside `()`, `[]`, and `{}`, so a
/// comparison inside a const-generic block does not open one; the `>` of `->` never closes one.
fn scan(text: &str) -> Scan<'_> {
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut stack: Vec<u8> = Vec::new();
    let mut balanced = true;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let angles_count = matches!(stack.last(), None | Some(b'<'));
        match bytes[i] {
            b'(' | b'[' | b'{' => stack.push(bytes[i]),
            b'<' if angles_count => stack.push(b'<'),
            b'>' if i > 0 && bytes[i - 1] == b'-' => {}
            b'>' if stack.last() == Some(&b'<') => {
                stack.pop();
            }
            closer @ (b')' | b']' | b'}') => {
                let opener = match closer {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                if stack.last() == Some(&opener) {
                    stack.pop();
                } else {
                    balanced = false;
                }
            }
            b':' if stack.is_empty() && bytes.get(i + 1) == Some(&b':') => {
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
    Scan {
        segments,
        balanced: balanced && stack.is_empty(),
    }
}

fn split_suffix(segment: &str) -> (&str, Option<u32>) {
    let Some(body) = segment.strip_suffix(']') else {
        return (segment, None);
    };
    let Some((name, digits)) = body.rsplit_once('[') else {
        return (segment, None);
    };
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if name.is_empty() || !canonical {
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
        let path = Self(text.to_owned());
        let valid = match path.form() {
            Form::Community(tag) | Form::Note(tag) => {
                !tag.is_empty() && !tag.contains("::") && !tag.contains('#')
            }
            Form::Path(path) => !path.is_empty(),
            Form::Heading { doc, slug, .. } => !doc.is_empty() && !slug.is_empty(),
            Form::Symbol { .. } => {
                let scan = scan(text);
                scan.balanced && scan.segments.iter().all(|segment| !segment.is_empty())
            }
        };
        if !valid {
            return Err(invalid(text, "not a semantic path"));
        }
        Ok(path)
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
