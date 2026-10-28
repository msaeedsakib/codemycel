//! LSP client, server provisioning, and the language-server resolver.

pub mod adapters;
pub mod client;
pub mod enrich;
pub mod provision;

pub use client::LspClient;
pub use enrich::{
    EnrichOptions, EnrichReport, Enricher, FileView, IndexView, LspResolver, Readiness,
    ServerLaunch, Session,
};
pub use provision::{Located, NoServer, Provisioner, Rejected};
