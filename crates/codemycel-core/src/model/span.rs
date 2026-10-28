use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: u32,
    pub end: u32,
}

/// A node's or impl block's extent: half-open bytes, 1-based inclusive lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    pub bytes: ByteRange,
    pub line_start: u32,
    pub line_end: u32,
}

/// A reference site: 1-based line, 0-based character column. Serialises as `[line, col]`, the
/// form stored in `edges.occurrences`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(from = "(u32, u32)", into = "(u32, u32)")]
pub struct Occurrence {
    pub line: u32,
    pub col: u32,
}

impl From<(u32, u32)> for Occurrence {
    fn from((line, col): (u32, u32)) -> Self {
        Self { line, col }
    }
}

impl From<Occurrence> for (u32, u32) {
    fn from(occurrence: Occurrence) -> Self {
        (occurrence.line, occurrence.col)
    }
}
