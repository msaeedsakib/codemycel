use std::collections::HashSet;

use codemycel_core::ParsedFile;
use tree_sitter::Node;

use crate::driver::LanguageHooks;

pub(super) struct Hooks;

impl LanguageHooks for Hooks {
    fn is_doc_comment(&self, text: &str) -> bool {
        text.starts_with("/**") && text != "/**/"
    }

    fn doc_wrapper_kinds(&self) -> &'static [&'static str] {
        &[
            "export_statement",
            "ambient_declaration",
            "lexical_declaration",
            "variable_declaration",
        ]
    }

    fn doc_skip_kinds(&self) -> &'static [&'static str] {
        &["decorator"]
    }

    /// Top-level declarations named in an `export { x }` or `export default x` of the same file
    /// are exported too.
    fn finish(&self, parsed: &mut ParsedFile, root: Node<'_>, source: &str) {
        let exported = exported_names(root, source);
        if exported.is_empty() {
            return;
        }
        for node in &mut parsed.nodes {
            if exported.contains(node.name.as_str()) && node.path.containers().is_empty() {
                node.visibility = Some("export".to_owned());
            }
        }
    }
}

fn exported_names<'s>(root: Node<'_>, source: &'s str) -> HashSet<&'s str> {
    let text = |node: Node<'_>| source.get(node.byte_range()).unwrap_or_default();
    let mut names = HashSet::new();
    let mut statements = root.walk();
    for statement in root.named_children(&mut statements) {
        if statement.kind() != "export_statement"
            || statement.child_by_field_name("source").is_some()
        {
            continue;
        }
        if let Some(value) = statement.child_by_field_name("value")
            && value.kind() == "identifier"
        {
            names.insert(text(value));
        }
        let mut clauses = statement.walk();
        for clause in statement.named_children(&mut clauses) {
            if clause.kind() != "export_clause" {
                continue;
            }
            let mut specifiers = clause.walk();
            for specifier in clause.named_children(&mut specifiers) {
                if let Some(name) = specifier.child_by_field_name("name") {
                    names.insert(text(name));
                }
            }
        }
    }
    names
}
