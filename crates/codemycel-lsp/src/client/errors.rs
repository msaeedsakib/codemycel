use std::io;
use std::time::Duration;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("starting the server: {0}")]
    Spawn(#[source] io::Error),
    #[error("the server closed its connection")]
    Closed,
    #[error("request timed out after {0:?}")]
    Timeout(Duration),
    #[error("server error {code}: {message}")]
    Response { code: i64, message: String },
    #[error("malformed message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cannot express {0} as a file URI")]
    Uri(String),
}
