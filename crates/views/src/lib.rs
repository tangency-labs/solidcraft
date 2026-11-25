//! The views of a design as plain data. A design keeps several views (Model, Sheet,
//! Render, Animation); this crate holds what they are, which one is shown and the saved form.
//! It knows no UI toolkit, no document and no app, so the commands, the tab strip, tiles and
//! windows all read the same [`ViewSet`], and a front end draws it however it likes.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod tiles;
mod view_set;
mod window_options;

pub use tiles::Axis;
pub use tiles::EMPTY;
pub use tiles::Edge;
pub use tiles::MAX_TILES;
pub use tiles::Placement;
pub use tiles::Preset;
pub use tiles::Rect;
pub use tiles::Splitter;
pub use tiles::Tiles;
pub use view_set::CameraLinkMode;
pub use view_set::DesignView;
pub use view_set::MAX_NAME;
pub use view_set::MAX_POPOUTS;
pub use view_set::MAX_VIEWS;
pub use view_set::MoveError;
pub use view_set::Moved;
pub use view_set::PopOut;
pub use view_set::SketchSession;
pub use view_set::ViewKind;
pub use view_set::ViewSet;
pub use view_set::WindowArrangement;
pub use view_set::WindowId;
pub use view_set::WindowLevel;
pub use view_set::WindowPlacement;
pub use view_set::WindowTarget;
pub use view_set::window_rect_for;
pub use window_options::CardHeaders;
pub use window_options::InterfaceSize;
pub use window_options::TopBar;
pub use window_options::WindowOptions;
