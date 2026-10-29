//! Web view: the Leptos front end and the loopback axum server that serves it.

pub mod canvas;
pub mod graph;

#[cfg(not(target_arch = "wasm32"))]
pub mod api;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

#[cfg(target_arch = "wasm32")]
pub mod app;

#[cfg(not(target_arch = "wasm32"))]
pub use server::{DEFAULT_PORT, UiServer};
