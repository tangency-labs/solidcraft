//! The SolidCraft egui front end.
//!
//! A thin shell over [`solidcraft_engine::Session`]: panels read engine state and act through
//! commands ([`SolveApp::run`]). Nothing here owns design data. The layout follows the familiar
//! parametric-CAD desktop: application bar, a toolbar of workspace tabs and panels, the browser
//! on the left, the 3D viewport with an axis view and navigation bar, the command dialog on the
//! right of the viewport, the command palette and a bottom or left-sidebar feature timeline.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]
use crate::ui_kit::{Container, Surface};

pub mod access;
pub mod agent_chat;
pub mod agent_cursor;
pub mod ai_connect;
pub mod bidi;
pub mod bom_panel;
pub mod browser;
pub mod browser_canvas;
pub mod browser_prompts;
pub mod canvas;
pub mod cli_link;
#[cfg(test)]
mod colour_tokens_tests;
#[cfg(test)]
mod colour_vision_tests;
pub mod command_help;
pub mod context_menu;
pub mod control;
pub mod control_job;
pub mod crash_ui;
pub mod deferred;
pub mod delete;
pub mod design_properties;
pub mod dialog_column;
mod dialog_descriptor;
pub mod dialogs;
pub mod dialogs_appearance;
pub mod dialogs_assembly;
pub mod dialogs_coupling;
pub mod dialogs_frame;
pub mod dialogs_gusset;
mod dialogs_hole;
pub mod dialogs_insert_drawing;
pub mod dialogs_library;
pub mod dialogs_motion;
pub mod dialogs_move;
pub mod dialogs_part_size;
pub mod dialogs_parts;
pub mod dialogs_plastic;
pub mod dialogs_save_mesh;
pub mod dialogs_sheet;
pub mod dialogs_surface;
pub mod dialogs_sweep;
mod dialogs_thread;
pub mod dialogs_view_dxf;
pub mod dim_view;
#[cfg(test)]
mod dimension_create_ui_tests;
pub mod documents;
pub mod drag_extrude;
pub mod drag_snap;
pub mod easyprint_ui;
pub mod edit_clipboard;
pub mod edit_in_place;
mod feature_edit;
#[cfg(test)]
mod font_literals_tests;
pub mod fonts;
pub mod frame;
pub mod frame_rate;
pub mod gizmo;
pub mod gpu;
pub mod help;
pub(crate) mod history_rows;
#[cfg(test)]
mod hole_direction_ui_tests;
pub(crate) mod hole_place;
#[cfg(test)]
mod hole_place_ui_tests;
#[cfg(test)]
mod hole_placement_ui_tests;
pub mod hole_presets;
pub(crate) mod hole_refs;
#[cfg(test)]
mod hole_refs_ui_tests;
pub mod home;
pub mod i18n;
#[cfg(test)]
mod i18n_menu_tests;
#[cfg(test)]
mod i18n_painted_tests;
#[cfg(test)]
mod i18n_ui_tests;
pub mod icon_svg;
pub mod icons;
pub mod inference;
pub mod insert_handles;
pub mod install;
pub mod issue_report;
pub mod joint_drag;
pub(crate) mod joint_handles;
pub mod keymap;
mod length_input;
mod linked_source;
#[cfg(test)]
mod loft_rails_dialog_tests;
#[cfg(test)]
mod look_at_tests;
#[cfg(test)]
mod manage_dialogs_tests;
mod material_card_review;
mod material_editor;

#[cfg(test)]
mod measurement_ui_tests;
#[cfg(test)]
mod menu_tests;
pub(crate) mod mesh_lod;
pub mod messages;
pub(crate) mod motion;
pub mod occurrence_drag;
#[cfg(test)]
mod pattern_arrow_tests;
#[cfg(test)]
mod prompt_cards_tests;
#[cfg(test)]
mod scale_tests;
#[cfg(test)]
mod surface_clamp_tests;
#[cfg(test)]
mod surface_errors_tests;
#[cfg(test)]
mod timeline_marks_tests;
#[cfg(test)]
mod status_tests {
    #[test]
    fn the_status_bar_names_commands_by_label() {
        assert_eq!(super::command_label("solid.fillet"), "Fillet");
        assert_eq!(
            super::friendly_error("solid.shell", "invalid parameters for `solid.shell`: offset: must be positive"),
            "Shell: offset: must be positive"
        );
        assert_eq!(super::friendly_error("edit.undo", "nothing to undo"), "Undo: nothing to undo");
        {
            // The label follows the interface language; the engine's words stay English.
            let _polish = crate::i18n::scope("pl");
            assert_eq!(super::friendly_error("edit.undo", "nothing to undo"), "Cofnij: nothing to undo");
            assert_eq!(
                super::friendly_error("sketch.dimension", "invalid parameters for `sketch.dimension`: value: `mm` is a unit name"),
                "Wymiar szkicu: value: `mm` is a unit name"
            );
        }
        assert_eq!(super::friendly_error("edit.undo", "nothing to undo"), "Undo: nothing to undo");
        let mut app = super::SolveApp::new(solidcraft_engine::Session::default(), Default::default());
        app.run("solid.box", serde_json::json!({"length": 5, "width": 5, "height": 5})).unwrap();
        assert_eq!(app.session.log.last().map(String::as_str), Some("Box done"));
        let _ = app.run("edit.redo", serde_json::json!({}));
        assert!(app.session.log.last().is_some_and(|l| l.starts_with("Redo: ") && !l.contains("edit.redo")), "{:?}", app.session.log.last());
    }
}
#[cfg(test)]
mod apply_off_thread_tests;
mod expression_complete;
#[cfg(test)]
mod extrude_auto_to_object_tests;
#[cfg(test)]
mod extrude_edit_tests;
mod extrude_preview;
mod file_load;
mod file_picker;
pub(crate) mod frame_watch;
#[cfg(test)]
mod history_off_thread_tests;
mod move_preview;
pub use file_picker::FilePickerFuture;
#[cfg(test)]
mod blend_arrow_tests;
#[cfg(test)]
mod blend_dialog_tests;
#[cfg(test)]
mod boundary_dialog_tests;
pub(crate) mod display_lod;
#[cfg(test)]
mod file_load_ui_tests;
#[cfg(test)]
mod fillet_spline_handle_tests;
pub mod mouse_gesture;
pub(crate) mod navigation;
pub mod navigation_bar;
mod navigation_bar_view;
pub use navigation::WebWheelGestures;
pub mod network_prompt;
pub mod op_auto;
pub mod open_elsewhere;
pub(crate) mod open_meshing;
pub mod params_dialog;
pub mod personal_sheet_rules;
pub(crate) mod pick_index;
pub(crate) mod place_gizmo;
pub(crate) mod plugins;
pub mod point_clouds;
pub mod prefs;
pub mod prefs_notice;
mod prefs_search;
#[cfg(test)]
mod preselect_tests;
pub mod preview;
mod primitive_placement;
#[cfg(test)]
mod primitive_units_tests;
mod print_face_ui;
pub(crate) mod quick_measure;
#[cfg(test)]
mod quick_measure_tests;
pub mod recovery_ui;
pub mod ref_images;
mod render_ui;
pub mod render_workspace;
#[cfg(test)]
mod revolve_edit_tests;
#[cfg(test)]
mod revolve_manipulator_tests;
#[cfg(test)]
mod round_pick_tests;
mod rule_editor;
#[cfg(test)]
mod rule_editor_tests;
pub mod save_job;
pub mod scenario;
pub(crate) mod scene_palette;
pub mod select_other;
pub mod selection;
pub mod settings_profiles;
#[cfg(test)]
mod settings_profiles_tests;
pub mod shell;
#[cfg(test)]
mod shell_face_sketch_ui_tests;
pub mod shortcut_box;
pub mod sketch3d;
#[cfg(test)]
mod sketch3d_frame_ui_tests;
pub(crate) mod sketch_attachment_ui;
#[cfg(test)]
mod sketch_attachment_ui_tests;
mod sketch_construction;
pub mod sketch_dims;
pub(crate) mod sketch_display;
#[cfg(test)]
mod sketch_display_ui_tests;
#[cfg(test)]
mod sketch_drag_ui_tests;
#[cfg(test)]
mod sketch_edit_tests;
mod sketch_families;
#[cfg(test)]
mod sketch_guidance_ui_tests;
pub mod sketch_modify_ui;
#[cfg(test)]
mod sketch_modify_ui_tests;
#[cfg(test)]
mod sketch_moving_click_tests;
pub mod sketch_palette;
mod sketch_pattern_tree;
pub mod sketch_report;
#[cfg(test)]
mod sketch_side_ui_tests;
mod sketch_state;
pub(crate) mod sketch_text;
mod sketch_text_handles;
mod sketch_text_move;
pub mod sketch_tools;
mod sketch_transform_ui;
pub mod smart_extrude;
#[cfg(test)]
mod smart_extrude_ui_tests;
pub mod space_mouse;
pub mod space_mouse_calibration;
pub mod space_mouse_model;
pub mod space_mouse_offer;
pub mod stand_up;
mod status_bar;
#[cfg(test)]
mod sweep_dialog_tests;
pub(crate) mod test_boat;
pub mod theme;
pub mod theme_files;
#[cfg(test)]
mod thin_extrude_tests;
pub mod timeline;
pub mod timeline_left;
pub mod titlebar;
pub mod tool_cursor;
pub mod tool_modifiers;
pub mod toolbar;
pub mod toolbar_custom;
pub mod tools;
pub mod ui_kit;
#[cfg(test)]
mod ui_kit_guard_tests;
pub mod update;
mod value_step;
pub(crate) mod view_angles;
mod view_confirm;
pub mod view_pick;
pub mod viewport;
mod viewport_details;
mod viewport_layout;
mod viewport_status;
mod viewport_status_fixture;
pub mod workspace;

use std::sync::mpsc::Receiver;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use solidcraft_engine::Session;
use solidcraft_engine::render::{Camera, CameraAnim, StandardView};

pub use control::{ControlRequest, ControlResponse};

/// How close (radians) the camera must be to a standard axis view for the #2682 mode
/// (`auto_ortho_faces`) to snap the projection to orthographic. One degree: exact axis views
/// and Shift-snapped orbits land inside it, free orbiting does not linger in it.
pub const ORTHO_FACE_TOLERANCE: f64 = 1.0_f64.to_radians();

/// UI state that automation can read and set (`ui.set`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UiState {
    /// Effective shell mode; automation can override the saved preference for this session.
    pub layout: shell::LayoutMode,
    /// Ribbon tab; SHEET METAL and the contextual SKETCH join the seven design tabs.
    pub tab: String,
    /// The toolbar tab a sketch was started from; finishing the sketch goes back to it (#3250).
    /// Not saved: it only lives while a sketch is open.
    #[serde(skip)]
    pub sketch_return_tab: Option<String>,
    /// Last contextual body/sketch, only used to select on entry.
    #[serde(skip)]
    pub ribbon_context: Option<String>,
    pub show_browser: bool,
    pub show_timeline: bool,
    pub show_grid: bool,
    pub show_navigation_bar: bool,
    pub perspective: bool,
    /// Perspective while orbiting, orthographic when the camera faces a standard axis view
    /// (#2682). Off by default; when on it decides the projection instead of `perspective`.
    pub auto_ortho_faces: bool,
    pub palette_open: bool,
    /// Dark theme (the default); a preference kept between runs.
    pub dark: bool,
    /// Selection filter: kinds a click doesn't pick (`vertices`, `edges`, `faces`, `sketch`).
    pub pick_off: Vec<String>,
    /// A click on a face picks its whole body.
    pub pick_bodies: bool,
    /// A face picks its exact component occurrence in Design and Render.
    pub pick_components: bool,
    /// 0: shaded with edges, 1: shaded, 2: wireframe, 3: shaded with hidden edges.
    pub visual_style: u8,
    /// A soft shadow on the ground under the model (seen from above).
    pub ground_shadow: bool,
    /// The world's up axis (Z or Y; #3): the camera, axis view, home view and ground follow it.
    pub up_axis: solidcraft_engine::render::UpAxis,
    /// The agent cursor: shown, speed, follow camera (#35).
    pub agent_cursor: agent_cursor::Settings,
    /// The running tool's icon rides beside the pointer in the viewport (View settings; off by
    /// default).
    pub tool_cursor: bool,
}

impl UiState {
    fn pick_mode(&self) -> &'static str {
        if self.pick_components {
            "component"
        } else if self.pick_bodies {
            "body"
        } else {
            "face"
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        UiState {
            layout: Default::default(),
            tab: "MODEL".into(),
            ribbon_context: None,
            sketch_return_tab: None,
            show_browser: true,
            show_timeline: true,
            show_grid: true,
            show_navigation_bar: true,
            perspective: false,
            auto_ortho_faces: false,
            palette_open: false,
            dark: true,
            pick_off: Vec::new(),
            pick_bodies: false,
            pick_components: false,
            visual_style: 0,
            ground_shadow: true,
            up_axis: Default::default(),
            agent_cursor: Default::default(),
            tool_cursor: false,
        }
    }
}

/// Host services injected by the app so this crate stays portable.
#[derive(Default)]
pub struct Services {
    /// Ask for a file to open. The extensions are the file type the command reads, offered as
    /// the picker's first filter (`&[]`: designs, CAD files and meshes, #1801).
    pub pick_open: Option<Box<dyn Fn(&[&str]) -> FilePickerFuture>>,
    pub pick_save: Option<Box<dyn Fn(&str, &[&str]) -> FilePickerFuture>>,
    /// Open a dialog for one file of a kind (its name, its extensions): on macOS a dialog allows
    /// only the types it names, so the agent chat's files need a dialog of their own. `None`: the
    /// Open dialog.
    pub pick_file: Option<Box<dyn Fn(&str, &[&str]) -> FilePickerFuture>>,
    /// Open a dialog asking for a program (a slicer); `None` when the host has no such dialog.
    pub pick_program: Option<Box<dyn Fn() -> FilePickerFuture>>,
    pub show_in_folder: Option<Box<dyn Fn(&str)>>,
    /// Where the host reads the anti-aliasing preference at start (a small JSON file).
    pub graphics_path: Option<String>,
    /// Linux: the window system this run uses, `wayland` or `x11` (Preferences shows it, #3083).
    pub window_system: Option<&'static str>,
    /// The desktop's dark (`Some(true)`) or light preference as the host reads it, for Theme:
    /// System where the window toolkit does not report it (Linux desktops, #7944). Called every
    /// frame, so it only reads a value kept up to date elsewhere. None, or `None` from it: the
    /// window toolkit's theme.
    pub desktop_dark: Option<Box<dyn Fn() -> Option<bool>>>,
    /// Adapter names and descriptive labels from the desktop renderer's chosen backends.
    pub graphics_adapters: Vec<(String, String)>,
    /// HTTPS GET for the update check (`update`); None: never check (browser, tests).
    pub http_get: Option<update::HttpGet>,
    /// HTTPS download to a file, with progress, for one-click updates (`update::install`).
    pub http_download: Option<update::install::HttpDownload>,
    /// The browser's offer to install the web app (`install`); None: no Install button (desktop).
    pub install_app: Option<install::InstallApp>,
    /// Hand a design's bytes to the user as a downloaded file (name, bytes); the browser build's
    /// File > Download design. None: the host saves real files, so there is nothing to offer.
    pub download_design: Option<Box<dyn Fn(&str, &[u8]) -> Result<(), String>>>,
    /// Whether a recent design that is not in memory can be brought back by the host (the browser
    /// build remembers files it saved to the disk); None: it can't.
    pub can_restore: Option<Box<dyn Fn(&str) -> bool>>,
    /// Bring such a design back and open it when it arrives (asks the user for access, so call it
    /// in the click that chose it).
    pub restore_file: Option<Box<dyn Fn(&str)>>,
    /// Show the designs the browser keeps for itself; None: the host has no such store.
    pub browser_files: Option<Box<dyn Fn()>>,
    /// HTTP for the agent chat's API agents (OpenAI-compatible and Anthropic-compatible endpoints); None: no API agents.
    pub agent_http: Option<agent_chat::backend::HttpFn>,
    /// The system keychain, for API agents' keys; None: keys can't be stored.
    pub secrets: Option<std::sync::Arc<dyn agent_chat::backend::Secrets>>,
    /// Upload a model to EasyPrint (Printables' slicer in the browser) and answer with the address
    /// to open (`easyprint_ui`); None: this host can't, and the menu doesn't offer it.
    pub easyprint_upload: Option<easyprint_ui::Upload>,
}

/// The view before a sketch opened and the view that faced the sketch then (#7038).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreSketchView {
    pub sketch: u64,
    pub before: Camera,
    pub facing: Camera,
}

/// The active component (and placement) before a sketch opened inside another one (#8127), and
/// the component the sketch opened in.
#[derive(Clone, Debug, PartialEq)]
pub struct PreSketchComponent {
    pub sketch: u64,
    pub component: u64,
    pub path: Option<Vec<u64>>,
    pub opened_in: u64,
}

/// Which late UI handler has first refusal of this frame's Escape. Reserving input
/// is distinct from having performed cancellation (`esc_handled`).
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum EscapeOwner {
    Dialog,
    Overlay,
}

pub struct SolveApp {
    pub(crate) viewport_status: ui_kit::StatusPresentation,
    pub(crate) viewport_status_fixture: viewport_status_fixture::Fixture,
    /// Compact browser embed: retain model panels and joint controls without application chrome.
    pub embedded: bool,
    pub session: Session,
    pub ui: UiState,
    pub(crate) shell: shell::State,
    pub cam: Camera,
    /// A view change in progress (axis view, Home, Fit); any mouse navigation cancels it.
    pub cam_anim: Option<CameraAnim>,
    /// Seconds since start, as of this frame.
    pub now: f64,
    /// The history marker before Edit Sketch rolled the timeline to the sketch; Finish Sketch
    /// restores it.
    pub pre_sketch_marker: Option<Option<usize>>,
    /// The views around opening the open sketch. Leaving it goes back to the earlier one as the
    /// "When a sketch ends" preference says (#7038).
    pub pre_sketch_cam: Option<PreSketchView>,
    /// The component active before Edit Sketch activated the one the sketch is in (#8127);
    /// leaving the sketch makes it active again.
    pub pre_sketch_component: Option<PreSketchComponent>,
    pub services: Services,
    file_picker: Option<file_picker::Pending>,
    /// A slicer started alone and waited for before the file is sent (Send to Slicer).
    pub(crate) slicer_wait: Option<dialogs_save_mesh::SlicerWait>,
    /// Open with EasyPrint: the upload in flight and the question before the first one.
    pub(crate) easyprint: easyprint_ui::State,
    /// A Save or Save As being written on a worker (see [`save_job`]).
    saving: Option<save_job::Saving>,
    pub viewport: viewport::ViewportState,
    pub render_workspace: render_workspace::State,
    pub tool: Option<tools::Tool>,
    pub dialog: Option<dialogs::Dialog>,
    /// The Change Parameters window, when open.
    pub params: Option<params_dialog::ParamsDialog>,
    /// The Design Properties window, when open.
    pub design_properties: Option<design_properties::DesignProperties>,
    /// The Sketch report window (`sketch.inspect` from the ribbon), when open.
    pub sketch_report: Option<sketch_report::SketchReport>,
    /// The Bill of Materials window, when open.
    pub bom: Option<bom_panel::BomPanel>,
    material_card_review: Option<material_card_review::ReviewWindow>,
    /// Context menus, rename box and Properties window.
    pub menu: context_menu::MenuState,
    /// Mouse gestures: the ring of tools and its customization window.
    pub gesture: mouse_gesture::State,
    /// Browser tree state (reveal, groups being edited, occurrence moves).
    pub tree: browser::TreeState,
    /// A saved design copied for insertion into another open document.
    pub linked_design_clipboard: Option<String>,
    /// The S box (command search at the cursor): recent and pinned commands.
    pub sbox: shortcut_box::ShortcutBox,
    /// Keyboard shortcuts the user changed.
    pub keymap: keymap::Keymap,
    /// The Help menu and the About window.
    pub help: help::HelpState,
    /// Every status message of this run, for the Messages window ([`messages`], #2763).
    pub messages: messages::Messages,
    /// An editable local issue draft and optional captured app frame.
    pub issue_report: issue_report::ReportState,
    /// The Crash Reports window.
    pub crashes: crash_ui::CrashUi,
    /// The update check and its window (`update`).
    pub updater: update::Updater,
    /// Safari's steps for installing the web app, while their window is open (`install`).
    pub install_steps: Option<install::Steps>,
    /// Toolbar buttons the user pinned, removed or reordered.
    pub toolbar_custom: toolbar_custom::ToolbarCustom,
    /// Named settings profiles (#6237).
    pub settings_profiles: settings_profiles::Profiles,
    /// Preferences kept between runs, and their window.
    pub preferences: prefs::Prefs,
    pub prefs_window: prefs::PrefsWindow,
    /// The slicers the Save as mesh dialog offers.
    pub slicer_menu: dialogs_save_mesh::SlicerMenu,
    pub preview: preview::PreviewState,
    /// OK pressed on a feature that takes a while: its evaluation runs off the window thread.
    pub applying: Option<dialogs::Applying>,
    pub(crate) editing_state: Option<dialogs::EditingState>,
    /// A linked component's design edited in place, with its assembly around it.
    pub edit_in_place: edit_in_place::EditInPlace,
    pub(crate) pending_feature_edit: Option<feature_edit::PendingFeatureEdit>,
    /// The open feature window's check of what its material does (#6901).
    pub op_check: op_auto::OpCheck,
    /// A command recomputing the history (the marker moved, undo) computing off the window
    /// thread, and those asked for meanwhile ([`deferred`]).
    pub deferred: Option<deferred::Deferred>,
    pub deferred_queue: Vec<(String, Value)>,
    /// A feature started from a sketch while the marker rolls back, and that sketch: it starts
    /// once the marker is back.
    pub start_after_roll: Option<(String, u64)>,
    pub(crate) timeline_playback: Option<timeline::Playback>,
    /// A delete's check of what it takes with it or breaks, computing off the window thread
    /// ([`deferred::DeleteCheck`], #2678).
    pub delete_check: Option<deferred::DeleteCheck>,
    /// How long OK waits for a feature before computing it off the window thread, saying so;
    /// `None`: it waits as long as it takes (headless use, tests). The desktop app sets it.
    pub apply_off_thread: Option<std::time::Duration>,
    /// Tests: the off-thread evaluation waits for this before it reports done.
    #[cfg(test)]
    pub(crate) apply_gate: Option<std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<()>>>>,
    /// The last command started interactively (id, label), for Repeat.
    pub last_command: Option<(String, String)>,
    /// Bodies copied with Copy (by name), pasted into the active component by Paste.
    pub copied_bodies: Vec<String>,
    /// Text a copy left for the system clipboard, written on the next frame (so a paste event
    /// follows: the window system sends one only while the clipboard holds text).
    pub(crate) clipboard_text: Option<String>,
    /// Component placements copied with Copy (by path): Paste places new instances of them in
    /// the active component, Paste New (Ctrl+Shift+V) independent copies.
    pub copied_placements: Vec<Vec<u64>>,
    /// Components, timeline features and bodies cut with Cut: they stay until Paste moves them
    /// into a component.
    pub cut: edit_clipboard::CutSet,
    /// Counts copies and cuts in every open design: the last one is what pastes.
    pub clipboard_stamp: u64,
    /// The copy or cut this design made last (`clipboard_stamp` then): when it isn't the last
    /// one, what this design copied is stale and another design's copy pastes.
    pub own_clipboard_stamp: u64,
    /// The bodies (or components) the last copy or cut took, with the design they were in, to
    /// paste into another design (`component.paste_from_design`).
    pub foreign_copy: Option<std::sync::Arc<solidcraft_engine::ForeignCopy>>,
    /// Esc was already acted on this frame (by the shortcuts), so later handlers skip it.
    pub esc_handled: bool,
    pub(crate) escape_owner: Option<EscapeOwner>,
    /// When Esc was last pressed (input time): a second press soon after finishes the sketch.
    pub last_escape: Option<f64>,
    pub status: Option<(String, f64, bool)>,
    /// The status message offers Undo (a click on it undoes the last step), as after an automatic stand-up.
    pub status_undo: bool,
    pub quit_requested: bool,
    /// Autosave into the recovery folder (the desktop app turns it on).
    pub autosave: Option<recovery_ui::Autosaves>,
    /// Lock files next to the open designs (the desktop app turns them on; #5467).
    pub locks: Option<open_elsewhere::Locks>,
    /// Opens waiting for "this design is open elsewhere" to be answered, first asked first.
    pub lock_prompts: Vec<open_elsewhere::LockPrompt>,
    /// A Save waiting for "the file changed on disk" to be answered.
    pub disk_prompt: bool,
    /// Update Links or Open Linked Design waiting for a network server to be allowed.
    pub network_prompt: Option<network_prompt::NetworkPrompt>,
    /// Designs left by crashed apps, offered for recovery.
    pub recoverable: Vec<solidcraft_engine::recovery::Entry>,
    /// Discard all was clicked in the recovery window: its confirmation is showing (#9299).
    pub recovery_discard_all: bool,
    /// The selection's measurement in the view's corner (#7000).
    pub(crate) quick_measure: quick_measure::QuickMeasure,
    /// The bodies of a just-opened Y-up file, offered to be stood up (`stand_up`).
    pub stand_up: Option<Vec<String>>,
    /// The placement of the component that file came in as: the offer turns it.
    pub stand_up_occurrence: Option<u64>,
    /// The file's header names an exporter that saves Y up (SolidWorks): the offer says so.
    pub stand_up_reason: bool,
    /// The file brought more than one body or a component tree: the offer says "assembly".
    pub stand_up_assembly: bool,
    /// The offer is the small note over the view, not the dialog (a file from a program not known
    /// to save Y up, Preferences › Design, #7717).
    pub stand_up_note: bool,
    /// When the note was first drawn (the window's clock), or since the pointer was last on it; it
    /// goes after `stand_up::NOTE_SECONDS`.
    pub stand_up_note_since: Option<f64>,
    /// The anti-aliasing sample counts the scene colour/depth formats support (empty without a GPU).
    pub msaa_supported: Vec<u32>,
    /// The startup check selected 1x; Graphics explains the workaround.
    pub msaa_compatibility: bool,
    /// Startup fallback diagnostic, also shown in Graphics preferences.
    pub graphics_fallback: Option<String>,
    /// Settings put back at start because they kept SolidCraft from starting, one line each,
    /// shown until dismissed (`prefs::show_reset_notice`).
    pub settings_reset: Vec<prefs::SettingReset>,
    /// The last command was a big operation: autosave after it.
    pub big_operation: bool,
    /// Autosave interval (minutes; 0 = only after big operations). A preference.
    pub autosave_minutes: f64,
    /// The application bar is the window's title bar (Windows, Linux: no OS decorations).
    pub custom_titlebar: bool,
    /// The OS hit test already offers the resize border ([`titlebar`]; the Windows desktop app),
    /// so no egui resize zones are laid out.
    pub native_resize_border: bool,
    /// macOS: the traffic lights sit over the application bar's left end.
    pub integrated_titlebar: bool,
    /// The desktop host installed native menus alongside the application bar controls.
    pub native_file_menu: bool,
    /// Build bodies' display meshes on the CPU even where the GPU could (safe mode). Read by
    /// [`SolveApp::set_wgpu`].
    pub cpu_meshing: bool,
    pub frame_ms: f64,
    /// The viewport's frame-rate readout ([`frame_rate`]).
    pub frame_rate: frame_rate::FrameRate,
    pub synthetic: Vec<egui::Event>,
    /// The modifiers of the synthetic press in progress (a Ctrl-drag keeps Ctrl while it moves).
    pub synthetic_mods: egui::Modifiers,
    control_rx: Option<Receiver<ControlRequest>>,
    /// Control requests that change the design, held while a file loads or an agent's design
    /// change computes.
    control_held: std::collections::VecDeque<ControlRequest>,
    /// An agent's design change computing off the window thread ([`control_job`]).
    control_job: Option<control_job::ControlJob>,
    /// Agent exports computing on copies of the design.
    control_answering: Vec<control_job::ControlJob>,
    /// The agent cursor acting out control requests.
    pub agent: agent_cursor::AgentCursor,
    /// The agent chat panel (docs/design/in-app-agent-chat.md).
    pub agent_chat: agent_chat::AgentChat,
    /// Making SolidCraft available to the AI apps installed here ([`ai_connect`]).
    pub ai_connect: ai_connect::State,
    pending_shots: Vec<(u64, Option<String>, std::sync::mpsc::Sender<Value>, f64)>,
    shot_chat_turns: std::collections::HashMap<u64, std::sync::Arc<std::sync::atomic::AtomicBool>>,
    queued_shots: Vec<(u64, f64, u32)>,
    shot_token: u64,
    styled: bool,
    fitted: bool,
    pending_fit: Option<(Camera, solidcraft_engine::geom::Aabb3, bool)>,
    /// Frames a pending fit has waited for a layout that can frame it (`resolve_fit`).
    fit_waits: u32,
    /// The sheet metal flat pattern's own pan and zoom.
    pub flat_view: dialogs_sheet::FlatView,
    /// The start page.
    pub home: home::HomeState,
    /// The 3D mouse: which device, its reading, and the calibration window.
    pub space_mouse: space_mouse::Input,
    pub file_load: file_load::FileLoad,
    /// An opened design's bodies still being meshed, off the UI thread.
    pub(crate) open_meshing: Option<open_meshing::Meshing>,
    /// A design is being put in the window and will be meshed after it: framing it uses outlines.
    pub(crate) open_meshing_next: bool,
    /// How long each frame takes on the window thread, by phase.
    pub(crate) frame_watch: frame_watch::Watch,
    /// The other open designs (tabs).
    pub docs: documents::Documents,
    /// The language the installed font fallback chain is ordered for (`fonts::fallback_order`);
    /// changing `preferences.language` re-installs the fonts on the next frame.
    applied_fonts_language: String,
    /// The interface font the fonts were last set up with (`Prefs::ui_font`).
    applied_ui_font: String,
    /// The fonts installed include the system's fallback faces.
    applied_system_fonts: bool,
}

impl Drop for SolveApp {
    /// The Home pictures it started are drawn before it goes: one still on the GPU as the
    /// process exits can crash the graphics driver.
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        home::finish_pictures(&mut self.home, std::time::Duration::from_secs(10));
    }
}

impl SolveApp {
    pub fn new(mut session: Session, services: Services) -> Self {
        #[cfg(test)]
        let image_host = Some(solidcraft_engine::render::job::CpuHost::cooperative());
        #[cfg(not(test))]
        let image_host = solidcraft_engine::render::job::CpuHost::for_platform().map_err(|e| log::error!("render image host: {e}")).ok();
        if let Err(e) = session.set_render_job_host(image_host.clone()) {
            log::error!("render image host: {e}");
        }
        let mut render_workspace = render_workspace::State::default();
        render_workspace.image_host = image_host;
        #[cfg(not(target_arch = "wasm32"))]
        solidcraft_engine::slicer_files::clean_old();
        let mut app = SolveApp {
            viewport_status: Default::default(),
            viewport_status_fixture: Default::default(),
            session,
            ui: UiState::default(),
            shell: Default::default(),
            cam: Camera::default(),
            cam_anim: None,
            now: 0.0,
            pre_sketch_marker: None,
            pre_sketch_cam: None,
            pre_sketch_component: None,
            services,
            file_picker: None,
            slicer_wait: None,
            easyprint: Default::default(),
            saving: None,
            viewport: viewport::ViewportState::default(),
            render_workspace,
            tool: None,
            dialog: None,
            params: None,
            design_properties: None,
            sketch_report: None,
            bom: None,
            material_card_review: None,
            menu: context_menu::MenuState::default(),
            gesture: mouse_gesture::State::default(),
            tree: browser::TreeState::default(),
            linked_design_clipboard: None,
            sbox: shortcut_box::ShortcutBox::default(),
            keymap: keymap::Keymap::default(),
            help: help::HelpState::default(),
            messages: messages::Messages::default(),
            issue_report: issue_report::ReportState::default(),
            crashes: crash_ui::CrashUi::default(),
            updater: update::Updater::default(),
            install_steps: None,
            toolbar_custom: toolbar_custom::ToolbarCustom::default(),
            settings_profiles: settings_profiles::Profiles::default(),
            preferences: prefs::Prefs::default(),
            prefs_window: prefs::PrefsWindow::default(),
            slicer_menu: dialogs_save_mesh::SlicerMenu::default(),
            preview: preview::PreviewState::default(),
            applying: None,
            editing_state: None,
            edit_in_place: Default::default(),
            pending_feature_edit: None,
            op_check: op_auto::OpCheck::default(),
            deferred: None,
            deferred_queue: Vec::new(),
            start_after_roll: None,
            timeline_playback: None,
            delete_check: None,
            apply_off_thread: None,
            #[cfg(test)]
            apply_gate: None,
            last_command: None,
            copied_bodies: Vec::new(),
            clipboard_text: None,
            copied_placements: Vec::new(),
            cut: Default::default(),
            clipboard_stamp: 0,
            own_clipboard_stamp: 0,
            foreign_copy: None,
            esc_handled: false,
            escape_owner: None,
            last_escape: None,
            status: None,
            status_undo: false,
            quit_requested: false,
            autosave: None,
            locks: None,
            lock_prompts: Vec::new(),
            disk_prompt: false,
            network_prompt: None,
            recoverable: Vec::new(),
            recovery_discard_all: false,
            quick_measure: Default::default(),
            stand_up: None,
            stand_up_occurrence: None,
            stand_up_reason: false,
            stand_up_assembly: false,
            stand_up_note: false,
            stand_up_note_since: None,
            msaa_supported: Vec::new(),
            msaa_compatibility: false,
            graphics_fallback: None,
            settings_reset: Vec::new(),
            big_operation: false,
            autosave_minutes: 5.0,
            custom_titlebar: false,
            native_resize_border: false,
            integrated_titlebar: false,
            native_file_menu: false,
            cpu_meshing: false,
            frame_ms: 0.0,
            frame_rate: frame_rate::FrameRate::default(),
            synthetic: Vec::new(),
            synthetic_mods: egui::Modifiers::default(),
            control_rx: None,
            agent_chat: Default::default(),
            ai_connect: Default::default(),
            control_held: Default::default(),
            control_job: None,
            control_answering: Vec::new(),
            agent: Default::default(),
            pending_shots: Vec::new(),
            shot_chat_turns: Default::default(),
            queued_shots: Vec::new(),
            shot_token: 0,
            styled: false,
            fitted: false,
            pending_fit: None,
            fit_waits: 0,
            flat_view: Default::default(),
            embedded: false,
            home: home::HomeState::default(),
            space_mouse: Default::default(),
            file_load: file_load::FileLoad::default(),
            open_meshing: None,
            open_meshing_next: false,
            frame_watch: Default::default(),
            docs: documents::Documents::default(),
            applied_fonts_language: "en".into(),
            applied_ui_font: String::new(),
            applied_system_fonts: false,
        };
        easyprint_ui::install(&mut app);
        app
    }

    /// Translate text using the active language preference.
    pub fn tr<'a>(&self, text: &'a str) -> &'a str {
        i18n::Locale::for_language(&self.preferences.language).text(text)
    }

    /// Translate a display label supplied by the engine, without changing its identifier.
    pub fn tr_label<'a>(&self, text: &'a str) -> &'a str {
        i18n::Locale::for_language(&self.preferences.language).label(text)
    }

    /// Draw the viewport on the GPU (eframe's wgpu render state, with the app's depth and
    /// stencil buffer bits and MSAA sample count). Without it the viewport renders on the CPU.
    /// The scene is drawn in a pass of the same kind as the window's.
    pub fn set_wgpu(&mut self, rs: &egui_wgpu::RenderState, depth_bits: u8, stencil_bits: u8, samples: u32) {
        let target = crate::gpu::GpuTarget {
            format: rs.target_format,
            depth: egui_wgpu::depth_format_from_bits(depth_bits, stencil_bits),
            samples: samples.max(1),
            tess: false,
            oit: false,
        };
        self.set_wgpu_targets(rs, target, target);
    }

    /// Draw the viewport on the GPU with the scene drawn offscreen: `depth_bits`/`stencil_bits`
    /// and `samples` are the scene's, while the window's pass (egui's) has no depth and one
    /// sample. Native hosts check the installed renderer and fall back to 1x only when that
    /// succeeds where multisampling fails. Returns the effective scene sample count.
    pub fn set_wgpu_split(&mut self, rs: &egui_wgpu::RenderState, depth_bits: u8, stencil_bits: u8, samples: u32) -> u32 {
        let scene = crate::gpu::GpuTarget {
            format: rs.target_format,
            depth: egui_wgpu::depth_format_from_bits(depth_bits, stencil_bits),
            samples: samples.max(1),
            tess: false,
            oit: false,
        };
        let window = crate::gpu::GpuTarget { format: rs.target_format, depth: None, samples: 1, tess: false, oit: false };
        self.set_wgpu_targets(rs, scene, window);
        self.viewport.gpu.map_or(samples, |g| g.samples)
    }

    fn set_wgpu_targets(&mut self, rs: &egui_wgpu::RenderState, scene: crate::gpu::GpuTarget, window: crate::gpu::GpuTarget) {
        #[allow(unused_mut)] // Only native hosts can wait for the startup readback.
        let (mut installed, stats, tess_ready) = gpu::install(rs, scene, window, !self.cpu_meshing);
        #[cfg(not(target_arch = "wasm32"))]
        if scene.depth.is_some() && window.depth.is_none() && window.samples == 1 {
            let mut renderer = rs.renderer.write();
            if let Some(res) = renderer.callback_resources.get_mut::<gpu::Renderer>() {
                match res.check_msaa(&rs.device, &rs.queue) {
                    Ok(solidcraft_gpu::view::MsaaCheck::Passed) => {}
                    Ok(solidcraft_gpu::view::MsaaCheck::Fallback) => {
                        installed.samples = 1;
                        self.msaa_compatibility = true;
                        eprintln!(
                            "SolidCraft graphics: anti-aliasing check failed at {}x and passed at 1x; using 1x for this session",
                            scene.samples
                        );
                    }
                    Ok(solidcraft_gpu::view::MsaaCheck::FailedAtOne) => {
                        eprintln!("SolidCraft graphics: anti-aliasing check also failed at 1x; keeping {}x", scene.samples);
                    }
                    Err(error) => eprintln!("SolidCraft graphics: anti-aliasing check error; keeping {}x: {error}", scene.samples),
                }
            }
        }
        self.viewport.gpu = Some(installed);
        self.viewport.tess_ready = tess_ready;
        self.viewport.draw_stats = Some(stats);
        // The device's own limits (what it was created with), not the adapter's: uploads are split
        // at the device's buffer size.
        self.render_workspace.geometry_budget = render_workspace::geometry_budget(rs.device.limits().max_buffer_size, cfg!(target_arch = "wasm32"));
        render_workspace::invalidate_scene(self);
        let (samples, has_depth) = self.viewport.gpu.map_or((1, false), |g| (g.samples.max(1), g.depth.is_some()));
        self.render_workspace.host = match render_workspace::host_executor(rs.device.limits().max_texture_dimension_2d, has_depth, samples) {
            Ok(executor) => Some(executor),
            Err(error) => {
                self.set_status(error, true);
                None
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let adapter = rs.adapter.clone();
            let key = render_workspace::adapter_key(&adapter.get_info());
            self.render_workspace.defer_path_host(key, move || solidcraft_gpu::path::host_on_adapter(&adapter).map_err(|e| e.to_string()));
        }
        render_workspace::sync_backend(self);
        ai_connect::tick(self);
        self.msaa_supported = rs.msaa_supported.clone();
        let info = rs.adapter.get_info();
        self.help.gpu = Some(format!("{} ({:?}, {:?})", info.name.trim(), info.backend, info.device_type));
        self.viewport.adapter = format!("{:?} · {}", info.backend, info.name);
    }

    /// Preferences kept between runs (JSON), for the host to store.
    pub fn prefs(&self) -> String {
        json!({
            "dark": self.ui.dark,
            "grid": self.ui.show_grid,
            "navigation_bar": self.ui.show_navigation_bar,
            "perspective": self.ui.perspective,
            "auto_ortho_faces": self.ui.auto_ortho_faces,
            "pick_bodies": self.ui.pick_bodies,
            "auto_project": self.session.auto_project,
            "visual_style": self.ui.visual_style,
            "browser_collapsed": self.tree.collapsed,
            "browser_expanded": self.tree.expanded,
            "recent": self.home.recent,
            "recent_opened_at": self.home.opened_at,
            "recent_forgotten": self.home.forgotten,
            "shortcut_box": self.sbox.prefs(),
            "shortcuts": self.keymap.prefs(),
            "toolbar": self.toolbar_custom.prefs(),
            "settings_profiles": self.settings_profiles.prefs(),
            "autosave_minutes": self.autosave_minutes,
            "ground_shadow": self.ui.ground_shadow,
            "agent_cursor": self.ui.agent_cursor,
            "preferences": self.preferences,
            "gpu_path_setup_seconds": self.render_workspace.setup_seconds,
        })
        .to_string()
    }

    /// Restore preferences saved with [`SolveApp::prefs`].
    pub fn load_prefs(&mut self, prefs: &str) {
        let Ok(v) = serde_json::from_str::<Value>(prefs) else { return };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool);
        self.home.opened_at = v.get("recent_opened_at").and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or_default();
        if let Some(d) = flag("dark") {
            self.ui.dark = d;
        }
        if let Some(t) = v.get("gpu_path_setup_seconds") {
            self.render_workspace.setup_seconds = render_workspace::load_setup_seconds(t);
        }
        if let Some(g) = flag("grid") {
            self.ui.show_grid = g;
        }
        if let Some(visible) = flag("navigation_bar") {
            self.ui.show_navigation_bar = visible;
        }
        if let Some(p) = flag("perspective") {
            self.ui.perspective = p;
        }
        if let Some(a) = flag("auto_ortho_faces") {
            self.ui.auto_ortho_faces = a;
        }
        if let Some(b) = flag("pick_bodies") {
            self.ui.pick_bodies = b;
        }
        if let Some(a) = flag("auto_project") {
            self.session.auto_project = a;
        }
        if let Some(m) = v.get("autosave_minutes").and_then(Value::as_f64).filter(|m| m.is_finite()) {
            self.autosave_minutes = m.clamp(0.0, 600.0);
        }
        if let Some(s) = v.get("visual_style").and_then(Value::as_u64) {
            self.ui.visual_style = u8::try_from(s.min(3)).unwrap_or(0);
        }
        // Browser folders the user opened or closed.
        let keys = |k: &str| -> std::collections::BTreeSet<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().take(10_000).filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
        self.tree.collapsed = keys("browser_collapsed");
        self.tree.expanded = keys("browser_expanded");
        self.home.recent = v
            .get("recent")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).take(home::MAX_RECENT).map(str::to_string).collect())
            .unwrap_or_default();
        self.home.forgotten = keys("recent_forgotten");
        if let Some(s) = v.get("shortcut_box") {
            self.sbox.load(s);
        }
        if let Some(s) = v.get("shortcuts") {
            self.keymap.load(s);
        }
        if let Some(t) = v.get("toolbar") {
            self.toolbar_custom.load(t);
        }
        if let Some(p) = v.get("settings_profiles") {
            self.settings_profiles.load(p);
        }
        if let Some(mut p) = v.get("preferences").filter(|p| p.is_object()).map(prefs::lenient) {
            if !prefs::MSAA.contains(&p.msaa) {
                p.msaa = prefs::Prefs::default().msaa;
            }
            self.ui.layout = p.layout;
            self.preferences = p;
        }
        // Before a design given at start opens: its paths follow the network preferences.
        prefs::sync_surface_analysis(self);
        prefs::apply_network(&self.preferences);
        // Before a file given at start opens: the largest file it may be (#4086).
        prefs::apply_file_limits(&self.preferences);
        // Saved before the theme was a preference: the Dark/Light flag says which.
        if v.get("preferences").and_then(|p| p.get("theme")).is_none() {
            self.preferences.theme = if self.ui.dark { "dark" } else { "light" }.into();
        }
        if let Some(g) = flag("ground_shadow") {
            self.ui.ground_shadow = g;
        }
        if let Some(a) = v.get("agent_cursor").and_then(|a| serde_json::from_value(a.clone()).ok()) {
            self.ui.agent_cursor = a;
        }
    }

    pub fn with_control(mut self, rx: Receiver<ControlRequest>) -> Self {
        self.control_rx = Some(rx);
        self
    }

    /// Run a command programmatically (JSON parameters, no dialogs).
    pub fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        if self.file_load.busy() || self.file_picker.is_some() || self.saving.is_some() {
            return Err("a file operation is in progress".into());
        }
        // Commands may start only detected slicers and the programs the user added under Custom….
        if matches!(id, "file.save_mesh" | "file.send_to_slicer") {
            solidcraft_engine::slicers::set_chosen_programs(&self.preferences.custom_slicers);
        }
        if id == "timeline.play" {
            let reply = self.session.execute(id, &params).map_err(|e| e.to_string())?;
            let action = serde_json::from_value(reply["action"].clone()).map_err(|e| e.to_string())?;
            timeline::request_playback(self, action)?;
            return Ok(reply);
        }
        let t0 = now_ms();
        let revision = self.session.revision;
        if matches!(id, "render.lights.solo" | "render.lights.mute") {
            render_workspace::light::validate_visibility(self, &params)?;
        }
        let params = if matches!(id, "render.start" | "render.preflight") { render_workspace::light::filter_request(self, params)? } else { params };
        let selected_before = self.session.selection.clone();
        let r = self.session.execute(id, &params).map_err(|e| e.to_string()).and_then(|reply| {
            render_workspace::apply_command(self, id, &reply)?;
            Ok(reply)
        });
        self.big_operation |= now_ms() - t0 >= recovery_ui::BIG_OPERATION_MS;
        // An Undo offered by the last toast is stale once another command changed the design.
        if self.status_undo && revision != self.session.revision {
            self.status_undo = false;
            self.status = None;
        }
        let done = self.command_completed(id, r);
        if self.session.revision != revision {
            timeline::stop_playback(self);
        }
        let selection_changed = self.session.selection != selected_before || matches!(id, "select.set" | "select.clear" | "select.same_look");
        if done.is_ok() && selection_changed {
            self.tree.clear_folder_pick();
        }
        // Render focus already applies its semantic selection after clearing object picks.
        // A tab switch brings in the other design's own Render selection.
        if done.is_ok() && !matches!(id, "view.render_select" | "view.next_tab" | "view.previous_tab") && selection_changed {
            if self.session.selection.is_empty() && render_workspace::active(self) {
                render_workspace::selection_editor(self, &render_workspace::RenderSelection::Nothing);
            }
            self.render_workspace.view.selection = if self.session.selection.is_empty() {
                render_workspace::RenderSelection::Nothing
            } else {
                render_workspace::RenderSelection::Object(self.session.selection.clone())
            };
        }
        if let Ok(v) = &done {
            if matches!(id, "render.lights.solo" | "render.lights.mute") {
                render_workspace::light::apply_visibility(self, id, v);
            }
            if matches!(id, "render.lights.set" | "render.lights.add" | "render.lights.duplicate" | "render.lights.remove") {
                render_workspace::light::edited(self, &params, v);
            }
        }
        done
    }

    /// Host effects shared by directly executed and already evaluated commands.
    pub(crate) fn command_completed(&mut self, id: &str, r: Result<Value, String>) -> Result<Value, String> {
        // The engine has no edit in place to start or finish; the window does, and says when it can't.
        let r = match solidcraft_engine::find_command(id).map(|spec| spec.id) {
            // `queued`: it waits for the end of the agent's turn.
            Some("component.edit_in_place") => r.and_then(|v| edit_in_place::command_ran(self, &v).map(|queued| with_queued(v, queued))),
            Some("component.finish_edit") => r.and_then(|v| edit_in_place::finish_command(self).map(|queued| with_queued(v, queued))),
            _ => r,
        };
        if let Ok(v) = &r {
            match id {
                "view.fit" | "view.home" => self.frame_view(id == "view.home"),
                "view.tool" => {
                    self.viewport.nav = match v.get("tool").and_then(Value::as_str) {
                        Some("orbit") => Some(viewport::NavMode::Orbit),
                        Some("pan") => Some(viewport::NavMode::Pan),
                        Some("zoom") => Some(viewport::NavMode::Zoom),
                        _ => None,
                    };
                }
                "view.spin" => {
                    self.cancel_view_animation();
                    self.viewport.spin = v.get("on").and_then(Value::as_bool).unwrap_or(!self.viewport.spin);
                }
                "view.visual_style" => {
                    if let Some(index) = v.get("index").and_then(Value::as_u64).and_then(|n| u8::try_from(n).ok()) {
                        self.ui.visual_style = index;
                    }
                }
                "view.grid" => self.ui.show_grid = v.get("on").and_then(Value::as_bool).unwrap_or(!self.ui.show_grid),
                "view.ground_shadow" => self.ui.ground_shadow = v.get("on").and_then(Value::as_bool).unwrap_or(!self.ui.ground_shadow),
                "view.navigation_bar" => {
                    self.ui.show_navigation_bar = v.get("visible").and_then(Value::as_bool).unwrap_or(!self.ui.show_navigation_bar)
                }
                "view.next_tab" | "view.previous_tab" => documents::cycle(self, id == "view.next_tab"),
                // An edit OK'd on a feature behind the history marker changes nothing in the view:
                // say why, or the edit looks as if it had silently failed (#8100).
                "timeline.redefine" if v.get("rolled_back").and_then(Value::as_bool) == Some(true) => {
                    let feature = v.get("feature").and_then(Value::as_u64).and_then(|f| self.session.doc.feature(f));
                    let name = feature.map(|f| f.name.clone()).unwrap_or_default();
                    self.set_status(i18n::format("timeline.edited_rolled_back", &[("name", &name)]), false);
                }
                _ => {}
            }
            if id == "view.render_inspector" {
                render_workspace::set_inspector_visible(self, v.get("visible").and_then(Value::as_bool).unwrap_or(true));
            }
            if id == "view.render_browser" {
                render_workspace::apply_browser_controls(self, v);
            }
            if id == "appearance.favorite" {
                render_workspace::assign::favorite_applied(self, v);
            }
            if id == "view.render_frame" {
                render_workspace::shot::apply_frame(self, v);
            }
            if id == "view.render_select" {
                use render_workspace::RenderSelection;
                let selected = match v.get("kind").and_then(Value::as_str) {
                    Some("environment") => RenderSelection::Environment,
                    Some("wear") => RenderSelection::Wear,
                    Some("lighting") => {
                        render_workspace::select_lighting(self);
                        RenderSelection::Lighting
                    }
                    Some("output") => RenderSelection::Output,
                    Some("sampling") => RenderSelection::Sampling,
                    Some("effects") => RenderSelection::Effects,
                    Some("check") => RenderSelection::Check,
                    Some("light") => {
                        render_workspace::select_lighting(self);
                        RenderSelection::Light(v.get("id").and_then(Value::as_str).unwrap_or_default().into())
                    }
                    Some("shot") => RenderSelection::Shot(v.get("id").and_then(Value::as_str).unwrap_or_default().into()),
                    _ => RenderSelection::Nothing,
                };
                render_workspace::selection_editor(self, &selected);
                self.render_workspace.view.selection = selected;
            }
            if id == "view.layout" {
                let layout =
                    if v.get("mode").and_then(Value::as_str) == Some("compact") { shell::LayoutMode::Docked } else { shell::LayoutMode::Floating };
                self.preferences.layout = layout;
                self.ui.layout = layout;
            }
            if matches!(id, "browser.expand_all" | "browser.collapse_all") {
                let strings = |key: &str| -> Vec<String> {
                    v.get(key)
                        .and_then(Value::as_array)
                        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).take(10_000).collect())
                        .unwrap_or_default()
                };
                browser::apply_fold(
                    self,
                    v.get("open").and_then(Value::as_bool).unwrap_or(id == "browser.expand_all"),
                    &strings("open_by_default"),
                    &strings("closed_by_default"),
                );
            }
            if id == "view.render_focus"
                && let Some(on) = v.get("on").and_then(Value::as_bool)
            {
                self.shell.render_focus = on;
                render_ui::reset_shell(self);
                self.shell.content = None;
            }
            if id == "view.render_browser"
                && let Some(open) = v.get("open").and_then(Value::as_bool)
            {
                self.shell.render_browser_open = open;
            }
            if id == "view.render_stage"
                && let Some(stage) = v.get("stage").and_then(Value::as_str)
            {
                render_workspace::select_stage(self, stage);
            }
            if id == "select.filter" {
                let mode = v.get("mode").and_then(Value::as_str);
                self.ui.pick_bodies = mode == Some("body");
                self.ui.pick_components = mode == Some("component");
                self.ui.pick_off.retain(|kind| kind != "faces");
            }
            if id == "view.render_dock"
                && let (Some(panel), Some(side)) = (v.get("panel").and_then(Value::as_str), v.get("side").and_then(Value::as_str))
            {
                let other = if side == "left" { "right" } else { "left" };
                if panel == "inspector" {
                    self.preferences.render_inspector_side = side.into();
                    self.preferences.render_browser_side = other.into();
                } else {
                    self.preferences.render_browser_side = side.into();
                    self.preferences.render_inspector_side = other.into();
                }
            }
            if id == "view.shots_strip"
                && let Some(visible) = v.get("visible").and_then(Value::as_bool)
            {
                self.preferences.render_shots_strip = visible;
            }
            if id == "view.history_layout" {
                if let Some(width) = v.get("width").and_then(Value::as_f64) {
                    self.preferences.timeline_left_width = ui_kit::history_snap_width(width as f32);
                }
                if let Some(dock) = v.get("dock").and_then(Value::as_str) {
                    self.preferences.timeline_left = !matches!(dock, "bottom" | "browser");
                    self.preferences.history_in_browser = dock == "browser";
                    self.preferences.timeline_under_browser = dock == "under_browser";
                    self.preferences.timeline_collapsed = false;
                    self.preferences.timeline_left_browser_collapsed = false;
                }
                if let Some(collapsed) = v.get("collapsed").and_then(Value::as_bool) {
                    self.preferences.timeline_collapsed = collapsed;
                }
            }
            if id == "view.perspective"
                && let Some(perspective) = v.get("enabled").and_then(Value::as_bool)
            {
                self.ui.perspective = perspective;
            }
            if id == "view.angle_readout" {
                self.preferences.show_view_angles = v.get("enabled").and_then(Value::as_bool).unwrap_or(false);
            }
            if matches!(id, "view.ambient_occlusion" | "view.graphics_preset") {
                self.preferences.ambient_occlusion = v.get("enabled").and_then(Value::as_bool).unwrap_or(false);
                if id == "view.graphics_preset" {
                    self.preferences.display_quality = v.get("display_quality").and_then(Value::as_str).unwrap_or("normal").to_string();
                    self.preferences.supersampling = v.get("supersampling").and_then(Value::as_f64).unwrap_or(1.0) as f32;
                    self.preferences.supersampling_adaptive = v.get("supersampling_adaptive").and_then(Value::as_bool).unwrap_or(true);
                }
            }
            if id == "view.navigate" {
                let to: Camera = serde_json::from_value(v.get("camera").cloned().unwrap_or_default()).map_err(|e| e.to_string())?;
                if v.get("continuous").and_then(Value::as_bool).unwrap_or(false) {
                    self.cancel_view_animation();
                    self.cam = to;
                } else if v.get("fit").and_then(Value::as_bool).unwrap_or(false) {
                    let bounds = solidcraft_engine::view::bounds(&self.session);
                    self.animate_to(self.fitted(to));
                    self.pending_fit = Some((to, bounds, true));
                } else {
                    self.animate_to(to);
                }
            }
            // Curves drawn in construction (or centerline) mode take that line type.
            sketch_palette::apply_mode(self, id, v);
        }
        // Settings profiles live in the app: the engine only checked the parameters.
        let r = match r {
            Ok(v) if id.starts_with("settings.profile.") => settings_profiles::command(self, id, &v),
            r => r,
        };
        self.finish_command(id, &r);
        if render_workspace::active(self)
            && matches!(id, "appearance.assign" | "appearance.paste")
            && let Ok(value) = &r
        {
            render_workspace::assign::assigned(self, value);
        }
        r
    }

    /// What follows a command's run, whether it ran here or on a worker (a save): its echo,
    /// the status line when it failed, what opening and saving update, and the toolbar tab.
    pub(crate) fn finish_command(&mut self, id: &str, r: &Result<Value, String>) {
        let id = solidcraft_engine::find_command(id).map_or(id, |spec| spec.id);
        match r {
            Ok(v) => {
                // Programmatic callers (the control channel, scripts) run outside a render
                // language scope, so resolve the echo from the saved preference instead of the
                // thread-local scope. The command's own label is translated too, so the echo
                // reads the same way the toolbar button it came from does.
                let language = i18n::Locale::for_language(&self.preferences.language);
                let label = language.label(command_label(id));
                // Frames of a gesture (a continuous view move, a part dragged by its joints) are
                // not echoed one by one.
                if v.get("continuous").and_then(Value::as_bool) != Some(true) {
                    self.session.echo(language.format("ui.command_done", &[("command", label)]));
                }
                if id == "file.new" {
                    // `file.new` starts from millimetres. Units of new designs still apply (#1205).
                    prefs::apply_new_design(self);
                }
                if matches!(id, "file.new" | "doc.open" | "doc.recover") {
                    documents::reset_chat(self);
                    self.edit_in_place.active = None;
                    render_workspace::reset_document(self);
                    dim_view::cancel();
                    design_properties::close_object(self);
                    timeline::stop_playback(self);
                    self.menu.props = None;
                    sketch_attachment_ui::clear(self);
                    // The old document is already replaced: discard its editor without
                    // applying cancellation hooks (such as restoring its timeline marker).
                    self.dialog = None;
                    self.applying = None;
                    self.editing_state = None;
                    self.pending_feature_edit = None;
                    self.tool = None;
                    self.preview.reset();
                    self.cam = Camera::default();
                    self.fit_view();
                }
                if matches!(id, "doc.open" | "file.save" | "file.save_as")
                    && let Some(p) = self.session.path.clone()
                {
                    self.home.add_recent(&p);
                    if matches!(id, "file.save" | "file.save_as") {
                        home::remember_session(&mut self.home, &self.session);
                        // Say where it went, as an export does (#8693).
                        let saved = self.tr("ui.design_saved").replace("{path}", &p);
                        self.set_status(saved, false);
                    }
                }
            }
            Err(e) => {
                let msg = friendly_error(id, e);
                self.session.echo(msg.clone());
                self.set_status(msg, true);
            }
        }
        if self.session.active_sketch.is_none() && self.ui.tab == "SKETCH" {
            self.tool = None;
        }
        workspace::sync_context(self);
    }

    /// Start the last tool or dialog started again, with its defaults (Repeat in the menus,
    /// the `edit.repeat` command and its key, Shift+R by default; #3045, #7077). `false` when
    /// none was started yet: the status line says so.
    pub fn repeat_last(&mut self) -> bool {
        let Some((id, _)) = self.last_command.clone() else {
            self.set_status(self.tr("ui.repeat_nothing").to_string(), false);
            return false;
        };
        self.start(&id);
        true
    }

    /// Start a command the way a toolbar click does: interactive tools, menus and dialogs for commands
    /// that have them, otherwise run it with defaults.
    pub fn start(&mut self, id: &str) {
        self.start_at(id, None);
    }

    /// Insert Design's file picker: another design becomes a linked component; a STEP or mesh
    /// file picked here is inserted the way Insert STEP or Insert Mesh would (#1240).
    pub(crate) fn pick_design_to_insert(&mut self) {
        self.pick_open(|app, path| {
            let Some(path) = path else { return };
            if solidcraft_engine::io::is_design_path(&path) {
                // Picked in the file dialog: its server is the user's choice.
                let hosts: Vec<String> = solidcraft_engine::io::vfs::network_host(&path).into_iter().collect();
                let inserted = solidcraft_engine::io::vfs::allowing_hosts(&hosts, || app.run("component.insert_design", json!({ "path": path })));
                if inserted.is_ok() {
                    app.fit_view();
                }
            } else {
                app.insert_path_async(&path);
            }
        });
    }

    /// A command that needs one choice from a short list: its menu under the button that started it
    /// (a second click closes it), or in the middle of the viewport when started without one
    /// (command search, a shortcut, `ui.start`).
    fn open_choice_menu(&mut self, opener: Option<egui::Rect>, target: context_menu::Target) {
        if let Some(rect) = opener {
            context_menu::toggle_for(self, rect, rect.left_bottom(), target);
        } else {
            let at = self.viewport.rect.map(|rect| rect.center()).unwrap_or(egui::pos2(300.0, 200.0));
            context_menu::open_for(self, at, target);
        }
    }

    /// Start an interactive command with its menu anchored below the invoking button.
    pub(crate) fn start_at(&mut self, id: &str, opener: Option<egui::Rect>) {
        self.start_with_variant_memory(id, true, opener, false);
    }

    /// A dedicated toolbar button starts its exact tool without replacing the sketch family's
    /// main button. Choosing a variant from a menu still uses `start` and remembers that choice.
    pub(crate) fn start_pinned(&mut self, id: &str, opener: Option<egui::Rect>) {
        self.start_with_variant_memory(id, false, opener, false);
    }

    /// Existing Section editor uses the normal command-start lifecycle, with explicit edit intent.
    pub(crate) fn start_section_edit(&mut self) {
        self.start_with_variant_memory("inspect.section", true, None, true);
    }

    fn start_with_variant_memory(&mut self, id: &str, remember_variant: bool, opener: Option<egui::Rect>, section_edit: bool) {
        self.tree.clear_folder_pick();
        if id == "timeline.play" {
            if let Err(why) = self.run(id, json!({})) {
                self.set_status(why, true);
            }
            return;
        }
        if id == "ui.palette" {
            shortcut_box::open(self, None);
            return;
        }
        timeline::stop_playback(self);
        // A file is loading: the design waits for it.
        if self.file_load.busy() || self.file_picker.is_some() {
            return;
        }
        if id != "occurrence.move" {
            joint_handles::close(self);
        }
        // Repeat Last Command starts the tool it names; it is never the one repeated itself.
        if id == "edit.repeat" {
            self.repeat_last();
            return;
        }
        // Front … Isometric (Ctrl+1…7 by default) turn the view as the view cube's faces do. The
        // flat pattern is a 2D page: it has no other side to turn to.
        if solidcraft_engine::standard_view(id).is_some() {
            if !dialogs_sheet::flat_active(self) {
                // `view.isometric` → "isometric", which `StandardView::parse` reads as Iso.
                self.animate_view(id.trim_start_matches("view."));
                self.home.open = false;
            }
            return;
        }
        // Print Face (Ctrl+B) acts on the selected face at once: it has no dialog.
        if id == "body.print_face" {
            print_face_ui::toggle(self);
            return;
        }
        if id == "view.normal_to" {
            if self.run(id, json!({"camera": self.cam})).is_ok() {
                self.follow_view_request(true);
                self.home.open = false;
            }
            return;
        }
        // A drawing tool on the SKETCH tab kept shown outside a sketch (#7904) starts Create
        // Sketch, on the selected plane or planar face or else on the plane picked next, and then
        // the tool, as the tool's shortcut does outside a sketch (`keymap::press`).
        if workspace::starts_a_sketch(self, id)
            && let Some(then) = solidcraft_engine::find_command(id).map(|c| c.id)
        {
            self.start("sketch.create");
            if let Some(d) = self.dialog.as_mut().filter(|d| matches!(d.kind, dialogs::Kind::Sketch)) {
                d.then_start = Some(then);
            }
            // The ribbon stays on SKETCH while the plane is picked, not on Create Sketch's MODEL.
            self.ui.tab = "SKETCH".into();
            return;
        }
        // A disabled command starts nothing, however it was reached. The toolbar greys its
        // button, but a shortcut, the command palette and the control channel all land here.
        // Its reason goes to the status line, so the refusal is never silent.
        if let Some(info) = solidcraft_engine::find_command(id).map(|c| c.info(&self.session))
            && let Some(why) = info.disabled_reason
        {
            let label = self.tr_label(info.label).to_string();
            let why = self.tr_label(&why).to_string();
            self.set_status(format!("{label}: {why}"), true);
            return;
        }
        if render_workspace::start_interactive(self, id) {
            return;
        }
        workspace::select_command_tab(self, id);
        if id == "occurrence.move" {
            joint_handles::start(self);
            return;
        }
        // One selected placement moves as a component, the same way as its Browser menu,
        // retaining the exact instance path and the grounding/rigid-group restrictions.
        // A selected body, or a component picked in the tree, opens Move/Copy on those bodies.
        if id == "solid.move"
            && let [solidcraft_engine::Sel::Occurrence { path }] = self.session.selection.as_slice()
        {
            let target = context_menu::Target::Occurrence { path: path.clone() };
            if let Some(item) = context_menu::items(self, &target).into_iter().find(|item| item.id == "ui.moveOccurrence") {
                context_menu::run_item(self, &item, egui::Pos2::ZERO);
            }
            return;
        }
        if id == "inspect.colored_components" {
            drop(self.run(id, serde_json::json!({})));
            return;
        }
        if id == "material.properties.validate" {
            self.material_card_review = Some(material_card_review::ReviewWindow::default());
            self.home.open = false;
            return;
        }
        if id == "library.export" {
            dialogs_library::start_export(self);
            return;
        }
        // A display toggle leaves the current modelling tool and camera in place.
        if matches!(
            id,
            "view.fit"
                | "view.home"
                | "view.spin"
                | "view.grid"
                | "view.ground_shadow"
                | "view.navigation_bar"
                | "view.next_tab"
                | "view.previous_tab"
        ) {
            let _ = self.run(id, json!({}));
            return;
        }
        if id == "view.layout" {
            let mode = if self.ui.layout == shell::LayoutMode::Floating { "compact" } else { "floating" };
            let _ = self.run(id, json!({"mode":mode}));
            return;
        }
        // A display choice opens its choices, as the navigation bar's menus do: run with no
        // parameters it could only fail (#8868).
        let choice = match id {
            "view.visual_style" => Some(context_menu::Target::VisualStyle),
            "view.tool" => Some(context_menu::Target::ViewTool),
            _ => None,
        };
        if let Some(target) = choice {
            self.open_choice_menu(opener, target);
            return;
        }
        if id == "view.render_focus" {
            let _ = self.run(id, json!({"on": !self.shell.render_focus}));
            return;
        }
        if id == "view.render_browser" {
            let _ = self.run(id, json!({"open": !self.shell.render_browser_open}));
            return;
        }
        if id == "view.shots_strip" {
            let _ = self.run(id, json!({"visible": !self.preferences.render_shots_strip}));
            return;
        }
        if id == "view.history_layout" {
            let _ = self.run(id, json!({}));
            return;
        }
        // Profiles are named and picked in Preferences.
        if id.starts_with("settings.profile.") {
            prefs::open_at(self, prefs::PAGE_GENERAL);
            return;
        }
        if id == "view.ambient_occlusion" {
            let _ = self.run(id, json!({"enabled": !self.preferences.ambient_occlusion}));
            return;
        }
        // Send to Slicer always asks which slicer (the dialog pre-selects the last one).
        if matches!(id, "file.send_to_slicer" | "file.open_in_easyprint") && !dialogs_save_mesh::available(self) {
            self.set_status(self.tr("ui.send_to_slicer_unavailable").to_string(), true);
            return;
        }
        if id == "view.perspective" {
            let _ = self.run(id, json!({"enabled": !self.ui.perspective}));
            return;
        }
        if id == "view.angle_readout" {
            let _ = self.run(id, json!({"enabled": !self.preferences.show_view_angles}));
            return;
        }
        // Another command ends Edit Text (its dialog is not a feature edit to keep).
        sketch_text_move::cancel_drag(self);
        if sketch_text::editing(self) || self.dialog.as_ref().is_some_and(|d| matches!(d.kind, dialogs::Kind::TextMove(_))) {
            dialogs::cancel(self);
        }
        // A component moved without Capture Position: ask first (Capture, Revert or Cancel).
        if browser::needs_capture(self, id) && !(id == "sketch.dimension" && self.session.active_sketch.is_some()) {
            self.tree.capture_prompt = Some(id.to_string());
            return;
        }
        // A line-type toggle changes the current drawing, including a partly picked shape.
        // Handle it before cancelling tools or inline dimensions.
        if id == "sketch.construction" {
            sketch_palette::toggle_construction(self);
            return;
        }
        dim_view::cancel();
        self.tool = None;
        self.home.open = false;
        if dialogs_sheet::start_export(self, id) {
            return;
        }
        if id == "component.finish_edit" {
            edit_in_place::finish(self);
            return;
        }
        // The picked drawing is placed (position and scale, previewed) before OK adds it.
        if let Some(ext) = id.strip_prefix("sketch.insert_").filter(|e| matches!(*e, "dxf" | "svg")) {
            let command = id.to_string();
            self.pick_open_for(&[ext], move |app, path| {
                if let Some(path) = path {
                    dialogs_insert_drawing::open(app, &command, path);
                }
            });
            return;
        }
        if id == "component.update_links" && self.edit_in_place.active.is_some() {
            // In a part edited in place it also refreshes the assembly around it.
            edit_in_place::refresh(self);
            if self.session.doc.links.is_empty() {
                return;
            }
        }
        if id == "component.update_links" && !self.session.doc.links.is_empty() {
            // Reading the linked designs again takes a while in a large assembly: off the window
            // thread, the overlay naming the one being read (#5329). A link on a network server
            // not allowed yet asks first.
            network_prompt::update_links(self);
            return;
        }
        if id == "component.insert_design" {
            // The designs open in other tabs first, each a click away, then Browse… (#8570).
            if self.preferences.insert_lists_open_designs && !documents::other_designs(self).is_empty() {
                if let Some(rect) = opener {
                    context_menu::toggle_for(self, rect, rect.left_bottom(), context_menu::Target::InsertDesign);
                } else {
                    let at = self.viewport.rect.map(|rect| rect.center()).unwrap_or(egui::pos2(300.0, 200.0));
                    context_menu::open_for(self, at, context_menu::Target::InsertDesign);
                }
            } else {
                self.pick_design_to_insert();
            }
            return;
        }
        if id == "component.relink" {
            linked_source::start_selected(self);
            return;
        }
        if matches!(id, "file.insert_step" | "file.insert_mesh" | "pointcloud.insert") {
            self.pick_open(|app, path| {
                if let Some(path) = path {
                    app.insert_path_async(&path);
                }
            });
            return;
        }
        if !section_edit && !sketch_tools::start_hook(self, id) {
            return;
        }
        if remember_variant && solidcraft_engine::find_command(id).is_some_and(|c| c.info(&self.session).enabled) {
            self.preferences.sketch_variants.remember(id);
        }
        if id == "document.units" {
            if let Some(rect) = opener {
                context_menu::toggle_for(self, rect, rect.left_bottom(), context_menu::Target::Units);
            } else {
                let at = self.viewport.rect.map(|rect| rect.center()).unwrap_or(egui::pos2(300.0, 200.0));
                context_menu::open_for(self, at, context_menu::Target::Units);
            }
            return;
        }
        if id == "parameters.change" {
            params_dialog::open(self);
            return;
        }
        if matches!(id, "document.metadata" | "document.metadata.set") {
            design_properties::open(self);
            return;
        }
        if id == "sketch.inspect" {
            sketch_report::open(self);
            return;
        }
        if id == "assembly.bom" {
            bom_panel::open(self);
            return;
        }
        // Inspect → Measure → Properties: the window, not a reply nobody sees (#8095).
        if matches!(id, "component.properties" | "occurrence.properties") {
            design_properties::open_selected(self);
            return;
        }
        if id == "sketch.finish" {
            self.finish_sketch();
            return;
        }
        if id == "sketch.cancel" {
            self.cancel_sketch();
            return;
        }
        // Press Pull: edges get a fillet, a fillet's round face reopens its fillet (its size
        // changes, as `solid.press_pull` does: #1932), other curved faces an offset, planar faces
        // and profiles an extrude.
        if id == "solid.press_pull" {
            if let Some(pending) = &mut self.pending_feature_edit {
                pending.then_start(id);
                return;
            }
            if self.session.selection.iter().any(|s| matches!(s, solidcraft_engine::Sel::Edge { .. })) {
                return self.start("solid.fillet");
            }
            let sel = &self.session.selection;
            let curved = sel.iter().find_map(|s| match s {
                solidcraft_engine::Sel::Face { body, index, .. } if dialogs::planar_face(&self.session, body, *index).is_none() => {
                    Some((body, *index))
                }
                _ => None,
            });
            if let Some((body, index)) = curved {
                if let Some(blend) = dialogs::blend_feature(&self.session, body, index) {
                    return self.edit_feature(blend);
                }
                return self.start("solid.offset_face");
            }
            let empty = sel.is_empty();
            self.start("solid.extrude");
            if empty
                && let Some(d) = &mut self.dialog
                && let Some(input) = d.inputs.first_mut()
            {
                input.accept |= selection::FACES | selection::EDGES;
                d.press_pull = true;
            }
            return;
        }
        let interactive = self.interactive(id);
        if interactive && let Some(pending) = &mut self.pending_feature_edit {
            pending.then_start(id);
            return;
        }
        // Cancel before finishing a sketch or waiting for its history marker.
        if interactive && (self.editing_state.is_some() || self.applying.is_some()) {
            dialogs::cancel(self);
        }
        let finished_sketch = self.session.active_sketch.filter(|_| ends_sketch(id) && self.interactive(id));
        if finished_sketch.is_some() {
            self.finish_sketch();
            // A feature committed before the marker is back would go in where the sketch was.
            if self.deferred.is_some() {
                self.start_after_roll = finished_sketch.map(|sketch| (id.to_string(), sketch));
                return;
            }
        }
        self.start_interactive(id, finished_sketch);
    }

    /// The rest of [`SolveApp::start`], once any sketch `id` ends is finished.
    pub(crate) fn start_interactive(&mut self, id: &str, finished_sketch: Option<u64>) {
        if let Some(spec) = solidcraft_engine::find_command(id)
            && self.interactive(id)
            && id != "sketch.create"
        {
            self.last_command = Some((spec.id.to_string(), spec.label.to_string()));
        }
        if id == "sketch.project"
            && self.preferences.project_options_dialog
            && let Some(mut d) = dialogs::Dialog::for_command(self, id)
        {
            if let Some(sketch) = finished_sketch {
                d.take_sketch_profile(&self.session, sketch);
            }
            if d.wants_picks() && !self.session.selection.is_empty() {
                let _ = self.session.execute("select.clear", &json!({}));
            }
            self.dialog = Some(d);
            return;
        }
        if let Some(mut t) = tools::Tool::for_command(id) {
            let state = self.session.world_state();
            let sketch = self.session.active_sketch.and_then(|s| state.sketch(s)).map(|ss| &ss.sketch);
            if let Some(params) = tools::constraint_preselection(id, &self.session.selection, sketch) {
                if self.run(id, params).is_ok() {
                    let _ = self.session.execute("select.clear", &json!({}));
                    self.set_status("Constraint applied", false);
                }
                return;
            }
            if id == "sketch.project"
                && let Some(refs) = tools::projection_preselection(&self.session.selection)
            {
                if self.run(id, json!({"refs":refs})).is_ok() {
                    let _ = self.session.execute("select.clear", &json!({}));
                    self.set_status("Projection applied", false);
                }
                return;
            }
            // Curve-pair constraints can consume an exact preselection. Leave ambiguous
            // selections to the ordinary pick tool; never silently constrain a subset.
            if matches!(
                id,
                "sketch.constraint.parallel"
                    | "sketch.constraint.perpendicular"
                    | "sketch.constraint.equal"
                    | "sketch.constraint.tangent"
                    | "sketch.constraint.concentric"
                    | "sketch.constraint.collinear"
            ) && let [solidcraft_engine::Sel::SketchCurve { id: a, .. }, solidcraft_engine::Sel::SketchCurve { id: b, .. }] =
                self.session.selection.as_slice()
                && a != b
            {
                let params = json!({ "a": a, "b": b });
                if self.run(id, params).is_ok() {
                    let _ = self.session.execute("select.clear", &json!({}));
                    self.set_status("Constraint applied", false);
                }
                return;
            }
            if id == "sketch.dimension" {
                t.picks = self
                    .session
                    .selection
                    .iter()
                    .filter_map(|s| match s {
                        solidcraft_engine::Sel::SketchCurve { id, .. } | solidcraft_engine::Sel::SketchPoint { id } => Some(id.clone()),
                        _ => None,
                    })
                    .take(2)
                    .collect();
                let _ = self.session.execute("select.clear", &json!({}));
            }
            self.tool = Some(t);
            self.set_status(tools::hint(id), false);
            return;
        }
        if let Some(mut d) = dialogs::Dialog::for_command(self, id) {
            if let Some(sketch) = finished_sketch {
                d.take_sketch_profile(&self.session, sketch);
            }
            // The pre-selection now belongs to the dialog's inputs.
            if d.wants_picks() && !self.session.selection.is_empty() {
                let _ = self.session.execute("select.clear", &json!({}));
            }
            self.dialog = Some(d);
            return;
        }
        // Undo and redo can recompute much of the history: off the window thread when slow.
        if matches!(id, "edit.undo" | "edit.redo") {
            let _ = self.run_heavy(id, json!({}));
            return;
        }
        // Change Part Size with no standard part selected (its dialog had nothing to open on).
        if id == "parts.resize" {
            dialogs_part_size::needs_a_part(self);
            return;
        }
        let _ = self.run(id, json!({}));
    }

    /// Does starting `id` open a tool or a dialog?
    fn interactive(&self, id: &str) -> bool {
        tools::Tool::for_command(id).is_some() || dialogs::Dialog::for_command(self, id).is_some()
    }

    /// Edit a feature: sketches open in sketch mode; other features roll the
    /// timeline back to just before themselves and reopen their dialog, filled in.
    pub fn edit_feature(&mut self, id: u64) {
        use solidcraft_engine::doc::FeatureKind;
        dialogs::cancel(self);
        let Some(f) = self.session.doc.feature(id) else { return };
        if matches!(f.kind, FeatureKind::Sketch { .. }) {
            self.edit_sketch(id);
            return;
        }
        let Some(idx) = self.session.doc.feature_index(id) else { return };
        let marker = self.finish_sketch_for_edit();
        self.tool = None;
        if self.session.doc.marker != marker {
            self.prepare_feature_edit(id, idx, marker);
            return;
        }
        let original = self.session.scratch();
        if dialogs::roll_marker(self, Some(idx)).is_err() {
            return;
        }
        self.open_feature_edit(id, marker, original);
    }

    fn open_feature_edit(&mut self, id: u64, marker: Option<usize>, original: Session) {
        self.editing_state = Some(dialogs::EditingState { source: self.session.scratch(), original });
        match dialogs::for_feature(self, id, marker) {
            Some(d) => {
                if !d.previews() {
                    self.editing_state = None;
                }
                self.dialog = Some(d);
            }
            None => {
                dialogs::restore_editing(self, marker);
                self.sbox.text = format!("timeline.edit {{\"feature\": {id}, \"set\": {{}}}}");
                self.ui.palette_open = true;
            }
        }
    }

    /// Edit a sketch: roll the timeline to just after it (back, so later features don't hide
    /// it, or forward from before it, where it isn't built yet: #2716), turn the camera to face
    /// it, and remember the marker so Finish Sketch puts it back. Editing the sketch that is already open does nothing; editing another one
    /// finishes the current sketch first, so edit sessions never nest.
    pub fn edit_sketch(&mut self, id: u64) {
        self.edit_sketch_in(id, None);
    }

    /// Edit a sketch as [`SolveApp::edit_sketch`] does, in one placement of its component (a
    /// sketch row inside a component in the browser): that component is active while the sketch
    /// is open, and the one active before comes back when the sketch ends (#8127).
    pub fn edit_sketch_in(&mut self, id: u64, placement: Option<(u64, &[u64])>) {
        let activate = |app: &mut Self| {
            if let Some((component, path)) = placement {
                let _ = app.run("component.activate", json!({"component": component, "occurrence": path.last(), "path": path}));
            }
        };
        if let solidcraft_engine::doc::FaceAttachmentStatus::Lost { message, .. } =
            self.session.placed_document().sketch_face_attachment(id, &self.session.model)
        {
            activate(self);
            browser::redefine_sketch(self, id);
            self.set_status(message, true);
            return;
        }
        if self.session.active_sketch == Some(id) {
            return;
        }
        let Some(idx) = self.session.doc.feature_index(id) else { return };
        let marker = self.finish_sketch_for_edit();
        let before = self.settled_cam();
        let context = (self.session.active_component, self.session.active_occurrence_path.clone());
        self.tool = None;
        self.dialog = None;
        activate(self);
        let rolled = self.session.doc.marker != Some(idx + 1) && self.run("timeline.roll_to", json!({ "position": idx + 1 })).is_ok();
        if self.run("sketch.edit", json!({ "sketch": id })).is_err() {
            if rolled || marker != self.session.doc.marker {
                self.restore_marker(marker);
            }
            self.restore_component(context.0, context.1);
            return;
        }
        self.pre_sketch_marker = (rolled || marker != self.session.doc.marker).then_some(marker);
        // `sketch.edit` itself activates the component a face-attached sketch was drawn in.
        let opened_in = self.session.active_component;
        let moved = (opened_in, &self.session.active_occurrence_path) != (context.0, &context.1);
        self.pre_sketch_component = moved.then_some(PreSketchComponent { sketch: id, component: context.0, path: context.1, opened_in });
        self.look_at_opened_sketch(before);
    }

    /// Make a component active again, in the placement it was active in.
    fn restore_component(&mut self, component: u64, path: Option<Vec<u64>>) {
        if (self.session.active_component, &self.session.active_occurrence_path) == (component, &path) {
            return;
        }
        let mut p = json!({ "component": component });
        if let Some(path) = path {
            p["occurrence"] = json!(path.last());
            p["path"] = json!(path);
        }
        let _ = self.run("component.activate", p);
    }

    /// Turn the view to face the sketch just opened, unless the "Turn to face a sketch when it
    /// opens" preference is off, and remember the view from `before` it and the one facing it for
    /// when the sketch ends (#7038). Returns whether the view turned.
    pub(crate) fn look_at_opened_sketch(&mut self, before: Camera) -> bool {
        let turn = self.preferences.sketch_open_look_at;
        if turn {
            dialogs::look_at_sketch(self);
        }
        self.pre_sketch_cam = self.session.active_sketch.map(|sketch| PreSketchView { sketch, before, facing: self.settled_cam() });
        turn
    }

    /// Where the camera is going: the end of the running camera animation, else where it is.
    pub(crate) fn settled_cam(&self) -> Camera {
        self.cam_anim.map_or(self.cam, |a| a.to)
    }

    /// Pass the original marker to the next editor. Restoring it between editors could finish
    /// in the background after the next rollback, moving the marker out from under that edit.
    fn finish_sketch_for_edit(&mut self) -> Option<usize> {
        let marker = self
            .pending_feature_edit
            .take()
            .map(|pending| pending.marker)
            .unwrap_or_else(|| self.pre_sketch_marker.take().unwrap_or(self.session.doc.marker));
        if self.session.active_sketch.is_some() {
            self.finish_sketch();
        }
        marker
    }

    /// Finish the active sketch: stop editing and restore the history marker Edit Sketch moved.
    /// The camera goes where the "When a sketch ends" preference says (#7038); by default it
    /// stays where it is, however the sketch is left.
    pub fn finish_sketch(&mut self) {
        self.leave_sketch("sketch.finish");
    }

    /// Cancel Sketch (#3838): leave the sketch without its changes. When the command refuses (the
    /// undo history no longer leads back to where the sketch was opened), the sketch stays open.
    pub fn cancel_sketch(&mut self) {
        self.leave_sketch("sketch.cancel");
    }

    fn leave_sketch(&mut self, command: &str) {
        sketch_text_move::cancel_drag(self);
        if sketch_text::editing(self) || self.dialog.as_ref().is_some_and(|d| matches!(d.kind, dialogs::Kind::TextMove(_))) {
            dialogs::cancel(self);
        }
        dim_view::cancel();
        self.tool = None;
        let open = self.session.active_sketch;
        if self.run(command, json!({})).is_err() && command != "sketch.finish" {
            return;
        }
        if let Some(marker) = self.pre_sketch_marker.take() {
            self.restore_marker(marker);
        }
        // Unless another component was activated meanwhile: that one stays.
        if let Some(pre) = self.pre_sketch_component.take().filter(|pre| Some(pre.sketch) == open && pre.opened_in == self.session.active_component) {
            self.restore_component(pre.component, pre.path);
        }
        // Only the views saved when this sketch opened: one opened another way saved none.
        if let Some(view) = self.pre_sketch_cam.take().filter(|v| Some(v.sketch) == open)
            && let Some(to) = self.preferences.sketch_exit_view.view_after(view.before, view.facing, self.settled_cam())
        {
            self.animate_to(to);
        }
    }

    fn restore_marker(&mut self, marker: Option<usize>) {
        // Rolling forward past the features after the sketch recomputes them (an edited sketch
        // under a text cut): off the window thread when it takes a while.
        let _ = self.run_heavy("timeline.roll_to", marker.map(|m| json!({ "position": m })).unwrap_or_else(|| json!({})));
    }

    /// Visibility in the editor's scene: hidden face-profile sources are temporarily shown
    /// without changing the session's visibility or either history stack.
    pub(crate) fn body_visible(&self, name: &str) -> bool {
        self.session.body_visible(name) || self.dialog.as_ref().is_some_and(|d| d.edit_source_bodies.iter().any(|source| source == name))
    }

    /// Everything shown as selected: the selection plus the open dialog's inputs.
    pub fn highlighted(&self) -> Vec<solidcraft_engine::Sel> {
        let mut v = self.session.selection.clone();
        if let Some(d) = &self.dialog {
            v.extend(d.highlighted(&self.session));
        }
        v.extend(sketch_text::highlighted(self));
        v.extend(sketch_text_move::highlighted(self));
        v
    }

    /// Is the browser's Origin folder shown? The design keeps it once it is toggled; until then
    /// the Hide Origin by Default preference decides.
    pub fn origin_shown(&self) -> bool {
        !self.session.visibility.origin_hidden.unwrap_or(self.preferences.hide_origin)
    }

    /// Show or hide the browser's Origin folder (saved with the design).
    pub fn set_origin_shown(&mut self, on: bool) {
        self.session.visibility.origin_hidden = Some(!on);
    }

    /// Is the origin widget shown? Origin planes and axes are drawn and pickable. While a dialog
    /// picks a plane ([`SolveApp::picking_plane`]) the Origin is always shown, drawn through the
    /// model (a plane inside a body stays visible), even when the browser hides it.
    pub fn origin_visible(&self) -> bool {
        self.origin_shown() || self.picking_plane()
    }

    /// Is Create Sketch open, waiting for its plane?
    pub fn picking_sketch_plane(&self) -> bool {
        self.dialog.as_ref().is_some_and(|d| matches!(d.kind, dialogs::Kind::Sketch))
    }

    /// Is a dialog waiting for a plane: Create Sketch, or a dialog whose active input takes origin
    /// planes (Mirror's plane, Offset Plane, Midplane, the construct tools)? The origin planes are
    /// shown for it, so they can be picked in the view rather than only in the browser (#2581).
    pub fn picking_plane(&self) -> bool {
        self.picking_sketch_plane() || self.dialog.as_ref().is_some_and(|d| d.inputs.get(d.active).is_some_and(|i| i.accept & selection::PLANES != 0))
    }

    /// The field of view the view is drawn with: the `perspective_fov` preference while the view is
    /// in perspective ([`Self::perspective_view`]), and 0 (parallel) otherwise. The one place the
    /// drawn angle is decided.
    pub fn view_fov(&self) -> f64 {
        if self.perspective_view() { prefs::perspective_fov(&self.preferences) } else { 0.0 }
    }

    /// Is the view drawn in perspective? The perspective preference, except while a sketch is
    /// open (always orthographic) or when the #2682 mode is on: then the view is orthographic
    /// exactly when the camera faces a standard axis view, and perspective otherwise.
    pub fn perspective_view(&self) -> bool {
        if self.session.active_sketch.is_some() {
            return false;
        }
        if self.ui.auto_ortho_faces {
            return self.cam.aligned_standard_view(ORTHO_FACE_TOLERANCE).is_none();
        }
        self.ui.perspective
    }

    pub fn set_status(&mut self, s: impl Into<String>, error: bool) {
        let (text, at) = (i18n::tr_diagnostic(&s.into()), now_ms());
        self.messages.push(&text, error, at);
        self.status = Some((text, at, error));
        self.status_undo = false;
    }

    pub fn open_path(&mut self, path: &str) {
        match documents::reuse_path(self, path) {
            Ok(true) => return,
            Err(e) => {
                self.set_status(e, true);
                return;
            }
            Ok(false) => {}
        }
        self.home.open = false;
        documents::prepare_open(self);
        if let Ok(r) = self.run("doc.open", json!({ "path": path })) {
            // An opened design comes up at the home view, not at the angle the design before it
            // was left at: `prepare_open` keeps the camera when it reuses a blank design.
            self.cam.set_view(StandardView::Iso);
            self.fit_view();
            stand_up::offer(self, path, &[], Some(&r));
        }
    }

    pub fn open_path_async(&mut self, path: &str) {
        file_load::begin(self, path, false);
    }
    pub fn insert_path_async(&mut self, path: &str) {
        file_load::begin(self, path, true);
    }

    /// Insert CAD bodies, meshes or sketch drawing geometry into the current design.
    pub fn insert_path(&mut self, path: &str) {
        let cmd = if solidcraft_engine::io::is_point_cloud_path(path) { "pointcloud.insert" } else { file_load::insert_command(path) };
        let before: Vec<String> = self.session.world_state().bodies.iter().map(|b| b.name.clone()).collect();
        if let Ok(r) = self.run(cmd, json!({ "path": path })) {
            self.fit_view();
            stand_up::offer(self, path, &before, Some(&r));
            if let Some(w) = r["warnings"].as_array().filter(|w| !w.is_empty()) {
                self.set_status(format!("imported with {} warning(s): {}", w.len(), w.first().and_then(Value::as_str).unwrap_or("")), false);
            }
        }
    }

    /// A dropped CAD, mesh, drawing or point cloud (PLY, XYZ, PTS) file joins a design that has
    /// features or point clouds; otherwise it opens.
    pub fn drop_path(&mut self, path: &str) {
        let import = solidcraft_engine::io::is_step_path(path)
            || solidcraft_engine::io::is_mesh_path(path)
            || solidcraft_engine::io::is_point_cloud_path(path)
            || solidcraft_engine::cmd::drawing_import_command(path).is_some();
        if import && (!self.session.doc.features.is_empty() || !self.session.doc.point_clouds.is_empty()) {
            self.insert_path_async(path);
        } else {
            self.open_path_async(path);
        }
    }

    /// Frame the model at once (programmatic use; the toolbar animates).
    /// Make `up` the world's up axis: the camera turns to the same standard view in that world
    /// (home) and frames the model.
    pub fn set_up_axis(&mut self, up: solidcraft_engine::render::UpAxis) {
        self.ui.up_axis = up;
        self.cam_anim = None;
        self.cam.up = up;
        self.cam.set_view(StandardView::Iso);
        self.fit_view();
    }

    /// How many frames a fit waits for a viewport that can frame it before it gives up (about
    /// two seconds): a window that never draws its viewport must not ask for frames forever.
    const FIT_WAITS: u32 = 120;

    pub fn fit_view(&mut self) {
        if render_workspace::result_active(self) {
            render_workspace::fit_result(self);
            return;
        }
        if dialogs_sheet::flat_active(self) {
            self.flat_view.fitted = false;
            return;
        }
        if render_workspace::active(self) {
            self.cancel_view_animation();
            match solidcraft_engine::product_scene::extract(&self.session) {
                Ok(scene) if !scene.bounds.is_empty() => self.fit_bounds(scene.bounds),
                Err(e) => self.set_status(e.to_string(), true),
                _ => {}
            }
            return;
        }
        self.cam_anim = None;
        self.viewport.orbit_free = None;
        let bounds = viewport::view_bounds(self);
        self.pending_fit = Some((self.cam, bounds, false));
        self.fit_waits = 0;
        self.cam = self.fitted(self.cam);
    }

    /// Frame `bounds` at once, keeping the view direction (programmatic use).
    pub fn fit_bounds(&mut self, bounds: solidcraft_engine::geom::Aabb3) {
        self.cancel_view_animation();
        self.viewport.orbit_free = None;
        self.pending_fit = Some((self.cam, bounds, false));
        self.fit_waits = 0;
        self.cam = self.fitted_bounds(self.cam, &bounds);
    }

    /// `cam` moved to frame the whole model.
    pub fn fitted(&self, mut cam: Camera) -> Camera {
        let b = viewport::view_bounds(self);
        if b.is_empty() {
            cam.target = solidcraft_engine::geom::Vec3::ZERO;
            cam.distance = 150.0;
        } else {
            cam = self.fitted_bounds(cam, &b);
        }
        cam
    }

    /// `cam` framing `bounds` in the viewport as it is laid out now, or `None` when this layout
    /// can't frame it: the viewport has not been drawn since the fit was asked for, so the
    /// rectangle to frame into is the one from before the Home screen or a dialog covered it.
    fn try_fitted_bounds(&self, cam: Camera, bounds: &solidcraft_engine::geom::Aabb3) -> Option<Camera> {
        let layout = self.viewport.layout.as_ref()?;
        if render_workspace::active(self) {
            return viewport_layout::fit_product_bounds(cam, bounds, &render_workspace::picture_layout(self, layout));
        }
        viewport_layout::fit_bounds(cam, bounds, layout, self.view_fov())
    }

    fn fitted_bounds(&self, mut cam: Camera, bounds: &solidcraft_engine::geom::Aabb3) -> Camera {
        if self.viewport.layout.is_some() {
            return self.try_fitted_bounds(cam, bounds).unwrap_or(cam);
        }
        // Framed at the angle the view draws with, keeping the camera's own.
        let previous = cam;
        cam.fov = self.view_fov();
        cam.fit(bounds);
        cam.fov = previous.fov;
        if cam.is_valid() { cam } else { previous }
    }

    fn resolve_fit(&mut self, ctx: &egui::Context) {
        viewport::refresh_layout(self, ctx);
        if self.viewport.layout.is_some()
            && let Some((cam, bounds, animate)) = self.pending_fit.take()
        {
            // Nothing to frame: the camera goes to the empty design's own distance.
            let want = if bounds.is_empty() { viewport::view_bounds(self) } else { bounds };
            let to = if want.is_empty() { Some(self.fitted(cam)) } else { self.try_fitted_bounds(cam, &want) };
            let Some(to) = to else {
                // The viewport hasn't been drawn since the fit was asked for (the Home screen was
                // over it, as when a sample or a file is opened from it), so framing it now would
                // frame it into the rectangle of whatever was there before and leave the design
                // wherever the camera happened to be. Keep the request and frame it when the
                // viewport is drawn; give up only if that never comes.
                if self.fit_waits < Self::FIT_WAITS {
                    self.fit_waits += 1;
                    self.pending_fit = Some((cam, bounds, animate));
                } else {
                    self.fit_waits = 0;
                    self.cam = self.fitted_bounds(cam, &want);
                }
                ctx.request_repaint();
                return;
            };
            self.fit_waits = 0;
            if animate {
                self.animate_to(to);
            } else {
                self.cam = to;
                self.cam_anim = None;
                // An open's framing settles here, once the layout is known. An open still meshing
                // takes this as the camera it framed from the outlines, so the framing it does
                // when the last body is ready isn't mistaken for the user having moved the view.
                open_meshing::note_framed(self);
            }
            ctx.request_repaint();
        }
    }

    /// Move the camera to `to` over half a second (ease in-out, orientation slerped).
    pub fn animate_to(&mut self, to: Camera) {
        self.pending_fit = None;
        self.viewport.orbit_free = None;
        if !to.is_valid() {
            return;
        }
        // Preferences › Navigation: animated (at a chosen speed) or immediate.
        if !self.preferences.look_at_anim {
            self.cam = to;
            self.cam_anim = None;
            return;
        }
        let mut anim = CameraAnim::new(self.cam, to, self.now);
        let speed = f64::from(self.preferences.look_at_speed);
        if speed.is_finite() && speed > 0.0 {
            anim.duration /= speed;
        }
        self.cam_anim = Some(anim);
    }

    /// Frame everything shown, keeping the orientation (animated; the selection is ignored).
    pub fn animate_fit_all(&mut self) {
        if dialogs_sheet::flat_active(self) {
            self.animate_view("fit");
            return;
        }
        if render_workspace::active(self) {
            // The Render preview frames its product scene (see `fit_view`).
            self.fit_view();
            return;
        }
        self.pending_fit = None;
        let request = Some((self.cam, viewport::view_bounds(self), true));
        let to = self.fitted(self.cam);
        self.animate_to(to);
        self.pending_fit = request;
    }

    /// Animated view changes for the axis view and navigation bar.
    pub fn animate_view(&mut self, v: &str) {
        if matches!(v, "fit" | "home") {
            let _ = self.run(if v == "fit" { "view.fit" } else { "view.home" }, json!({}));
        } else if let Some(view) = StandardView::parse(v) {
            self.pending_fit = None;
            let mut to = self.cam;
            to.set_view(view);
            self.animate_to(to);
        }
    }

    /// Apply view.fit / view.home to the active canvas, retaining each canvas's framing policy.
    fn frame_view(&mut self, home: bool) {
        if render_workspace::result_active(self) {
            render_workspace::fit_result(self);
            return;
        }
        // The flat pattern is a 2D view: Fit and Home frame it, and the Design camera stays put.
        if dialogs_sheet::flat_active(self) {
            self.flat_view.fitted = false;
            return;
        }
        if render_workspace::active(self) {
            if home {
                self.cam.set_view(StandardView::Iso);
            }
            self.fit_view();
            return;
        }
        self.pending_fit = None;
        let mut to = self.cam;
        if home {
            to.set_view(StandardView::Iso);
            self.pending_fit = Some((to, viewport::view_bounds(self), true));
            to = self.fitted(to);
        } else {
            // With something selected, Fit frames the selection.
            match viewport::selection_bounds(self) {
                Some(bounds) => {
                    self.pending_fit = Some((to, bounds, true));
                    to = self.fitted_bounds(to, &bounds);
                }
                None => {
                    self.pending_fit = Some((to, viewport::view_bounds(self), true));
                    to = self.fitted(to);
                }
            }
        }
        let request = self.pending_fit.take();
        self.animate_to(to);
        self.pending_fit = request;
    }

    /// Move the viewport to the camera a command asked for (`view.apply`): animated like a view
    /// change, or at once (control requests, so a screenshot right after shows it).
    pub fn follow_view_request(&mut self, animate: bool) {
        let Some(mut to) = self.session.camera.take() else { return };
        if render_workspace::active(self) {
            self.ui.up_axis = to.up;
        } else {
            to.up = self.cam.up;
        }
        if animate {
            self.animate_to(to);
        } else if to.is_valid() {
            self.cancel_view_animation();
            self.viewport.orbit_free = None;
            self.cam = to;
        }
    }

    /// Stop any view animation where it is (the user took over the camera).
    pub fn cancel_view_animation(&mut self) {
        self.pending_fit = None;
        self.cam_anim = None;
    }

    /// Is the 3D view dark? As the window, unless Preferences give the view its own (#4146).
    pub fn viewport_dark(&self) -> bool {
        self.preferences.viewport_theme.dark().unwrap_or(self.ui.dark)
    }

    /// Hand the view to an external navigation device without a pending mouse orbit or spin
    /// immediately taking it back on the next frame.
    pub fn begin_external_navigation(&mut self) {
        self.cancel_view_animation();
        self.viewport.orbit_free = None;
        self.viewport.spin = false;
    }

    fn step_view_animation(&mut self, ctx: &egui::Context) {
        let Some(anim) = self.cam_anim else { return };
        let (cam, done) = anim.sample(self.now);
        self.cam = if cam.is_valid() { cam } else { anim.to };
        if done {
            self.cam_anim = None;
        } else {
            ctx.request_repaint();
        }
    }

    /// Per-frame logic before layout.
    pub fn logic(&mut self, ctx: &egui::Context) {
        frame_watch::begin(self);
        gpu::tick_tessellation(self);
        file_picker::tick(self, ctx);
        dialogs_save_mesh::tick_wait(self, ctx);
        easyprint_ui::tick(self, ctx);
        render_workspace::sync_backend(self);
        render_workspace::tick(self, ctx);
        file_load::tick(self, ctx);
        save_job::tick(self, false);
        edit_in_place::tick(self);
        let typing = ctx.egui_wants_keyboard_input();
        if ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1) || (i.key_pressed(egui::Key::BrowserBack) && !typing)) {
            edit_in_place::back(self);
        }
        if self.saving.is_some() || self.edit_in_place.refreshing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        open_meshing::tick(self, ctx);
        open_meshing::refresh(self, ctx.input(|i| i.time));
        // Routing survives a layout re-pass, but never the next input frame. The legacy
        // policy retains its original per-pass cancellation flag behavior.
        let first_pass = ctx.current_pass_index() == 0;
        if first_pass {
            self.escape_owner = None;
        }
        if first_pass || !crate::viewport_status::enabled(self) {
            self.esc_handled = false;
        }
        prefs::apply(self, ctx);
        // The view's own dark or light only changes tokens, not egui's visuals: no restyle.
        theme::set_viewport_dark(self.preferences.viewport_theme.dark());
        theme::set_viewport_background(self.preferences.viewport_background);
        if !self.styled || theme::is_dark() != self.ui.dark || theme::vision() != self.preferences.color_vision {
            theme::set_dark(self.ui.dark);
            theme::set_vision(self.preferences.color_vision);
            theme::apply(ctx);
            self.styled = true;
        }
        if !self.fitted {
            self.fit_view();
            self.fitted = true;
        }
        self.now = ctx.input(|i| i.time);
        if self.cam.up != self.ui.up_axis {
            self.set_up_axis(self.ui.up_axis);
        }
        self.step_view_animation(ctx);
        self.poll_control_jobs(ctx);
        self.drain_control(ctx);
        self.follow_view_request(true);
        agent_cursor::step(self, ctx);
        browser::eye_drag_frame(self, ctx);
        if !self.synthetic.is_empty() {
            ctx.request_repaint();
        }
        issue_report::tick(self, ctx);
        self.collect_screenshots(ctx);
        self.issue_screenshots(ctx);
        if self.issue_report.draft.is_none() && self.file_picker.is_none() {
            // Completion owns its keys before either CAD shortcuts or text editors see them.
            if !keymap::capturing(self) {
                expression_complete::begin_frame(ctx);
            }
            if !self.embedded {
                toolbar::shortcuts(self, ctx);
            }
        }
        recovery_ui::tick(self);
        open_elsewhere::tick(self, ctx);
        update::tick(self, ctx);
        if self.issue_report.draft.is_none() && self.file_picker.is_none() {
            // Files dropped on the theme Import tile are themes (#8226).
            prefs::take_theme_drops(self, ctx);
            // Files dropped on the agent chat attach to its next message instead.
            agent_chat::take_drops(self, ctx);
            for f in ctx.input(|i| i.raw.dropped_files.clone()) {
                let p = f.path().to_string_lossy().to_string();
                if !p.is_empty() {
                    self.drop_path(&p);
                }
            }
        }
        control::publish_design(self, &control::DESIGN_WATCH);
    }

    /// The frame's input before the app reads it: synthetic events, then what is typed while a
    /// capture hides the report waits for it (the control channel's typing too).
    pub fn raw_input_hook(&mut self, raw: &mut egui::RawInput) {
        self.synthetic_input(raw);
        issue_report::hold_input(self, raw);
    }

    /// Inject synthetic events (one pointer event per frame).
    fn synthetic_input(&mut self, raw: &mut egui::RawInput) {
        if self.synthetic.is_empty() {
            return;
        }
        // Text and modifier prefixes may precede a pointer event. Never drain past that event.
        let n = self
            .synthetic
            .iter()
            .position(|e| {
                matches!(
                    e,
                    egui::Event::PointerMoved(_)
                        | egui::Event::PointerButton { .. }
                        | egui::Event::PointerGone
                        | egui::Event::MouseWheel { .. }
                        | egui::Event::Key { pressed: false, .. }
                )
            })
            .map_or(self.synthetic.len(), |i| i + 1);
        let follow = self.ui.agent_cursor.show;
        for event in self.synthetic.drain(..n) {
            // The agent cursor shows the agent's own pointer input too, not only its commands.
            match &event {
                egui::Event::PointerMoved(at) if follow => self.agent.follow_input(*at, false, self.now),
                egui::Event::PointerButton { pos, pressed, .. } if follow => self.agent.follow_input(*pos, *pressed, self.now),
                _ => {}
            }
            match &event {
                egui::Event::ModifiersChanged(modifiers) => self.synthetic_mods = *modifiers,
                egui::Event::PointerButton { pos, modifiers, pressed, .. } => {
                    raw.events.push(egui::Event::PointerMoved(*pos));
                    raw.events.push(egui::Event::ModifiersChanged(*modifiers));
                    // A drag keeps its press's modifiers until the release.
                    self.synthetic_mods = if *pressed { *modifiers } else { egui::Modifiers::NONE };
                }
                egui::Event::Key { modifiers, .. } => raw.events.push(egui::Event::ModifiersChanged(*modifiers)),
                egui::Event::PointerMoved(_) => raw.events.push(egui::Event::ModifiersChanged(self.synthetic_mods)),
                _ => {}
            }
            raw.events.push(event);
        }
    }

    /// Lay out the whole window.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        value_step::set_enabled(ui.ctx(), self.preferences.step_values);
        canvas::set_boxes_shown(ui.ctx(), self.preferences.value_boxes_in_view);
        let t0 = now_ms();
        // The first frames draw with the bundled faces while the system's CJK, Hangul and Thai
        // faces are read on a thread; the fonts are installed again once they are there.
        crate::fonts::load_system_fonts_in_background(ui.ctx());
        let system_fonts = crate::fonts::system_fonts_ready();
        if self.preferences.language != self.applied_fonts_language
            || self.preferences.ui_font != self.applied_ui_font
            || (system_fonts && !self.applied_system_fonts)
        {
            self.applied_fonts_language.clone_from(&self.preferences.language);
            self.applied_ui_font.clone_from(&self.preferences.ui_font);
            self.applied_system_fonts = system_fonts;
            crate::fonts::setup_fonts_without_waiting(ui.ctx(), &self.preferences.language, &self.preferences.ui_font);
        }
        let _language = crate::i18n::scope(&self.preferences.language);
        let mut report_input = None;
        issue_report::CadInput::block(self, ui, &mut report_input);
        self.cad_ui(ui, &mut report_input);
        drop(report_input);
        // After everything else, on every page (the start page too, which returns early from
        // `cad_ui`): the window's edges resize it (#1381).
        if self.custom_titlebar && !self.native_resize_border {
            titlebar::resize_zones(ui);
        }
        if self.file_picker.is_none() {
            issue_report::show(self, ui.ctx());
        }
        if !self.home.open {
            agent_cursor::paint(self, ui.ctx());
        }
        if self.file_picker_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        control::publish_design(self, &control::DESIGN_WATCH);
        self.frame_ms = now_ms() - t0;
        frame_watch::end(self);
    }

    fn cad_ui(&mut self, ui: &mut egui::Ui, report_input: &mut Option<issue_report::CadInput>) {
        if self.embedded {
            self.ui.layout = shell::LayoutMode::Docked;
            self.home.open = false;
            self.ui.show_browser = true;
            self.ui.show_timeline = true;
        }
        self.ui.layout.publish(ui.ctx());
        self.preferences.dialog_buttons.publish(ui.ctx());
        self.preferences.toast_place.publish(ui.ctx());
        scenario::clear_handles();
        self.poll_feature_edit(ui.ctx());
        self.poll_deferred(ui.ctx());
        self.poll_delete_check(ui.ctx());
        timeline::tick_playback(self, ui.ctx());
        if !self.embedded {
            toolbar::app_bar(self, ui);
        }
        issue_report::CadInput::block(self, ui, report_input);
        file_load::show(self, ui.ctx());
        open_meshing::show(self, ui.ctx());
        documents::prompt(self, ui.ctx());
        open_elsewhere::show(self, ui.ctx());
        network_prompt::show(self, ui.ctx());
        update::show(self, ui.ctx());
        install::show(self, ui.ctx());
        // Settings › AI's Add agent dialog: Settings opens over Home too (#3483).
        if agent_chat::shown(self) {
            agent_chat::settings::dialog(self, ui.ctx());
        }
        if self.home.open {
            home::show(self, ui);
            if self.home.open {
                // Agent detection runs in the background and is only received by `agent_chat::poll`
                // Polling here is required for the detection to complete on Home
                agent_chat::poll(self, ui.ctx());
                // Preferences and Keyboard Shortcuts open from Home too (its links, the gear, Ctrl+,).
                prefs::show(self, ui.ctx());
                return;
            }
        }
        // The status bar first: it spans the window's width below History and the side panels.
        status_bar::show(self, ui);
        let floating = render_workspace::active(self) || ui_kit::Container::new(ui.ctx(), ui_kit::Surface::Panel).floating();
        timeline::follow_view_pick(self);
        render_ui::focus_shortcut(self, ui);
        let show_timeline = self.ui.show_timeline && !render_workspace::active(self) && !self.preferences.history_in_browser;
        let left_timeline = show_timeline && self.preferences.timeline_left;
        timeline_left::animate_folds(self, ui.ctx());
        if !floating && !self.embedded {
            toolbar::toolbar(self, ui);
        }
        theme::TabAccent::publish(self, ui.ctx());
        browser::publish_row_prefs(self, ui.ctx());
        if !floating {
            agent_chat::show(self, ui);
        }
        if !floating && show_timeline && !left_timeline {
            timeline::timeline(self, ui);
        }
        // Browser hugs its contents in either layout; it never becomes a full-height blank column.
        if !self.ui.show_browser || render_ui::focused(self) {
            self.shell.browser = None;
            shell::register_panel(self, "render:browser", None, None);
            shell::register_panel(self, "render:appearances", None, None);
        }
        if floating || left_timeline || !show_timeline {
            self.shell.history = None;
        }
        // The model draws next: a dialog the toolbar closed last frame no longer shows its sketch.
        dialogs::sync_feature_edit_sketch(self);
        let full = ui.available_rect_before_wrap();
        if floating {
            let cards = toolbar::shell_cards(self, full);
            self.shell.cards = vec![cards.workspace, cards.ribbon];
            self.shell.history = if left_timeline {
                let below_ribbon = egui::Rect::from_min_max(egui::pos2(full.left(), cards.bottom), full.max);
                // A dialog in the left column holds the History's place.
                (!dialog_column::holds(self)).then(|| timeline_left::floating_rect(self, below_ribbon))
            } else {
                show_timeline.then(|| timeline::floating_rect(full))
            };
            // Draw the scene first, then let the unchanged panel layouts cover it. The
            // viewport keeps its full projection even underneath the browser and cards.
            preview::update(self, ui.ctx());
            // A camera drag that began on the model keeps capture across cards. New
            // gestures over chrome are blocked, as are selection drags into the ribbon.
            let continuing_navigation = ui.input(|i| {
                (i.pointer.middle_down() || i.pointer.secondary_down())
                    && i.pointer.press_origin().is_some_and(|p| full.contains(p) && !shell::covered(self, p))
            });
            let blocked = !continuing_navigation && ui.input(|i| i.pointer.latest_pos()).is_some_and(|p| shell::covered(self, p));
            let mut viewport_ui = ui.new_child(egui::UiBuilder::new().max_rect(full).id_salt("floating_viewport"));
            if blocked {
                // Disabled only to shield the scene from input under a card or menu; it must
                // not fade, or the whole viewport dims whenever the pointer is over the shell.
                viewport_ui.visuals_mut().disabled_alpha = 1.0;
                viewport_ui.disable();
            }
            self.show_workspace(&mut viewport_ui);
            shell::paint_top_corners(ui.painter(), full, theme::Tokens::get().app_bar);
            toolbar::toolbar(self, ui);
            theme::TabAccent::publish(self, ui.ctx());
        }
        if floating && show_timeline && !left_timeline {
            timeline::timeline(self, ui);
        }
        dialog_column::layout(self, ui.available_rect_before_wrap());
        if left_timeline {
            timeline_left::left_panel(self, ui);
        }
        if render_workspace::active(self) {
            render_ui::shots_strip(self, ui);
            render_workspace::panel(self, ui);
        }
        if self.ui.show_browser && !render_ui::focused(self) {
            browser::browser(self, ui);
        }
        if floating {
            agent_chat::show(self, ui);
        }
        Container::new(ui.ctx(), Surface::Viewport)
            .central()
            .frame(Container::new(ui.ctx(), Surface::Viewport).edge_frame(egui::Color32::TRANSPARENT))
            .show(ui, |ui| {
                let panel_content = ui.max_rect();
                self.shell.content = Some(shell::workspace_content(self, panel_content));
                if floating {
                    self.shell.panels = [
                        egui::Rect::from_min_max(egui::pos2(full.left(), panel_content.top()), egui::pos2(panel_content.left(), full.bottom())),
                        egui::Rect::from_min_max(egui::pos2(panel_content.right(), panel_content.top()), full.max),
                    ]
                    .into_iter()
                    .filter(|r| r.is_positive())
                    .collect();
                    viewport::refresh_layout(self, ui.ctx());
                } else {
                    preview::update(self, ui.ctx());
                    self.show_workspace(ui);
                }
            });
        if render_workspace::active(self) {
            render_ui::overlays(self, ui);
        }
        // Before the dialog: a click on the view's OK or Cancel is for this frame's dialog.
        view_confirm::show(self, ui.ctx());
        dialogs::show(self, ui.ctx());
        issue_report::CadInput::block(self, ui, report_input);
        sketch_palette::show(self, ui.ctx());
        self.resolve_fit(ui.ctx());
        params_dialog::show(self, ui.ctx());
        design_properties::show(self, ui.ctx());
        sketch_report::show(self, ui.ctx());
        bom_panel::show(self, ui.ctx());
        material_card_review::show(self, ui.ctx());
        recovery_ui::show(self, ui.ctx());
        stand_up::show(self, ui.ctx());
        prefs_notice::show(self, ui.ctx());
        context_menu::show(self, ui.ctx());
        mouse_gesture::show(self, ui.ctx());
        delete::show(self, ui.ctx());
        easyprint_ui::show(self, ui.ctx());
        linked_source::show_copies(self, ui.ctx());
        joint_handles::show_warning(self, ui.ctx());
        shortcut_box::show(self, ui.ctx());
        prefs::show(self, ui.ctx());
        toolbar_custom::window(self, ui.ctx());
    }

    fn show_workspace(&mut self, ui: &mut egui::Ui) {
        if render_workspace::active(self) {
            render_workspace::show(self, ui);
        } else {
            viewport::show(self, ui);
        }
    }

    fn drain_control(&mut self, ctx: &egui::Context) {
        /// Requests run on this thread, so a flood would starve rendering. The rest stay
        /// queued for the next frame.
        const PER_FRAME: usize = 64;

        /// Held requests beyond this are refused rather than kept.
        const MAX_HELD: usize = 256;

        // Held requests (and the agent chat's, which it hands over here) run even when the app
        // has no control channel of its own.
        if self.control_rx.is_none() && self.control_held.is_empty() {
            return;
        }
        let rx = self.control_rx.take();
        for _ in 0..PER_FRAME {
            // A file loading, a save being written, an agent's design change computing, or the
            // agent cursor acting one out: commands that change the design wait their turn (the
            // window, inspection and screenshots still answer, #1989). `engine.script` runs
            // directly, past `SolveApp::run`'s guard, so without the save here it changed the
            // design the save was taken up for (#6003).
            let waits =
                self.file_load.busy() || self.file_picker.is_some() || self.saving.is_some() || self.control_job.is_some() || self.agent.busy();
            // A screenshot waiting for an opened design to be meshed holds what came after it.
            let shot_waits = open_meshing::active(self) && self.control_held.front().is_some_and(|r| r.method == "ui.screenshot");
            // Held requests go first, in order, once nothing makes them wait.
            let held = if waits || shot_waits { None } else { self.control_held.pop_front() };
            let Some(req) = held.or_else(|| rx.as_ref().and_then(|rx| rx.try_recv().ok())) else { break };
            if req.refuse_cancelled_chat() {
                continue;
            }
            if shot_waits {
                if self.control_held.len() < MAX_HELD {
                    self.control_held.push_back(req);
                } else {
                    let _ = req.reply.send(json!({"ok": false, "error": "a file operation is in progress"}));
                }
                continue;
            }
            if (waits && control_job::changes_the_design(&req.method))
                || (self.file_picker_busy() && matches!(req.method.as_str(), "ui.file" | "ui.home"))
                // A screenshot shows the whole design: it waits until an opened one is meshed.
                || (open_meshing::active(self) && req.method == "ui.screenshot")
            {
                if self.control_held.len() < MAX_HELD {
                    self.control_held.push_back(req);
                } else {
                    let _ = req.reply.send(json!({"ok": false, "error": "a file operation is in progress"}));
                }
                continue;
            }
            req.started.store(true, std::sync::atomic::Ordering::Relaxed);
            // Opening or importing a file: answered when the file is in, without freezing the window.
            let Some(req) = file_load::control(self, req) else {
                ctx.request_repaint();
                continue;
            };
            if agent_cursor::intercept(self, &req) {
                ctx.request_repaint();
                continue;
            }
            // A slow design change or export: computed off the window thread (#1989).
            let Some(req) = self.offload_control(ctx, req) else {
                ctx.request_repaint();
                continue;
            };
            self.run_control(ctx, req);
        }
        self.control_rx = rx;
    }

    /// Run a control request here, now, and reply (or queue the screenshot it asks for).
    pub(crate) fn run_control(&mut self, ctx: &egui::Context, req: ControlRequest) {
        if req.refuse_cancelled_chat() {
            return;
        }
        let reply = req.reply.clone();
        match control::handle(self, ctx, &req) {
            control::Outcome::Done(v) => {
                let _ = reply.send(v);
            }
            // A window that is minimised or covered (another macOS Space) presents no frame:
            // say so at once rather than after the deadline (#1677).
            control::Outcome::Screenshot { .. } if window_hidden(ctx) => {
                let _ = reply.send(json!({"ok": false, "hidden": true, "error": "no frame was presented (window hidden?); use ui.render"}));
            }
            control::Outcome::Screenshot { path } => {
                self.shot_token += 1;
                let token = self.shot_token;
                if let Some(turn) = req.chat_turn {
                    self.shot_chat_turns.insert(token, turn);
                }
                self.queued_shots.push((token, now_ms() + 150.0, 0));
                self.pending_shots.push((token, path, reply, now_ms() + 8000.0));
            }
        }
        ctx.request_repaint();
    }

    fn issue_screenshots(&mut self, ctx: &egui::Context) {
        let now = now_ms();
        // A view that just moved is drawn with fewer AO samples: shoot the full-quality frame
        // that follows (#7165), or after half a second a view that keeps moving as it is.
        let settled = viewport::ao_settled(self, ctx);
        self.queued_shots.retain_mut(|(token, at, frames)| {
            *frames += 1;
            if now >= *at && *frames >= 3 && (settled || now >= *at + 500.0) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(*token)));
                false
            } else {
                true
            }
        });
        if !self.queued_shots.is_empty() || !self.pending_shots.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }

    fn collect_screenshots(&mut self, ctx: &egui::Context) {
        // A screenshot requested in one tab must never return or save a frame of another.
        let chat_turns = &mut self.shot_chat_turns;
        let queued_shots = &mut self.queued_shots;
        self.pending_shots.retain(|(token, _, reply, _)| {
            if control::chat_turn_cancelled(chat_turns.get(token).map(std::sync::Arc::as_ref)) {
                chat_turns.remove(token);
                queued_shots.retain(|(queued, ..)| queued != token);
                let _ = reply.send(control::cancelled_chat_response());
                false
            } else {
                true
            }
        });
        if self.pending_shots.is_empty() && !issue_report::pending(self) {
            return;
        }
        let events: Vec<_> = ctx.input(|i| {
            i.raw
                .events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::Screenshot { user_data, image, .. } => {
                        let token = user_data.data.as_ref().and_then(|d| d.downcast_ref::<u64>()).copied()?;
                        Some((token, image.clone()))
                    }
                    _ => None,
                })
                .collect()
        });
        for (token, image) in events {
            issue_report::receive(self, token, image.clone());
            if let Some(i) = self.pending_shots.iter().position(|(t, ..)| *t == token) {
                let (_, path, reply, _) = self.pending_shots.remove(i);
                self.shot_chat_turns.remove(&token);
                let _ = reply.send(control::save_screenshot(&image, path.as_deref()));
            }
        }
        let now = now_ms();
        // Hidden while waiting: no frame is coming.
        let hidden = window_hidden(ctx);
        self.pending_shots.retain(|(token, _, reply, deadline)| {
            if now < *deadline && !hidden {
                return true;
            }
            let _ = reply.send(json!({"ok": false, "error": "no frame was presented (window hidden?); use ui.render"}));
            self.shot_chat_turns.remove(token);
            false
        });
    }
}

/// The window is minimised or fully covered, so it presents no frames (#1677). Unknown counts as
/// shown: the screenshot deadline still ends a wait for a frame that never comes.
fn window_hidden(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().visible()) == Some(false)
}

/// An edit in place command's result, saying when it waits for the end of the agent's turn.
fn with_queued(mut result: Value, queued: bool) -> Value {
    if queued && let Some(o) = result.as_object_mut() {
        o.insert("queued".into(), Value::Bool(true));
    }
    result
}

/// A command's name as people see it (its label), for the status bar.
pub fn command_label(id: &str) -> &str {
    solidcraft_engine::find_command(id).map_or(id, |c| c.label)
}

/// An engine error with the command's label instead of its id: "Shell: offset must be …".
/// The command label and catalogued engine messages follow the interface language.
pub fn friendly_error(id: &str, e: &str) -> String {
    let label = i18n::tr_label(command_label(id));
    let msg = e.replace(&format!("invalid parameters for `{id}`: "), "").replace(&format!("{id}: "), "").replace(&format!("`{id}`"), label);
    let msg = i18n::tr_label(&msg);
    if msg.starts_with(label) { msg.to_string() } else { format!("{label}: {msg}") }
}

pub fn now_ms() -> f64 {
    use web_time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

#[cfg(test)]
mod picker_test_support;
#[cfg(test)]
mod test_support;

/// Toolbar panels whose commands build features from the model; they finish an open sketch first.
/// Construct, Inspect and Insert keep the sketch open.
const SKETCH_ENDING_PANELS: [&str; 3] = ["CREATE", "MODIFY", "ASSEMBLE"];
const SKETCH_TAB: &str = "SKETCH";

fn ends_sketch(id: &str) -> bool {
    solidcraft_engine::find_command(id).is_some_and(|c| c.tab != SKETCH_TAB && SKETCH_ENDING_PANELS.contains(&c.panel) && c.id != "sketch.create")
}
