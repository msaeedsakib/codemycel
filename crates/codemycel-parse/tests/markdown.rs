#![allow(clippy::unwrap_used)]

use codemycel_core::RepoPath;
use codemycel_parse::markdown::{DocParse, parse};

fn doc(source: &str) -> DocParse {
    parse(&RepoPath::new("docs/guide.md").unwrap(), source.as_bytes()).unwrap()
}

fn refs(references: &[codemycel_parse::markdown::Reference]) -> Vec<String> {
    references
        .iter()
        .map(|r| format!("{}:{} {} from {}", r.at.line, r.at.col, r.text, r.from.path))
        .collect()
}

#[test]
fn headings_become_nodes_with_unique_slugs() {
    let parsed = doc(
        "# Intro\n\nText.\n\n## Install\n\nInstall\n-------\n\n## Install ##\n\n### Getting Started: v2!\n",
    );
    let headings: Vec<String> = parsed
        .headings
        .iter()
        .map(|h| {
            let span = h.span.unwrap();
            format!(
                "{} {} {}-{}",
                h.path, h.name, span.line_start, span.line_end
            )
        })
        .collect();
    assert_eq!(
        headings,
        [
            "docs/guide.md#intro Intro 1-12",
            "docs/guide.md#install Install 5-6",
            "docs/guide.md#install[1] Install 7-9",
            "docs/guide.md#install[2] Install 10-12",
            "docs/guide.md#getting-started-v2 Getting Started: v2! 12-12",
        ]
    );
    assert_eq!(parsed.contains.len(), 5);
    assert!(
        parsed
            .contains
            .iter()
            .all(|e| e.src.path.as_str() == "docs/guide.md")
    );
    assert_eq!(parsed.doc.span.unwrap().line_end, 12);
}

#[test]
fn heading_links_keep_only_their_text() {
    let parsed = doc(
        "# See [Guide][g] here\n\n## Also [Inline](x.md \"T\")\n\n## [Both][] and [short] ##\n\nSetext [Link](<a b.md>)\n---\n\n## Install [beta] and [Foo][nope]\n\n[g]: g.md\n[both]: b.md\n[short]: s.md\n",
    );
    let headings: Vec<String> = parsed
        .headings
        .iter()
        .map(|h| format!("{} {}", h.path, h.name))
        .collect();
    assert_eq!(
        headings,
        [
            "docs/guide.md#see-guide-here See Guide here",
            "docs/guide.md#also-inline Also Inline",
            "docs/guide.md#both-and-short Both and short",
            "docs/guide.md#setext-link Setext Link",
            "docs/guide.md#install-beta-and-foonope Install [beta] and [Foo][nope]",
        ]
    );
}

#[test]
fn fenced_code_is_ignored_for_mentions() {
    let parsed = doc(
        "Uses `Store`.\n\n```rust\nlet x = `Hidden`;\n```\n\n    `Indented`\n\n# Heading with `Inline`\n",
    );
    assert_eq!(
        refs(&parsed.mentions),
        [
            "1:5 Store from docs/guide.md",
            "9:15 Inline from docs/guide.md#heading-with-inline",
        ]
    );
}

#[test]
fn mention_strips_trailing_parens_and_leading_dot() {
    let parsed = doc("# API\n\nCall `open()`, then `.close`, or `.run()`; not `()`.\n");
    let texts: Vec<&str> = parsed.mentions.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["open", "close", "run"]);
}

#[test]
fn table_cells_are_read_for_mentions_and_links() {
    let parsed = doc("# Table\n\n| name | see |\n|---|---|\n| `Store` | [lib](lib.rs) |\n");
    assert_eq!(
        refs(&parsed.mentions),
        ["5:2 Store from docs/guide.md#table"]
    );
    assert_eq!(
        refs(&parsed.links),
        ["5:12 docs/lib.rs from docs/guide.md#table"]
    );
}

#[test]
fn reference_links_resolve_through_their_definitions() {
    let parsed = doc(
        "# Refs\n\n[Full][Setup  Guide] [setup guide][] [Setup Guide] [web] [nowhere] ![img][setup guide]\n\n\
         [setup\n guide]: <../setup.md#install> \"Setup\"\n[SETUP GUIDE]: ignored.md\n[web]: https://example.com\n",
    );
    assert_eq!(
        refs(&parsed.links),
        [
            "3:0 setup.md from docs/guide.md#refs",
            "3:21 setup.md from docs/guide.md#refs",
            "3:37 setup.md from docs/guide.md#refs",
        ]
    );
}

#[test]
fn relative_links_resolve_against_the_document() {
    let parsed = doc(
        "# Links\n\n[lib](../crate/src/lib.rs) [same](./other.md#part) [sp](<my file.rs>) \
         [enc](my%20file.rs) [web](https://example.com/a.rs) [frag](#links) \
         [abs](/etc/passwd) [out](../../outside.rs)\n",
    );
    let targets: Vec<&str> = parsed.links.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        targets,
        [
            "crate/src/lib.rs",
            "docs/other.md",
            "docs/my file.rs",
            "docs/my file.rs"
        ]
    );
    assert!(
        parsed
            .links
            .iter()
            .all(|l| l.from.path.as_str() == "docs/guide.md#links")
    );
}
