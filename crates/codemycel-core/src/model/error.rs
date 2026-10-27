use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ModelError {
    #[error("invalid repository path `{path}`: {reason}")]
    InvalidRepoPath { path: String, reason: &'static str },
    #[error("invalid semantic path `{text}`: {reason}")]
    InvalidSemanticPath { text: String, reason: &'static str },
    #[error("unknown {type_name} `{value}`")]
    UnknownVariant {
        type_name: &'static str,
        value: String,
    },
}
