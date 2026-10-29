mod routes;

use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use codemycel_core::query::{Answer, JsonFormatter, QueryEngine, QueryError};
use codemycel_core::store::{SCHEMA_VERSION, Store, StoreError};
use codemycel_index::{Registry, RepoLocation};
use serde_json::{Value, json};
use thiserror::Error;

pub use routes::routes;

/// Why a request got no answer. Every variant renders as a JSON object with `outcome` and
/// `message`, as the CLI's `--json` does.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("no index: run codemycel init in {repo}")]
    NoIndex { repo: String },
    #[error(
        "index schema {found} is out of date (expected {SCHEMA_VERSION}): run codemycel init in {repo}"
    )]
    OutOfDate { repo: String, found: String },
    #[error(transparent)]
    Query(#[from] QueryError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("the request handler stopped: {0}")]
    Join(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, outcome) = match &self {
            ApiError::NoIndex { .. } => (StatusCode::SERVICE_UNAVAILABLE, "no_index"),
            ApiError::OutOfDate { .. } => (StatusCode::SERVICE_UNAVAILABLE, "out_of_date"),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "error"),
        };
        let body = json!({ "outcome": outcome, "message": self.to_string() });
        (status, Json(body)).into_response()
    }
}

/// The repository the API answers for. Every request opens its index read-only afresh, so the
/// answers follow syncs made by other processes.
#[derive(Clone)]
struct Repo(Arc<RepoLocation>);

impl Repo {
    fn open(&self) -> Result<Store, ApiError> {
        let repo = self.0.root.display().to_string();
        let db = self.0.index_path();
        if !db.exists() {
            return Err(ApiError::NoIndex { repo });
        }
        match Store::open(&db, true) {
            Ok(store) => Ok(store),
            Err(StoreError::SchemaMismatch { found, .. }) => {
                Err(ApiError::OutOfDate { repo, found })
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Runs `read` on the index off the async workers.
    async fn read<T: Send + 'static>(
        &self,
        read: impl FnOnce(&Repo, Store) -> Result<T, ApiError> + Send + 'static,
    ) -> Result<T, ApiError> {
        let repo = self.clone();
        tokio::task::spawn_blocking(move || read(&repo, repo.open()?))
            .await
            .map_err(|error| ApiError::Join(error.to_string()))?
    }

    /// Runs one query tool and renders its answer with `JsonFormatter`.
    async fn answer(
        &self,
        run: impl FnOnce(&QueryEngine<'_>) -> Result<Answer, QueryError> + Send + 'static,
    ) -> Result<Json<Value>, ApiError> {
        self.read(|repo, store| {
            let registry = Registry::default();
            let engine = QueryEngine::open(&repo.0.root, store, registry.adapters());
            Ok(Json(JsonFormatter::render(&run(&engine)?)))
        })
        .await
    }
}
