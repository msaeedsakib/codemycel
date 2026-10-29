mod embed;

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};

use crate::api;

pub const DEFAULT_PORT: u16 = 7111;

/// The web view bound to 127.0.0.1, serving the embedded bundle at `/` and the read-only API
/// under `/api` for one repository.
pub struct UiServer {
    listener: TcpListener,
    repo_root: PathBuf,
}

fn is_loopback_host(host: &str, port: u16) -> bool {
    let port_str = port.to_string();
    if let Some((name, host_port)) = host.rsplit_once(':') {
        if host_port == port_str {
            return name.eq_ignore_ascii_case("127.0.0.1")
                || name.eq_ignore_ascii_case("localhost")
                || name.eq_ignore_ascii_case("[::1]");
        }
    } else if port == 80 {
        return host.eq_ignore_ascii_case("127.0.0.1")
            || host.eq_ignore_ascii_case("localhost")
            || host.eq_ignore_ascii_case("[::1]");
    }
    false
}

async fn check_host(State(port): State<u16>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok());
    match host {
        Some(host) if is_loopback_host(host, port) => next.run(request).await,
        _ => StatusCode::FORBIDDEN.into_response(),
    }
}

impl UiServer {
    /// Binds `port`, or the first free port from `DEFAULT_PORT` when `port` is `None`.
    pub fn bind(repo_root: &Path, port: Option<u16>) -> io::Result<Self> {
        match port {
            Some(port) => Ok(Self {
                listener: TcpListener::bind((Ipv4Addr::LOCALHOST, port))?,
                repo_root: repo_root.to_owned(),
            }),
            None => Self::bind_from(repo_root, DEFAULT_PORT),
        }
    }

    /// Binds the first free port from `first` up.
    pub fn bind_from(repo_root: &Path, first: u16) -> io::Result<Self> {
        for port in first..=u16::MAX {
            match TcpListener::bind((Ipv4Addr::LOCALHOST, port)) {
                Ok(listener) => {
                    return Ok(Self {
                        listener,
                        repo_root: repo_root.to_owned(),
                    });
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::AddrInUse | io::ErrorKind::PermissionDenied
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AddrInUse,
            format!("no free port from {first}"),
        ))
    }

    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves until the process ends.
    pub fn serve(self) -> io::Result<()> {
        self.listener.set_nonblocking(true)?;
        let port = self.listener.local_addr()?.port();
        let app = Self::router(&self.repo_root, port);
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(async move {
                let listener = tokio::net::TcpListener::from_std(self.listener)?;
                axum::serve(listener, app).await
            })
    }

    pub fn router(repo_root: &Path, port: u16) -> Router {
        Router::new()
            .nest("/api", api::routes(repo_root))
            .fallback(embed::asset)
            .layer(middleware::from_fn_with_state(port, check_host))
    }
}
