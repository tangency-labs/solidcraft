//! The control channel any app may share (docs/control-protocol.md): the per-launch token and its
//! files ([`auth`]), and what a request still running does when it finishes ([`Effect`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod auth;
mod effect;

pub use effect::Effect;
