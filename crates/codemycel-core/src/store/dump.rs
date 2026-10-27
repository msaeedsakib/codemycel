use rusqlite::{Connection, Row};

use super::error::Result;

fn sorted_lines(
    conn: &Connection,
    sql: &str,
    line: impl FnMut(&Row<'_>) -> rusqlite::Result<String>,
) -> Result<String> {
    let mut stmt = conn.prepare_cached(sql)?;
    let mut lines = stmt
        .query_map([], line)?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    lines.sort();
    let mut out = String::new();
    for line in lines {
        out.push_str(&line);
        out.push('\n');
    }
    Ok(out)
}

fn line_range(start: Option<u32>, end: Option<u32>) -> String {
    match (start, end) {
        (Some(start), Some(end)) => format!("{start}-{end}"),
        _ => "-".to_owned(),
    }
}

pub(super) fn nodes(conn: &Connection) -> Result<String> {
    sorted_lines(
        conn,
        "SELECT semantic_path, kind, line_start, line_end FROM nodes",
        |row| {
            Ok(format!(
                "{}\t{}\t{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                line_range(row.get(2)?, row.get(3)?)
            ))
        },
    )
}

pub(super) fn edges(conn: &Connection) -> Result<String> {
    sorted_lines(
        conn,
        "SELECT s.semantic_path, e.kind, d.semantic_path, json_array_length(e.occurrences)
         FROM edges e JOIN nodes s ON s.id = e.src JOIN nodes d ON d.id = e.dst",
        |row| {
            Ok(format!(
                "{}\t{}\t{}\t{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?
            ))
        },
    )
}

pub(super) fn unresolved(conn: &Connection) -> Result<String> {
    sorted_lines(
        conn,
        "SELECT f.path, u.line, u.col, u.role, u.text, u.reason
         FROM unresolved u JOIN files f ON f.id = u.file_id",
        |row| {
            Ok(format!(
                "{}:{}:{}\t{}\t{}\t{}",
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
                row.get::<_, u32>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?
            ))
        },
    )
}

pub(super) fn impl_records(conn: &Connection) -> Result<String> {
    sorted_lines(
        conn,
        "SELECT f.path, i.self_type_text, i.trait_text, i.line_start, i.line_end
         FROM impl_records i JOIN files f ON f.id = i.file_id",
        |row| {
            Ok(format!(
                "{}\t{}\t{}\t{}",
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.as_deref().unwrap_or("-"),
                line_range(row.get(3)?, row.get(4)?)
            ))
        },
    )
}
