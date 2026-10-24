//! Tokenised dense card library. Import elements through `crate::ui_kit`.
//! `Panel` owns card composition; rows own aligned label/control columns and retain egui focus.
//! Geometry, typography and colours come from `theme::Tokens`. See `docs/ui-kit.md`.
mod actions;
mod assistant;
mod assistant_entry;
mod checks;
mod choice;
mod color;
mod confirm;
pub mod container;
mod disclosure;
mod fields;
mod file_view;
mod history;
mod layout;
mod look_chain;
mod menu;
mod metrics;
pub(crate) mod panel;
mod preferences;
mod preferences_assistant;
mod preferences_figures;
mod preferences_navigation;
mod preferences_small;
mod radial;
mod reports;
mod search;
mod shortcuts;
mod sphere;
mod status;
mod table;
mod text;

pub(crate) use actions::*;
pub use actions::{Button, button};
pub(crate) use assistant::*;
pub(crate) use assistant_entry::*;
pub(crate) use checks::*;
pub(crate) use choice::*;
pub(crate) use color::*;
pub(crate) use confirm::{Confirm, Tone};
pub use container::{Container, DockEdges, Edge, Surface};
pub(crate) use disclosure::*;
pub use fields::cell_editor;
pub(crate) use fields::*;
pub(crate) use file_view::{FileView, RecentRow, RecoveryMessage, SampleCard};
pub(crate) use history::*;
pub(crate) use layout::*;
pub use look_chain::{LookStep, look_chain};
pub(crate) use menu::{Menu, MenuItem, rename_menu, workspace_menu};
pub(crate) use metrics::{ContentWidth, header_height, row_height, table_row_height};
pub use panel::Window;
pub(crate) use panel::{ComputeStatus, FeatureOptions, Panel, Part, WindowPage, panel_header_hover};
#[cfg(test)]
pub(crate) use panel::{collapsible_header, header};
pub(crate) use preferences::*;
pub(crate) use preferences_assistant::*;
pub(crate) use preferences_navigation::*;
pub(crate) use preferences_small::*;
pub(crate) use radial::{RADIAL_RADIUS, radial_menu};
pub(crate) use reports::*;
pub(crate) use search::{CommandSearch, SearchBadge, SearchFieldGeometry};
pub(crate) use shortcuts::*;
pub use sphere::{FinishKind, Sphere, sphere_thumbnail};
pub(crate) use status::*;
pub(crate) use table::*;

#[cfg(test)]
mod gallery;
#[cfg(test)]
mod slider_tests;
#[cfg(test)]
#[path = "test_support_tests.rs"]
mod test_support;

pub(crate) use layout::label as row_label;

/// Whether `need` points fit in `have`, answered so that a scrollbar coming and going (it takes
/// `ui.spacing().scroll.allocated_width()` of the width) does not flip it: what has fitted keeps
/// fitting until it misses by more than the scrollbar's width, and what has not fitted must fit
/// outright. With nothing remembered yet it is the generous answer, so the first frame with a
/// scrollbar and the first without agree. Keyed by `key` within `ui`.
pub(crate) fn fits_steadily(ui: &egui::Ui, key: impl std::hash::Hash + std::fmt::Debug, need: f32, have: f32) -> bool {
    let slack = ui.spacing().scroll.allocated_width();
    let id = ui.make_persistent_id(("fits_steadily", key));
    let was = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(true);
    let fits = need <= have + if was { slack } else { 0.0 };
    ui.data_mut(|data| data.insert_temp(id, fits));
    fits
}
