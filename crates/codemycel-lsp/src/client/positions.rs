use codemycel_core::Occurrence;
use lsp_types::Position;

/// Converts between codemycel positions (1-based line, 0-based character column), byte offsets,
/// and LSP positions (0-based line, UTF-16 column) over one file's text.
#[derive(Clone, Debug)]
pub struct LineIndex {
    text: String,
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { text, line_starts }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    fn line(&self, row: usize) -> Option<&str> {
        let start = *self.line_starts.get(row)?;
        let end = self
            .line_starts
            .get(row + 1)
            .map_or(self.text.len(), |next| next - 1);
        let line = &self.text[start..end];
        Some(line.strip_suffix('\r').unwrap_or(line))
    }

    pub fn to_lsp(&self, at: Occurrence) -> Option<Position> {
        let row = usize::try_from(at.line).ok()?.checked_sub(1)?;
        let line = self.line(row)?;
        let col = usize::try_from(at.col).ok()?;
        let character: usize = line.chars().take(col).map(char::len_utf16).sum();
        Some(Position::new(
            u32::try_from(row).ok()?,
            u32::try_from(character).ok()?,
        ))
    }

    pub fn from_lsp(&self, position: Position) -> Option<Occurrence> {
        let row = usize::try_from(position.line).ok()?;
        let line = self.line(row)?;
        let (chars, _) = self.walk_utf16(line, position.character);
        Some(Occurrence {
            line: position.line.checked_add(1)?,
            col: u32::try_from(chars).ok()?,
        })
    }

    /// The byte offset of an LSP position; a column past the line's end clamps to the end.
    pub fn offset(&self, position: Position) -> Option<u32> {
        let row = usize::try_from(position.line).ok()?;
        let start = *self.line_starts.get(row)?;
        let line = self.line(row)?;
        let (_, bytes) = self.walk_utf16(line, position.character);
        u32::try_from(start + bytes).ok()
    }

    /// The LSP position of a byte offset; an offset inside a character points at its start.
    pub fn position(&self, offset: u32) -> Option<Position> {
        let offset = usize::try_from(offset).ok()?.min(self.text.len());
        let row = self.line_starts.partition_point(|&start| start <= offset) - 1;
        let line = &self.text[self.line_starts[row]..];
        let character: usize = line
            .char_indices()
            .take_while(|(i, _)| self.line_starts[row] + i < offset)
            .map(|(_, c)| c.len_utf16())
            .sum();
        Some(Position::new(
            u32::try_from(row).ok()?,
            u32::try_from(character).ok()?,
        ))
    }

    /// Characters and bytes of `line` before UTF-16 column `character`.
    fn walk_utf16(&self, line: &str, character: u32) -> (usize, usize) {
        let mut units = 0;
        let mut chars = 0;
        let mut bytes = 0;
        for c in line.chars() {
            if units >= character as usize {
                break;
            }
            units += c.len_utf16();
            chars += 1;
            bytes += c.len_utf8();
        }
        (chars, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surrogate_pairs_count_twice() {
        let index = LineIndex::new("fn a() {}\nlet 😀 = b;\r\nc\n");
        let at = Occurrence { line: 2, col: 8 };
        let lsp = index.to_lsp(at);
        assert_eq!(lsp, Some(Position::new(1, 9)));
        assert_eq!(lsp.and_then(|p| index.from_lsp(p)), Some(at));
        assert_eq!(index.offset(Position::new(1, 9)), Some(10 + 11));
        assert_eq!(index.offset(Position::new(2, 0)), Some(10 + 15));
        assert_eq!(index.position(10 + 11), Some(Position::new(1, 9)));
    }
}
