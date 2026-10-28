//! MCP server over stdio.

mod instructions;
mod server;
mod session;
mod tools;

pub use server::serve;
