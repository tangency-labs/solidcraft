//! SolidCraft in a VR headset (#2172), through OpenXR.
//!
//! - [`view`]: where the model is in the room, and how each eye sees it. Plain
//!   maths, built and tested everywhere.
//! - [`input`]: the controllers, what the viewer reads from them and the bindings it suggests.
//!   Plain values, tested without a headset.
//! - [`plan`]: what a session decides from what the runtime reports (picture format and sizes,
//!   what each session state asks for). Plain values, tested without a headset.
//! - [`installed`] (Windows and Linux): whether this computer has OpenXR, from files and settings
//!   alone, for a run that does not start VR.
//! - [`Runtime`] and [`Session`] (Windows and Linux): the OpenXR session itself, on a Vulkan
//!   device the runtime creates and the window draws with too.
//!
//! The crate knows nothing of the viewport's renderer or the UI: the app draws the pictures a
//! [`Session`] hands out.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(unsafe_code)]

pub mod input;
pub mod plan;
pub mod view;

#[cfg(any(target_os = "windows", target_os = "linux"))]
mod installed;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod session;

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use installed::{Found, installed};

#[cfg(any(target_os = "windows", target_os = "linux"))]
pub use session::{Error, Frame, Gpu, ModelAsset, ModelPose, Picture, Runtime, Session, Status};
