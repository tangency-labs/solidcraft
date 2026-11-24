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
mod plugin;
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
pub(crate) use plugin::queue_selection;
pub(crate) use preferences::PanelLayoutSnapshot;
pub(crate) use preferences::panel_layout_matches;
pub(crate) use preferences::panel_layout_snapshot;
pub use preferences::*;
pub(crate) use production::PanelVisibilitySnapshot;
pub(crate) use production::SizedPanel;
pub(crate) use production::apply_placement_command;
pub(crate) use production::bottom_band_height;
pub(crate) use production::place_assistant;
pub(crate) use production::refresh_production;
pub(crate) use production::register_content_focus;
pub(crate) use production::report_content_composer_height;
pub(crate) use production::report_content_extent;
pub(crate) use production::shared_content_composer_height;
pub(crate) use production::shared_content_measurement;
pub(crate) use production::sized_panels;
pub use runtime::*;
pub use specimen::*;
pub use surfaces::*;
pub use workspace::*;

#[cfg(test)]
mod tests;
