//! MCP server machinery any app may share: JSON-RPC 2.0 over stdio (newline-delimited), the MCP
//! lifecycle, the tools and resources methods, tool schemas with the check of a call's arguments
//! against them, and the content blocks tools reply with (text, images).
//!
//! An app implements [`Tools`]: what its tools are, how each one runs, its instructions and its
//! resources. [`Server`] speaks the protocol around them. Nothing here knows any app's design,
//! engine or renderer.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod content;
mod schema;
mod server;

pub use content::{base64, image_result, text_result};
pub use schema::{Effect, check_arguments, object, string, tool};
pub use server::{Info, PROTOCOL_VERSION, SUPPORTED_VERSIONS, Server, Tools};

#[cfg(test)]
mod tests;
