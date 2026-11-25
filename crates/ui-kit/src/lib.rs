//! The UI kit: tokenised dense controls (rows, fields, menus, panels, windows) and the components
//! built from them (the ribbon, app bar, history bar, navigation bar, Browser, view compass,
//! preferences window, start page, window chrome and theme files). Everything is drawn from plain data the host
//! passes or installs (its icon set, glyphs, tab table, text and built-in themes) and returns what
//! the user chose; nothing here reads an app's state.
//! `Panel` owns card composition; rows own aligned label/control columns and retain egui focus.
//! Geometry, typography and colours come from `solidcraft_theme::Tokens`. See `docs/ui-kit.md`.

// Components: each takes plain data and returns the action chosen.
pub mod app_bar;
pub mod app_bar_items;
pub mod app_bar_layout;
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
pub mod tint;
pub mod toast;
pub mod view_compass;
pub mod view_layout_menu;
pub mod window_chrome;
pub mod window_options_menu;
mod window_preview;

mod actions;
mod assistant;
mod assistant_entry;
mod attention;
#[cfg(not(target_arch = "wasm32"))]
mod blur_plan;
mod checks;
mod choice;
mod color;
mod compare;
mod confirm;
pub mod container;
mod dash;
mod details;
mod disclosure;
mod drag;
mod fields;
pub mod file_view;
mod framing;
mod grid;
mod grid_tsv;
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
mod part_card;
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
pub mod reveal;
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

pub use actions::Button;
pub use actions::button;
pub use actions::*;
pub use assistant::*;
pub use assistant_entry::*;
pub use attention::*;
pub use checks::*;
pub use choice::*;
pub use color::*;
pub use compare::*;
pub use confirm::Confirm;
pub use confirm::Tone;
pub use confirm::dialog_mnemonics;
pub use confirm::set_dialog_mnemonics;
pub use container::BackingTooltipExt;
pub use container::Container;
pub use container::DockEdges;
pub use container::Edge;
pub use container::LayoutMode;
pub use container::SHELL_GAP;
pub use container::Surface;
pub use container::input_blocked;
pub use container::set_input_blocked;
pub use dash::dashed_line;
pub use details::*;
pub use disclosure::*;
pub use drag::*;
pub use fields::Expressions;
pub use fields::cell_editor;
pub use fields::install_expressions;
pub use fields::wide_cell_editor;
pub use fields::*;
pub use file_view::FileView;
pub use file_view::Icon;
pub use file_view::RecentRow;
pub use file_view::RecoveryMessage;
pub use file_view::SampleCard;
pub use framing::*;
pub use grid::CellEditor;
pub use grid::CellKind;
pub use grid::Edit;
pub use grid::Gesture;
pub use grid::Grid;
pub use grid::GridColumn;
pub use grid::GridOutput;
pub use grid::GridSource;
pub use grid::RowRef;
pub use grid_tsv::read_tsv;
pub use grid_tsv::write_tsv;
pub use history::*;
pub use history_group::*;
pub use keyboard_focus::*;
pub use layout::*;
pub use look_chain::LookStep;
pub use look_chain::look_chain;
pub use menu::Item;
pub use menu::ItemIcon;
pub use menu::LIST_W;
pub use menu::Menu;
pub use menu::MenuItem;
pub use menu::WorkspaceMenu;
pub use menu::WorkspaceStyle;
pub use menu::rename_menu;
pub use menu::workspace_menu;
pub use metrics::ContentWidth;
pub use metrics::dimension;
pub use metrics::header_height;
pub use metrics::row_height;
pub use metrics::table_row_height;
pub use name_list::NameList;
pub use name_list::NameRow;
pub use name_list::name_list;
pub use navigation::NavigationEdge;
pub use navigation::NavigationIdle;
pub use navigation::NavigationSize;
pub use navigation::*;
pub use navigation_menu::*;
pub use navigation_settings::*;
pub use panel::ComputeStatus;
pub use panel::DialogButtons;
pub use panel::FeatureOptions;
pub use panel::InspectorAction;
pub use panel::InspectorNames;
pub use panel::Panel;
pub use panel::Part;
pub use panel::StackCard;
pub use panel::StackSlot;
pub use panel::Window;
pub use panel::WindowPage;
pub use panel::WorkspaceOptions;
#[cfg(any(test, feature = "test-support"))]
pub use panel::collapsible_header;
#[cfg(any(test, feature = "test-support"))]
pub use panel::header;
pub use panel::inspector_header;
pub use panel::panel_header_hover;
pub use panel::within;
pub use panel_host::*;
pub use part_card::{PartCard, PartCardSize, PartFace, fitted, folder_button, part_card, part_row};
pub use preferences::*;
pub use preferences_assistant::*;
#[allow(unused_imports, reason = "composed by the Preferences pages of #9875 to #9883")]
pub use preferences_cards::*;
pub use preferences_figure_data::paint_aa_2x;
pub use preferences_figure_data::paint_aa_4x;
pub use preferences_figure_data::paint_aa_8x;
pub use preferences_figure_data::paint_aa_off;
pub use preferences_figure_data::paint_ao_on;
pub use preferences_figure_data::paint_frame_rate;
pub use preferences_figure_data::paint_quick_measure;
pub use preferences_figure_data::paint_tess_coarse;
pub use preferences_figure_data::paint_tess_fine;
pub use preferences_figure_data::paint_tess_normal;
pub use preferences_figure_data::paint_tess_ultra;
pub use preferences_figure_data::paint_vertex_markers;
pub use preferences_figure_data::paint_view_angles;
pub use preferences_figure_data::paint_view_grid;
pub use preferences_figure_data::paint_view_perspective;
pub use preferences_figure_data::paint_view_shadow;
pub use preferences_figures::ThemeSample;
pub use preferences_figures::paint_theme_thumbnail_rounded;
pub use preferences_navigation::*;
pub use preferences_small::*;
pub use radial::RADIAL_RADIUS;
pub use radial::radial_menu;
pub use reports::*;
pub use review::ReviewAndApply;
pub use search::CommandSearch;
pub use search::SearchBadge;
pub use search::SearchFieldGeometry;
pub use shortcuts::*;
pub use sphere::FinishKind;
pub use sphere::Sphere;
pub use sphere::SphereTile;
pub use sphere::sphere_cursor;
pub use sphere::sphere_grid;
pub use sphere::sphere_grid_drag;
#[cfg(any(test, feature = "test-support"))]
pub use sphere::sphere_preview;
pub use sphere::sphere_preview_with_source;
pub use sphere::sphere_thumbnail;
pub use stages::Cards;
pub use stages::StageAction;
pub use stages::StageBar;
pub use status::*;
pub use table::*;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::checkbox_shapes as test_checkbox_shapes;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::raster as test_raster;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::raster as raster_ui;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::textures_update as test_textures_update;
#[cfg(any(test, feature = "test-support"))]
pub use test_support::textures_update as update_test_textures;
#[cfg(any(test, feature = "test-support"))]
pub use text::ellipsize_middle;
pub use text::paint_text;
pub use text_menu::menu_open as text_menu_open;
pub use theme_picker::*;
pub use toast::measure_inline_notice;
pub use tree::*;
pub use value_focus::*;
pub use viewport_status::*;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod blur_plan_tests;
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
