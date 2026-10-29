const START: &str = "<!-- codemycel:start -->";
const END: &str = "<!-- codemycel:end -->";

pub const BLOCK: &str = "<!-- codemycel:start -->
## codemycel
This machine has codemycel, a verified knowledge graph of indexed repositories, exposed as MCP tools (search, symbol, impact, trace, overview, unresolved, status, note, notes).
- In an indexed repo, prefer `symbol`/`impact` over grep when you need callers, implementers, or blast radius; the graph is verified, counts are lower bounds.
- Before editing a symbol, run `impact` on it.
- If `status` says the repo is not indexed, you may run `codemycel init` or ask the user.
- Record durable findings with `note` so later sessions see them.
- Every answer ends with a freshness line; if files are stale, re-read those files.
<!-- codemycel:end -->";

/// The byte range of the block in `text`, end marker included.
fn find(text: &str) -> Option<(usize, usize)> {
    let start = text.find(START)?;
    let end = start + text[start..].find(END)? + END.len();
    Some((start, end))
}

/// `text` with the block replaced, or appended after one newline so that `remove` can take
/// exactly what was added.
pub fn insert(text: Option<&str>) -> String {
    let text = text.unwrap_or("");
    if let Some((start, end)) = find(text) {
        return format!("{}{BLOCK}{}", &text[..start], &text[end..]);
    }
    if text.is_empty() {
        format!("{BLOCK}\n")
    } else {
        format!("{text}\n{BLOCK}\n")
    }
}

/// `text` without the block, its trailing newline, and the newline before it unless that would
/// join two lines.
pub fn remove(text: &str) -> String {
    let Some((start, end)) = find(text) else {
        return text.to_owned();
    };
    let after = &text[end..];
    let after = after.strip_prefix('\n').unwrap_or(after);
    let before = &text[..start];
    let before = match before.strip_suffix('\n') {
        Some(stripped) if after.is_empty() || stripped.ends_with('\n') => stripped,
        _ => before,
    };
    format!("{before}{after}")
}

pub fn holds(text: Option<&str>) -> bool {
    text.and_then(find).is_some()
}
