//! Stored preferences on the Sketch page (`PAGE_SKETCH`).
//!
//! Each entry is one field of `Prefs`, in `preference_settings!`'s form:
//! `field: Type = default, PAGE[, accept(range)][ => control(...)];`. Add a new preference
//! to the file of the page whose Defaults resets it, in alphabetical order by field name, so
//! two new preferences rarely land on the same line.

macro_rules! sketch_preferences {
    ($next:ident $(, $rest:ident)* ; $($fields:tt)*) => {
        $next! { $($rest),* ; $($fields)*
            /// A centre-point arc runs the way the cursor goes round its centre from the start (#6645).
            /// Off: it always runs counter-clockwise from the start, as it did before.
            center_arc_follows_cursor: bool = true, PAGE_SKETCH;
            /// How large constraint glyphs are drawn: a multiple of their 15-pixel badge, letter and icon
            /// included, so they can be read at any zoom (#10794). 1 (the default) is the size they always had.
            constraint_glyph_scale: f32 = 1.0, PAGE_SKETCH, accept(crate::sketch_tools::GLYPH_SCALES) => slider("prefs.constraint_glyph_size", crate::sketch_tools::GLYPH_SCALES, 0.25, |v| format!("{:.0}%", v * 100.0), "prefs:constraint_glyph_size");
            /// The construction line tools as before #15066: the Construction/Normal switch under the
            /// Length box, and the Sketch Palette's Line Type icons leave these tools alone. Off (the
            /// default): those icons set the tools' line type.
            construction_line_classic: bool = false, PAGE_SKETCH;
            /// The colour a sketch dimension draws its lines, arrows and value in, or `None` for the
            /// theme's dimension colours (Preferences › Sketch › Dimension color, #14121). Selection, hover
            /// and conflicts keep their own colours.
            dimension_colour: Option<[u8; 3]> = None, PAGE_SKETCH;
            /// Where a dimension's value is typed: in a box on the dimension (the default), or in a small
            /// window at it whose arrows step the value (#7246, `dim_view::window`).
            dimension_input: DimensionInput = DimensionInput::View, PAGE_SKETCH;
            dimension_step_deg: f64 = 1.0, PAGE_SKETCH, accept(DIMENSION_STEP_DEG);
            /// What the dimension window's arrows, Up/Down and the wheel add to a length (millimetres,
            /// converted to the unit the value is written in) and to an angle (degrees).
            dimension_step_mm: f64 = 1.0, PAGE_SKETCH, accept(DIMENSION_STEP_MM);
            /// The size of a sketch dimension's text as a factor of the theme's size (Preferences › Sketch ›
            /// Dimension text size). 1 draws it as the theme does; it also applies to Show Dimensions
            /// (#14121). A saved value outside the accepted range (a hand edit) reads as 1.
            dimension_text_scale: f32 = 1.0, PAGE_SKETCH, accept(DIMENSION_TEXT_SCALE);
            /// The Ellipse tool draws the major and minor axes as construction lines with the ellipse, so
            /// its half-axes and axes can be dimensioned. Off: the ellipse alone, and D on it gives its
            /// minor radius, labelled R, as before.
            ellipse_axes: bool = true, PAGE_SKETCH;
            /// Two quick presses of Esc in a sketch, with nothing left to cancel, finish it (the default,
            /// as before). Off: Esc only cancels the tool, dialog or selection and the sketch stays open;
            /// Finish Sketch ends it (#12069).
            escape_twice_finishes_sketch: bool = true, PAGE_SKETCH;
            /// The first length dimension of a sketch with nothing sized yet scales the whole drawing to
            /// its value (#1365). Off: it sizes only what it measures (#4109).
            first_dimension_scales: bool = true, PAGE_SKETCH;
            /// With `first_dimension_scales`: the drawing scales about the sketch origin even when nothing
            /// ties it there, so a shape drawn away from the origin moves as it grows (the behaviour
            /// before #11043). Off (the default): such a shape scales about its own middle and stays put.
            first_dimension_scales_about_origin: bool = false, PAGE_SKETCH;
            /// Hide constraint glyphs when the sketch is zoomed out so far that a millimetre of the plane
            /// is smaller than `sketch_tools::MIN_GLYPH_PX_PER_MM` on screen (#8980) and the sketch spans
            /// less than `sketch_tools::MIN_GLYPH_SPAN_PX` (#10518). Off: glyphs stay at a fixed screen
            /// size at every zoom, as before.
            hide_constraint_glyphs_zoomed_out: bool = true, PAGE_SKETCH;
            /// In the Line tool, a press, a drag and the release draw one line from the press to the
            /// release and end there (#7247); clicks still draw a chain. Off (the default): a drag is a
            /// click that moved on the way, placing a point where it is let go (#2448).
            line_drag_draws_one: bool = false, PAGE_SKETCH;
            /// In the Line tool, leaving the chain's last point and resting back on it switches to a
            /// tangent arc, and the same again back to lines (#9489). Off (the default): resting there
            /// does nothing, and tangent arcs are the Arc tool's.
            line_hover_tangent_arc: bool = false, PAGE_SKETCH;
            /// Whether Alt/Option also frees gizmo / canvas manipulator arrows (Primary always does).
            manipulator_free_alt: bool = true, PAGE_SKETCH;
            /// The most points or curves one sketch holds (#10874): from the default (20000), which
            /// sketches have always had, up to 200000, so a dense SVG or DXF drawing fits in one sketch.
            max_sketch_points: usize = solidcraft_engine::sketch::limits::DEFAULT_POINTS, PAGE_SKETCH,
                accept(solidcraft_engine::sketch::limits::DEFAULT_POINTS..=solidcraft_engine::sketch::limits::CEILING_POINTS);
            /// Web app Pencil mode: pen taps toggle items and a pen double-tap takes the whole body or
            /// sketch; finger touches in the view only navigate, and a finger double-tap on a flat face
            /// starts a sketch on it. Off (the default): pens and fingers act as the mouse does.
            pencil_mode: bool = false, PAGE_SKETCH;
            /// Prefer movable sketch points on exact selection ties; off keeps point insertion order.
            prefer_free_sketch_points: bool = true, PAGE_SKETCH;
            /// Open Project's options before picking; off restores its previous direct-pick workflow.
            project_options_dialog: bool = true, PAGE_SKETCH;
            /// A click inside a region for the Entities of Transform, Move/Copy, Scale, Mirror and the
            /// sketch patterns also picks the curves of the region's holes (#8390). Off (the default):
            /// it picks the region's outline, and a shape inside it is picked by a click inside that
            /// shape (#14496).
            region_pick_takes_holes: bool = false, PAGE_SKETCH;
            /// A click on the ribbon button of the sketch tool that is running ends the tool, so clicks
            /// in the view select again (#10151). Off: the click starts the tool again, as before.
            ribbon_click_ends_tool: bool = true, PAGE_SKETCH;
            /// Opt in to displaying and selecting drawn 3D curve nodes as construction references.
            select_3d_curve_points: bool = false, PAGE_SKETCH;
            /// Allow ordinary edge picks through faces in Shaded with hidden edges.
            /// Off preserves the visible-only picking used in v0.30.1.
            select_hidden_edges: bool = false, PAGE_SKETCH;
            /// Include hidden geometry and a small pointer neighbourhood in Select Other.
            select_other_hidden: bool = true, PAGE_SKETCH;
            /// When true: Shift+click adds without removing; Primary toggles.
            /// When false (default): Shift and Primary both toggle.
            selection_shift_add_only: bool = false, PAGE_SKETCH;
            /// Pressing an active tool or dialog's shortcut again closes it, as Esc does. Off: the key
            /// starts the tool or dialog again.
            shortcut_again_ends_tool: bool = true, PAGE_SKETCH;
            /// A sketch tool's shortcut pressed again before the first point steps to the tool's next
            /// creation mode (#2459). Off (the default): the key starts the tool again, as it always has.
            shortcut_cycles_modes: bool = false, PAGE_SKETCH;
            /// Drawing lines a point up with the sketch's points on both axes at once, keeping the
            /// horizontal or vertical snap to the previous point, and the click adds the alignments
            /// (#7828). Off by default: only with the previous point, one axis at a time, as before.
            sketch_align_points: bool = false, PAGE_SKETCH;
            /// A shape drawn in a sketch gets all its dimensions at once: the typed values, and every
            /// other size the shape has at its size as drawn (`sketch.draw.dimensioned` with `as_drawn`).
            /// Shapes `solidcraft_engine::dimension_roles` doesn't list are drawn as before.
            sketch_auto_dimensions: bool = false, PAGE_SKETCH;
            /// With `sketch_auto_dimensions`: new shapes are also placed from the sketch origin until they
            /// are fully constrained (`sketch.draw.dimensioned` with `locate`). A shape already anchored to
            /// the origin or to placed geometry gets nothing more.
            sketch_auto_locate: bool = false, PAGE_SKETCH;
            /// While the open sketch's constraints conflict, the curves and points a failing constraint
            /// holds, and its glyph or dimension, are drawn in `sketch_conflict` (#7244). Off: only the
            /// glyphs and dimensions are red, in `error`, as before.
            sketch_conflict_colour: bool = true, PAGE_SKETCH;
            /// Show the construction-line family in Sketch CREATE; off restores the earlier menu.
            sketch_construction_lines: bool = true, PAGE_SKETCH;
            /// In an open sketch with no tool running, the pointer's sketch coordinates beside it (#11070).
            /// Off: only a sketch tool shows them, while it places a point, as before.
            sketch_cursor_coordinates: bool = true, PAGE_SKETCH;
            /// Where the view goes when a sketch ends (#7038). Keep (the default) leaves it as it is.
            sketch_exit_view: SketchExitView = SketchExitView::default(), PAGE_SKETCH;
            /// A sketch started on a face of a body in another component than the active one activates
            /// that component first, so the sketch belongs to it (`sketch.create` `activate_part`,
            /// #10215). Off (the default): the sketch belongs to the active component, as before.
            sketch_face_activates_part: bool = false, PAGE_SKETCH;
            /// Use the only straight construction line in the preselection as Mirror's reference.
            sketch_mirror_auto_line: bool = true, PAGE_SKETCH;
            /// Opening or editing a sketch turns the view to face it (the default). Off: the view stays as
            /// it is, and Look At faces the sketch when asked.
            sketch_open_look_at: bool = true, PAGE_SKETCH;
            /// Snap a drawing point to where the two most recent centre or midpoint guides cross.
            /// Off: the guides stay visual and the cursor does not move (#8694).
            sketch_recent_snap: bool = false, PAGE_SKETCH;
            /// A point snaps to the crossings of the lines the grid draws, and to the finer lattice
            /// between them. Off: the point snaps to the finer lattice only.
            sketch_snap_to_visible_grid: bool = true, PAGE_SKETCH;
            /// Shift while drawing a Two-Point or Center Rectangle makes it a square and adds an Equal
            /// constraint (#18114). Off (the default): Shift prefers a line midpoint there, as it always has.
            sketch_square_rectangles: bool = false, PAGE_SKETCH => check_tr("prefs.sketch_square_rectangles");
            /// A sketch's icon in History and the browser takes the colour of its solve state, and its
            /// tooltip names the state (#7245). Off: every sketch has the same icon, as before.
            sketch_state_icons: bool = true, PAGE_SKETCH;
            /// Temporary horizontal/vertical guides from hovered centres and edge midpoints.
            sketch_tracking_guides: bool = true, PAGE_SKETCH;
            /// While a drawing tool waits for a point, hovered sketch points (ends, corners, free
            /// points) start tracking guides too (#9105). Off: only centres and midpoints, as before.
            sketch_tracking_points: bool = true, PAGE_SKETCH;

            // ---- Sketch / selection modifier bindings (classic defaults; alternate mapping is opt-in) ----
            /// Modifier that suppresses sketch auto-snap while drawing or dragging a sketch point.
            /// Default: Alt/Option. Set to Ctrl/Cmd for the alternate mapping.
            suppress_sketch_snap: ModifierCombo = ModifierCombo::CTRL_ALT, PAGE_SKETCH;
        }
    };
}
