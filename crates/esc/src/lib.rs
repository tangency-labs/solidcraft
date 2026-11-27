//! esc, editable SolidCraft: designs written as text files that agents edit directly and the
//! GUI edits through intents. This crate reads and writes the text: the lossless parser, the one
//! canonical layout, the checks that need no geometry, and the edit intents.
//!
//! Input is hostile: reads are bounded, nesting is capped, and every problem is a [`Diagnostic`]
//! with a stable code.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod check;
pub mod diagnostic;
pub mod edit;
pub mod fmt;
pub mod model;
pub mod parse;
pub mod project;
pub mod syntax;
pub mod units;
pub mod vocabulary;

pub use diagnostic::{Code, Diagnostic, Located};
