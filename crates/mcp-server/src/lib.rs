//! MCP server machinery any app may share: JSON-RPC 2.0 over stdio (newline-delimited), the MCP
//! lifecycle, the tools, resources, prompts and completion methods, tool schemas with the check of
//! a call's arguments against them, and the content blocks tools reply with (text, images).
//!
//! An app implements [`Tools`]: what its tools are, how each one runs, its instructions, and the
//! resources, resource templates and prompts it serves, if any. [`Server`] speaks the protocol
//! around them and tells a client at `initialize` only what the app serves. Nothing here knows any
//! app's design, engine or renderer.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod content;
mod schema;
mod server;

pub use content::Contents;
pub use content::base64;
pub use content::image_result;
pub use content::text_result;
pub use schema::Effect;
pub use schema::check_arguments;
pub use schema::object;
pub use schema::string;
pub use schema::tool;
pub use server::Cancelled;
pub use server::Incoming;
pub use server::Info;
pub use server::PROTOCOL_VERSION;
pub use server::ProgressReporter;
pub use server::Reference;
pub use server::SUPPORTED_VERSIONS;
pub use server::Server;
pub use server::Tools;
pub use server::cancelled_notification;
pub use server::progress_notification;
pub use server::progress_token;
pub use server::read_message;
pub use server::response;

#[cfg(test)]
mod tests;
