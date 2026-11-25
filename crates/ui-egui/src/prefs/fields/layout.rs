//! Stored preferences on the Layout page (`PAGE_LAYOUT`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! layout_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// When components' bodies sit in a Bodies folder in the browser (#4217).
            browser_bodies_folders: BodiesFolders = BodiesFolders::Several, PAGE_LAYOUT;
            /// Each body row in the Browser folds open on the timeline features that built it, in
            /// timeline order (#11945). Off by default: body rows have no fold arrow, as before.
            browser_body_features: bool = false, PAGE_LAYOUT;
            /// Every component placement in the browser starts open, as before #9274, instead of closed
            /// until the user opens it. Off by default.
            browser_components_open: bool = false, PAGE_LAYOUT;
            /// Each tree row's eye between its fold arrow and its radio, as before #8718, instead of in a
            /// column at the rows' right end. Off by default.
            browser_eye_left: bool = false, PAGE_LAYOUT;
            /// Folders in the Browser start closed, and placements too whatever "Components start open"
            /// says, and they open only on request (#8768): with their arrow, Expand All or Find in
            /// Browser, which opens just the rows down to its body and closes again the ones the last find
            /// opened. A body picked in the 3D view leaves the Browser as it is, and only the body Find in
            /// Browser found has the rows above it marked, so a large assembly's placements are not named
            /// again after each edit. Off by default: folders start open, placements as "Components start
            /// open" says, and a pick in the 3D view opens the Browser down to its body (#3005), as before.
            browser_rows_start_closed: bool = false, PAGE_LAYOUT;
            /// Use distinct tab treatments for the focused view, cards, hidden views, and pop-outs.
            clear_view_tabs: bool = true, PAGE_LAYOUT;
            /// Search commands opens centred over the view instead of in the ribbon's top-right corner
            /// (#18309); the shortcut, results and keys are the same. Off by default: it opens in place.
            command_search_centred: bool = false, PAGE_LAYOUT;
            /// Group Construct menus under Planes, Axes and Points headings; off keeps the original command order.
            construct_menu_groups: bool = true, PAGE_LAYOUT => check_id_help("prefs.construct_menu_groups", "prefs.construct_menu_groups_help");
            /// A design opens with the views it had when it was last closed.
            design_reopen_views: bool = false, PAGE_LAYOUT;
            /// Opening a sheet or workspace that a view already shows activates that view.
            design_reuse_views: bool = true, PAGE_LAYOUT;
            /// Render and Animate open as views of their design.
            design_sub_tab_render: bool = true, PAGE_LAYOUT;
            /// A drawing sheet opens as a view (a sub-tab) of its design.
            design_sub_tab_sheets: bool = true, PAGE_LAYOUT;
            /// A design's views (Model, Sheet, Render, Animation) are tabs inside its tab; off, the
            /// design keeps one tab and a view switcher lists them.
            design_views_as_tabs: bool = false, PAGE_LAYOUT;
            /// Where a dialog's OK and Cancel sit: under its inputs (the default) or in its title row (#6708).
            dialog_buttons: crate::ui_kit::DialogButtons = Default::default(), PAGE_LAYOUT;
            /// Drawing properties use the same left/right, resizable panel pattern as Assistant.
            drawing_panel_side: String = "right".into(), PAGE_LAYOUT;
            drawing_panel_width: f32 = 320.0, PAGE_LAYOUT, accept(240.0..=560.0);
            /// Drawing sheet strip placement; independent of Design's feature history.
            drawing_sheet_dock: String = "top".into(), PAGE_LAYOUT;
            /// With several features selected, an entry of a feature's menu (History bar, vertical
            /// History, Browser) acts on all of them or cannot be used. Off: the menus as they were, where
            /// Edit, Rename, Suppress, Roll back, Find and Copy Message act on the right-clicked feature
            /// alone.
            feature_menus_act_on_selection: bool = true, PAGE_LAYOUT;
            /// While a file opens, is inserted or its links are read again, the design waits for it
            /// behind a card with the file's progress and Cancel, as it always did. Off (the default): the
            /// operation is a row of the operations panel and the design stays usable meanwhile.
            file_operations_wait: bool = false, PAGE_LAYOUT;
            /// Toolbar tabs hidden by the person. An empty list keeps every existing tab visible.
            hidden_toolbar_tabs: Vec<String> = Vec::new(), PAGE_LAYOUT;
            /// On, the History bar folds long runs of one component's features into chips and starts
            /// feature folders folded, and a STEP insert that brings more than one item goes into a
            /// `Group #N` folder (#8885). Off, the default, shows each feature, starts folders open and
            /// makes no folder, as the bar always has.
            history_bar_groups: bool = false, PAGE_LAYOUT;
            /// A History folder whose features are all of one kind (Holes, say) shows their icon instead of
            /// the folder icon, in both Histories. Off by default: every folder shows the folder icon.
            history_folder_kind_icon: bool = false, PAGE_LAYOUT;
            /// On, two or more joints show as one button with their count in the History, and a click
            /// shows each one (#8884). Off, the default, shows each joint as the History always has.
            history_group_joints: bool = false, PAGE_LAYOUT;
            /// Compact history: the browser lists each component's features in history order with the
            /// marker, and neither timeline is shown.
            history_in_browser: bool = false, PAGE_LAYOUT;
            /// With "History steps by component" on, Last (and Forward onto the last slot) stop just after
            /// the active component's last feature, as they did before #15974, leaving later features of
            /// other components rolled back. Off, the default: they go to the end of the design.
            history_last_stops_at_component: bool = false, PAGE_LAYOUT;
            /// A Play History button in the History controls, between Previous and Next (#3688). Off
            /// gives the four controls History had before; right-clicking the controls toggles it too.
            history_play_button: bool = true, PAGE_LAYOUT;
            /// Whether vertical History lists a sketch under the feature built from it.
            history_sketches: HistorySketches = HistorySketches::default(), PAGE_LAYOUT;
            /// The History controls (First, Back, Forward, Last) step through the features the bottom bar
            /// shows: one component's while it shows one (#11430). Off: they step through every feature of
            /// the design, as they did before.
            history_steps_component: bool = true, PAGE_LAYOUT;
            /// Each design tab keeps its open dialog or tool, and its toolbar tab, while another design is
            /// shown, and has them back when it is shown again (#9360). Off by default: a switch cancels
            /// them and shows the MODEL tab.
            keep_work_on_tab_switch: bool = false, PAGE_LAYOUT;
            /// Shared workspace composition; system windows and popups retain their purpose.
            layout: crate::ui_kit::LayoutMode = Default::default(), PAGE_LAYOUT;
            /// Restore the former Surface Create and Convert sections.
            legacy_surface_toolbar: bool = false, PAGE_LAYOUT => check_id("prefs.legacy_surface_toolbar");
            /// Preview card density in the document browser; Large preserves the existing layout.
            library_thumbnail_size: crate::library::ThumbnailSize = crate::library::ThumbnailSize::Large, PAGE_LAYOUT;
            /// Preview panel placement and reflow while dragging. Off shows the drop outline.
            live_panel_drag_preview: bool = true, PAGE_LAYOUT;
            /// Ordered view controls and their appearance.
            #[serde(default)]
            navigation_bar: crate::navigation_bar::NavigationBar = Default::default(), PAGE_LAYOUT;
            /// The operations under way, with their progress and Cancel, in a panel in the view's
            /// bottom-right corner. Off: a "Computing" notice and the agent's chip, as before.
            operations_panel: bool = true, PAGE_LAYOUT;
            /// Show engine-derived persistent selection in the ribbon; off retains neutral buttons.
            persistent_ribbon_pressed: bool = true, PAGE_LAYOUT;
            /// Constraints pinned to the sketch ribbon's Constrain panel are drawn as small cells in its
            /// constraint grid, like the default six. Off: they are full-size buttons, as before.
            pinned_constraints_in_grid: bool = true, PAGE_LAYOUT;
            /// Display defaults copied into a newly detached design window.
            #[serde(deserialize_with = "pop_out_window_defaults")]
            pop_out_window_defaults: solidcraft_views::WindowOptions = solidcraft_views::WindowOptions::new_pop_out(), PAGE_LAYOUT;
            /// Each design tab keeps its toolbar tab (MODEL, SURFACE, ...) while another design is shown,
            /// with or without `keep_work_on_tab_switch`. Off: a switch shows the MODEL tab, as before.
            remember_toolbar_tab: bool = false, PAGE_LAYOUT;
            /// Show the ribbon's panel drop-downs as a grid of large icons, their names in the tooltips,
            /// instead of a list of small icons with names (#4739).
            ribbon_icon_grid: bool = false, PAGE_LAYOUT;
            /// Show command names beneath ribbon icons, independently of workspace layout.
            ribbon_labels: bool = true, PAGE_LAYOUT;
            /// After a ribbon panel menu is open, hover another panel to switch to its menu.
            ribbon_menus_follow_hover: bool = false, PAGE_LAYOUT;
            /// Leave the commands the ribbon shows as buttons (pinned ones) out of its panel drop-downs,
            /// which then list only the rest (#4739).
            ribbon_menus_skip_pinned: bool = false, PAGE_LAYOUT;
            /// Hovering a result in Search commands shows the command's place in the ribbon: its tab,
            /// its button or its row in a section's drop-down (#7287). A command started from the search
            /// blinks there, and a running command that sits in a drop-down marks its section label. Off
            /// by default: the ribbon stays as it was.
            ribbon_reveal: bool = false, PAGE_LAYOUT;
            /// Fold the ribbon to its tab row, so the view has the room its panels took (#10112). A tab's
            /// click shows its panels over the view until a command starts or the pointer leaves them.
            /// Off by default: the whole ribbon shows. `view.ribbon_tabs_only` switches it.
            ribbon_tabs_only: bool = false, PAGE_LAYOUT;
            /// With views as tabs off, a design's tab always shows its view switcher (even with one view),
            /// and entering Render opens a view of its own, as in 0.43.0. Off: the tab looks as
            /// it did before design views, with no switcher until the user opens a second view.
            show_design_view_switcher: bool = false, PAGE_LAYOUT;
            /// Show the optional document browser beside the design.
            show_document_browser: bool = false, PAGE_LAYOUT;
            /// Keep the SKETCH tab in the ribbon, right after MODEL, while no sketch is open; its drawing
            /// tools then start a sketch first (#7904).
            sketch_tab_always: bool = false, PAGE_LAYOUT;
            /// A thin bar along the window's bottom edge: what is being edited, the open sketch's state
            /// and the design's units (#7243). Off by default.
            status_bar: bool = false, PAGE_LAYOUT;
            /// A double-click on a design tab renames the design in the tab; off, it maximizes the window
            /// as the rest of the title bar does, as before.
            tab_double_click_renames: bool = true, PAGE_LAYOUT;
            /// Two-dimensional (2D) Timeline with multi-track component layout,
            /// two-tier header, movable corridor brackets, and feature grouping.
            timeline_2d: bool = false, PAGE_LAYOUT;
            /// Whether unoccupied cells in the 2D timeline grid display subtle alignment dots (·).
            timeline_2d_cell_dots: bool = true, PAGE_LAYOUT;
            /// Allow activating components directly by clicking component tracks or radio indicators in the 2D timeline.
            timeline_2d_component_activation: bool = true, PAGE_LAYOUT;
            /// How unsuppressing an upstream feature affects downstream dependents in the 2D timeline DAG.
            timeline_2d_unsuppress_mode: Timeline2dUnsuppressMode = Default::default(), PAGE_LAYOUT;
            /// Vertical History starts with the active component's Bodies folder and the Origin (#6490).
            timeline_origin_bodies: bool = false, PAGE_LAYOUT;
            /// Where notices and offers stack: the view's bottom-right corner (the default) or the bottom
            /// centre, as before #7893.
            toast_place: crate::viewport::ToastPlace = Default::default(), PAGE_LAYOUT;
            /// While a dialog or a sketch is open, OK and Cancel in the view's top right corner, with the
            /// axis view moved down below them (#6709). Off by default.
            view_confirm_corner: bool = false, PAGE_LAYOUT;
            /// Windows only: while the window is dragged by its title bar, nothing is drawn, so the window
            /// follows the pointer smoothly; animations and progress pause and the picture catches up when
            /// the drag ends (#13991). Off (the default): the window keeps drawing during the drag and
            /// every move step renders a frame, as before. Passed to winit at once, no restart
            /// ([`WINDOWS_SMOOTH_WINDOW_DRAG_AVAILABLE`]).
            windows_smooth_window_drag: bool = false, PAGE_LAYOUT;
            /// Focused views choose the workspace; the switcher focuses a recent matching view.
            /// Off retains the workspace switcher's independent, legacy behaviour.
            workspace_follows_view: bool = true, PAGE_LAYOUT => check_id("prefs.workspace_follows_view");
        }
    };
}
