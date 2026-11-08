//! The control channel any app may share (docs/control-protocol.md): the per-launch token and its
//! files ([`auth`]), the loopback server that admits a connection by the token and hands each
//! request to the window ([`server`]), the request the window answers ([`ControlRequest`]), the
//! design watch subscribed connections wait on ([`DesignWatch`]), and what a request still running
//! does when it finishes ([`Effect`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auth;
mod effect;
mod request;
#[cfg(not(target_arch = "wasm32"))]
pub mod server;

pub use effect::Effect;
pub use request::{ControlRequest, ControlResponse, DESIGN_WATCH, DesignWatch, Subscriber, cancelled_chat_response, chat_turn_cancelled};
