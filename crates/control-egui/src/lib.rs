//! The window's own control requests (docs/control-protocol.md) any app answers alike over egui:
//! `ui.screenshot` ([`screenshot::Shots`]) and `ui.resize` ([`window::resize`]). The token, the
//! server and the requests themselves are `solidcraft-control`, which needs no UI toolkit.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod screenshot;
pub mod window;
