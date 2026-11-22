//! The views of a design as plain data. A design keeps several views (Model, Sheet,
//! Render, Animation); this crate holds what they are, which one is shown and the saved form.
//! It knows no UI toolkit, no document and no app, so the commands, the tab strip, tiles and
//! windows all read the same [`ViewSet`], and a front end draws it however it likes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod tiles;
mod view_set;
mod window_options;

pub use tiles::{Axis, EMPTY, Edge, MAX_TILES, Placement, Preset, Rect, Splitter, Tiles};
pub use view_set::{
    CameraLinkMode, DesignView, MAX_NAME, MAX_POPOUTS, MAX_VIEWS, MoveError, Moved, PopOut, SketchSession, ViewKind, ViewSet, WindowArrangement,
    WindowId, WindowLevel, WindowPlacement, WindowTarget, window_rect_for,
};
pub use window_options::{CardHeaders, InterfaceSize, TopBar, WindowOptions};
