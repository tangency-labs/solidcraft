//! The views of a design as plain data. A design keeps several views (Model, Sketch, Sheet,
//! Render, Animation); this crate holds what they are, which one is shown and the saved form.
//! It knows no UI toolkit, no document and no app, so the commands, the tab strip, tiles and
//! windows all read the same [`ViewSet`], and a front end draws it however it likes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod tiles;
mod view_set;

pub use tiles::{Axis, Edge, MAX_TILES, Preset, Rect, Splitter, Tiles};
pub use view_set::{DesignView, MAX_NAME, MAX_VIEWS, ViewKind, ViewSet};
