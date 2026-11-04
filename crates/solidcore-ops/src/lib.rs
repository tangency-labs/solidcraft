//! # solidcore-ops
//!
//! The modelling operations applications built on solidcore call: each a sequence of kernel
//! calls, with the record converted through `solidcore-convert` (docs/layers.md). Ops chooses
//! (entities, parameters, defaults), sequences kernel calls, converts, and compares the numbers
//! the kernel returns with tolerances. It computes no geometric quantity of its own: no square
//! root, trigonometry, product, normalising or solve (clippy `disallowed-methods`, this crate's
//! `clippy.toml`). Arithmetic operators cannot be banned by clippy: review is the guard.
//!
//! What every family shares:
//! - [`session`]: the one process kernel, the fixes the application names, the panic guard, the
//!   call running now and the failure callback;
//! - [`Error`]: why an operation failed, as data (the application words it);
//! - [`Body`]: a kernel body, with primitives, copy and transform, counts and volume, `.smb`
//!   bytes ([`file`]) and the validity check ([`check`]);
//! - `finish`: the result contract, from what the input was (crate-internal: the families use it);
//! - [`provenance`]: the hook an application's construction history rides on.
//!
//! Operations change the body they are given, as the kernel does. An application that keeps
//! its input copies it first ([`Body::copy`]).
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]

pub mod body;
pub mod check;
pub mod error;
pub mod file;
pub mod offset;
pub mod provenance;
pub mod session;

pub use body::{make_box, make_cone, make_cylinder, make_sphere, make_torus, Body};
pub use check::{CheckLevel, CheckReport};
pub use error::{Error, FitKind, Result, RoomReason};
pub use offset::{offset_faces, offset_sheet, shell, thicken};

#[cfg(test)]
mod tests;
