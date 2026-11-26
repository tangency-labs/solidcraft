//! Stored preferences on the Design page (`PAGE_DESIGN`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! design_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            angle_decimals: u8 = 1, PAGE_DESIGN;
            /// The template new drawings of an assembly start from: an assembly's built-in, by name (ISO A2
            /// when not set, or set to one this version doesn't have for an assembly).
            assembly_drawing_template: String = template::default_for(true).into(), PAGE_DESIGN;
            /// Hovering an Origin plane, axis or point row in the Browser shows that item in the view,
            /// for the design's Origin and each part's, also while it is hidden (#15658): a hidden item
            /// shows only during the hover, its eye and saved visibility unchanged. Off: a hover lights
            /// only an origin plane that is already drawn, as before.
            browser_hover_shows_origin: bool = true, PAGE_DESIGN => check_id("prefs.browser_hover_shows_origin");
            /// First or third angle for new drawings (third when not set, or set to one this version
            /// doesn't know).
            default_projection: Projection = Projection::Third, PAGE_DESIGN;
            /// Units of new designs.
            default_units: String = "mm".into(), PAGE_DESIGN;
            /// The folder Save and Save As open in for a design that has never been saved (#5522). Empty:
            /// the folder last used, as before. The desktop app reads it; the web app has no folders.
            designs_folder: String = String::new(), PAGE_DESIGN;
            /// A double-click on a component, in the browser or on its face in the view, opens a linked
            /// one's design with the assembly around it (`edit_in_place`) and activates any other. Off:
            /// a double-click in the browser activates every component and one in the view does nothing,
            /// as before.
            double_click_components: bool = true, PAGE_DESIGN => check_id("settings.double_click_components");
            /// Scales true size on the drawing sheet so 100 % measures 1 mm as 1 mm on this screen, against
            /// a ruler held to the 100 mm bar in Preferences.
            drawing_calibration: f32 = 1.0, PAGE_DESIGN, accept(0.5..=2.0) => slider("ui.sheet_page.calibration", 0.5..=2.0, 0.005, |v| format!("{v:.3}×"), "prefs:drawing_calibration");
            /// The on-screen drawing canvas pattern: datum_cloud, grid or none (default none, a plain background).
            drawing_sheet_backdrop: String = "none".into(), PAGE_DESIGN;
            /// The drawing sheet's colour on screen: `auto` (follows the theme: White in a light theme, the
            /// theme's dark sheet in a dark one), `white`, `blueprint` or `prussian_blue`
            /// (`crate::drawing_sheet_page::SheetColour`). Screen only: printed
            /// and exported sheets are always white, and nothing reaches the design or undo.
            drawing_sheet_colour: String = "white".into(), PAGE_DESIGN;
            /// Where the Drawing workspace shows its sheet chooser.
            drawing_sheets_visibility: String = "multiple".into(), PAGE_DESIGN;
            /// The file name File › Export and Save As Mesh offer: `{design}` and `{body}` stand for the
            /// design's and the exported body's names (`export_name`). Empty: each export offers the name
            /// it needs: the design name for the whole design, the body name for a subset. The body-first
            /// default can be kept by specifying the body template explicitly.
            export_name: String = String::new(), PAGE_DESIGN;
            /// The folder export dialogs (File › Export, Save As Mesh, drawings and other saved files)
            /// open in, until another folder is chosen in one (#5522). Empty: the folder last used.
            exports_folder: String = String::new(), PAGE_DESIGN;
            /// GLB export writes each component occurrence as a named node holding its part's bodies, so a
            /// viewer can move each part on its own (#5296). Off: every body under the root in world
            /// coordinates, named as the model names it, as SolidCraft 0.40 wrote it.
            glb_export_hierarchy: bool = true, PAGE_DESIGN => check_id_help("ui.glb_export_hierarchy", "ui.glb_export_hierarchy_help");
            /// A STEP part SMLib's check fails is healed of its tolerance-scale defects (edges shorter
            /// than their tolerance, slivers between coincident edges, curve pieces of no length) in the
            /// background once it is imported, and Repair heals a part first (#17689). Off: the part
            /// stays as read, with its check's warning, and Repair does not heal first, as before.
            heal_tolerance_defects: bool = true, PAGE_DESIGN => check_id("prefs.heal_tolerance_defects");
            /// Whether the browser's Origin folder shows in designs that don't keep its eye themselves.
            hide_origin: OriginDisplay = OriginDisplay::default(), PAGE_DESIGN;
            /// Insert Design lists the designs open in other tabs, each inserted with one click, then
            /// Browse… for a file (#8570). Off: it opens the file picker at once, as before.
            insert_lists_open_designs: bool = true, PAGE_DESIGN => check_id("settings.insert_lists_open_designs");
            /// Dragging a part a ball joint holds to a part fixed in place turns it like a trackball: by
            /// the smallest turn about the joint, on a sphere at least the part's size (`joint_drag`,
            /// #13262). Off: the grabbed point goes to the pointer and the part may spin about it, as
            /// before.
            joint_drag_trackball: bool = true, PAGE_DESIGN => check_id("prefs.joint_drag_trackball");
            joint_moving_first: bool = false, PAGE_DESIGN => check_id_help("settings.joint_moving_first", "settings.joint_moving_first_help");
            /// Decimals shown for lengths and angles.
            length_decimals: u8 = 2, PAGE_DESIGN;
            /// A saved design's tab and its Browser top row are named after its file, so a Save As or a
            /// renamed file shows (#7267). Off: both show the name stored in the design, as before.
            names_from_file: bool = true, PAGE_DESIGN => check_id("settings.names_from_file");
            /// After a STEP or IGES file opens, offer to stand a Y-up part up (`stand_up`).
            offer_stand_up: bool = true, PAGE_DESIGN => check_tr("ui.offer_to_stand_up_y_up_files");
            /// The X, Y and Z origin axes run through the origin to the edges of the view, both ways, so
            /// they stay in sight as references wherever the view is (#11123). Off by default: short axes
            /// from the origin, solid then dashed, as before.
            origin_axes_across_view: bool = false, PAGE_DESIGN => check_id("prefs.origin_axes_across_view");
            /// The names the browser, dialogs and lists show for the origin planes (#6491).
            origin_plane_names: OriginPlaneNames = OriginPlaneNames::default(), PAGE_DESIGN;
            /// The size of the origin planes' squares in the view, as a factor of the size they take on
            /// screen by default (#21191). 1 draws them as before; the origin axes keep their length. A
            /// saved value outside the accepted range (a hand edit) reads as 1.
            origin_plane_scale: f32 = 1.0, PAGE_DESIGN, accept(ORIGIN_PLANE_SCALE);
            /// The most parameters Down arrow lists in an empty expression box (#9246): fewer keeps the
            /// list light in a large design.
            parameter_list_rows: u16 = 50, PAGE_DESIGN;
            /// The template new drawings of a part start from: a part's built-in, by name (ISO A3 when not set,
            /// or set to one this version doesn't have for a part).
            part_drawing_template: String = template::default_for(false).into(), PAGE_DESIGN;
            /// Components pasted from another design (tab) go to the origin of the component they are
            /// pasted into, the first one's origin on it and the others kept where they are from it,
            /// each one's contents where they are from its own origin; a whole design copied and pasted
            /// as a linked component goes there too (#17621). Off: copied components go where they were
            /// in the world in their design, and a copied design under the pointer, as before.
            paste_components_at_origin: bool = false, PAGE_DESIGN => check_id_help("prefs.paste_components_at_origin", "prefs.paste_components_at_origin_help");
            /// Reapply a file's stand-up answer when its original bytes have not changed.
            remember_stand_up: bool = true, PAGE_DESIGN => check_id("prefs.remember_stand_up");
            /// Reopen an unchanged CAD import from the existing saved-design model cache.
            reuse_imports: bool = true, PAGE_DESIGN => check_id("prefs.reuse_imports");
            /// How the offer is made for a file whose header names no program known to save Y up: a note
            /// over the view (the default), the dialog, or nothing (#7717).
            stand_up_unknown: StandUpUnknown = StandUpUnknown::default(), PAGE_DESIGN;
            /// Up/Down and the wheel over a focused value field step its plain number (`value_step`,
            /// #2320). Off by default: the keys move the text cursor and the wheel scrolls, as before.
            step_values: bool = false, PAGE_DESIGN => check_id("ui.step_values");
            /// Default modelling orientation: `z` (Z up) or `y` (Y up) (#3).
            up_axis: String = "z".into(), PAGE_DESIGN;
            /// A design coming back to its tab reads again the linked designs whose files changed since
            /// it last read them. Off: they wait for Update Links, as before.
            update_changed_links: bool = true, PAGE_DESIGN => check_id("settings.update_changed_links");
            /// When SMLib's full check runs while features are computed (`diagnostics.validation`):
            /// `each_feature` (the default) checks the bodies each feature keeps once; `each_step`, as
            /// before, checks after every sweep, boolean and blend; `off` (experimental) never does.
            validation: solidcraft_engine::kernel::Validation = solidcraft_engine::kernel::Validation::EachFeature, PAGE_DESIGN;
            /// A command's value (Extrude's Distance, Fillet's Radius…) in a box beside its handle in the
            /// view (`canvas::boxes_shown`). Off: it is typed in the command window only, which takes the
            /// keyboard after a pick instead, and nothing covers the model (#9479).
            value_boxes_in_view: bool = true, PAGE_DESIGN => check_id("prefs.value_boxes_in_view");
        }
    };
}
