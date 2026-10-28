use std::collections::BTreeSet;
use std::fmt::Write as _;

use crate::query::answer::{
    Answer, Card, Direction, EdgeLine, Line, NodeLine, Section, Truncation,
};
use crate::query::freshness::{CoverageState, Freshness};
use crate::{Coverage, VerifiedBy};

/// Renders an answer as line-oriented text: sections with `name:` headers, cards, `←`/`→` edge
/// lines, then the truncation and freshness footers.
pub struct TextFormatter;

const INDENT: &str = "  ";

impl TextFormatter {
    pub fn render(answer: &Answer) -> String {
        let mixed = Self::mixed_sources(answer);
        let mut out = String::new();
        for section in answer.sections.iter().filter(|s| !s.lines.is_empty()) {
            if section.header {
                out.push_str(&Self::header(section));
                out.push('\n');
            }
            for line in &section.lines {
                out.push_str(&Self::line(line, Self::indent(section), mixed));
                out.push('\n');
            }
        }
        if let Some(truncated) = &answer.truncated {
            out.push_str(&Self::truncation(truncated));
            out.push('\n');
        }
        out.push_str(&Self::freshness(&answer.freshness));
        out.push('\n');
        out
    }

    pub(crate) fn header(section: &Section) -> String {
        format!("{}:", section.name)
    }

    pub(crate) fn indent(section: &Section) -> &'static str {
        if section.header { INDENT } else { "" }
    }

    /// Whether edge lines come from both SCIP and a language server, so each names its source.
    pub(crate) fn mixed_sources(answer: &Answer) -> bool {
        let sources: BTreeSet<VerifiedBy> = answer
            .sections
            .iter()
            .flat_map(|section| &section.lines)
            .filter_map(|line| match line {
                Line::Edge(edge) => Some(edge.verified_by),
                _ => None,
            })
            .filter(|by| matches!(by, VerifiedBy::Scip | VerifiedBy::Lsp))
            .collect();
        sources.len() > 1
    }

    /// One entry, possibly several text lines, without a trailing newline.
    pub(crate) fn line(line: &Line, indent: &str, mixed: bool) -> String {
        match line {
            Line::Card(card) => Self::card(card, indent),
            Line::Edge(edge) => format!("{indent}{}", Self::edge(edge, mixed)),
            Line::Node(node) => {
                let mut out = format!("{indent}{}", Self::identity(node));
                if let (Some(file), Some(line)) = (&node.file, node.line_start) {
                    let _ = write!(out, " {file}:{line}");
                }
                if let Some(signature) = &node.signature {
                    let _ = write!(out, "  {signature}");
                }
                out
            }
            Line::Unresolved(line) => format!(
                "{indent}L{}:{} {}  {}  ({})",
                line.line, line.col, line.role, line.text, line.reason
            ),
            Line::Note(note) => {
                let mut out = format!("{indent}{} {} on {}", note.kind, note.uuid, note.target);
                if note.orphaned {
                    out.push_str(" (orphaned)");
                }
                let _ = write!(out, " ({})", day(&note.updated));
                for body in note.body.lines() {
                    let _ = write!(out, "\n{indent}{INDENT}{body}");
                }
                out
            }
            Line::Source(source) => format!("{indent}{:>4}  {}", source.line, source.text),
            Line::Field { key, value } => format!("{indent}{key}  {value}"),
            Line::Text { text } => format!("{indent}{text}"),
        }
    }

    pub(crate) fn truncation(truncated: &Truncation) -> String {
        let counts: Vec<String> = truncated
            .sections
            .iter()
            .map(|count| format!("{} of {} {}", count.shown, count.total, count.name))
            .collect();
        format!(
            "truncated: showing {} · {}",
            counts.join(", "),
            truncated.hint
        )
    }

    pub(crate) fn freshness(freshness: &Freshness) -> String {
        let commit = freshness
            .indexed_commit
            .as_deref()
            .map_or("no git", |commit| &commit[..commit.len().min(7)]);
        let mut out = format!(
            "index: {commit} ({} files changed since)",
            freshness.changed_files
        );
        if !freshness.coverage.is_empty() {
            let languages: Vec<String> = freshness
                .coverage
                .iter()
                .map(|language| {
                    let states: Vec<String> =
                        language.states.iter().map(|s| coverage_state(*s)).collect();
                    format!("{} {}", language.language, states.join(", "))
                })
                .collect();
            let _ = write!(out, " · coverage: {}", languages.join(" · "));
        }
        if !freshness.stale.is_empty() {
            let stale: Vec<&str> = freshness.stale.iter().map(|p| p.as_str()).collect();
            let _ = write!(out, " · stale in this answer: {}", stale.join(", "));
        }
        out
    }

    fn identity(node: &NodeLine) -> String {
        let prefix = if node.derived { "derived:" } else { "" };
        format!("{prefix}{} ({})", node.path, node.kind)
    }

    fn card(card: &Card, indent: &str) -> String {
        let node = &card.node;
        let mut out = format!("{indent}{}", Self::identity(node));
        match (node.line_start, node.line_end) {
            (Some(start), Some(end)) if end != start => {
                let _ = write!(out, " L{start}-{end}");
            }
            (Some(start), _) => {
                let _ = write!(out, " L{start}");
            }
            _ => {}
        }
        if let Some(visibility) = &card.visibility {
            let _ = write!(out, " {visibility}");
        }
        let sub = format!("{indent}{INDENT}");
        if let Some(signature) = &node.signature {
            let _ = write!(out, "\n{sub}sig  {signature}");
        }
        if let Some(doc) = &card.doc {
            let _ = write!(out, "\n{sub}doc  {doc}");
        }
        if let Some(community) = &card.community {
            let _ = write!(out, "\n{sub}in   derived:{}", community.label);
        }
        let counts = &card.counts;
        if !counts.is_empty() {
            let _ = write!(
                out,
                "\n{sub}callers {} · callees {} · impls {} · overrides {} · unresolved {} · external {}",
                counts.callers,
                counts.callees,
                counts.impls,
                counts.overrides,
                counts.unresolved,
                counts.external
            );
        }
        for note in &card.notes {
            let _ = write!(
                out,
                "\n{sub}{} \"{}\" ({})",
                note.kind,
                note.first_line,
                day(&note.updated)
            );
        }
        out
    }

    fn edge(edge: &EdgeLine, mixed: bool) -> String {
        let arrow = match edge.direction {
            Direction::In => '←',
            Direction::Out => '→',
        };
        let mut out = format!("{arrow} {}  {}", edge.kind, Self::identity(&edge.node));
        if let Some(line) = edge.node.line_start {
            let _ = write!(out, " L{line}");
        }
        if !edge.sites.is_empty() {
            let sites: Vec<String> = edge.sites.iter().map(|line| format!("L{line}")).collect();
            let _ = write!(out, " [{}]", sites.join(", "));
        }
        match edge.verified_by {
            VerifiedBy::Syntactic => out.push_str(" (syntactic)"),
            VerifiedBy::Scip | VerifiedBy::Lsp if mixed => {
                let _ = write!(out, " verified: {}", edge.verified_by);
            }
            _ => {}
        }
        if let Some(via) = &edge.via {
            let _ = write!(out, " via {via}");
        }
        out
    }
}

fn coverage_state(state: CoverageState) -> String {
    match state {
        CoverageState::Verified {
            by: Coverage::ScipAndLsp,
        } => "scip ✓ lsp ✓".to_owned(),
        CoverageState::Verified { by: Coverage::None } => "none".to_owned(),
        CoverageState::Verified { by } => format!("{by} ✓"),
        CoverageState::Degraded { reason } => format!("degraded: {reason}"),
        CoverageState::NotVerified => "not verified".to_owned(),
    }
}

/// The date of an RFC 3339 time.
fn day(time: &str) -> &str {
    time.get(..10).unwrap_or(time)
}
