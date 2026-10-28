use std::str::FromStr;

use codemycel_core::{CandidateRole, NodeKind};
use tree_sitter::{Language, Query};

use super::{DriverError, QuerySet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SymbolCapture {
    Def(NodeKind),
    Name,
    Body,
    Visibility,
    ReturnType,
    Overload,
    Impl,
    ImplType,
    ImplTypeName,
    ImplTrait,
    ImplBody,
    Ignored,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LocalCapture {
    Scope,
    Definition,
    Exclude,
    Ignored,
}

pub(super) struct Compiled {
    pub symbols: Query,
    pub symbol_captures: Vec<SymbolCapture>,
    pub candidates: Query,
    pub candidate_captures: Vec<Option<CandidateRole>>,
    pub locals: Query,
    pub local_captures: Vec<LocalCapture>,
}

impl Compiled {
    pub fn new(language: &Language, sources: QuerySet) -> Result<Self, DriverError> {
        let symbols = compile(language, "symbols", sources.symbols)?;
        let symbol_captures = classify(&symbols, "symbols", symbol_capture)?;
        let candidates = compile(language, "candidates", sources.candidates)?;
        let candidate_captures = classify(&candidates, "candidates", |name| {
            match name.strip_prefix("cand.") {
                Some(role) => CandidateRole::from_str(role).ok().map(Some),
                None => name.starts_with('_').then_some(None),
            }
        })?;
        let locals = compile(language, "locals", sources.locals)?;
        let local_captures = classify(&locals, "locals", |name| match name {
            "local.scope" => Some(LocalCapture::Scope),
            "local.definition" => Some(LocalCapture::Definition),
            "local.exclude" => Some(LocalCapture::Exclude),
            "local.reference" => Some(LocalCapture::Ignored),
            _ => name.starts_with('_').then_some(LocalCapture::Ignored),
        })?;
        Ok(Self {
            symbols,
            symbol_captures,
            candidates,
            candidate_captures,
            locals,
            local_captures,
        })
    }
}

fn compile(language: &Language, query: &'static str, source: &str) -> Result<Query, DriverError> {
    Query::new(language, source).map_err(|source| DriverError::Query { query, source })
}

fn classify<T>(
    query: &Query,
    label: &'static str,
    capture: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, DriverError> {
    query
        .capture_names()
        .iter()
        .map(|name| {
            capture(name).ok_or_else(|| DriverError::UnknownCapture {
                query: label,
                name: (*name).to_owned(),
            })
        })
        .collect()
}

fn symbol_capture(name: &str) -> Option<SymbolCapture> {
    let capture = match name {
        "def.name" => SymbolCapture::Name,
        "def.body" => SymbolCapture::Body,
        "def.visibility" => SymbolCapture::Visibility,
        "def.return_type" => SymbolCapture::ReturnType,
        "def.overload" => SymbolCapture::Overload,
        "impl" => SymbolCapture::Impl,
        "impl.type" => SymbolCapture::ImplType,
        "impl.type_name" => SymbolCapture::ImplTypeName,
        "impl.trait" => SymbolCapture::ImplTrait,
        "impl.body" => SymbolCapture::ImplBody,
        _ if name.starts_with('_') => SymbolCapture::Ignored,
        _ => {
            let kind = NodeKind::from_str(name.strip_prefix("def.")?).ok()?;
            let extracted_elsewhere = matches!(
                kind,
                NodeKind::File
                    | NodeKind::Package
                    | NodeKind::Doc
                    | NodeKind::Heading
                    | NodeKind::Community
                    | NodeKind::Note
            );
            if extracted_elsewhere {
                return None;
            }
            SymbolCapture::Def(kind)
        }
    };
    Some(capture)
}
