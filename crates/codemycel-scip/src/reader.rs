use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use codemycel_core::RepoPath;
use codemycel_core::config::Config;
use protobuf::Message;
use scip::types::{self, PositionEncoding, SymbolRole};

use crate::ScipError;

/// Finds and loads SCIP indexes.
pub struct ScipImporter;

impl ScipImporter {
    /// The configured `scip.path` (relative to the repository root unless absolute) when it
    /// names an existing file. The default path is `index.scip` at the repository root.
    pub fn discover(repo_root: &Path, config: &Config) -> Option<PathBuf> {
        let path = if config.scip_path.is_absolute() {
            config.scip_path.clone()
        } else {
            repo_root.join(&config.scip_path)
        };
        path.is_file().then_some(path)
    }

    pub fn load(path: &Path) -> Result<ScipIndex, ScipError> {
        let bytes = std::fs::read(path).map_err(|source| ScipError::Read {
            path: path.to_owned(),
            source,
        })?;
        let index = types::Index::parse_from_bytes(&bytes).map_err(|source| ScipError::Decode {
            path: path.to_owned(),
            source,
        })?;
        Ok(ScipIndex::new(
            index,
            blake3::hash(&bytes).to_hex().to_string(),
        ))
    }
}

/// How a document counts the character part of its positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Encoding {
    Utf8,
    Utf16,
    Utf32,
}

/// A 0-based, half-open range in a document's encoding units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Range {
    pub start_line: u32,
    pub start: u32,
    pub end_line: u32,
    pub end: u32,
}

impl Range {
    /// `[line, start, end]` or `[start line, start, end line, end]`.
    fn parse(range: &[i32]) -> Option<Self> {
        let n = |i: usize| u32::try_from(*range.get(i)?).ok();
        match range.len() {
            3 => Some(Self {
                start_line: n(0)?,
                start: n(1)?,
                end_line: n(0)?,
                end: n(2)?,
            }),
            4 => Some(Self {
                start_line: n(0)?,
                start: n(1)?,
                end_line: n(2)?,
                end: n(3)?,
            }),
            _ => None,
        }
    }

    pub fn contains(&self, line: u32, unit: u32) -> bool {
        (self.start_line, self.start) <= (line, unit) && (line, unit) < (self.end_line, self.end)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    pub range: Range,
    pub symbol: String,
    pub definition: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub encoding: Encoding,
    pub occurrences: Vec<Occurrence>,
}

/// A loaded SCIP index: its documents by repository path, where each global symbol is defined,
/// the packages of the defined symbols, and which symbols implement which. Local symbols are
/// dropped on load.
#[derive(Clone, Debug)]
pub struct ScipIndex {
    hash: String,
    pub(crate) documents: BTreeMap<RepoPath, Document>,
    pub(crate) definitions: HashMap<String, Vec<(RepoPath, Range)>>,
    pub(crate) packages: HashSet<String>,
    pub(crate) implementers: HashMap<String, Vec<String>>,
    pub(crate) covered: BTreeSet<RepoPath>,
}

/// A global symbol's scheme and package: its first four space-separated fields, where a double
/// space is an escaped space inside a field.
pub(crate) fn package(symbol: &str) -> Option<&str> {
    let mut fields = 0;
    let mut chars = symbol.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        if c != ' ' {
            continue;
        }
        if chars.next_if(|(_, next)| *next == ' ').is_some() {
            continue;
        }
        fields += 1;
        if fields == 4 {
            return Some(&symbol[..at]);
        }
    }
    None
}

impl ScipIndex {
    fn new(index: types::Index, hash: String) -> Self {
        let mut documents = BTreeMap::new();
        let mut definitions: HashMap<String, Vec<(RepoPath, Range)>> = HashMap::new();
        let mut implementers: HashMap<String, Vec<String>> = HashMap::new();
        let mut relate = |info: &types::SymbolInformation| {
            for relationship in &info.relationships {
                if relationship.is_implementation {
                    implementers
                        .entry(relationship.symbol.clone())
                        .or_default()
                        .push(info.symbol.clone());
                }
            }
        };
        for info in &index.external_symbols {
            relate(info);
        }
        for document in &index.documents {
            for info in &document.symbols {
                relate(info);
            }
            let Ok(path) = RepoPath::new(&document.relative_path) else {
                tracing::warn!(path = %document.relative_path, "SCIP document path is not a repository path");
                continue;
            };
            let encoding = match document.position_encoding.enum_value() {
                Ok(PositionEncoding::UTF8CodeUnitOffsetFromLineStart) => Encoding::Utf8,
                Ok(PositionEncoding::UTF32CodeUnitOffsetFromLineStart) => Encoding::Utf32,
                _ => Encoding::Utf16,
            };
            let occurrences: Vec<Occurrence> = document
                .occurrences
                .iter()
                .filter(|o| !o.symbol.is_empty() && !scip::symbol::is_local_symbol(&o.symbol))
                .filter_map(|o| {
                    Some(Occurrence {
                        range: Range::parse(&o.range)?,
                        symbol: o.symbol.clone(),
                        definition: o.symbol_roles & SymbolRole::Definition as i32 != 0,
                    })
                })
                .collect();
            for occurrence in occurrences.iter().filter(|o| o.definition) {
                definitions
                    .entry(occurrence.symbol.clone())
                    .or_default()
                    .push((path.clone(), occurrence.range));
            }
            documents.insert(
                path,
                Document {
                    encoding,
                    occurrences,
                },
            );
        }
        for implementer in implementers.values_mut() {
            implementer.sort();
            implementer.dedup();
        }
        let packages = definitions
            .keys()
            .filter_map(|symbol| package(symbol))
            .map(ToOwned::to_owned)
            .collect();
        Self {
            hash,
            documents,
            definitions,
            packages,
            implementers,
            covered: BTreeSet::new(),
        }
    }

    /// BLAKE3 of the index file's bytes, hex.
    pub fn hash(&self) -> &str {
        &self.hash
    }

    /// Files the index has a document for, sorted.
    pub fn documents(&self) -> impl Iterator<Item = &RepoPath> {
        self.documents.keys()
    }
}
