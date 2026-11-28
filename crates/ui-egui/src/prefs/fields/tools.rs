//! Stored preferences on the Tools page (`PAGE_TOOLS`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! tools_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// Explicit assembly Move uses joint handles; off keeps the previous Move panel.
            assembly_move_handles: bool = true, PAGE_TOOLS => check_id("joint_move.use_handles");
            /// Feature windows choose Join, New Body or Cut from what their material touches (`op_auto`,
            /// #6901). Off: they keep the operation they open with (Extrude its own Join or Cut), as before.
            auto_operation: bool = true, PAGE_TOOLS => check_id("prefs.auto_operation");
            /// Work on the model directly in the viewport (pull a face to extrude or cut); off by default.
            direct_interaction: bool = false, PAGE_TOOLS;
            /// Drive Joints shows the moved parts' old place as a ghost (its Show ghost box, #5905).
            drive_joints_ghost: bool = true, PAGE_TOOLS => check_tr("prefs.design_tools.ghost");
            /// Dragging Extrude's distance arrow stops at flat faces parallel to the profile over it and
            /// outlines them (`extrude_face_snap`, #2398). Off: the drag moves in round steps only.
            extrude_arrow_face_stops: bool = true, PAGE_TOOLS => check_id("prefs.extrude_arrow_face_stops");
            /// Extrude: with a profile picked, a face, vertex or parallel plane picked off its plane
            /// switches the extent to To Object (off: picks only add profiles, as before). A click into the
            /// Profiles box makes the next picks profiles again.
            extrude_auto_to_object: bool = true, PAGE_TOOLS => check_tr("ui.extrude_auto_to_object");
            /// The length a new Extrude starts at with `extrude_start` `custom` (any length expression).
            extrude_custom_distance: String = "10 mm".into(), PAGE_TOOLS;
            /// Where a new Extrude starts its distance: `fixed` (the default: 10 in the design's unit, as
            /// before), `last_used` (the last new extrude's distance) or `custom` (`extrude_custom_distance`).
            extrude_start: ExtrudeStart = ExtrudeStart::default(), PAGE_TOOLS;
            /// Extrude turns the view to isometric each time it opens, so the depth can be seen (#9176).
            /// Off (the default): the view stays as it is.
            extrude_view_isometric: bool = false, PAGE_TOOLS => check_id("prefs.extrude_view_isometric");
            /// With `extrude_view_isometric`: when Extrude closes (OK or Cancel) the view goes back to
            /// where it was before it turned (#9176). Off (the default): it stays isometric.
            extrude_view_return: bool = false, PAGE_TOOLS => check_id("prefs.extrude_view_return");
            /// A new Solid Fillet opens at the radius the tool ships with, 2 mm, shown in the design's
            /// unit ("0.07874 in" in an inch design): a radius is bound by the faces beside its edges, and
            /// the number read in the design's unit ("2 in" = 50.8 mm) never fits the part (#12418). On: it
            /// opens at the number 2 read in the design's unit, as before.
            fillet_legacy_radius: bool = false, PAGE_TOOLS => check_id_help("prefs.fillet_legacy_radius", "prefs.fillet_legacy_radius.help");
            /// The holes one Hole command makes (two or more) go into a feature folder in History, closed
            /// at first (`hole_place`). Off by default: each hole is its own entry in History, as before.
            hole_group: bool = false, PAGE_TOOLS => check_id("prefs.design_tools.hole_group");
            /// A placed Single hole has a handle that drags it, and a pick elsewhere on a face that has a
            /// hole moves the hole there (`hole_place`). Off by default: a second pick on its face takes
            /// the hole out, and a hole is moved by taking it out and picking again.
            hole_moves: bool = false, PAGE_TOOLS => check("Move holes by clicking or dragging");
            /// A face takes several Single holes: a pick elsewhere on a face that has a hole adds another,
            /// and a pick on a hole takes it out (`hole_place`). Off by default: a face takes one Single
            /// hole, and a second pick on it takes the hole out (or moves it, with `hole_moves`).
            hole_several: bool = false, PAGE_TOOLS => check_id("prefs.design_tools.hole_several");
            /// Placing a Single hole snaps to the middle of the face and the centres of its round edges
            /// (`hole_place`). Off by default: a hole goes where it is clicked.
            hole_snaps: bool = false, PAGE_TOOLS => check("Snap to face and edge centres");
            /// In Fillet and Chamfer the edges a click would take are drawn thin in `hover_edge_lit`
            /// (orange in Light and Dark): the edge under the pointer, or the edges of the face under it.
            /// Off (the default): the edge or face is lit as everywhere else. Switched in the tool's
            /// Options section, never in Preferences; remembered between uses.
            hover_edge_lit: bool = false, PAGE_TOOLS;
            /// Degrees per step near the assembly Move ring.
            joint_rotation_coarse: f64 = 10.0, PAGE_TOOLS;
            /// Degrees per step after pulling the cursor outside the precision guide.
            joint_rotation_fine: f64 = 0.1, PAGE_TOOLS;
            /// Show world X/Y/Z component dimensions beside the total distance in Measure.
            measure_axis_distances: bool = true, PAGE_TOOLS;
            /// While Move's triad drags a component drawn as a layer of its own, what is under the pointer
            /// is still picked and highlighted, as before #7273. Off by default: such a drag picks nothing,
            /// so a step neither meshes the dragged bodies where they stand nor looks up the Browser row a
            /// highlight follows (`drag_layer::holds_pointer`).
            move_drag_highlights: bool = false, PAGE_TOOLS => check_id("prefs.move_drag_highlights");
            /// A new Extrude, Revolve, Loft, Sweep or primitive (Box, Cylinder, ...) opens in New Body and
            /// keeps it, as if New Body were picked by hand: `auto_operation` doesn't choose for it (#19870).
            /// Off by default: the window opens and chooses as before. Editing a feature keeps its operation.
            new_body_default: bool = false, PAGE_TOOLS => check_id("prefs.new_body_default");
            /// Handles on the preview of a feature placed on a face (Rest, Boss, Snap Fit, Emboss, and the
            /// Box, Cylinder, Cone, Sphere, Torus and Coil): a marker that drags it across the model and,
            /// where it can turn in the face, a ring that turns it. Off by default: a Rest, Boss or Snap
            /// Fit is then placed by its click only, as before, and its Position input takes faces only; an
            /// Emboss by its fields; a primitive by its fields, its inputs or the selection it opened with.
            placement_handles: bool = false, PAGE_TOOLS => check_tr("ui.placement_handles");
            /// Reuse a command's safe values and options after it succeeds, until the app closes. Off by default: each dialog opens with its usual defaults.
            remember_dialog_values: bool = false, PAGE_TOOLS => check_id_help("prefs.remember_dialog_values", "prefs.remember_dialog_values.help");
            /// Snap points (#5912): wherever a point is picked (Move, Align and Measure so far, #5881),
            /// hovering an edge, face or sketch curve shows its centre, middle and end points to snap to. On by default; off, a pick takes only
            /// what is under the pointer, as before.
            snap_points: bool = true, PAGE_TOOLS => check_id("settings.snap_points");
        }
    };
}
