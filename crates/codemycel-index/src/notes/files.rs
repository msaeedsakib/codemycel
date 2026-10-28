use codemycel_core::{Note, NoteId, NoteKind};
use thiserror::Error;

const DELIMITER: &str = "---";

#[derive(Debug, Error, PartialEq, Eq)]
pub(crate) enum NoteFileError {
    #[error("the file does not start with a `---` front matter line")]
    NoFrontMatter,
    #[error("the front matter has no closing `---` line")]
    Unterminated,
    #[error("front matter line {0} is not `key: value`")]
    NotAKeyValue(usize),
    #[error("`{0}` appears twice in the front matter")]
    Duplicate(String),
    #[error("the front matter has no `{0}`")]
    Missing(&'static str),
    #[error("`{key}` is invalid: {reason}")]
    Invalid { key: &'static str, reason: String },
}

/// The file name of the note with this uuid: `<uuid>.md`.
pub(crate) fn file_name(uuid: &NoteId) -> String {
    format!("{}.md", uuid.0)
}

/// The uuid a note file is named after, when `file_name` is `<uuid>.md` with the uuid in its
/// lowercase hyphenated form.
pub(crate) fn uuid_of(file_name: &str) -> Option<NoteId> {
    let stem = file_name.strip_suffix(".md")?;
    let uuid = uuid::Uuid::parse_str(stem).ok()?;
    (uuid.hyphenated().to_string() == stem).then(|| NoteId(stem.to_owned()))
}

pub(crate) fn is_uuid(id: &NoteId) -> bool {
    uuid_of(&file_name(id)).is_some()
}

/// The note file: YAML front matter with the target as a double-quoted string, then the body
/// and a final newline.
pub(crate) fn render(note: &Note) -> String {
    let target = serde_json::Value::String(note.target_semantic_path.clone()).to_string();
    format!(
        "{DELIMITER}\ntarget: {target}\nkind: {}\ncreated: {}\nupdated: {}\n{DELIMITER}\n{}\n",
        note.kind, note.created, note.updated, note.body
    )
}

/// Reads a note file. Values may be plain, single-quoted, or double-quoted YAML scalars; unknown
/// keys are ignored. A file with CRLF line endings is read as if it had LF endings.
pub(crate) fn parse(uuid: NoteId, text: &str) -> Result<Note, NoteFileError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalised;
    let text = if text.starts_with("---\r\n") {
        normalised = text.replace("\r\n", "\n");
        normalised.as_str()
    } else {
        text
    };
    let rest = text
        .strip_prefix("---\n")
        .ok_or(NoteFileError::NoFrontMatter)?;
    let mut offset = 0;
    let mut body = None;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end_matches('\n') == DELIMITER {
            body = Some(&rest[offset..]);
            break;
        }
    }
    let body = body.ok_or(NoteFileError::Unterminated)?;
    let front = &rest[..offset];
    let mut target = None;
    let mut kind = None;
    let mut created = None;
    let mut updated = None;
    for (number, line) in front.lines().enumerate() {
        let line = line.trim_end();
        if line == DELIMITER || line.trim_start().is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or(NoteFileError::NotAKeyValue(number + 2))?;
        let (key, slot) = match key.trim() {
            "target" => ("target", &mut target),
            "kind" => ("kind", &mut kind),
            "created" => ("created", &mut created),
            "updated" => ("updated", &mut updated),
            _ => continue,
        };
        if slot.is_some() {
            return Err(NoteFileError::Duplicate(key.to_owned()));
        }
        *slot = Some(scalar(key, value.trim())?);
    }
    let target = target.ok_or(NoteFileError::Missing("target"))?;
    if target.is_empty() {
        return Err(invalid("target", "empty".to_owned()));
    }
    let kind: NoteKind = kind
        .ok_or(NoteFileError::Missing("kind"))?
        .parse()
        .map_err(|e: codemycel_core::ModelError| invalid("kind", e.to_string()))?;
    let created = timestamp("created", created)?;
    let updated = timestamp("updated", updated)?;
    Ok(Note {
        uuid,
        target_semantic_path: target,
        kind,
        body: body.strip_suffix('\n').unwrap_or(body).to_owned(),
        created,
        updated,
    })
}

fn timestamp(key: &'static str, value: Option<String>) -> Result<String, NoteFileError> {
    let value = value.ok_or(NoteFileError::Missing(key))?;
    value
        .parse::<jiff::Timestamp>()
        .map_err(|e| invalid(key, e.to_string()))?;
    Ok(value)
}

fn scalar(key: &'static str, value: &str) -> Result<String, NoteFileError> {
    if value.starts_with('"') {
        return serde_json::from_str::<String>(value).map_err(|e| invalid(key, e.to_string()));
    }
    if let Some(quoted) = value.strip_prefix('\'') {
        let inner = quoted
            .strip_suffix('\'')
            .ok_or_else(|| invalid(key, "unterminated single quote".to_owned()))?;
        return Ok(inner.replace("''", "'"));
    }
    let plain = match value.find(" #") {
        Some(comment) => &value[..comment],
        None => value,
    };
    Ok(plain.trim_end().to_owned())
}

fn invalid(key: &'static str, reason: String) -> NoteFileError {
    NoteFileError::Invalid { key, reason }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(target: &str, body: &str) -> Note {
        Note {
            uuid: NoteId("0b9d3f1e-6c1a-4d8e-9f00-2a3b4c5d6e7f".to_owned()),
            target_semantic_path: target.to_owned(),
            kind: NoteKind::Note,
            body: body.to_owned(),
            created: "2026-10-28T09:00:00Z".to_owned(),
            updated: "2026-10-28T10:30:00Z".to_owned(),
        }
    }

    #[test]
    fn awkward_targets_and_bodies_round_trip() {
        for target in [
            "src/a.rs::<Foo as Display>::fmt",
            "docs/guide.md#set-up[1]",
            r#"src/a.ts::Obj::"weird \" name""#,
            "community:1a2b3c4d",
            "it's: a # path\\with\tescapes",
            "repo",
        ] {
            for body in [
                "",
                "one line",
                "---\nnot front matter\n",
                "trailing\n\n",
                "a\r\nb",
            ] {
                let original = note(target, body);
                let text = render(&original);
                assert_eq!(parse(original.uuid.clone(), &text).unwrap(), original);
            }
        }
    }

    #[test]
    fn hand_written_crlf_file_with_plain_and_single_quoted_values_is_read() {
        let text = "---\r\ntarget: 'src/a.rs::it''s'\r\nkind: label # display\r\ncreated: 2026-10-28T09:00:00Z\r\nupdated: 2026-10-28T09:00:00+02:00\r\nauthor: ignored\r\n---\r\nline one\r\nline two\r\n";
        let parsed = parse(NoteId("x".to_owned()), text).unwrap();
        assert_eq!(parsed.target_semantic_path, "src/a.rs::it's");
        assert_eq!(parsed.kind, NoteKind::Label);
        assert_eq!(parsed.updated, "2026-10-28T09:00:00+02:00");
        assert_eq!(parsed.body, "line one\nline two");
    }

    #[test]
    fn malformed_front_matter_is_rejected() {
        let id = || NoteId("x".to_owned());
        assert_eq!(
            parse(id(), "no front matter"),
            Err(NoteFileError::NoFrontMatter)
        );
        assert_eq!(
            parse(id(), "---\ntarget: a\n"),
            Err(NoteFileError::Unterminated)
        );
        let missing_kind =
            "---\ntarget: a\ncreated: 2026-10-28T09:00:00Z\nupdated: 2026-10-28T09:00:00Z\n---\n";
        assert_eq!(
            parse(id(), missing_kind),
            Err(NoteFileError::Missing("kind"))
        );
        let bad_time =
            "---\ntarget: a\nkind: note\ncreated: yesterday\nupdated: 2026-10-28T09:00:00Z\n---\n";
        assert!(matches!(
            parse(id(), bad_time),
            Err(NoteFileError::Invalid { key: "created", .. })
        ));
    }

    #[test]
    fn only_canonical_uuid_file_names_are_notes() {
        assert!(uuid_of("0b9d3f1e-6c1a-4d8e-9f00-2a3b4c5d6e7f.md").is_some());
        assert!(uuid_of("0B9D3F1E-6C1A-4D8E-9F00-2A3B4C5D6E7F.md").is_none());
        assert!(uuid_of("0b9d3f1e-6c1a-4d8e-9f00-2a3b4c5d6e7f.md.tmp").is_none());
        assert!(uuid_of("../escape.md").is_none());
    }
}
