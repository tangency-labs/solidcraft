//! Opt-in common panel runtime and retained production content bindings.
//! Model/commands, controller state, presentation and layout have separate owners.
mod app;
mod drag_paint;
mod fixtures;
mod host;
mod interaction;
mod layout;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
mod native_ui;
mod preferences;
mod production;
mod runtime;
mod specimen;
mod surfaces;
mod workspace;

pub use app::*;
pub use fixtures::*;
pub use host::*;
pub use interaction::*;
pub use layout::*;
#[cfg(not(target_arch = "wasm32"))]
pub use native::NativeMonitor;
pub use preferences::*;
pub(crate) use preferences::{PanelLayoutSnapshot, panel_layout_matches, panel_layout_snapshot};
pub(crate) use production::{
    PanelVisibilitySnapshot, apply_placement_command, place_assistant, refresh_production, register_content_focus, report_content_composer_height,
    report_content_extent, shared_content_composer_height, shared_content_measurement,
};
pub use runtime::*;
pub use specimen::*;
pub use surfaces::*;
pub use workspace::*;

#[cfg(test)]
mod tests;
