//! Opt-in common panel runtime and retained production content bindings.
//! Model/commands, controller state, presentation and layout have separate owners.
mod app;
mod fixtures;
mod host;
mod interaction;
mod layout;
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
pub use preferences::*;
pub(crate) use preferences::{PanelLayoutSnapshot, panel_layout_matches, panel_layout_snapshot};
pub(crate) use production::{
    production_active, refresh_production, register_content_focus, report_content_composer_height, report_content_extent, reveal_render_library,
    shared_content_composer_height, shared_content_measurement,
};
pub use runtime::*;
pub use specimen::*;
pub use surfaces::*;
pub use workspace::*;

#[cfg(test)]
mod tests;
