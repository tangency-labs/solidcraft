//! Opt-in common panel runtime and retained production content bindings.
//! Model/commands, controller state, presentation and layout have separate owners.
mod app;
mod fixtures;
mod plugin;
mod preferences;
mod production;
mod specimen;

pub use app::*;
pub use fixtures::*;
pub(crate) use plugin::queue_selection;
pub(crate) use preferences::PanelLayoutSnapshot;
pub(crate) use preferences::panel_layout_matches;
pub(crate) use preferences::panel_layout_snapshot;
pub use preferences::*;
pub(crate) use production::PanelVisibilitySnapshot;
pub(crate) use production::apply_placement_command;
pub(crate) use production::place_assistant;
pub(crate) use production::refresh_production;
pub(crate) use production::register_content_focus;
pub(crate) use production::report_content_composer_height;
pub(crate) use production::report_content_extent;
pub(crate) use production::shared_content_composer_height;
pub(crate) use production::shared_content_measurement;
pub use solidcraft_ui_kit::panel_runtime::*;
pub use specimen::*;

#[cfg(test)]
mod tests;
