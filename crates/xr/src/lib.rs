//! SolidCraft in a VR headset (#2172), through OpenXR.
//!
//! - [`view`]: where the window and the model are in the room, and how each eye sees them. Plain
//!   maths, built and tested everywhere.
//! - [`plan`]: what a session decides from what the runtime reports (picture format and sizes,
//!   what each session state asks for). Plain values, tested without a headset.
//! - [`Runtime`] and [`Session`] (Windows and Linux): the OpenXR session itself, on a Vulkan
//!   device the runtime creates and the window draws with too.
//!
//! The crate knows nothing of the viewport's renderer or the UI: the app draws the pictures a
//! [`Session`] hands out.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(unsafe_code)]

pub mod plan;
pub mod view;

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod session;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use session::{Error, Frame, Gpu, Picture, Runtime, Session, Status, WindowLayer};
