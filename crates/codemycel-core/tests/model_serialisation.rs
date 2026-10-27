use std::collections::BTreeSet;
use std::fmt::Debug;
use std::str::FromStr;

use codemycel_core::{
    CandidateRole, EdgeKind, FileKind, LabelSource, Language, NodeKind, NoteKind, Occurrence,
    SkipReason, UnresolvedReason, VerifiedBy,
};

fn assert_round_trips<T>(all: &[T], as_str: fn(T) -> &'static str)
where
    T: Copy + Debug + PartialEq + FromStr + serde::Serialize,
    <T as FromStr>::Err: Debug,
{
    for &variant in all {
        let text = as_str(variant);
        assert!(
            !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{text} is not snake_case"
        );
        assert_eq!(text.parse::<T>().unwrap(), variant);
        assert_eq!(serde_json::to_string(&variant).unwrap(), format!("\"{text}\""));
    }
    let distinct: BTreeSet<_> = all.iter().map(|&v| as_str(v)).collect();
    assert_eq!(distinct.len(), all.len());
}

fn strings<T: Copy>(all: &[T], as_str: fn(T) -> &'static str) -> Vec<&'static str> {
    all.iter().map(|&v| as_str(v)).collect()
}

#[test]
fn every_enum_round_trips_through_as_str() {
    assert_round_trips(Language::ALL, Language::as_str);
    assert_round_trips(NodeKind::ALL, NodeKind::as_str);
    assert_round_trips(EdgeKind::ALL, EdgeKind::as_str);
    assert_round_trips(VerifiedBy::ALL, VerifiedBy::as_str);
    assert_round_trips(CandidateRole::ALL, CandidateRole::as_str);
    assert_round_trips(UnresolvedReason::ALL, UnresolvedReason::as_str);
    assert_round_trips(FileKind::ALL, FileKind::as_str);
    assert_round_trips(SkipReason::ALL, SkipReason::as_str);
    assert_round_trips(NoteKind::ALL, NoteKind::as_str);
    assert_round_trips(LabelSource::ALL, LabelSource::as_str);
    assert!("Function".parse::<NodeKind>().is_err());
}

#[test]
fn language_strings_match_config_keys() {
    assert_eq!(
        strings(Language::ALL, Language::as_str),
        ["rust", "typescript", "javascript"]
    );
}

#[test]
fn node_kind_strings_match_spec() {
    assert_eq!(
        strings(NodeKind::ALL, NodeKind::as_str),
        [
            "file", "module", "class", "struct", "enum", "enum_member", "trait", "interface",
            "type_alias", "function", "method", "field", "constant", "variable", "package", "doc",
            "heading", "community", "note",
        ]
    );
}

#[test]
fn edge_kind_strings_match_spec() {
    assert_eq!(
        strings(EdgeKind::ALL, EdgeKind::as_str),
        [
            "contains", "defines", "imports", "exports", "calls", "references", "extends",
            "implements", "overrides", "depends_on", "mentions", "member_of", "annotates",
        ]
    );
}

#[test]
fn verified_by_and_role_strings_match_spec() {
    assert_eq!(
        strings(VerifiedBy::ALL, VerifiedBy::as_str),
        ["scip", "lsp", "syntactic", "derived", "note"]
    );
    assert_eq!(
        strings(CandidateRole::ALL, CandidateRole::as_str),
        ["call", "type", "import", "export", "extends", "implements", "value"]
    );
}

#[test]
fn unresolved_reason_strings_match_spec() {
    assert_eq!(
        strings(UnresolvedReason::ALL, UnresolvedReason::as_str),
        [
            "no_definition",
            "server_unavailable",
            "external_dependency",
            "unindexed_file",
            "unmapped",
        ]
    );
}

#[test]
fn occurrences_serialise_as_line_col_pairs() {
    let occurrences = vec![Occurrence { line: 42, col: 10 }, Occurrence { line: 7, col: 0 }];
    let json = serde_json::to_string(&occurrences).unwrap();
    assert_eq!(json, "[[42,10],[7,0]]");
    let back: Vec<Occurrence> = serde_json::from_str(&json).unwrap();
    assert_eq!(back, occurrences);
}
