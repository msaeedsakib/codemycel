use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use codemycel_index::SessionOptions;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{ServerHandler, ServiceExt, tool_handler};

use crate::instructions::INSTRUCTIONS;
use crate::session::Session;

const READY_POLL: Duration = Duration::from_secs(1);

#[derive(Clone)]
pub struct Codemycel {
    pub(crate) session: Arc<Session>,
    tool_router: ToolRouter<Self>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Codemycel {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("codemycel", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// Serves the MCP tools for the repository at `repo_root` over stdio until stdin closes or
/// SIGTERM arrives, then stops the session's watcher and servers and releases its locks.
pub fn serve(repo_root: &Path, options: SessionOptions) -> std::io::Result<()> {
    let session = Arc::new(Session::new(repo_root, options));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(run(session.clone()));
    runtime.shutdown_timeout(Duration::from_secs(1));
    session.stop();
    result
}

async fn run(session: Arc<Session>) -> std::io::Result<()> {
    let poll = tokio::spawn(wait_for_index(session.clone()));
    let server = Codemycel {
        session,
        tool_router: Codemycel::tool_router(),
    };
    let result = match server.serve(rmcp::transport::stdio_pipes()).await {
        Ok(running) => {
            let cancel = running.cancellation_token();
            tokio::select! {
                quit = running.waiting() => quit.map(drop).map_err(std::io::Error::other),
                () = terminated() => {
                    cancel.cancel();
                    Ok(())
                }
            }
        }
        Err(error) => Err(std::io::Error::other(error)),
    };
    poll.abort();
    result
}

/// Resolves on SIGTERM (Ctrl-C on Windows); never, if the handler cannot be installed.
async fn terminated() {
    #[cfg(unix)]
    if let Ok(mut terminate) =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    {
        terminate.recv().await;
        return;
    }
    #[cfg(windows)]
    if tokio::signal::ctrl_c().await.is_ok() {
        return;
    }
    std::future::pending::<()>().await;
}

/// Starts the session as soon as the index is ready, without waiting for a tool call.
async fn wait_for_index(session: Arc<Session>) {
    let mut interval = tokio::time::interval(READY_POLL);
    while !session.is_running() {
        interval.tick().await;
        let session = session.clone();
        let _ = tokio::task::spawn_blocking(move || session.refresh()).await;
    }
}
