use std::str::FromStr;

use rusqlite::Row;
use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

use crate::{
    CandidateRole, Coverage, DegradeReason, EdgeKind, FileKind, LabelSource, Language, NodeKind,
    NoteKind, Occurrence, RepoPath, SemanticPath, ServerSource, SkipReason, UnresolvedReason,
    VerifiedBy,
};

fn parse_text<T: FromStr>(value: ValueRef<'_>) -> FromSqlResult<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value
        .as_str()?
        .parse()
        .map_err(|e| FromSqlError::Other(Box::new(e)))
}

macro_rules! text_column {
    ($($ty:ty => $text:expr),+ $(,)?) => {$(
        impl ToSql for $ty {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                let text: &str = $text(self);
                Ok(ToSqlOutput::from(text))
            }
        }

        impl FromSql for $ty {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                parse_text(value)
            }
        }
    )+};
}

impl ToSql for RepoPath {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for RepoPath {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        RepoPath::new(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

text_column!(
    SemanticPath => SemanticPath::as_str,
    Language => |v: &Language| v.as_str(),
    NodeKind => |v: &NodeKind| v.as_str(),
    EdgeKind => |v: &EdgeKind| v.as_str(),
    VerifiedBy => |v: &VerifiedBy| v.as_str(),
    CandidateRole => |v: &CandidateRole| v.as_str(),
    UnresolvedReason => |v: &UnresolvedReason| v.as_str(),
    FileKind => |v: &FileKind| v.as_str(),
    SkipReason => |v: &SkipReason| v.as_str(),
    NoteKind => |v: &NoteKind| v.as_str(),
    LabelSource => |v: &LabelSource| v.as_str(),
    DegradeReason => |v: &DegradeReason| v.as_str(),
    ServerSource => |v: &ServerSource| v.as_str(),
    Coverage => |v: &Coverage| v.as_str(),
);

pub(crate) fn hash_at(row: &Row<'_>, idx: usize) -> rusqlite::Result<Option<blake3::Hash>> {
    let Some(bytes) = row.get::<_, Option<Vec<u8>>>(idx)? else {
        return Ok(None);
    };
    let array: [u8; 32] = bytes.try_into().map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            idx,
            rusqlite::types::Type::Blob,
            "hash is not 32 bytes".into(),
        )
    })?;
    Ok(Some(blake3::Hash::from_bytes(array)))
}

pub(crate) fn occurrences_at(row: &Row<'_>, idx: usize) -> rusqlite::Result<Vec<Occurrence>> {
    let text: String = row.get(idx)?;
    parse_json(idx, &text)
}

pub(crate) fn json_at<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<T>> {
    row.get::<_, Option<String>>(idx)?
        .map(|text| parse_json(idx, &text))
        .transpose()
}

fn parse_json<T: serde::de::DeserializeOwned>(idx: usize, text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}
