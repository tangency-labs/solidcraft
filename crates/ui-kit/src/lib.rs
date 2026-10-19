//! The UI kit: tokenised dense controls (rows, fields, menus, panels, windows) and the components
//! built from them (the ribbon, app bar, history bar, navigation bar, Browser, view compass,
//! preferences window, start page, window chrome and theme files). Everything is drawn from plain data the host
//! passes or installs (its icon set, glyphs, tab table, text and built-in themes) and returns what
//! the user chose; nothing here reads an app's state.
//! `Panel` owns card composition; rows own aligned label/control columns and retain egui focus.
//! Geometry, typography and colours come from `solidcraft_theme::Tokens`. See `docs/ui-kit.md`.

// Components: each takes plain data and returns the action chosen.
pub mod app_bar;
pub mod blur;
pub mod browser;
pub mod frame;
pub mod history_bar;
pub mod nav_bar;
pub mod nav_bar_layout;
pub mod navigation_bar;
pub mod prefs_window;
pub mod ribbon;
pub mod start_page;
pub mod theme_files;
pub mod theme_options;
pub mod toast;
pub mod view_compass;
pub mod window_chrome;

mod actions;
mod assistant;
mod assistant_entry;
mod attention;
mod checks;
mod choice;
mod color;
mod compare;
mod confirm;
pub mod container;
mod dash;
mod details;
pub mod dialog_focus;
mod disclosure;
mod drag;
mod fields;
pub mod file_view;
mod framing;
mod history;
mod history_group;
pub mod icons;
mod keyboard_focus;
mod layout;
mod look_chain;
pub mod menu;
mod metrics;
mod name_list;
mod navigation;
mod navigation_menu;
mod navigation_settings;
pub mod panel;
mod panel_host;
mod preferences;
mod preferences_assistant;
#[allow(dead_code, reason = "composed by the Preferences pages of #9875 to #9883")]
pub mod preferences_cards;
#[allow(dead_code, reason = "painted by the Preferences pages of #9875 to #9883")]
pub mod preferences_figure_data;
#[allow(dead_code, reason = "painted by the Preferences pages of #9875 to #9883")]
mod preferences_figure_ops;
pub mod preferences_figures;
mod preferences_layout;
mod preferences_navigation;
mod preferences_small;
mod radial;
pub mod recent_colors;
mod reports;
mod review;
mod search;
pub mod settings_search;
mod shortcuts;
mod sphere;
mod stages;
mod status;
mod table;
mod text;
mod text_menu;
mod theme_picker;
mod tree;
pub mod type_face;
mod value_focus;
mod viewport_status;
mod wheel;

pub use actions::*;
pub use actions::{Button, button};
pub use assistant::*;
pub use assistant_entry::*;
pub use attention::*;
pub use checks::*;
pub use choice::*;
pub use color::*;
pub use compare::*;
pub use confirm::{Confirm, Tone, dialog_mnemonics, set_dialog_mnemonics};
pub use container::{BackingTooltipExt, Container, DockEdges, Edge, LayoutMode, Surface};
pub use container::{SHELL_GAP, input_blocked, set_input_blocked};
pub use dash::dashed_line;
pub use details::*;
pub use disclosure::*;
pub use drag::*;
pub use fields::*;
pub use fields::{Expressions, cell_editor, install_expressions, wide_cell_editor};
pub use file_view::Icon;
pub use file_view::{FileView, RecentRow, RecoveryMessage, SampleCard};
pub use framing::*;
pub use history::*;
pub use history_group::*;
pub use keyboard_focus::*;
pub use layout::*;
pub use look_chain::{LookStep, look_chain};
pub use menu::{Item, ItemIcon, LIST_W};
pub use menu::{Menu, MenuItem, WorkspaceMenu, WorkspaceStyle, rename_menu, workspace_menu};
pub use metrics::{ContentWidth, dimension, header_height, row_height, table_row_height};
pub use name_list::{NameList, NameRow, name_list};
pub use navigation::*;
pub use navigation::{NavigationEdge, NavigationIdle, NavigationSize};
pub use navigation_menu::*;
pub use navigation_settings::*;
pub use panel::{
    ComputeStatus, FeatureOptions, InspectorAction, InspectorNames, Panel, Part, WindowPage, WorkspaceOptions, inspector_header, panel_header_hover,
    within,
};
pub use panel::{DialogButtons, Window};
#[cfg(any(test, feature = "test-support"))]
pub use panel::{collapsible_header, header};
pub use panel_host::*;
pub use preferences::*;
pub use preferences_assistant::*;
#[allow(unused_imports, reason = "composed by the Preferences pages of #9875 to #9883")]
pub use preferences_cards::*;
pub use preferences_figure_data::{
    paint_aa_2x, paint_aa_4x, paint_aa_8x, paint_aa_off, paint_ao_on, paint_frame_rate, paint_quick_measure, paint_tess_coarse, paint_tess_fine,
    paint_tess_normal, paint_tess_ultra, paint_vertex_markers, paint_view_angles, paint_view_grid, paint_view_perspective, paint_view_shadow,
};
pub use preferences_figures::{ThemeSample, paint_theme_thumbnail_rounded};
pub use preferences_navigation::*;
pub use preferences_small::*;
pub use radial::{RADIAL_RADIUS, radial_menu};
pub use reports::*;
pub use review::ReviewAndApply;
pub use search::{CommandSearch, SearchBadge, SearchFieldGeometry};
pub use shortcuts::*;
#[cfg(any(test, feature = "test-support"))]
pub use sphere::sphere_preview;
pub use sphere::{FinishKind, Sphere, sphere_thumbnail};
pub use sphere::{SphereTile, sphere_cursor, sphere_grid, sphere_grid_drag, sphere_preview_with_source};
pub use stages::{Cards, StageAction, StageBar};
pub use status::*;
pub use table::*;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::{checkbox_shapes as test_checkbox_shapes, raster as test_raster, textures_update as test_textures_update};
#[cfg(any(test, feature = "test-support"))]
pub use test_support::{raster as raster_ui, textures_update as update_test_textures};
#[cfg(any(test, feature = "test-support"))]
pub use text::ellipsize_middle;
pub use text::paint_text;
pub use text_menu::menu_open as text_menu_open;
pub use theme_picker::*;
pub use tree::*;
pub use value_focus::*;
pub use viewport_status::*;

#[cfg(any(test, feature = "test-support"))]
#[path = "gallery_tests.rs"]
pub mod gallery;
#[cfg(test)]
mod slider_tests;
#[cfg(any(test, feature = "test-support"))]
#[path = "test_support_tests.rs"]
pub mod test_support;

pub use layout::label as row_label;

/// Whether `need` points fit in `have`, answered so that a scrollbar coming and going (it takes
/// `ui.spacing().scroll.allocated_width()` of the width) does not flip it: what has fitted keeps
/// fitting until it misses by more than the scrollbar's width, and what has not fitted must fit
/// outright. With nothing remembered yet it is the generous answer, so the first frame with a
/// scrollbar and the first without agree. Keyed by `key` within `ui`.
pub fn fits_steadily(ui: &egui::Ui, key: impl std::hash::Hash + std::fmt::Debug, need: f32, have: f32) -> bool {
    let slack = ui.spacing().scroll.allocated_width();
    let id = ui.make_persistent_id(("fits_steadily", key));
    let was = ui.data(|data| data.get_temp::<bool>(id)).unwrap_or(true);
    let fits = need <= have + if was { slack } else { 0.0 };
    ui.data_mut(|data| data.insert_temp(id, fits));
    fits
}

// Share the existing CPU rasteriser with page-level artwork regressions.
#[cfg(any(test, feature = "test-support"))]
pub use test_support::raster as raster_test_shapes;

#[cfg(any(test, feature = "test-support"))]
pub use test_support::textures_update;
