//! Stored preferences on the General page (`PAGE_GENERAL`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! general_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// Opening, inserting or dropping a file whose format has import options (STEP, IGES,
            /// FreeCAD, Rhino, glTF) asks them first (`import_options`); off imports at once with the defaults,
            /// as before.
            ask_import_options: bool = true, PAGE_GENERAL => check_id("settings.ask_import_options");
            /// Import with cadmpeg: a STEP, IGES, FreeCAD or Rhino file is read from cadmpeg's model
            /// straight into B-rep records: a STEP or IGES file decoded by cadmpeg's STEP or IGES decoder
            /// instead of SolidCraft's own reader, and a FreeCAD or Rhino file's solids instead of
            /// restated as STEP by cadmpeg's STEP writer and read by SolidCraft's STEP reader. Off (the
            /// default) keeps SolidCraft's readers for all four, as before.
            /// Export STEP and IGES with cadmpeg: File › Export of a STEP or IGES file asks `file.export`
            /// for cadmpeg's writer (`writer: cadmpeg`), which writes the design through cadmpeg's model.
            /// Off (the default) keeps SolidCraft's own STEP and IGES writers, as before.
            cadmpeg_export: bool = false, PAGE_GENERAL => check_id("settings.cadmpeg_export");
            cadmpeg_import: bool = false, PAGE_GENERAL => check_id("settings.cadmpeg_import");
            /// Check for updates at start and once a day (docs/updates.md).
            check_updates: bool = true, 0;
            /// With a dialog open, typing the first letter of a button's label presses it: O for OK,
            /// C for Cancel (#9841). Off by default: typing does nothing to the buttons, as before.
            /// A letter that fits two buttons does nothing, and a focused text field keeps the keystroke.
            dialog_first_letter_buttons: bool = false, PAGE_GENERAL => check_id("prefs.dialog_first_letter_buttons");
            /// Export STL, Export 3MF and Send to Slicer start with the design's body in Bodies when it
            /// has just one (#13098). Off: the list starts empty, as before.
            export_selects_only_body: bool = true, 0;
            /// A FreeCAD file opens or inserts as its sketches and features, rebuilt; off brings in only
            /// its solids as one Import feature, as before.
            freecad_history: bool = true, PAGE_GENERAL => check_id("settings.freecad_history");
            /// Print faces show in the view at all times and are saved in the design, as before #11512.
            /// Off (the default): they show only while the Send to Slicer window is open, a save leaves
            /// them out, and a design that has them opens without them.
            keep_print_faces: bool = false, PAGE_GENERAL;
            keep_unsaved_also_asks: bool = true, PAGE_GENERAL;
            /// Web app (#10220): keep each open design's unsaved changes current in the browser's
            /// private storage, so closing or reloading the tab doesn't ask through the browser's own
            /// "leave site?" box; the next visit offers them back. Off: the browser asks, as before.
            keep_unsaved_in_browser: bool = false, PAGE_GENERAL;
            /// Interface language: a registered locale ID, or `system` to follow the system's language
            /// on every start (#10138). A saved language keeps winning, so existing users don't change.
            language: String = "system".into(), 0;
            /// Folders linked files are loaded from when a design opens, besides the design's own folder
            /// (`link_folders_prompt`): those the user answered Load for. Kept in the user's settings,
            /// never in a design.
            linked_file_folders: Vec<String> = Vec::new(), PAGE_GENERAL;
            /// The largest design file opened or saved, in MB (#4086): from the default (256) up to 16 GB.
            max_design_file_mb: u64 = DEFAULT_DESIGN_MB, PAGE_GENERAL;
            /// The largest STEP or import file opened, in MB (#4086): from the default (512) up to 16 GB.
            /// `--max-file-size` overrides it for one run. The browser keeps the default.
            max_step_file_mb: u64 = DEFAULT_STEP_MB, PAGE_GENERAL;
            /// The servers the files a design names may be on ("Always allow this server").
            network_hosts: Vec<String> = Vec::new(), PAGE_GENERAL;
            /// The files a design names (linked designs, font files) may be on any network location, for
            /// organisations that keep their designs on file servers. Off (the default): only on the
            /// servers in `network_hosts`, or one the user allows when asked (`network_prompt`).
            network_locations: bool = false, PAGE_GENERAL => check_tr("prefs.network_locations");
            /// Those designs' files, in tab order, as the last quit left them.
            open_designs: Vec<String> = Vec::new(), PAGE_GENERAL;
            /// How strongly a print face stands out in the viewport (#8429): the share of the section
            /// colour in its tint, 0 to 1; its outline follows. 0 draws no highlight.
            print_face_highlight: f32 = 0.2, PAGE_GENERAL, accept(0.0..=1.0);
            /// Reopen the saved designs that were open when the app was last closed (#5542).
            reopen_designs: bool = true, PAGE_GENERAL;
            /// A right-click in the view with a dialog open confirms its input: on to the next one, then OK.
            /// Off: it does nothing there, as before (#6159).
            right_click_confirms: bool = false, PAGE_GENERAL => check("Right-click confirms dialog inputs", "With a dialog open, a right-click in the view goes on to its next empty input, and on the last one runs OK");
            /// Ask separately when closing several unsaved designs, as before grouped saving.
            save_prompt_one_at_a_time: bool = false, PAGE_GENERAL => check_id("prefs.save_prompt_one_at_a_time");
            /// The Discord button at the status bar's right end; the × on it turns it off.
            show_discord_button: bool = true, PAGE_GENERAL => check_id("prefs.show_discord_button");
            /// The Donate button (Ko-fi) at the status bar's right end. The button's × turns it off; the Help
            /// menu, About and the start page keep their link.
            show_donate_button: bool = true, PAGE_GENERAL => check_id("prefs.show_donate_button");
            /// Show Home on an ordinary empty launch and after closing the last design.
            show_home_on_startup: bool = true, PAGE_GENERAL;
            /// Slicers that can load a file into the window that is already open do so (default on).
            slicer_single_instance: bool = true, 0;
        }
    };
}
