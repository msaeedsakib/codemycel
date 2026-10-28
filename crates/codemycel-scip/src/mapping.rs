use codemycel_core::{Node, NodeKind, NodeRef, RepoPath, SemanticPath, SourceFile};

use crate::reader::{Encoding, Range};

/// One parsed file's text split into lines, with its nodes, for converting SCIP positions.
pub(crate) struct FileLines<'a> {
    path: &'a RepoPath,
    text: &'a str,
    starts: Vec<usize>,
    pub nodes: &'a [Node],
}

impl<'a> FileLines<'a> {
    pub fn new(file: &SourceFile<'a>) -> Self {
        let starts = std::iter::once(0)
            .chain(file.text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self {
            path: file.path,
            text: file.text,
            starts,
            nodes: file.nodes,
        }
    }

    fn line(&self, row: u32) -> Option<(usize, &'a str)> {
        let row = usize::try_from(row).ok()?;
        let start = *self.starts.get(row)?;
        let end = self
            .starts
            .get(row + 1)
            .map_or(self.text.len(), |next| next - 1);
        let line = &self.text[start..end];
        Some((start, line.strip_suffix('\r').unwrap_or(line)))
    }

    /// A 0-based character column on `row` in `encoding` units.
    pub fn units(&self, row: u32, chars: u32, encoding: Encoding) -> Option<u32> {
        let (_, line) = self.line(row)?;
        let units: usize = line
            .chars()
            .take(usize::try_from(chars).ok()?)
            .map(|c| unit_len(c, encoding))
            .sum();
        u32::try_from(units).ok()
    }

    /// The byte offset of a position given in `encoding` units.
    pub fn offset(&self, row: u32, units: u32, encoding: Encoding) -> Option<u32> {
        let (start, line) = self.line(row)?;
        let mut seen = 0;
        let mut bytes = 0;
        for c in line.chars() {
            if seen >= units as usize {
                break;
            }
            seen += unit_len(c, encoding);
            bytes += c.len_utf8();
        }
        u32::try_from(start + bytes).ok()
    }

    /// The innermost node whose selection holds the range's start, else the file node for a
    /// range at the top of the file.
    pub fn node_at(&self, range: Range, encoding: Encoding) -> Option<NodeRef> {
        let offset = self.offset(range.start_line, range.start, encoding)?;
        let node = self
            .nodes
            .iter()
            .filter_map(|node| Some((node, node.selection?)))
            .filter(|(_, sel)| sel.start <= offset && offset <= sel.end)
            .min_by_key(|(_, sel)| sel.end - sel.start)
            .map(|(node, _)| node.node_ref());
        node.or_else(|| {
            (range.start_line == 0 && range.start == 0)
                .then(|| SemanticPath::for_file(self.path).ok())
                .flatten()
                .map(|path| NodeRef::new(path, NodeKind::File))
        })
    }

    pub fn node(&self, node: &NodeRef) -> Option<&'a Node> {
        self.nodes
            .iter()
            .find(|n| n.path == node.path && n.kind == node.kind)
    }
}

fn unit_len(c: char, encoding: Encoding) -> usize {
    match encoding {
        Encoding::Utf8 => c.len_utf8(),
        Encoding::Utf16 => c.len_utf16(),
        Encoding::Utf32 => 1,
    }
}
