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
mod details;
mod disclosure;
mod drag;
mod fields;
mod file_view;
mod framing;
mod history;
mod layout;
mod look_chain;
mod menu;
mod metrics;
mod name_list;
mod navigation;
mod navigation_menu;
mod navigation_settings;
pub(crate) mod panel;
mod preferences;
mod preferences_assistant;
#[allow(dead_code, reason = "composed by the Preferences pages of #9875 to #9883")]
mod preferences_cards;
#[allow(dead_code, reason = "painted by the Preferences pages of #9875 to #9883")]
mod preferences_figure_data;
#[allow(dead_code, reason = "painted by the Preferences pages of #9875 to #9883")]
mod preferences_figure_ops;
mod preferences_figures;
mod preferences_layout;
mod preferences_navigation;
mod preferences_small;
mod radial;
mod reports;
mod review;
mod search;
mod shortcuts;
mod sphere;
mod stages;
mod status;
mod table;
mod text;
mod text_menu;
mod tree;
mod value_focus;
mod viewport_status;

pub(crate) use actions::*;
pub use actions::{Button, button};
pub(crate) use assistant::*;
pub(crate) use assistant_entry::*;
#[cfg(test)]
pub(crate) use checks::tests::{raster as raster_ui, textures_update as update_test_textures};
#[cfg(test)]
pub(crate) use checks::tests::{raster as test_raster, textures_update as test_textures_update};
pub(crate) use checks::*;
pub(crate) use choice::*;
pub(crate) use color::*;
pub(crate) use confirm::{Confirm, Tone};
pub use container::{Container, DockEdges, Edge, Surface};
pub(crate) use details::*;
pub(crate) use disclosure::*;
pub(crate) use drag::*;
pub(crate) use fields::*;
pub use fields::{cell_editor, wide_cell_editor};
pub(crate) use file_view::{FileView, RecentRow, RecoveryMessage, SampleCard};
pub(crate) use framing::*;
pub(crate) use history::*;
pub(crate) use layout::*;
pub use look_chain::{LookStep, look_chain};
pub(crate) use menu::{Menu, MenuItem, rename_menu, workspace_menu};
pub(crate) use metrics::{ContentWidth, header_height, row_height, table_row_height};
pub use name_list::{NameList, NameRow, name_list};
pub(crate) use navigation::*;
pub(crate) use navigation_menu::*;
pub(crate) use navigation_settings::*;
pub(crate) use panel::{
    ComputeStatus, FeatureOptions, InspectorAction, Panel, Part, WindowPage, WorkspaceOptions, inspector_header, panel_header_hover,
};
pub use panel::{DialogButtons, Window};
#[cfg(test)]
pub(crate) use panel::{collapsible_header, header};
pub(crate) use preferences::*;
pub(crate) use preferences_assistant::*;
#[allow(unused_imports, reason = "composed by the Preferences pages of #9875 to #9883")]
pub(crate) use preferences_cards::*;
pub(crate) use preferences_figure_data::{
    paint_aa_2x, paint_aa_4x, paint_aa_8x, paint_aa_off, paint_ao_on, paint_browser_card, paint_browser_column, paint_frame_rate,
    paint_history_bottom, paint_history_in_browser, paint_history_left, paint_history_under_browser, paint_quick_measure, paint_tess_coarse,
    paint_tess_fine, paint_tess_normal, paint_tess_ultra, paint_vertex_markers, paint_view_angles, paint_view_grid, paint_view_perspective,
    paint_view_shadow,
};
pub(crate) use preferences_figures::{ThemeSample, paint_theme_system, paint_theme_thumbnail};
pub(crate) use preferences_navigation::*;
pub(crate) use preferences_small::*;
pub(crate) use radial::{RADIAL_RADIUS, radial_menu};
pub(crate) use reports::*;
pub(crate) use review::ReviewAndApply;
pub(crate) use search::{CommandSearch, SearchBadge, SearchFieldGeometry};
pub(crate) use shortcuts::*;
#[cfg(test)]
pub(crate) use sphere::sphere_preview;
pub use sphere::{FinishKind, Sphere, sphere_thumbnail};
pub(crate) use sphere::{SphereTile, sphere_cursor, sphere_grid, sphere_grid_drag, sphere_preview_with_source};
pub(crate) use stages::{StageAction, StageBar};
pub(crate) use status::*;
pub(crate) use table::*;
#[cfg(test)]
pub(crate) use text::ellipsize_middle;
pub(crate) use text_menu::menu_open as text_menu_open;
pub(crate) use tree::*;
pub(crate) use value_focus::*;
pub(crate) use viewport_status::*;

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

// Share the existing CPU rasteriser with page-level artwork regressions.
#[cfg(test)]
pub(crate) use checks::tests::raster as raster_test_shapes;
