//! The control channel any app may share (docs/control-protocol.md): the per-launch token and its
//! files ([`auth`]), the loopback server that admits a connection by the token and hands each
//! request to the window ([`server`]), the request the window answers ([`ControlRequest`]), the
//! document watch subscribed connections wait on ([`DocumentWatch`]), and what a request still running
//! does when it finishes ([`Effect`]), and the window's own requests any app answers alike over
//! egui: `ui.screenshot` ([`screenshot::Shots`]) and `ui.resize` ([`window::resize`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auth;
mod effect;
mod request;
pub mod screenshot;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;
pub mod window;

pub use effect::Effect;
pub use request::{ControlRequest, ControlResponse, DOCUMENT_WATCH, DocumentWatch, Subscriber, cancelled_chat_response, chat_turn_cancelled};
