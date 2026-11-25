//! Stored preferences with no Preferences page (no page's Defaults resets them).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! unpaged_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// "Always allow" answers: per agent, the tools it may use without asking.
            agent_always_allow: std::collections::BTreeMap<String, Vec<String>> = Default::default();
            /// The agent a new conversation starts with (an agent's id; empty: the first one available).
            agent_default: String = String::new();
            /// Custom instructions (#8220): text the agent chat adds to its system note when a session
            /// starts (`agent_chat::acp::system_note`). No page reset discards this user-authored text.
            agent_instructions: String = String::new();
            /// The agent the chat last started (an agent's id; empty: none yet).
            agent_last: String = String::new();
            /// The model last chosen in the chat, per agent: put back when the agent offers it again.
            agent_models: std::collections::BTreeMap<String, String> = Default::default();
            /// Agents the user added (the installed ones are found by themselves). API keys are never
            /// here: they live in the system keychain.
            agents: Vec<crate::agent_chat::agents::AgentConfig> = Vec::new();
            /// The programs the user added to the slicer list, newest first.
            custom_slicers: Vec<String> = Vec::new();
            /// Imported theme files (`theme_files`), kept here so they also work in the browser.
            #[serde(deserialize_with = "themes_that_read")]
            custom_themes: Vec<crate::theme_files::ThemeFile> = Vec::new();
            /// The user has agreed to Open with EasyPrint uploading a model to Printables' servers.
            easyprint_consent: bool = false;
            /// The distance the last new extrude was applied with, in mm (empty until one is); kept when
            /// defaults are restored.
            extrude_last_distance: String = String::new();
            /// An image (Canvas or Decal) placed on the plane of the sketch being edited gets a
            /// construction rectangle around it in that sketch and follows it, so the sketch's constraints
            /// and dimensions place and size the image (#18405). A mode of the Canvas and Decal tools, set
            /// in the Sketch Palette while one runs and kept for the next time; no Preferences page shows
            /// or resets it. Off by default, so placing an image works as it always has: on its own, with
            /// Frame Sketch to tie it to a rectangle when asked.
            frame_images_in_sketch: bool = false;
            /// The grid turns to the upright origin plane facing a side view (#1470). Off by default: the
            /// grid stays on the ground plane (XY) whatever the view.
            grid_follows_view: bool = false => check_id("ui.grid_follows_view");
            /// Personal Hole parameter combinations, shared by every design in this profile.
            /// No page reset discards this user-authored library.
            hole_presets: crate::hole_presets::Library = crate::hole_presets::Library::default();
            /// The Parameters dialog applies an expression as soon as its field is left (#9279). Off: the
            /// edits wait in the dialog until Apply or OK sets them together, in one rebuild and one undo
            /// step. Its checkbox is in the dialog itself.
            parameters_apply_live: bool = true;
            /// Model Parameters are listed under the component each feature belongs to (#20648); off keeps
            /// the flat list in listing order. Its check box is in the Parameters window, next to the list.
            params_group_by_component: bool = false;
            /// Personal Sheet Metal Rules, offered by the rule manager of every design in this profile
            /// (#9346). No page reset discards this user-authored library.
            personal_sheet_rules: crate::personal_sheet_rules::Library = crate::personal_sheet_rules::Library::default();
            /// Approved install permissions survive page resets and Cancel, like installed files.
            plugin_grants: std::collections::BTreeMap<String, solidcraft_engine::plugin::Grants> = std::collections::BTreeMap::new();
            /// Importer options, by plugin id and entry name: each entry's own "Remember these options"
            /// choice and its last-run values. Bookkeeping, never design data.
            plugin_importer_options: std::collections::BTreeMap<String, std::collections::BTreeMap<String, crate::plugin_menu::ImporterMemory>> = std::collections::BTreeMap::new();
            /// Resource identities and history belong to the user, never the design.
            render_library_favorites: std::collections::BTreeSet<(String, String)> = Default::default();
            render_library_recent: std::collections::VecDeque<(String, String)> = Default::default();
            /// Render shell preference, independent of the document and Design history.
            render_shots_strip: bool = true;
            /// Save as mesh: the last format (`stl` or `3mf`). Older versions also kept a "send to slicer"
            /// choice here (`save_mesh_send`, `save_mesh_slicer`); a settings file that still has them loads,
            /// and they are ignored.
            save_mesh_format: String = "stl".into();
            /// Save as mesh: the last quality (`standard`, `fine` or `finest`, #4747).
            save_mesh_quality: String = "standard".into();
            /// Send to Slicer: the last slicer used (an installed slicer's id or a program path).
            send_slicer: String = String::new();
            /// The 3-Point Arc's third point snaps to the arc tangent to the line or arc it starts or ends, or to the arc
            /// whose centre is on a sketch point, and the click adds the Tangent or Coincident constraint (#20719). A mode of the 3-Point Arc tool, set
            /// in the Sketch Palette while it runs and kept for the next time; no Preferences page shows or
            /// resets it. It also turns on that tool's Radius and Sweep boxes for the third point and the cross at
            /// the arc's centre. Off (the default): the tool is as it always was, its point snaps on the curve and
            /// shows only the pointer's coordinates.
            sketch_arc_tangent_snap: bool = false;
            /// The variant last started in each split tool family (Rectangle, Circle, Arc, Slot, Polygon, Line,
            /// Spline): the family's button and shortcut start it again (`sketch_families`).
            sketch_variants: crate::sketch_families::Choices = crate::sketch_families::Choices::default();
            /// Send to Slicer: the folder the file is kept in for each saved design (its path), asked for in
            /// the Send window (#18311). A design with no entry keeps it in the slicer folder; one that was
            /// never saved has none.
            slicer_copy_folders: std::collections::BTreeMap<String, String> = Default::default();
            /// Send to Slicer: a file kept in a folder is replaced each time, not numbered (#18311).
            slicer_copy_overwrite: bool = false;
            /// The line about sending again replacing the slicer's file has been shown.
            slicer_resend_hint_seen: bool = false;
            /// The 3D mice read through smed (`smed V:P`) that are done taking over what was saved for them
            /// read directly (`space_mouse::adopt_direct_settings`): they have a calibration or button jobs of
            /// their own, carried over or set before, so a later reset of those stays a reset. Kept when settings
            /// are reset, like `space_mouse_offered`.
            #[serde(deserialize_with = "devices_that_read")]
            space_mouse_adopted: std::collections::BTreeSet<String> = Default::default();
            /// Per 3D mouse, what each of its buttons does, by number from 1, once the user has changed
            /// any of them; until then a measured device has its printed keys' jobs
            /// (`space_mouse::default_button_jobs`). Kept, like calibrations, when settings are reset.
            #[serde(deserialize_with = "button_jobs_that_read")]
            space_mouse_buttons: std::collections::BTreeMap<String, std::collections::BTreeMap<u8, crate::space_mouse::ButtonJob>> = Default::default();
            /// Per 3D mouse (`space_mouse::Input::device`), the calibration that puts its axes on
            /// 3Dconnexion's. A device without one is read as it reports. Page resets keep calibrations.
            #[serde(deserialize_with = "calibrations_that_read")]
            space_mouse_calibrations: std::collections::BTreeMap<String, solidcraft_space_mouse::Calibration> = Default::default();
            /// Per 3D mouse, what each button does when held past the device's long-press threshold, once the
            /// user has changed any; until then the measured defaults (`space_mouse::default_long_button_jobs`).
            /// A button without one does its short job. Only devices read through a driver that reports long
            /// presses (smed) have them. Kept like `space_mouse_buttons` when settings are reset.
            #[serde(deserialize_with = "button_jobs_that_read")]
            space_mouse_long_buttons: std::collections::BTreeMap<String, std::collections::BTreeMap<u8, crate::space_mouse::ButtonJob>> = Default::default();
            /// The 3D mice (`space_mouse::Input::device`) whose calibration has been offered and dismissed, so
            /// the offer (`space_mouse_offer`) is made once per device. A saved entry that is not text is dropped.
            /// Kept when settings are reset, like the calibrations and button mappings it goes with: resetting
            /// the page would otherwise bring the offer back for every device.
            #[serde(deserialize_with = "devices_that_read")]
            space_mouse_offered: std::collections::BTreeSet<String> = Default::default();
            /// Oldest-first bounded file answers; resetting settings preserves user history.
            stand_up_answers: crate::stand_up::Answers = Default::default();
            /// When the last update check ran, and the versions skipped or put off (`update::Memo`).
            update: crate::update::Memo = crate::update::Memo::default();
            /// URDF export writes motion links as `<mimic>`. Default false: each joint is free, as
            /// before; the options dialog remembers the last choice.
            urdf_mimic: bool = false;
            /// Export URDF with Options writes the meshes where the mesh prefix points, making its
            /// folder. Default false: next to the URDF whatever the prefix says; the options dialog
            /// remembers the last choice.
            urdf_place_meshes: bool = false;
            /// Share anonymous usage statistics (docs/privacy.md, `usage_stats`): on unless the person,
            /// the environment or the organization turns them off. A build without the `usage-stats`
            /// feature keeps the value and does nothing with it.
            usage_statistics: bool = true;
            /// A small dot at every vertex wherever edges are drawn (#7834). Off by default: the viewport
            /// draws edges only, as it always has.
            vertex_markers: bool = false;
            /// Hidden edges in the current-view DXF export (false: omit, true: dashed).
            view_dxf_dashed: bool = false;
            /// Model-space chord tolerance for current-view DXF curves, in millimetres.
            view_dxf_tolerance: f64 = 0.01;
            /// Where the current-view DXF drawing puts its 0,0: "target" (the point the camera looks
            /// at) or "origin" (the model's origin as seen from the camera).
            view_dxf_zero: String = "target".into();
            /// Named, app-wide arrangements of design windows.
            view_window_setups: BTreeMap<String, serde_json::Value> = BTreeMap::new();
        }
    };
}
