mod embed;

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::path::{Path, PathBuf};

use axum::Router;

use crate::api;

pub const DEFAULT_PORT: u16 = 7111;

/// The web view bound to 127.0.0.1, serving the embedded bundle at `/` and the read-only API
/// under `/api` for one repository.
pub struct UiServer {
    listener: TcpListener,
    repo_root: PathBuf,
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
        let app = Self::router(&self.repo_root);
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(async move {
                let listener = tokio::net::TcpListener::from_std(self.listener)?;
                axum::serve(listener, app).await
            })
    }

    pub fn router(repo_root: &Path) -> Router {
        Router::new()
            .nest("/api", api::routes(repo_root))
            .fallback(embed::asset)
    }
}
