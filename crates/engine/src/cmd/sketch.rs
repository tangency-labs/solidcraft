//! Sketch commands: create/edit/finish sketches, draw curves, constraints and dimensions.

use serde_json::{Value, json};
use solidcraft_doc::{FeatureKind, PlaneRef, expr::Kind};
use solidcraft_geom::{Plane, Vec2, Vec3};
use solidcraft_sketch::{Analysis, Budget, ConstraintKind, CurveKind, Hold, PointExprs, Sketch, SolveReport, TwoCircles, solve, solve_with};

use super::{CommandSpec, in_sketch};
use crate::params::{bad, bool_, expr, num, req_expr, req_vec2, str_, string_list, vec2, vec3};
use crate::{EngineError, Result, Session, SketchEntry};

#[path = "sketch_dimensioned.rs"]
mod dimensioned;
#[path = "sketch_drag.rs"]
mod drag;
#[path = "sketch_face.rs"]
pub(super) mod face;
use super::sketch_store;
pub use dimensioned::dimension_roles;
use dimensioned::draw as draw_dimensioned;
use drag::drag_entity;

/// The `connect` parameter every drawing command takes (see `connect_points`).
macro_rules! connect_doc {
    () => {
        "; connect?: bool (default true: a new point landing exactly on an existing point, the origin too, is made coincident with it)"
    };
}

macro_rules! model_operand_doc {
    () => {
        "; any of them may be a model edge {edge: [x,y,z], body?} or vertex {vertex: [x,y,z], body?}, projected into the sketch first (reported in created)"
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.create", "Create Sketch", create_sketch)
        .at("SOLID", "CREATE")
        .icon("sketch")
        .params("plane: XY|XZ|YZ | construction plane name or id | {origin?: [x,y,z] numbers in mm, default [0,0,0] (not an expression), x_dir, y_dir} (in world coordinates unless frame says otherwise; a named plane is the active component's own) | {face: [world x,y,z], body?: exact world body, index?: face index, path?: complete occurrence ids, fixed?: bool}; new face attachments require a unique certified source (a flat face of a sheet metal body attaches to its panel instead, in the flat pattern's coordinates), fixed:true explicitly freezes the selected plane; offset?: expr, flip?: bool | \"x\" | \"y\" (not on a face: sketch the plane from its other side, so the normal and the default extrude direction are reversed; a half turn about the plane's own x or y axis, true picks the one keeping the sketch's y axis upward; offset still moves along the unflipped normal), frame?: world (default) | component (which coordinates an explicit plane is in; the reply's `frame` says which, with a `note` when the active component is moved or turned), project_edges?: bool, name?"),
    CommandSpec::new("sketch.edit", "Edit Sketch", edit_sketch).noundo().icon("sketch").params("sketch: id or name"),
    CommandSpec::new("sketch.finish", "Finish Sketch", finish_sketch).at("SKETCH", "FINISH SKETCH").icon("finish").enabled(in_sketch).noundo(),
    CommandSpec::new("sketch.cancel", "Cancel Sketch", cancel_sketch)
        .at("SKETCH", "FINISH SKETCH")
        .icon("sketch_cancel")
        .enabled(in_sketch)
        .params("Leaves the sketch without keeping what changed since it was created or opened for editing: the design goes back to how it was then (a new sketch is removed), as one step that Undo brings back. Refused when the undo history no longer leads back to that point (use Undo instead)"),
    CommandSpec::new("sketch.line", "Line", draw_line)
        .at("SKETCH", "CREATE")
        .icon("line")
        .key("L")
        .enabled(in_sketch)
        .params(concat!("points: [[x,y] | \"l1.end\" | {at, ref}…] (x and y each a number or an expression over the design's parameters, \"W / 2\": such a point is fixed where it comes to and follows the parameters; likewise wherever a point argument also takes a point id), closed?: bool, construction?: bool, ids?: [..], infer?: bool (add horizontal/vertical)", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.draw.dimensioned", "Draw Dimensioned Sketch Geometry", draw_dimensioned).icon("dimension")
        .enabled(in_sketch)
        .params(concat!("command: sketch creation command, params: object, dimensions: object of role to numeric or expression value, or `name = expression` (also `#name = expression`), which defines the user parameter `name` as sketch.dimension does (the reply's `defined` lists the names) (optional with as_drawn or locate), as_drawn?: bool (also dimension every other role of the shape at its size as drawn; a radius or diameter given takes the place of the other, a circle otherwise takes its diameter, and a size the sketch already determines, such as the angle of a line inferred horizontal, is left out), locate?: bool (then place the shape from the sketch origin until nothing of it is left free: horizontal and vertical distances from the origin to its points, centres first, each kept only where it removes freedom, so a shape anchored to the origin or to placed geometry gets none; a point on an axis goes onto that axis instead of a 0 distance; the reply's located lists them); creates geometry and all driving dimensions atomically in one undo step", created_doc!())),
    CommandSpec::new("sketch.rectangle.two_point", "2-Point Rectangle", rect_two_point)
        .at("SKETCH", "CREATE")
        .icon("rect")
        .key("R")
        .enabled(in_sketch)
        .params(concat!("p0, p1: opposite corners [x,y]; construction?. Returns sides {bottom, right, top, left} (line ids) and corners {bottom_left, bottom_right, top_right, top_left} (point ids)", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.rectangle.three_point", "3-Point Rectangle", rect_three_point)
        .at("SKETCH", "CREATE")
        .icon("rect3")
        .enabled(in_sketch)
        .params(concat!("p0, p1: first edge; p2: point on the opposite edge; construction?: bool", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.rectangle.rounded", "Rounded Rectangle", rect_rounded)
        .at("SKETCH", "CREATE")
        .icon("rect_rounded")
        .enabled(in_sketch)
        .params(concat!("p0, p1: opposite corners [x,y]; radius: expr (the corner radius, less than half the shorter side); construction?. A 2-point rectangle whose four corners are filleted alike: one radius dimension (`param`), the other arcs equal to it. Returns sides {bottom, right, top, left} (line ids) and arcs (the corner arcs: bottom-left, bottom-right, top-right, top-left)", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.rectangle.center", "Center Rectangle", rect_center)
        .at("SKETCH", "CREATE")
        .icon("rect_center")
        .enabled(in_sketch)
        .params(concat!("center, corner; construction?: bool. Returns sides {bottom, right, top, left} (line ids) and corners {bottom_left, bottom_right, top_right, top_left} (point ids)", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.circle.center", "Center Diameter Circle", circle_center)
        .at("SKETCH", "CREATE")
        .icon("circle")
        .key("C")
        .enabled(in_sketch)
        .params(concat!(
            "center: [x,y] or point ref, radius | diameter: number; through?: point ref (a point the circle passes through, kept on it: where the tool's second click snapped); construction?: bool",
            connect_doc!(),
            created_doc!()
        )),
    CommandSpec::new("sketch.circle.two_point", "2-Point Circle", circle_two)
        .at("SKETCH", "CREATE")
        .icon("circle2")
        .enabled(in_sketch)
        .params(concat!("p0, p1: diameter ends; construction?: bool", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.circle.three_point", "3-Point Circle", circle_three)
        .at("SKETCH", "CREATE")
        .icon("circle3")
        .enabled(in_sketch)
        .params(concat!("p0, p1, p2: [x,y] or point ref (a referenced point stays on the circle); construction?: bool", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.arc.three_point", "3-Point Arc", arc_three).at("SKETCH", "CREATE").icon("arc3").enabled(in_sketch).params(concat!("start, end, through: [x,y] or point ref (a referenced through point stays on the arc); construction?: bool", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.arc.center_point", "Center Point Arc", arc_center)
        .at("SKETCH", "CREATE")
        .icon("arc_center")
        .enabled(in_sketch)
        .params(concat!("center, start, end (counter-clockwise from start; clockwise?: bool for clockwise) | center, start, sweep: signed degrees; construction?: bool", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.polygon.inscribed", "Inscribed Polygon", polygon_inscribed)
        .at("SKETCH", "CREATE")
        .icon("polygon")
        .enabled(in_sketch)
        .params(concat!("center, radius (to vertices), sides, angle?: deg", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.polygon.circumscribed", "Circumscribed Polygon", polygon_circumscribed)
        .at("SKETCH", "CREATE")
        .icon("polygon_circumscribed")
        .enabled(in_sketch)
        .params(concat!("center, radius (to edge midpoints), sides, angle?: deg", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.polygon.edge", "Edge Polygon", polygon_edge)
        .at("SKETCH", "CREATE")
        .icon("polygon_edge")
        .enabled(in_sketch)
        .params(concat!("p0, p1: one edge, sides", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.slot.center_to_center", "Center to Center Slot", slot_c2c)
        .at("SKETCH", "CREATE")
        .icon("slot")
        .enabled(in_sketch)
        .params(concat!("p0, p1: arc centres, width", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.slot.overall", "Overall Slot", slot_overall)
        .at("SKETCH", "CREATE")
        .icon("slot_overall")
        .enabled(in_sketch)
        .params(concat!("p0, p1: overall ends, width", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.point", "Point", draw_point).at("SKETCH", "CREATE").icon("point").enabled(in_sketch).params(concat!("point: [x,y] (each a number or an expression over the design's parameters: the point is then fixed where it comes to and follows them) or a snap: \"mid:<line>\" (held at the line's midpoint), \"on:<curve>:x,y\" (on the curve near x,y), {vertex: [x,y,z]} or {on_edge: [x,y,z]} of the model; id?", connect_doc!(), created_doc!())),
    CommandSpec::new("sketch.dimension", "Sketch Dimension", dimension).at("SKETCH", "CREATE").icon("dimension").key("D").enabled(in_sketch).params(
        "entities: [refs] (a line midpoint is \"mid:line_id\"; a model edge or vertex, {edge: [x,y,z], body?} or {vertex: [x,y,z], body?}, is projected into the sketch first and measured there: a point against the projected edge measures the distance to it), type?: auto|distance|horizontal|distance_x|vertical|distance_y|length|radius|diameter|angle, value?: number or expression (default: current; a bare number is in the design's units, e.g. 2 means 2 in in an inch design; give a unit such as \"50 mm\" to override), or `name = expression` (e.g. \"width = 40\"; also `#name = expression`), which adds the user parameter `name` with that expression and dimensions with `name` (refused when the name is taken or reserved; the reply's `defined` names it); text_at?: [x, y]. Two points, two midpoints or a line off the axes: with auto, text_at inside the box they span (as opposite corners) or in its outer corners is the distance between them, above or below the box horizontal, beside it vertical. Two lines: the angle between them (acute, or 180° − it when the value is nearer that) unless text_at picks a sector. A line and a circle or arc: the distance from the line to its edge (the gap, or how far the edge reaches past a line that cuts it; 0 is tangent); dimension the centre point for the distance to the centre. A point and a circle or arc: the distance from the point to its edge (or how far within it a point inside lies). Two circles or arcs: the gap between their edges (or how far they overlap when they cross, or the narrowest gap between them when one lies inside the other), or the centre-to-centre distance when they are the two caps of a slot. An arc and one of its end points: the arc's length (type arc_length does it from the arc alone). A dimension the sketch already determines is refused (naming what determines it); if_redundant?: \"driven\" places it as a driven (reference) dimension instead. scale_sketch?: bool (the dimension tool sends true): when this is the sketch's first length dimension and nothing in the sketch fixes a size (no other driving length dimension, nothing fixed or projected), the whole sketch scales about its origin to the value instead of distorting. Returns measured in unit (mm or deg), and scaled: the factor when the sketch was scaled",
    ),
    CommandSpec::new("sketch.constraint.horizontal_vertical", "Horizontal/Vertical", c_horizontal_vertical)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_hv")
        .enabled(in_sketch)
        .params("line: id | points: [a, b] (point ids or {midpoint: line_id}; accepted midpoints reuse or create a point with a Midpoint constraint); mode?: horizontal|vertical"),
    CommandSpec::new("sketch.constraint.coincident", "Coincident", c_coincident)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_coincident")
        .enabled(in_sketch)
        .params(concat!("a, b: points, a point and a curve or sketch axis (x / y: the sketch's own axes, unless a curve has that id; an axis becomes a linked construction line on it, reported in created), or two distinct lines (joins their closest endpoints), or two circles/arcs (the second lies on the first: concentric and equal, `constraints` lists both)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.tangent", "Tangent", c_tangent)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_tangent")
        .enabled(in_sketch)
        .params(concat!("a, b: curves or sketch axes (x / y: the sketch's own axes, unless a curve has that id; an axis becomes a linked construction line on it, reported in created)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.equal", "Equal", c_equal)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_equal")
        .enabled(in_sketch)
        .params(concat!("a, b: lines or circles/arcs; or curves: [2 to 200 lines or circles/arcs], each made equal to the first, in one step", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.parallel", "Parallel", c_parallel)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_parallel")
        .enabled(in_sketch)
        .params(concat!("a, b: lines or sketch axes (x / y: the sketch's own axes, unless a curve has that id; an axis becomes a linked construction line on it, reported in created)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.perpendicular", "Perpendicular", c_perpendicular)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_perpendicular")
        .enabled(in_sketch)
        .params(concat!("a, b: lines or sketch axes (x / y: the sketch's own axes, unless a curve has that id; an axis becomes a linked construction line on it, reported in created)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.fix", "Fix/UnFix", c_fix)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_fix")
        .enabled(in_sketch)
        .params("entity: point or curve | entities: [points and curves]; fixed?: bool (default: fix them all unless all are fixed, then unfix)"),
    CommandSpec::new("sketch.constraint.midpoint", "MidPoint", c_midpoint)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_midpoint")
        .enabled(in_sketch)
        .params(concat!("point, line | a, b in pick order: a point and a line (either way round), or two lines whose midpoints meet (the first stays, the second moves)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.concentric", "Concentric", c_concentric)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_concentric")
        .enabled(in_sketch)
        .params(concat!("a, b: circles/arcs", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.collinear", "Collinear", c_collinear)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_collinear")
        .enabled(in_sketch)
        .params(concat!("a, b: lines or sketch axes (x / y: the sketch's own axes, unless a curve has that id; an axis becomes a linked construction line on it, reported in created)", model_operand_doc!())),
    CommandSpec::new("sketch.constraint.symmetry", "Symmetry", c_symmetry)
        .at("SKETCH", "CONSTRAINTS")
        .icon("c_symmetry")
        .enabled(in_sketch)
        .params(concat!("a, b: two points or two curves (lines mirror their ends, circles/arcs their centres); line: symmetry line id, or x / y for the sketch's own axes (a curve with that id wins; an axis becomes a linked construction line on it, reported in created); or point (or a point id as line, \"origin\" too): symmetric about that point, each pair joined by a construction line it is the midpoint of", model_operand_doc!())),
    CommandSpec::new("sketch.construction", "Normal/Construction", construction)
        .at("SKETCH", "CREATE")
        .icon("construction")
        .key("X")
        .enabled(in_sketch)
        .params("curves: [ids], value?: bool (default: construction unless all of them already are)"),
    CommandSpec::new("sketch.delete", "Delete Sketch Entities", sketch_delete).icon("delete")
        .enabled(in_sketch)
        .params("entities: [curve, point or constraint ids]"),
    CommandSpec::new("sketch.move_point", "Drag Sketch Point", move_point).icon("sketch_move").enabled(in_sketch).params("point: ref, to: [x,y]"),
    CommandSpec::new("sketch.drag", "Drag Sketch Geometry", drag).icon("sketch_move").enabled(in_sketch).live(drag_live).params(
        "entity: point or curve, from: [x,y] where it was grabbed, to: [x,y], live?: bool: points and the material point grabbed on a line move toward `to` within the design constraints. When exact placement is impossible, bounded feasible steps minimize cursor distance. Splines slide by to - from; a circle or arc grabbed on its edge takes the radius to `to`. `with?`: [point or curve ids] moves the rest of a selection with the entity as one translation (circles keep their radii). Long moves retain the solution branch; arcs cannot sweep through their starts. Returns `reached`: progress projected onto the requested move (0..1), `at`: the grabbed position for the next frame, and `retry`: budget-limited progress can continue in another frame. `live: true` shares a work budget across held-solve decomposition and cursor correction; ordinary recovery and the final model evaluation retain their existing limits, not a wall-clock deadline. Later features stay stale until `sketch.drag_end`; `dof` stays from before the drag",
    ),
    CommandSpec::new("sketch.drag_end", "End Sketch Drag", drag_end).icon("sketch_move").enabled(in_sketch).doc_only().params(
        "constraint?: {command: a sketch.constraint.* id, params}: ends a drag made of `sketch.drag` `live: true` frames: adds the constraint it let go on (not kept if it over-constrains the sketch), then evaluates the whole model once; returns the constraint's result and the sketch's solved state and degrees of freedom",
    ),
    CommandSpec::new("sketch.dimension_text", "Move Dimension Text", dimension_text).icon("edit")
        .enabled(in_sketch)
        .params("dimension: constraint id or parameter name; at: [x,y] text centre (sketch coordinates) | reset: true (default place)"),
    CommandSpec::new("sketch.options", "Sketch Palette Options", sketch_options).icon("sketch").params(
        "sketch?: id|name (default active); any of show_profile, show_points, show_dimensions, show_constraints, show_projected, show_construction, grid, snap, slice, sketch_3d: bool. Returns them all",
    ),
    CommandSpec::new("sketch.chain", "Chain", chain_cmd).icon("chain")
        .enabled(in_sketch)
        .noundo()
        .params("curve: id: the curves joined to it end to end (what a double-click selects)"),
    CommandSpec::new("sketch.solve", "Solve Sketch", solve_cmd).icon("compute")
        .enabled(in_sketch)
        .noundo()
        .params("reports the solver status and degrees of freedom"),
];

// ---------------------------------------------------------------------------------------------
// Helpers

/// Edit a sketch: clone it, apply `f`, solve with the document's dimension values and store
/// it. With `strict`, an edit that makes the constraints unsolvable is rejected.
pub(super) fn edit<T>(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    strict: bool,
    f: impl FnOnce(&mut Sketch, &mut solidcraft_doc::Document) -> Result<T>,
) -> Result<(T, Value)> {
    let id = sketch_store::target(s, p, cmd)?;
    let (out, rep) = edit_solved(s, id, strict, Analysis::Freedom, |sk, doc| Ok((f(sk, doc)?, Left::Unsolved)))?;
    Ok((out, rep.map(|rep| report_json(&rep)).unwrap_or(Value::Null)))
}

/// A solve's outcome as commands return it.
pub(super) fn report_json(rep: &SolveReport) -> Value {
    json!({"solved": rep.keeps_orientation(), "dof": rep.dof})
}

/// What an [`edit_solved`] step leaves for the edit's own solve.
pub(super) enum Left {
    /// The sketch, solved from where it is.
    Unsolved,
    /// The sketch, solved keeping the orientation of these positions (the last solved ones,
    /// before a drag left a step placed but unsolved).
    MovedFrom(Vec<Vec2>),
    /// The sketch, solved with the dimensions' values (from the parameters) applied.
    Solved,
}

impl Left {
    /// Solved again by the edit even when already solved (for its report).
    pub(super) fn solve_again(self) -> Left {
        match self {
            Left::Solved => Left::Unsolved,
            left => left,
        }
    }
}

/// `edit` of sketch `id` for a step that may solve the sketch itself: when it returns
/// [`Left::Solved`] (and offsets need not be recomputed) the sketch is not solved again and the
/// report is `None`.
/// `analysis` must be `Full` when the report is published without a model evaluation (a live
/// drag); command JSON only consumes freedom, and its later model evaluation publishes a full report.
pub(super) fn edit_solved<T>(
    s: &mut Session,
    id: u64,
    strict: bool,
    analysis: Analysis,
    f: impl FnOnce(&mut Sketch, &mut solidcraft_doc::Document) -> Result<(T, Left)>,
) -> Result<(T, Option<SolveReport>)> {
    let mut doc = (*s.doc).clone();
    let mut sk = sketch_store::read(&doc, id)?.clone();
    if sk.patterns.iter().any(|p| p.count_expression.is_some() || !p.expressions.is_empty()) {
        doc.apply_pattern_values(&doc.param_values().0, &mut sk)?;
    }
    // The next frame of a live gesture on this sketch, nothing else run since the last one: its
    // links are as that frame projected them, from a model that hasn't changed (#3222).
    let fresh = s
        .live_links
        .as_ref()
        .is_some_and(|(sketch, after, revision)| *sketch == id && std::sync::Arc::ptr_eq(after, &s.doc) && *revision == s.revision);
    if !fresh {
        super::sketch_project::refresh(s, &doc, id, &mut sk);
    }
    // The stored sketch is solved for its stored values.
    let stored = sk.driving_values();
    // The constraints the sketch has before the edit: a conflict message must not name one the
    // edit itself adds (the trial already holds it under the id it would get, #9735).
    let pre: Vec<String> = sk.constraints.iter().map(|c| c.id.clone()).collect();
    let (out, left) = f(&mut sk, &mut doc)?;
    // A curve that took an offset spline's generated node (drawn from its end) gets its own point there.
    sk.detach_offset_points()?;
    let offsets = !sk.offsets.is_empty() || !sk.freeform_offsets.is_empty();
    let rep = match left {
        // A drag step solved it (held, with the dimensions' values applied).
        Left::Solved if !offsets => None,
        // A drag left a step placed but unsolved: solve it keeping the orientation of the last
        // solved positions. A drag changes no values, so there is nothing to continue.
        Left::MovedFrom(reference) => {
            doc.apply_dimension_values(&doc.param_values().0, &mut sk)?;
            sk.refresh_offsets()?;
            let unsolved = sk.clone();
            let first_analysis = if offsets { Analysis::StatusOnly } else { analysis };
            let mut rep = counted_solve(&mut sk, &Hold::default(), Some(&reference), first_analysis, None);
            if offsets {
                sk.refresh_offsets()?;
                rep = counted_solve(&mut sk, &Hold::default(), Some(&reference), analysis, None);
            }
            Some(settle(strict, rep, &mut sk, unsolved, &pre)?)
        }
        // Solved for the document's values, continuing from the stored solved state.
        Left::Solved | Left::Unsolved => {
            let (vals, _) = doc.param_values();
            sk.refresh_offsets()?;
            let unsolved = sk.clone();
            #[cfg(test)]
            solve_counts::record(analysis);
            let rep = doc.solve_sketch_with_analysis(&vals, &mut sk, &stored, None, analysis)?;
            Some(settle(strict, rep, &mut sk, unsolved, &pre)?)
        }
    };
    sketch_store::checked(&sk)?;
    sketch_store::write(&mut doc, id, sk)?;
    *s.doc_mut() = doc;
    Ok((out, rep))
}

/// An edit's solve outcome: with `strict`, a solve that fails or turns the sketch over is an
/// error; otherwise such a sketch is stored as the edit left it before the solve, its values
/// and geometry together, so the stored sketch stays solved for its stored values.
/// `pre` are the constraint ids the sketch had before the edit: the trial already holds any
/// constraint the edit adds, under the id it would get, so a conflict list that named it would
/// name an id the sketch does not have yet (#9735).
fn settle(strict: bool, mut rep: SolveReport, sk: &mut Sketch, unsolved: Sketch, pre: &[String]) -> Result<SolveReport> {
    if strict && !rep.keeps_orientation() {
        if rep.folded.is_empty() && rep.oversized.is_none() && !rep.failing.is_empty() {
            let had: Vec<String> = rep.failing.iter().filter(|id| pre.iter().any(|p| p == *id)).cloned().collect();
            if had.is_empty() {
                return Err(EngineError::Other(
                    "that would over-constrain the sketch (it conflicts with what the other constraints already determine)".into(),
                ));
            }
            rep.failing = had;
        }
        return Err(EngineError::Other(rep.failure(|f| format!("that would over-constrain the sketch (conflicts with {f})"))));
    }
    // A solve that throws points far beyond the sketch found a far solution of its constraints,
    // not the one the edit means (a line squeezed to a point under a direction constraint, an
    // angle between sides that can't take it): it counts as one that fails.
    let far = flung(&unsolved, sk);
    if strict && far {
        return Err(EngineError::Other(
            "that would throw points of the sketch far away: its constraints have no solution near the sketch as it is".into(),
        ));
    }
    // A solve that shrinks a line or circle to a point met its constraints only by squeezing
    // that curve away (a corner made the midpoint of the opposite side of its rectangle).
    let gone = shrunk(&unsolved, sk);
    if strict && let Some(curve) = &gone {
        return Err(EngineError::Other(format!("that would shrink {curve} to a point: its constraints can only be met without it")));
    }
    if !rep.keeps_orientation() || far || gone.is_some() {
        *sk = unsolved;
    }
    Ok(rep)
}

/// The first line or circle that solving `before` into `after` shrank to a point: longer (or
/// wider) than 1e-3 before and shorter than 1e-5 after. A length a dimension asks for is never
/// that short. 1e-6 let a side squeezed to 1.2e-6 by one solve through, and the next solve
/// took it the rest of the way.
fn shrunk(before: &Sketch, after: &Sketch) -> Option<String> {
    let size = |sk: &Sketch, i: usize| match sk.curves.get(i).map(|c| &c.kind) {
        Some(CurveKind::Line { .. }) => line_pts(sk, i).map(|(a, b)| a.dist(b)),
        Some(CurveKind::Arc { .. } | CurveKind::Circle { .. }) => sk.radius(i),
        _ => None,
    };
    (0..before.curves.len().min(after.curves.len()))
        .find(|&i| matches!((size(before, i), size(after, i)), (Some(b), Some(a)) if b > 1e-3 && a < 1e-5))
        .and_then(|i| after.curves.get(i).map(|c| c.id.clone()))
}

/// A solve moving some point further than this many times the sketch's size (and its largest
/// dimension value) has thrown the geometry to a far solution.
const FLUNG: f64 = 1000.0;

/// Why a solve from `before` into `after` collapses the sketch, in the words `settle` uses to
/// refuse an edit's solve: a line or circle shrunk to a point, or points thrown far away.
/// `None` when the solved shape is one the edit can mean. Value edits (`Session::settle_value_edit`)
/// run the same checks, so a parameter, a configuration row or the dimension editor cannot
/// store a sketch whose curve has been squeezed away and which can never be solved back.
pub(crate) fn collapse_reason(before: &Sketch, after: &Sketch) -> Option<String> {
    if let Some(curve) = shrunk(before, after) {
        return Some(format!("that would shrink {curve} to a point: its constraints can only be met without it"));
    }
    if flung(before, after) {
        return Some("that would throw points of the sketch far away: its constraints have no solution near the sketch as it is".into());
    }
    None
}

/// Did solving `before` into `after` throw some point further than [`FLUNG`] times the sketch's
/// size, or to no number at all?
fn flung(before: &Sketch, after: &Sketch) -> bool {
    let (lo, hi) = before
        .points
        .iter()
        .map(|p| p.pos)
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .fold((Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)), |(lo, hi), p| {
            (Vec2::new(lo.x.min(p.x), lo.y.min(p.y)), Vec2::new(hi.x.max(p.x), hi.y.max(p.y)))
        });
    let size = if lo.x <= hi.x { lo.dist(hi) } else { 0.0 };
    let longest = after.driving_values().values().map(|v| v.abs()).filter(|v| v.is_finite()).fold(0.0, f64::max);
    let reach = FLUNG * (size + longest).max(1.0);
    after.points.iter().zip(&before.points).any(|(a, b)| !(a.pos.dist(b.pos) <= reach))
}

/// The sketch solves of `edit` and drags, counted in tests (see [`solve_with`]).
pub(super) fn counted_solve(
    sk: &mut Sketch,
    hold: &Hold,
    reference: Option<&[Vec2]>,
    analysis: Analysis,
    budget: Option<&mut Budget>,
) -> SolveReport {
    #[cfg(test)]
    solve_counts::record(analysis);
    solve_with(sk, hold, reference, analysis, budget)
}

/// Inside an `edit` closure: solve the sketch for the document's parameter values from its
/// stored solved state, as the edit's own solve does, for commands that work on the current
/// geometry.
pub(super) fn solve_current(sk: &mut Sketch, doc: &solidcraft_doc::Document) -> Result<SolveReport> {
    let stored = sk.driving_values();
    let (values, _) = doc.param_values();
    Ok(doc.solve_sketch_with_analysis(&values, sk, &stored, None, Analysis::Freedom)?)
}

/// Solves run on this thread by [`counted_solve`] and the edit's own solve: (all of them, those
/// with a rank analysis).
#[cfg(test)]
pub(super) mod solve_counts {
    use std::cell::Cell;

    use solidcraft_sketch::Analysis;

    thread_local! {
        static COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
    }

    pub fn record(analysis: Analysis) {
        COUNTS.with(|c| {
            let (n, full) = c.get();
            c.set((n + 1, full + usize::from(analysis != Analysis::StatusOnly)));
        });
    }

    /// The counts since the last call.
    pub fn take() -> (usize, usize) {
        COUNTS.with(|c| c.replace((0, 0)))
    }
}

/// How close (mm) a new point must land to an existing one to be connected to it.
const CONNECT_TOL: f64 = 1e-6;

/// New points (index `first` on) that land on an existing point, the origin included, get a
/// coincident constraint to it, as when a click snaps there; `connect: false` leaves them free.
fn connect_points(sk: &mut Sketch, first: usize, p: &Value) -> Result<Vec<String>> {
    if !bool_(p, "connect").unwrap_or(true) {
        return Ok(Vec::new());
    }
    // The existing points by x, for a range lookup per new point.
    let mut old: Vec<(Vec2, usize)> = (0..first.min(sk.points.len())).filter_map(|i| Some((sk.point(i)?, i))).collect();
    old.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    let mut ties = Vec::new();
    for q in first..sk.points.len() {
        let Some(at) = sk.point(q) else { continue };
        let from = old.partition_point(|(o, _)| o.x < at.x - CONNECT_TOL);
        let hit =
            old.iter().skip(from).take_while(|(o, _)| o.x <= at.x + CONNECT_TOL).filter(|(o, _)| o.dist(at) < CONNECT_TOL).map(|(_, i)| *i).min();
        if let Some(o) = hit {
            ties.push((o, q));
        }
    }
    ties.into_iter().map(|(o, q)| add_c(sk, ConstraintKind::Coincident { p: o, q })).collect()
}

/// `edit` for drawing commands whose step returns (curve ids, constraint ids): the points it
/// adds are connected to the existing points they land on (see [`connect_points`]).
pub(super) fn draw(
    s: &mut Session,
    p: &Value,
    cmd: &str,
    f: impl FnOnce(&mut Sketch, &mut solidcraft_doc::Document) -> Result<(Vec<String>, Vec<String>)>,
) -> Result<((Vec<String>, Vec<String>), Value)> {
    edit(s, p, cmd, false, |sk, doc| {
        let first = sk.points.len();
        let (curves, mut cons) = f(sk, doc)?;
        cons.extend(connect_points(sk, first, p)?);
        drop_implied_axes(sk, &mut cons);
        Ok((curves, cons))
    })
}

/// A point argument: `[x, y]` (new point), a point reference string, or `{at, ref}`.
#[derive(Clone)]
pub(super) enum PArg {
    At(Vec2),
    Ref(usize, Vec2),
}

impl PArg {
    pub(super) fn pos(&self) -> Vec2 {
        match self {
            PArg::At(p) | PArg::Ref(_, p) => *p,
        }
    }
    pub(super) fn idx(&self) -> Option<usize> {
        match self {
            PArg::Ref(i, _) => Some(*i),
            PArg::At(_) => None,
        }
    }
}

pub(super) fn parg(sk: &mut Sketch, doc: &solidcraft_doc::Document, v: &Value, cmd: &str) -> Result<PArg> {
    if let Some(p) = vec2(v) {
        return Ok(PArg::At(p));
    }
    if let Some((at, exprs)) = expr_point(doc, v, cmd)? {
        return Ok(match exprs {
            Some(exprs) => PArg::Ref(add_placed_point(sk, at, exprs, None)?, at),
            None => PArg::At(at),
        });
    }
    let r = match v {
        Value::String(r) => Some(r.as_str()),
        Value::Object(o) => o.get("ref").and_then(Value::as_str),
        _ => None,
    };
    if let Some(r) = r {
        let i = sk.resolve_point(r).ok_or_else(|| bad(cmd, format!("unknown sketch point `{r}`")))?;
        return Ok(PArg::Ref(i, sk.point(i).unwrap_or_default()));
    }
    if let Some(at) = v.get("at").and_then(vec2) {
        return Ok(PArg::At(at));
    }
    Err(bad(cmd, "a point must be [x, y], a point id like \"l1.end\", or {at, ref}"))
}

pub(super) fn req_parg(sk: &mut Sketch, doc: &solidcraft_doc::Document, p: &Value, k: &str, cmd: &str) -> Result<PArg> {
    parg(sk, doc, p.get(k).ok_or_else(|| bad(cmd, format!("missing `{k}`")))?, cmd)
}

const POINT_MUST: &str = "a point's coordinates must be numbers or expressions over the design's parameters";

/// A point given as `[x, y]` with a coordinate typed as text: where it is now and, when a
/// coordinate names a parameter, the expressions that place it (none: a plain point).
pub(super) fn expr_point(doc: &solidcraft_doc::Document, v: &Value, cmd: &str) -> Result<Option<(Vec2, Option<PointExprs>)>> {
    let Some([x, y]) = v.as_array().map(Vec::as_slice) else { return Ok(None) };
    if !x.is_string() && !y.is_string() {
        return Ok(None);
    }
    let (px, ex) = super::sketch_create::text_number(doc, x, cmd, "point x", POINT_MUST, false)?;
    let (py, ey) = super::sketch_create::text_number(doc, y, cmd, "point y", POINT_MUST, false)?;
    let exprs = (ex.is_some() || ey.is_some()).then_some(PointExprs { x: ex, y: ey });
    Ok(Some((Vec2::new(px, py), exprs)))
}

/// A new point placed by parameters: fixed where its expressions come to, and moved there
/// again whenever the parameters change.
pub(super) fn add_placed_point(sk: &mut Sketch, at: Vec2, exprs: PointExprs, id: Option<&str>) -> Result<usize> {
    let i = sk.add_point(at, id)?;
    if let Some(q) = sk.points.get_mut(i) {
        q.fixed = true;
        q.exprs = Some(Box::new(exprs));
    }
    Ok(i)
}

pub(super) fn curve_ref(sk: &Sketch, v: Option<&Value>, cmd: &str, what: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{what}` must be a curve id")))?;
    sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown sketch curve `{id}`")))
}

/// A line a constraint refers to: a curve id, or `x` / `y` for the sketch's own axes (a curve
/// with that id wins). An axis becomes the sketch's linked construction line on it
/// ([`Sketch::sketch_axis_line`]), added on first use and reported in `created`.
pub(super) fn line_or_axis_ref(sk: &mut Sketch, v: Option<&Value>, cmd: &str, what: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{what}` must be a curve id, or x / y for a sketch axis")))?;
    curve_or_axis(sk, id)?.ok_or_else(|| bad(cmd, format!("unknown sketch curve `{id}` (x and y name the sketch's own axes)")))
}

/// Curve `id`, or the line of sketch axis `id` (see [`line_or_axis_ref`]); `None` when neither.
pub(super) fn curve_or_axis(sk: &mut Sketch, id: &str) -> Result<Option<usize>> {
    match sk.curve_index(id) {
        Some(c) => Ok(Some(c)),
        None => Ok(sk.sketch_axis_line(id)?),
    }
}

pub(super) fn point_ref(sk: &Sketch, v: Option<&Value>, cmd: &str, what: &str) -> Result<usize> {
    let id = v.and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{what}` must be a point reference")))?;
    sk.resolve_point(id).ok_or_else(|| bad(cmd, format!("unknown sketch point `{id}`")))
}

pub(super) fn ids_of(sk: &Sketch, curves: &[usize]) -> Vec<String> {
    curves.iter().filter_map(|c| sk.curves.get(*c).map(|c| c.id.clone())).collect()
}

pub(super) fn add_c(sk: &mut Sketch, k: ConstraintKind) -> Result<String> {
    Ok(sk.add_constraint(k, None)?)
}

pub(super) fn mark_construction(sk: &mut Sketch, curves: &[usize], p: &Value) {
    if bool_(p, "construction").unwrap_or(false) {
        for c in curves {
            if let Some(cu) = sk.curves.get_mut(*c) {
                cu.construction = true;
            }
        }
    }
}

/// What a command added to a sketch: the ids of the points, curves, constraints and dimensions
/// in `after` that `before` does not have (`None`: a new sketch), in creation order, plus the
/// 3D curves (`wires`) when there are any. Ids are stable and never reused in a sketch, so they
/// can be used in later commands (`sketch.dimension`, `sketch.constraint.*`, `sketch.delete`…).
pub(crate) fn created(before: Option<&Sketch>, after: &Sketch) -> Value {
    use std::collections::HashSet;
    let empty = Sketch::new();
    let before = before.unwrap_or(&empty);
    let old: HashSet<&str> = before
        .points
        .iter()
        .map(|x| x.id.as_str())
        .chain(before.curves.iter().map(|x| x.id.as_str()))
        .chain(before.constraints.iter().map(|x| x.id.as_str()))
        .chain(before.wires.iter().map(|x| x.id.as_str()))
        .collect();
    let new = |id: &&str| !old.contains(*id);
    let points: Vec<&str> = after.points.iter().map(|x| x.id.as_str()).filter(new).collect();
    let curves: Vec<&str> = after.curves.iter().map(|x| x.id.as_str()).filter(new).collect();
    let (dims, cons): (Vec<_>, Vec<_>) = after.constraints.iter().filter(|c| new(&c.id.as_str())).partition(|c| c.kind.is_dimension());
    let ids = |v: Vec<&solidcraft_sketch::Constraint>| v.into_iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    let mut out = json!({"points": points, "curves": curves, "constraints": ids(cons), "dimensions": ids(dims)});
    let wires: Vec<&str> = after.wires.iter().map(|x| x.id.as_str()).filter(new).collect();
    if !wires.is_empty()
        && let Some(o) = out.as_object_mut()
    {
        o.insert("wires".into(), json!(wires));
    }
    out
}

pub(super) fn result(sk_out: (Vec<String>, Vec<String>), info: Value) -> Value {
    json!({"curves": sk_out.0, "constraints": sk_out.1, "sketch": info})
}

// ---------------------------------------------------------------------------------------------
// Sketch lifecycle

pub(crate) fn author_path_for_rebind(s: &Session, feature: &solidcraft_doc::Feature, p: &Value, cmd: &str) -> Result<Vec<u64>> {
    let mut saved = None;
    if let FeatureKind::Sketch { plane, .. } = &feature.kind {
        let mut plane = plane;
        loop {
            match plane {
                PlaneRef::Offset { base, .. }
                | PlaneRef::ThroughPoint { base, .. }
                | PlaneRef::AtAngle { base, .. }
                | PlaneRef::Flipped { base, .. } => plane = base,
                PlaneRef::Face { anchor: Some(anchor), .. } => {
                    saved = Some(anchor.author_path.clone());
                    break;
                }
                _ => break,
            }
        }
    }
    let path = if let Some(value) = p.get("author_path") {
        value
            .as_array()
            .filter(|path| path.len() <= 64)
            .ok_or_else(|| bad(cmd, "author_path must contain at most64 ids"))?
            .iter()
            .map(|id| id.as_u64().filter(|id| *id != 0).ok_or_else(|| bad(cmd, "invalid author_path id")))
            .collect::<Result<Vec<_>>>()?
    } else if let Some(path) = saved {
        path
    } else if feature.component == s.active_component && s.active_occurrence_path.is_some() {
        s.active_occurrence_path.clone().unwrap_or_default()
    } else {
        let placements = super::component::properties::placements(&s.placed_document(), cmd)?;
        let paths: Vec<_> = placements.into_iter().filter(|(component, _, _)| *component == feature.component).map(|(_, path, _)| path).collect();
        if paths.len() != 1 {
            return Err(bad(cmd, "sketch author placement is ambiguous; provide exact author_path or activate the intended occurrence"));
        }
        paths.into_iter().next().ok_or_else(|| bad(cmd, "sketch author placement is missing"))?
    };
    if feature.component != 0 && path.is_empty() {
        return Err(bad(cmd, "a component sketch needs an explicit complete author_path"));
    }
    solidcraft_doc::EmbossFace::component_frame(&s.placed_document(), feature.component, &path)?;
    Ok(path)
}

pub(super) fn plane_ref(s: &Session, p: &Value, cmd: &str) -> Result<PlaneRef> {
    let base = match p.get("plane") {
        None => PlaneRef::Origin { name: "XY".into() },
        // An origin plane, or a construction plane by name or id.
        Some(v @ (Value::String(_) | Value::Number(_))) => super::features::plane_param(s, Some(v), cmd)?,
        Some(v @ Value::Object(o)) => {
            if o.contains_key("face") {
                face::capture(s, v, cmd)?
            } else {
                // A missing origin is the world origin; one that is given but is not three numbers
                // (an expression, a number with a unit) is an error, not a silent [0, 0, 0].
                let origin = match v.get("origin") {
                    None | Some(Value::Null) => Vec3::ZERO,
                    Some(o) => vec3(o)
                        .ok_or_else(|| bad(cmd, "plane `origin` must be three numbers [x, y, z] in mm (expressions are not evaluated here)"))?,
                };
                let x = v.get("x_dir").and_then(vec3).ok_or_else(|| bad(cmd, "plane needs x_dir"))?;
                let y = v.get("y_dir").and_then(vec3).ok_or_else(|| bad(cmd, "plane needs y_dir"))?;
                PlaneRef::Custom { plane: Plane::new(origin, x, y).ok_or_else(|| bad(cmd, "degenerate plane"))? }
            }
        }
        _ => return Err(bad(cmd, "`plane` must be XY, XZ, YZ, a construction plane name or id, or an object")),
    };
    Ok(match expr(p, "offset") {
        Some(d) => PlaneRef::Offset { base: Box::new(base), distance: d },
        None => base,
    })
}

/// `frame` of `sketch.create` and `sketch.redefine` (#6000): which coordinates an explicit
/// `{origin, x_dir, y_dir}` plane is in, `world` (the default) or the component's own. `None`
/// for a named plane or a face, which are where they are.
pub(super) fn plane_frame(p: &Value, cmd: &str) -> Result<Option<&'static str>> {
    let explicit = p.get("plane").is_some_and(Value::is_object) && !face::is_face(p);
    let frame = match p.get("frame") {
        None | Some(Value::Null) => None,
        Some(Value::String(f)) if f == "world" => Some("world"),
        Some(Value::String(f)) if f == "component" => Some("component"),
        Some(_) => return Err(bad(cmd, "`frame` must be \"world\" or \"component\"")),
    };
    match (explicit, frame) {
        (true, f) => Ok(Some(f.unwrap_or("world"))),
        (false, None) => Ok(None),
        (false, Some(_)) => {
            Err(bad(cmd, "`frame` is for an explicit plane {origin, x_dir, y_dir}: a named plane is the component's own, and a face is where it is"))
        }
    }
}

/// The reply's `frame` for an explicit plane, with a note when a world plane went into a moved
/// or turned component (`placement`: that component's), where it differs from the component's own.
pub(super) fn frame_reply(out: &mut Value, frame: Option<&str>, placement: &solidcraft_doc::Mat) {
    let Some(o) = out.as_object_mut() else { return };
    let Some(frame) = frame else { return };
    o.insert("frame".into(), json!(frame));
    if frame == "world" && !solidcraft_doc::is_identity(placement) {
        o.insert(
            "note".into(),
            json!("the plane was read in world coordinates and the component is moved or turned, so it is stored at that world place in the component; frame: \"component\" reads it in the component's own coordinates"),
        );
    }
}

/// The indices of `m`'s triangles by face, for `faces` faces (triangles of a face past that
/// are left out): one pass, so testing every face costs the triangle count once, not per face.
pub(super) fn face_triangles(m: &solidcraft_geom::Mesh, faces: usize) -> Vec<Vec<usize>> {
    let mut by_face = vec![Vec::new(); faces];
    for (i, f) in m.tri_face.iter().enumerate().take(m.triangles.len()) {
        if let Some(tris) = by_face.get_mut(*f as usize) {
            tris.push(i);
        }
    }
    by_face
}

/// Does a face, the triangles `tris` of `m` (from [`face_triangles`]), contain point `p` (within
/// `tol` of the face plane)?
pub(super) fn face_contains(m: &solidcraft_geom::Mesh, tris: &[usize], p: Vec3, tol: f64) -> bool {
    tris.iter().filter_map(|i| m.triangles.get(*i)).any(|t| {
        let Some([a, b, c]) = m.tri(t) else { return false };
        let n = (b - a).cross(c - a);
        let Some(nn) = n.normalized() else { return false };
        if (p - a).dot(nn).abs() > tol {
            return false;
        }
        let s1 = (b - a).cross(p - a).dot(n);
        let s2 = (c - b).cross(p - b).dot(n);
        let s3 = (a - c).cross(p - c).dot(n);
        (s1 >= 0.0 && s2 >= 0.0 && s3 >= 0.0)
            || (s1 <= 0.0 && s2 <= 0.0 && s3 <= 0.0)
            // Exact curved boundaries can sit just outside their tessellated chords.
            || p.dist_to_segment(a, b).min(p.dist_to_segment(b, c)).min(p.dist_to_segment(c, a)) <= tol * 0.1
    })
}

fn create_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.create";
    // These name the plane, so they belong inside `plane`. Next to it they were ignored and the
    // sketch went to XY (or the plane given) without a word.
    for (key, example) in [
        ("face", r#"{"plane": {"face": [x, y, z]}}"#),
        ("origin", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
        ("x_dir", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
        ("y_dir", r#"{"plane": {"origin": [x, y, z], "x_dir": [..], "y_dir": [..]}}"#),
    ] {
        if p.get(key).is_some() {
            return Err(bad(cmd, format!("`{key}` belongs inside `plane`: {example}")));
        }
    }
    let frame = plane_frame(p, cmd)?;
    let mut plane = plane_ref(s, p, cmd)?;
    if let Some(about_y) = flip_param(s, p, &plane, cmd)? {
        plane = PlaneRef::Flipped { base: Box::new(plane), about_y };
    }
    let mut out = create_sketch_with_plane(s, p, plane, None)?;
    frame_reply(&mut out, frame, &s.active_component_frame());
    Ok(out)
}

/// `flip` of `sketch.create`: `None` keeps the plane's own side, otherwise the axis to turn
/// about (`true`: about y when the plane's y axis points upward, so it stays up, else about x).
fn flip_param(s: &Session, p: &Value, plane: &PlaneRef, cmd: &str) -> Result<Option<bool>> {
    let about_y = match p.get("flip") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => return Ok(None),
        Some(Value::String(a)) if a.eq_ignore_ascii_case("x") => false,
        Some(Value::String(a)) if a.eq_ignore_ascii_case("y") => true,
        Some(Value::Bool(true)) => {
            let (vals, _) = s.doc.param_values();
            let resolved = s.doc.resolve_plane_in(&vals, plane, 0, Some(&s.model.state()))?;
            resolved.y.z > 1e-9
        }
        Some(_) => return Err(bad(cmd, "`flip` must be true, false, \"x\" or \"y\"")),
    };
    if face::is_face(p) {
        return Err(bad(cmd, "`flip` is for origin, construction and explicit planes; a face already faces outward"));
    }
    Ok(Some(about_y))
}

/// Used only by face Extrude/Revolve/PressPull, preserving pre-certificate linked profiles.
pub(super) fn create_face_profile(s: &mut Session, p: &Value, body: &str, index: usize) -> Result<Value> {
    let value = p.get("plane").ok_or_else(|| bad("face profile", "missing picked face plane"))?;
    let plane = face::capture_internal(s, value, "face profile")?;
    create_sketch_with_plane(s, p, plane, Some((body, index)))
}

pub(super) fn create_sketch_with_plane(s: &mut Session, p: &Value, plane: PlaneRef, internal_body: Option<(&str, usize)>) -> Result<Value> {
    let before = entry_point(s);
    if face::is_face(p) {
        face::capture_positions(s, &plane)?;
    }
    // Validate the plane resolves.
    let (vals, _) = s.doc.param_values();
    s.doc.resolve_plane_in(&vals, &plane, 0, Some(&s.model.state()))?;
    let name = str_(p, "name");
    let mut kind = FeatureKind::Sketch { plane, sketch: Sketch::new() };
    if !face::is_face(p) {
        super::component::to_active_frame(s, &mut kind);
    }
    let id = s.doc_mut().add_feature(kind, name)?;
    let comp = s.active_component;
    if let Some(f) = s.doc_mut().feature_mut(id) {
        f.component = comp;
    }
    s.active_sketch = Some(id);
    s.sketch_entry = Some(SketchEntry { sketch: id, ..before });
    let name = s.doc.feature(id).map(|f| f.name.clone()).unwrap_or_default();
    // Sketching on a face projects the face's edges (unless turned off).
    let face = p.get("plane").and_then(|v| v.get("face")).and_then(vec3);
    let mut link = None;
    if let Some(at) = face
        && bool_(p, "project_edges").unwrap_or(s.auto_project)
    {
        link = super::sketch_project::auto_project_face(s, id, at)?;
        if link.is_none()
            && let Some((body, index)) = internal_body
        {
            link = Some(super::sketch_project::project_internal_face(s, id, body, index)?);
        }
    }
    let attachment = s.doc.feature(id).and_then(|feature| match &feature.kind {
        FeatureKind::Sketch { plane, .. } => Some(if face::anchor(plane).is_some() {
            "exact_face"
        } else if solidcraft_doc::sheet::plane_sheet_face(plane).is_some() {
            "sheet_face"
        } else if internal_body.is_some() {
            "internal_legacy_face"
        } else {
            "fixed"
        }),
        _ => None,
    });
    Ok(json!({"sketch": id, "name": name, "projected": link,"attachment":attachment}))
}

fn edit_sketch(s: &mut Session, p: &Value) -> Result<Value> {
    let key = match p.get("sketch") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(x)) => x.clone(),
        _ => return Err(bad("sketch.edit", "`sketch` must be an id or a name")),
    };
    let f = s.doc.find_feature(&key).ok_or_else(|| bad("sketch.edit", format!("no sketch `{key}`")))?;
    if !matches!(f.kind, FeatureKind::Sketch { .. }) {
        return Err(bad("sketch.edit", format!("`{key}` is not a sketch")));
    }
    let (id, name, component) = (f.id, f.name.clone(), f.component);
    let authored_path = match &f.kind {
        FeatureKind::Sketch { plane, .. } => face::anchor(plane).map(|anchor| anchor.author_path.clone()),
        _ => None,
    };
    if let Some(path) = authored_path {
        solidcraft_doc::EmbossFace::component_frame(&s.placed_document(), component, &path)
            .map_err(|error| bad("sketch.edit", format!("{error}; explicitly rebind the author placement before editing")))?;
        s.active_component = component;
        s.active_occurrence_path = (!path.is_empty()).then_some(path);
    }
    if s.active_sketch != Some(id) || s.sketch_entry.as_ref().is_none_or(|entry| entry.sketch != id) {
        s.sketch_entry = Some(SketchEntry { sketch: id, ..entry_point(s) });
    }
    s.active_sketch = Some(id);
    s.revision += 1;
    Ok(json!({"sketch": id, "name": name}))
}

/// The design and undo history as they are now, where a sketch is entered (its id set after).
fn entry_point(s: &Session) -> SketchEntry {
    SketchEntry { sketch: 0, doc: s.doc.clone(), undo_len: s.undo.len(), undo_top: s.undo.last().map(|snap| snap.doc.clone()) }
}

/// Cancel Sketch (#3838): the design as it was when the open sketch was entered. The entry is
/// used only while the undo history still runs through it: the same steps before it, and every
/// step since taken in this sketch. Otherwise the steps since can't be told apart from others,
/// and Undo is the way back.
fn cancel_sketch(s: &mut Session, _p: &Value) -> Result<Value> {
    let cmd = "sketch.cancel";
    let id = sketch_store::active(s).ok_or_else(|| bad(cmd, "no sketch is being edited"))?;
    let entry = s.sketch_entry.clone().filter(|entry| entry.sketch == id);
    let current = entry.filter(|entry| {
        let top = entry.undo_len.checked_sub(1).and_then(|i| s.undo.get(i)).map(|snap| &snap.doc);
        let same_before = s.undo.len() >= entry.undo_len
            && match (top, &entry.undo_top) {
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
        // The step that created the sketch was taken before it was open: its state is the entry's.
        let since = s.undo.get(entry.undo_len..).unwrap_or_default();
        same_before
            && since.iter().enumerate().all(|(i, snap)| snap.active_sketch == Some(id) || (i == 0 && std::sync::Arc::ptr_eq(&snap.doc, &entry.doc)))
    });
    let Some(entry) = current else {
        return Err(bad(cmd, "the undo history no longer leads back to where this sketch was opened: use Undo to go back"));
    };
    let created = entry.doc.feature(id).is_none();
    // As Undo does: the session evaluates the design it is given.
    s.doc = entry.doc;
    s.active_sketch = None;
    s.sketch_entry = None;
    s.selection.clear();
    s.revision += 1;
    Ok(json!({"sketch": id, "removed": created}))
}

fn finish_sketch(s: &mut Session, _p: &Value) -> Result<Value> {
    let id = s.active_sketch.take();
    s.sketch_entry = None;
    s.revision += 1;
    let profiles = id.and_then(|i| s.model.state().sketch(i).map(|ss| ss.profiles.len())).unwrap_or(0);
    Ok(json!({"sketch": id, "profiles": profiles}))
}

// ---------------------------------------------------------------------------------------------
// Drawing

/// The id a drawing command gives its geometry, checked against the names that geometry
/// will carry: a curve's id goes into the name of the faces the curve makes
/// (`F2:side:<id>`), and a name reads some characters as its own. An id holding one of
/// them would split every name it lands in, so it is refused here (#5858).
fn named_id(cmd: &str, id: Option<&str>) -> Result<Option<String>> {
    match id {
        Some(i) if !solidcraft_doc::persistent_name::text_fits_name(i) => {
            Err(bad(cmd, format!("the id `{i}` cannot be written into a name whole; give an id without `|`, `,`, `~`, `#` or `]`")))
        }
        Some(i) => Ok(Some(i.to_string())),
        None => Ok(None),
    }
}

fn draw_line(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.line";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let list = p.get("points").and_then(Value::as_array).cloned().ok_or_else(|| bad(cmd, "`points` must be a list"))?;
    if list.len() < 2 || list.len() > 10_000 {
        return Err(bad(cmd, "a line needs 2 or more points"));
    }
    let closed = bool_(p, "closed").unwrap_or(false);
    let infer = bool_(p, "infer").unwrap_or(false);
    let ids = string_list(cmd, p, "ids")?;
    for id in &ids {
        named_id(cmd, Some(id))?;
    }
    let ((curves, cons), info) = draw(s, p, cmd, |sk, doc| {
        let args: Vec<PArg> = list.iter().map(|v| parg(sk, doc, v, cmd)).collect::<Result<_>>()?;
        let n = args.len();
        let segs = if closed { n } else { n - 1 };
        let mut pts: Vec<Option<usize>> = args.iter().map(PArg::idx).collect();
        let mut curves = Vec::new();
        let mut cons = Vec::new();
        for i in 0..segs {
            let j = (i + 1) % n;
            let (Some(a), Some(b)) = (args.get(i), args.get(j)) else { continue };
            if a.pos().dist(b.pos()) < 1e-9 {
                return Err(bad(cmd, "zero-length line"));
            }
            let id = ids.get(i).map(String::as_str);
            let c = sk.add_line(a.pos(), b.pos(), pts.get(i).copied().flatten(), pts.get(j).copied().flatten(), id)?;
            if let Some(CurveKind::Line { a: pa, b: pb }) = sk.curves.get(c).map(|c| c.kind.clone()) {
                if let Some(slot) = pts.get_mut(i) {
                    *slot = Some(pa);
                }
                if let Some(slot) = pts.get_mut(j) {
                    *slot = Some(pb);
                }
            }
            if infer {
                let d = b.pos() - a.pos();
                let axis = if d.y.abs() <= d.len() * 1e-9 {
                    cons.push(add_c(sk, ConstraintKind::Horizontal { l: c })?);
                    true
                } else if d.x.abs() <= d.len() * 1e-9 {
                    cons.push(add_c(sk, ConstraintKind::Vertical { l: c })?);
                    true
                } else {
                    false
                };
                if let Some(k) = infer_at_start(sk, c, axis) {
                    cons.push(add_c(sk, k)?);
                }
            }
            curves.push(c);
        }
        mark_construction(sk, &curves, p);
        Ok((ids_of(sk, &curves), cons))
    })?;
    Ok(result((curves, cons), info))
}

/// A relation the new line `c` has with the curve it starts from (drawn exactly so): tangent
/// to an arc, or perpendicular to a line (parallel continuations are left alone). `axis`: the
/// line already got horizontal/vertical, so a perpendicular to an axis-aligned line would be
/// redundant.
fn infer_at_start(sk: &Sketch, c: usize, axis: bool) -> Option<ConstraintKind> {
    let CurveKind::Line { a, b } = sk.curves.get(c)?.kind else { return None };
    let (pa, pb) = (sk.point(a)?, sk.point(b)?);
    let u = (pb - pa).normalized()?;
    for (i, o) in sk.curves.iter().enumerate() {
        if i == c || !o.kind.uses(a) {
            continue;
        }
        match o.kind {
            CurveKind::Arc { c: cc, .. } => {
                let r = (pa - sk.point(cc)?).normalized()?;
                if r.dot(u).abs() < 1e-9 {
                    return Some(ConstraintKind::Tangent { a: i, b: c });
                }
            }
            CurveKind::Line { a: oa, b: ob } => {
                let v = (sk.point(ob)? - sk.point(oa)?).normalized()?;
                let o_axis = v.x.abs() < 1e-9 || v.y.abs() < 1e-9;
                if u.dot(v).abs() < 1e-9 && !(axis && o_axis) {
                    return Some(ConstraintKind::Perpendicular { a: i, b: c });
                }
            }
            _ => {}
        }
    }
    None
}

/// Closed polygon of lines through new or referenced corner points.
fn polygon_lines(sk: &mut Sketch, corners: &[Vec2]) -> Result<(Vec<usize>, Vec<usize>)> {
    let pts: Vec<usize> = corners.iter().map(|c| sk.add_point(*c, None)).collect::<std::result::Result<_, _>>()?;
    let n = pts.len();
    let mut lines = Vec::new();
    for i in 0..n {
        let (Some(a), Some(b)) = (pts.get(i), pts.get((i + 1) % n)) else { continue };
        lines.push(sk.add_line_pts(*a, *b, None)?);
    }
    Ok((lines, pts))
}

fn rect_constraints(sk: &mut Sketch, l: &[usize]) -> Result<Vec<String>> {
    let mut cons = Vec::new();
    if let [a, b, c, d] = l[..] {
        cons.push(add_c(sk, ConstraintKind::Horizontal { l: a })?);
        cons.push(add_c(sk, ConstraintKind::Horizontal { l: c })?);
        cons.push(add_c(sk, ConstraintKind::Vertical { l: b })?);
        cons.push(add_c(sk, ConstraintKind::Vertical { l: d })?);
    }
    Ok(cons)
}

/// Whether line `l` being horizontal (`horizontal`) or vertical already follows from another
/// constraint than `own`: its ends coincide with the ends of a line constrained the same way (a
/// side two rectangles share), or with two points constrained level (or plumb).
fn axis_implied(sk: &Sketch, l: usize, horizontal: bool, own: &str) -> bool {
    let Some(CurveKind::Line { a, b }) = sk.curves.get(l).map(|c| &c.kind) else { return false };
    let (a, b) = (*a, *b);
    // Coincident points as one (union-find over the coincident constraints).
    let mut parent: Vec<usize> = (0..sk.points.len()).collect();
    let root = |parent: &[usize], mut i: usize| {
        for _ in 0..parent.len() {
            match parent.get(i) {
                Some(&up) if up != i => i = up,
                _ => break,
            }
        }
        i
    };
    for c in &sk.constraints {
        if let ConstraintKind::Coincident { p, q } = c.kind {
            let (rp, rq) = (root(&parent, p), root(&parent, q));
            if rp != rq
                && let Some(slot) = parent.get_mut(rp)
            {
                *slot = rq;
            }
        }
    }
    let (ra, rb) = (root(&parent, a), root(&parent, b));
    sk.constraints.iter().filter(|c| c.id != own).any(|c| {
        let ends = match (&c.kind, horizontal) {
            (ConstraintKind::Horizontal { l: m }, true) | (ConstraintKind::Vertical { l: m }, false) if *m != l => {
                match sk.curves.get(*m).map(|c| &c.kind) {
                    Some(CurveKind::Line { a, b }) => Some((*a, *b)),
                    _ => None,
                }
            }
            (ConstraintKind::HorizontalPoints { p, q }, true) | (ConstraintKind::VerticalPoints { p, q }, false) => Some((*p, *q)),
            _ => None,
        };
        ends.is_some_and(|(p, q)| {
            let (rp, rq) = (root(&parent, p), root(&parent, q));
            rp != rq && ((rp, rq) == (ra, rb) || (rp, rq) == (rb, ra))
        })
    })
}

/// Drawing adds horizontal/vertical only where it is not already implied: a rectangle drawn
/// onto another's side shares that side's direction through the two coincident corners, so a
/// second "vertical" there would only be redundant. Drops such constraints among `cons` (the
/// drawing's own) from the sketch and the list.
fn drop_implied_axes(sk: &mut Sketch, cons: &mut Vec<String>) {
    let mut dropped = Vec::new();
    for id in cons.iter() {
        let axis = sk.constraints.iter().find(|c| &c.id == id).and_then(|c| match c.kind {
            ConstraintKind::Horizontal { l } => Some((l, true)),
            ConstraintKind::Vertical { l } => Some((l, false)),
            _ => None,
        });
        if let Some((l, horizontal)) = axis
            && axis_implied(sk, l, horizontal, id)
        {
            sk.constraints.retain(|c| &c.id != id);
            dropped.push(id.clone());
        }
    }
    cons.retain(|id| !dropped.contains(id));
}

/// Which side and corner of an axis-aligned rectangle is which, from its lines and corner
/// points as `polygon_lines` made them (counter-clockwise from the bottom-left corner), so a
/// caller can dimension it without inspecting the sketch first.
fn rect_names(sk: &Sketch, l: &[usize], pts: &[usize]) -> Value {
    let curve = |i: usize| l.get(i).and_then(|c| sk.curves.get(*c)).map(|c| c.id.clone());
    let point = |i: usize| pts.get(i).and_then(|q| sk.points.get(*q)).map(|q| q.id.clone());
    json!({
        "sides": {"bottom": curve(0), "right": curve(1), "top": curve(2), "left": curve(3)},
        "corners": {"bottom_left": point(0), "bottom_right": point(1), "top_right": point(2), "top_left": point(3)},
    })
}

/// `result` plus the rectangle's named sides and corners.
fn rect_result(sk_out: (Vec<String>, Vec<String>), names: Value, info: Value) -> Value {
    let mut out = result(sk_out, info);
    if let (Some(o), Value::Object(n)) = (out.as_object_mut(), names) {
        o.extend(n);
    }
    out
}

fn rect_two_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.rectangle.two_point";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    if (a.x - b.x).abs() < 1e-9 || (a.y - b.y).abs() < 1e-9 {
        return Err(bad(cmd, "the corners must differ in x and y"));
    }
    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    let ((out, names), info) = edit(s, p, cmd, false, |sk, _| {
        let first = sk.points.len();
        let (l, pts) = polygon_lines(sk, &[lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)])?;
        let mut cons = rect_constraints(sk, &l)?;
        mark_construction(sk, &l, p);
        cons.extend(connect_points(sk, first, p)?);
        drop_implied_axes(sk, &mut cons);
        Ok(((ids_of(sk, &l), cons), rect_names(sk, &l, &pts)))
    })?;
    Ok(rect_result(out, names, info))
}

/// A 2-point rectangle with its corners filleted, as one undo step: the rectangle and the
/// fillet run in a scratch session that replaces the design only when both succeed.
fn rect_rounded(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.rectangle.rounded";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let r_expr = req_expr(cmd, p, "radius")?;
    let r = s.doc.eval(&r_expr, Kind::Length).map_err(|e| bad(cmd, format!("radius: {e}")))?;
    if !(r > 1e-9 && r < 1e8) {
        return Err(bad(cmd, "radius must be positive"));
    }
    let half = 0.5 * (a.x - b.x).abs().min((a.y - b.y).abs());
    if half > 1e-9 && r >= half - 1e-9 {
        return Err(bad(cmd, format!("the radius must be less than half the shorter side ({half:.3} mm)")));
    }
    let mut rect = p.clone();
    if let Some(o) = rect.as_object_mut() {
        o.remove("radius");
    }
    let mut scratch = s.scratch();
    let mut out = scratch.execute("sketch.rectangle.two_point", &rect)?;
    // The fillet rounds each corner by the two sides meeting there, not by the corner point: a
    // corner that lands on another curve's end is shared with it, and rounding the point would
    // weld the two together, leaving the corner on three curves (#7878).
    let sides: Vec<String> =
        ["bottom", "right", "top", "left"].iter().filter_map(|k| out["sides"].get(*k).and_then(Value::as_str).map(str::to_string)).collect();
    if sides.len() != 4 {
        return Err(bad(cmd, "the rectangle's sides are missing"));
    }
    let (bottom, right, top, left) = (&sides[0], &sides[1], &sides[2], &sides[3]);
    let mut fillet = json!({
        "points": [
            {"a": bottom, "b": left},
            {"a": bottom, "b": right},
            {"a": right, "b": top},
            {"a": top, "b": left},
        ],
        "radius": r_expr,
    });
    if let Some(sketch) = p.get("sketch") {
        fillet["sketch"] = sketch.clone();
    }
    let rounded = scratch.execute("sketch.fillet", &fillet)?;
    // Session::execute records the wrapper's before snapshot. Nested history must not leak.
    scratch.undo.clear();
    s.accept_computed(scratch);
    let arcs = rounded.get("curves").cloned().unwrap_or_else(|| json!([]));
    if let Some(o) = out.as_object_mut() {
        // The sharp corners are gone (the lines end at the arcs now).
        o.remove("corners");
        if let (Some(Value::Array(curves)), Value::Array(extra)) = (o.get_mut("curves"), &arcs) {
            curves.extend(extra.iter().cloned());
        }
        if let (Some(Value::Array(cons)), Some(Value::Array(extra))) = (o.get_mut("constraints"), rounded.get("constraints")) {
            cons.extend(extra.iter().cloned());
        }
        o.insert("arcs".into(), arcs);
        o.insert("param".into(), rounded.get("param").cloned().unwrap_or(Value::Null));
        if let Some(info) = rounded.get("sketch") {
            o.insert("sketch".into(), info.clone());
        }
    }
    Ok(out)
}

fn rect_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.rectangle.center";
    let (c, k) = (req_vec2(cmd, p, "center")?, req_vec2(cmd, p, "corner")?);
    let h = Vec2::new((k.x - c.x).abs(), (k.y - c.y).abs());
    if h.x < 1e-9 || h.y < 1e-9 {
        return Err(bad(cmd, "the corner must differ from the centre in x and y"));
    }
    let ((out, names), info) = edit(s, p, cmd, false, |sk, _| {
        let first = sk.points.len();
        let (l, pts) = polygon_lines(sk, &[c - h, Vec2::new(c.x + h.x, c.y - h.y), c + h, Vec2::new(c.x - h.x, c.y + h.y)])?;
        let mut cons = rect_constraints(sk, &l)?;
        // Both diagonals (construction), the centre at their crossing. Only the first carries
        // the midpoint constraint: diagonals of a parallelogram bisect each other, so a second
        // one would be redundant (over-constrained).
        if let (Some(p0), Some(p1), Some(p2), Some(p3)) = (pts.first(), pts.get(1), pts.get(2), pts.get(3)) {
            let diag = sk.add_line_pts(*p0, *p2, None)?;
            let diag2 = sk.add_line_pts(*p1, *p3, None)?;
            for d in [diag, diag2] {
                if let Some(cu) = sk.curves.get_mut(d) {
                    cu.construction = true;
                }
            }
            let cp = sk.add_point(c, None)?;
            cons.push(add_c(sk, ConstraintKind::Midpoint { p: cp, l: diag })?);
        }
        mark_construction(sk, &l, p);
        cons.extend(connect_points(sk, first, p)?);
        drop_implied_axes(sk, &mut cons);
        Ok(((ids_of(sk, &l), cons), rect_names(sk, &l, &pts)))
    })?;
    Ok(rect_result(out, names, info))
}

fn rect_three_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.rectangle.three_point";
    let (a, b, c) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?, req_vec2(cmd, p, "p2")?);
    let d = (b - a).normalized().ok_or_else(|| bad(cmd, "p0 and p1 must differ"))?;
    let n = d.perp();
    let w = (c - a).dot(n);
    if w.abs() < 1e-9 {
        return Err(bad(cmd, "p2 must not be on the first edge"));
    }
    let (out, info) = draw(s, p, cmd, |sk, _| {
        let (l, _) = polygon_lines(sk, &[a, b, b + n * w, a + n * w])?;
        let mut cons = Vec::new();
        if let [l0, l1, l2, l3] = l[..] {
            cons.push(add_c(sk, ConstraintKind::Perpendicular { a: l0, b: l1 })?);
            cons.push(add_c(sk, ConstraintKind::Parallel { a: l0, b: l2 })?);
            cons.push(add_c(sk, ConstraintKind::Parallel { a: l1, b: l3 })?);
        }
        mark_construction(sk, &l, p);
        Ok((ids_of(sk, &l), cons))
    })?;
    Ok(result(out, info))
}

pub(super) fn radius_arg(p: &Value, cmd: &str) -> Result<f64> {
    let r = match (num(p, "radius"), num(p, "diameter")) {
        (Some(r), _) => r,
        (None, Some(d)) => d / 2.0,
        _ => return Err(bad(cmd, "needs `radius` or `diameter`")),
    };
    if r > 1e-9 { Ok(r) } else { Err(bad(cmd, "radius must be positive")) }
}

fn circle_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.circle.center";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let r = radius_arg(p, cmd)?;
    let id = named_id(cmd, str_(p, "id"))?;
    let (out, info) = draw(s, p, cmd, |sk, doc| {
        let c = req_parg(sk, doc, p, "center", cmd)?;
        // The origin is the sketch's frame, not a hold on the geometry: a circle centred there
        // keeps a centre of its own, tied to the origin as any other drawn geometry is, so it
        // is not fixed geometry the sketch cannot move (#5034). A centre held by a parameter or
        // by projected geometry keeps that hold: the circle follows it.
        let own = c.idx().is_some_and(|i| sk.point_index("origin") == Some(i));
        let ci = sk.add_circle(c.pos(), r, if own { None } else { c.idx() }, id.as_deref())?;
        mark_construction(sk, &[ci], p);
        // A point the circle was drawn through stays on it (#3099): with a placed centre, that
        // sets the radius.
        let mut cons = Vec::new();
        if let Some(v) = p.get("through")
            && let PArg::Ref(q, _) = parg(sk, doc, v, cmd)?
        {
            cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: q, c: ci })?);
        }
        Ok((ids_of(sk, &[ci]), cons))
    })?;
    Ok(result(out, info))
}

fn circle_two(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.circle.two_point";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let r = a.dist(b) / 2.0;
    if r < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let (out, info) = draw(s, p, cmd, |sk, _| {
        let ci = sk.add_circle((a + b) * 0.5, r, None, None)?;
        mark_construction(sk, &[ci], p);
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

/// Circle through three points.
pub(super) fn circumcircle(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-12 {
        return None;
    }
    let (a2, b2, c2) = (a.len2(), b.len2(), c.len2());
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let o = Vec2::new(ux, uy);
    let r = o.dist(a);
    (o.is_finite() && r.is_finite() && r < 1e8).then_some((o, r))
}

fn circle_three(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.circle.three_point";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let (out, info) = draw(s, p, cmd, |sk, doc| {
        let (a, b, c) = (req_parg(sk, doc, p, "p0", cmd)?, req_parg(sk, doc, p, "p1", cmd)?, req_parg(sk, doc, p, "p2", cmd)?);
        let (o, r) = circumcircle(a.pos(), b.pos(), c.pos()).ok_or_else(|| bad(cmd, "the points are collinear"))?;
        let ci = sk.add_circle(o, r, None, None)?;
        mark_construction(sk, &[ci], p);
        // A click snapped to a point keeps the circle on it (#9858), as the centre circle's
        // `through` click does (#3099) and the 3-Point Arc's clicks do.
        let mut cons = Vec::new();
        for q in [a, b, c] {
            if let Some(point) = q.idx() {
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: point, c: ci })?);
            }
        }
        Ok((ids_of(sk, &[ci]), cons))
    })?;
    Ok(result(out, info))
}

fn arc_three(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.arc.three_point";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let id = named_id(cmd, str_(p, "id"))?;
    let (out, info) = draw(s, p, cmd, |sk, doc| {
        let (a, b) = (req_parg(sk, doc, p, "start", cmd)?, req_parg(sk, doc, p, "end", cmd)?);
        let t = req_parg(sk, doc, p, "through", cmd)?;
        let (o, _) = circumcircle(a.pos(), t.pos(), b.pos()).ok_or_else(|| bad(cmd, "the points are collinear"))?;
        // Counter-clockwise from start to end must pass the through point; otherwise swap.
        let ang = |q: Vec2| (q - o).angle();
        let ccw_span = |from: f64, to: f64| (to - from).rem_euclid(std::f64::consts::TAU);
        let ccw = ccw_span(ang(a.pos()), ang(t.pos())) < ccw_span(ang(a.pos()), ang(b.pos()));
        let (s0, e0) = if ccw { (a, b) } else { (b, a) };
        let ci = sk.add_arc(o, s0.pos(), e0.pos(), [None, s0.idx(), e0.idx()], id.as_deref())?;
        if let Some(c) = sk.curves.get_mut(ci) {
            c.reversed = !ccw;
        }
        mark_construction(sk, &[ci], p);
        let mut cons = Vec::new();
        if let Some(point) = t.idx() {
            cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: point, c: ci })?);
        }
        Ok((ids_of(sk, &[ci]), cons))
    })?;
    Ok(result(out, info))
}

fn arc_center(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.arc.center_point";
    let p = &super::sketch_project::snap_points(s, p, cmd)?;
    let id = named_id(cmd, str_(p, "id"))?;
    let sweep = num(p, "sweep");
    let (out, info) = draw(s, p, cmd, |sk, doc| {
        let c = req_parg(sk, doc, p, "center", cmd)?;
        let a = req_parg(sk, doc, p, "start", cmd)?;
        if let Some(sw) = sweep {
            // Start point and signed sweep (degrees); clockwise arcs keep `<id>.start` at the start.
            if !(sw.abs() > 1e-9 && sw.abs() < 360.0) {
                return Err(bad(cmd, "`sweep` must be between -360 and 360 degrees (not 0)"));
            }
            let end = c.pos() + Vec2::from_angle((a.pos() - c.pos()).angle() + sw.to_radians()) * a.pos().dist(c.pos());
            let cid = match &id {
                Some(i) => i.clone(),
                None => sk.fresh("a")?,
            };
            let ai = match a.idx() {
                Some(i) => i,
                None => sk.add_point(a.pos(), Some(&format!("{cid}.start")))?,
            };
            let bi = sk.add_point(end, Some(&format!("{cid}.end")))?;
            let ci = if sw > 0.0 {
                sk.add_arc(c.pos(), a.pos(), end, [c.idx(), Some(ai), Some(bi)], Some(&cid))?
            } else {
                let ci = sk.add_arc(c.pos(), end, a.pos(), [c.idx(), Some(bi), Some(ai)], Some(&cid))?;
                if let Some(cu) = sk.curves.get_mut(ci) {
                    cu.reversed = true;
                }
                ci
            };
            mark_construction(sk, &[ci], p);
            return Ok((ids_of(sk, &[ci]), Vec::new()));
        }
        let b = req_parg(sk, doc, p, "end", cmd)?;
        // Clockwise from start to end (#6645): stored counter-clockwise from the end and reversed,
        // as a negative sweep is. The start keeps the radius and `<id>.start`.
        let ci = if bool_(p, "clockwise").unwrap_or(false) {
            let r = a.pos().dist(c.pos());
            let end = c.pos() + (b.pos() - c.pos()).normalized().ok_or_else(|| bad(cmd, "`end` must not be on `center`"))? * r;
            let cid = match &id {
                Some(i) => i.clone(),
                None => sk.fresh("a")?,
            };
            let ai = match a.idx() {
                Some(i) => i,
                None => sk.add_point(a.pos(), Some(&format!("{cid}.start")))?,
            };
            let bi = match b.idx() {
                Some(i) => i,
                None => sk.add_point(end, Some(&format!("{cid}.end")))?,
            };
            let ci = sk.add_arc(c.pos(), end, a.pos(), [c.idx(), Some(bi), Some(ai)], Some(&cid))?;
            if let Some(cu) = sk.curves.get_mut(ci) {
                cu.reversed = true;
            }
            ci
        } else {
            sk.add_arc(c.pos(), a.pos(), b.pos(), [c.idx(), a.idx(), b.idx()], id.as_deref())?
        };
        mark_construction(sk, &[ci], p);
        Ok((ids_of(sk, &[ci]), Vec::new()))
    })?;
    Ok(result(out, info))
}

fn polygon(s: &mut Session, p: &Value, cmd: &str, center: Vec2, r_vertex: f64, start: f64, n: usize, inscribed: bool) -> Result<Value> {
    let mut support_circle = String::new();
    let (out, info) = draw(s, p, cmd, |sk, _| {
        let corners: Vec<Vec2> = (0..n).map(|i| center + Vec2::from_angle(start + std::f64::consts::TAU * i as f64 / n as f64) * r_vertex).collect();
        let (l, pts) = polygon_lines(sk, &corners)?;
        let mut cons = Vec::new();
        for w in l.windows(2) {
            if let [a, b] = w {
                cons.push(add_c(sk, ConstraintKind::Equal { a: *a, b: *b })?);
            }
        }
        // Construction circle tying the polygon to its centre.
        let rc = if inscribed { r_vertex } else { r_vertex * (std::f64::consts::PI / n as f64).cos() };
        let circ = sk.add_circle(center, rc, None, None)?;
        support_circle = sk.curves.get(circ).map(|c| c.id.clone()).unwrap_or_default();
        if let Some(cu) = sk.curves.get_mut(circ) {
            cu.construction = true;
        }
        if inscribed {
            for pt in &pts {
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: *pt, c: circ })?);
            }
        } else {
            // Equal sides tangent to one circle don't make a polygon regular: every other corner
            // stays free (a rhombus round a circle), and a drag pulled it out of shape (#7395).
            // The corners on a second circle, concentric, make it regular as the inscribed one
            // is; one side tangent to the support circle then sets its size (the others follow).
            let corner = sk.add_circle(center, r_vertex, None, None)?;
            if let Some(cu) = sk.curves.get_mut(corner) {
                cu.construction = true;
            }
            cons.push(add_c(sk, ConstraintKind::Concentric { a: circ, b: corner })?);
            for pt in &pts {
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: *pt, c: corner })?);
            }
            if let Some(first) = l.first() {
                cons.push(add_c(sk, ConstraintKind::Tangent { a: *first, b: circ })?);
            }
        }
        Ok((ids_of(sk, &l), cons))
    })?;
    let mut value = result(out, info);
    value["support_circle"] = json!(support_circle);
    Ok(value)
}

fn sides(p: &Value, cmd: &str) -> Result<usize> {
    let n = num(p, "sides").unwrap_or(6.0);
    if !(3.0..=200.0).contains(&n) || n.fract() != 0.0 {
        return Err(bad(cmd, "sides must be an integer from 3 to 200"));
    }
    Ok(n as usize)
}

fn polygon_inscribed(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.polygon.inscribed";
    let c = req_vec2(cmd, p, "center")?;
    let r = radius_arg(p, cmd)?;
    let n = sides(p, cmd)?;
    let a = num(p, "angle").unwrap_or(90.0).to_radians();
    polygon(s, p, cmd, c, r, a, n, true)
}

fn polygon_circumscribed(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.polygon.circumscribed";
    let c = req_vec2(cmd, p, "center")?;
    let r = radius_arg(p, cmd)?;
    let n = sides(p, cmd)?;
    let a = num(p, "angle").unwrap_or(90.0).to_radians();
    polygon(s, p, cmd, c, r / (std::f64::consts::PI / n as f64).cos(), a + std::f64::consts::PI / n as f64, n, false)
}

fn polygon_edge(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.polygon.edge";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let n = sides(p, cmd)?;
    let e = a.dist(b);
    if e < 1e-9 {
        return Err(bad(cmd, "the points must differ"));
    }
    let rv = e / (2.0 * (std::f64::consts::PI / n as f64).sin());
    let apothem = rv * (std::f64::consts::PI / n as f64).cos();
    let mid = (a + b) * 0.5;
    let dir = (b - a).normalized().unwrap_or(Vec2::X);
    let center = mid + dir.perp() * apothem;
    let start = (a - center).angle();
    polygon(s, p, cmd, center, rv, start, n, true)
}

pub(super) fn slot(s: &mut Session, p: &Value, cmd: &str, c0: Vec2, c1: Vec2, w: f64) -> Result<Value> {
    let h = w / 2.0;
    let d = (c1 - c0).normalized().ok_or_else(|| bad(cmd, "the slot ends must differ"))?;
    let n = d.perp();
    let (out, info) = draw(s, p, cmd, |sk, _| {
        let pts: Vec<usize> =
            [c0 - n * h, c1 - n * h, c1 + n * h, c0 + n * h].iter().map(|q| sk.add_point(*q, None)).collect::<std::result::Result<_, _>>()?;
        let (p0, p1, p2, p3) = (pts[0], pts[1], pts[2], pts[3]);
        let l1 = sk.add_line_pts(p0, p1, None)?;
        let a1 = sk.add_arc(c1, c1 - n * h, c1 + n * h, [None, Some(p1), Some(p2)], None)?;
        let l2 = sk.add_line_pts(p2, p3, None)?;
        let a0 = sk.add_arc(c0, c0 + n * h, c0 - n * h, [None, Some(p3), Some(p0)], None)?;
        let mut cons = Vec::new();
        for (l, a) in [(l1, a1), (l2, a1), (l1, a0), (l2, a0)] {
            cons.push(add_c(sk, ConstraintKind::Tangent { a: l, b: a })?);
        }
        cons.push(add_c(sk, ConstraintKind::Equal { a: a0, b: a1 })?);
        Ok((ids_of(sk, &[l1, a1, l2, a0]), cons))
    })?;
    Ok(result(out, info))
}

fn slot_c2c(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.slot.center_to_center";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let w = num(p, "width").filter(|w| *w > 1e-9).ok_or_else(|| bad(cmd, "`width` must be positive"))?;
    slot(s, p, cmd, a, b, w)
}

fn slot_overall(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.slot.overall";
    let (a, b) = (req_vec2(cmd, p, "p0")?, req_vec2(cmd, p, "p1")?);
    let w = num(p, "width").filter(|w| *w > 1e-9).ok_or_else(|| bad(cmd, "`width` must be positive"))?;
    let d = (b - a).normalized().ok_or_else(|| bad(cmd, "the slot ends must differ"))?;
    if a.dist(b) <= w {
        return Err(bad(cmd, "the slot must be longer than it is wide"));
    }
    slot(s, p, cmd, a + d * (w / 2.0), b - d * (w / 2.0), w)
}

fn draw_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point";
    let id = named_id(cmd, str_(p, "id"))?;
    // A point snapped while drawing (`"mid:<line>"`, `"on:<curve>:x,y"`, a model vertex or edge):
    // the snap makes it, held by its constraint, as it does for a line's end. Taken as bare
    // coordinates it was free to move off the midpoint it was put on (#4691).
    let snapped = p.get("point").is_some_and(|v| {
        v.as_str().is_some_and(|t| ["mid:", "on:", "vertex:"].iter().any(|k| t.starts_with(k)))
            || v.get("vertex").is_some()
            || v.get("on_edge").is_some()
    });
    if snapped {
        let given = p.get("point").cloned().unwrap_or(Value::Null);
        let p = &super::sketch_project::snap_points(s, p, cmd)?;
        // A snap it can't read is left as it was given, which names no point: refused, as a line's
        // end refuses it, rather than answered as if a point had been made.
        let made = str_(p, "point")
            .filter(|_| p.get("point") != Some(&given))
            .filter(|id| {
                sketch_store::target(s, p, cmd)
                    .ok()
                    .and_then(|sid| sketch_store::read(&s.doc, sid).ok())
                    .is_some_and(|sk| sk.resolve_point(id).is_some())
            })
            .map(str::to_string);
        let Some(pid) = made else {
            return Err(bad(
                cmd,
                format!(
                    "`point` {given} is not a snap it takes: \"mid:<line>\", \"on:<curve>:x,y\", \"vertex:x,y,z\", {{vertex: [x,y,z]}} or {{on_edge: [x,y,z]}}"
                ),
            ));
        };
        let ((), info) = edit(s, p, cmd, false, |_, _| Ok(()))?;
        return Ok(json!({"point": pid, "constraints": [], "sketch": info}));
    }
    let ((pid, cons), info) = edit(s, p, cmd, false, |sk, doc| {
        let i = match expr_point(doc, p.get("point").unwrap_or(&Value::Null), cmd)? {
            Some((at, Some(exprs))) => add_placed_point(sk, at, exprs, id.as_deref())?,
            Some((at, None)) => sk.add_point(at, id.as_deref())?,
            None => sk.add_point(req_vec2(cmd, p, "point")?, id.as_deref())?,
        };
        let cons = connect_points(sk, i, p)?;
        Ok((sk.points.get(i).map(|q| q.id.clone()).unwrap_or_default(), cons))
    })?;
    Ok(json!({"point": pid, "constraints": cons, "sketch": info}))
}

// ---------------------------------------------------------------------------------------------
// Constraints

fn constrain(s: &mut Session, p: &Value, cmd: &str, f: impl FnOnce(&mut Sketch) -> Result<ConstraintKind>) -> Result<Value> {
    let mut before = None;
    let mut residual = None;
    let (id, info) = edit(s, p, cmd, true, |sk, _| {
        let k = f(sk)?;
        let rep = solve(&mut sk.clone());
        before = Some(rep.dof);
        let order = entity_order(sk, p);
        let id = add_c(sk, k)?;
        residual = residual_of(sk, &id);
        // A constraint that already holds needs nothing moved (even when both entities are
        // fully determined: it is then redundant, see `redundancy`).
        let holds = residual.is_some_and(|r| r <= solidcraft_sketch::CONSISTENT);
        if let Some((first, second)) = order.filter(|_| !holds) {
            apply_in_order(sk, &rep, &[first], second, cmd, std::slice::from_ref(&id))?;
        }
        Ok(id)
    })?;
    let by = redundancy(s, p, cmd, before, &info, residual, &id)?;
    constraint_result(p, cmd, id, info, by, Value::Null)
}

/// A sketch entity a constraint is applied to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Ent {
    Point(usize),
    Curve(usize),
}

fn ent(sk: &Sketch, r: &str) -> Option<Ent> {
    sk.resolve_point(r).map(Ent::Point).or_else(|| sk.curve_index(r).or_else(|| sk.sketch_axis_curve(r)).map(Ent::Curve))
}

/// The entities of a two-entity constraint in the order they were picked (`a` then `b`, or
/// `point` then `line`).
fn entity_order(sk: &Sketch, p: &Value) -> Option<(Ent, Ent)> {
    let r = |k: &str| p.get(k).and_then(Value::as_str).and_then(|r| ent(sk, r));
    Some((r("a").or_else(|| r("point"))?, r("b").or_else(|| r("line"))?))
}

/// Held in place: fixed or projected, or fully determined by the constraints already there.
fn ent_locked(sk: &Sketch, rep: &SolveReport, e: Ent) -> bool {
    match e {
        Ent::Point(i) => sk.point_locked(i) || rep.point_determined.get(i).copied().unwrap_or(false),
        Ent::Curve(c) => sk.curve_locked(c) || rep.curve_determined.get(c).copied().unwrap_or(false),
    }
}

/// Hold entities: their points, and with `radius` their radii too.
fn hold_of(sk: &Sketch, ents: &[Ent], radius: bool) -> Hold {
    let mut h = Hold::default();
    for e in ents {
        match *e {
            Ent::Point(i) => h.points.push(i),
            Ent::Curve(c) => {
                h.points.extend(sk.curves.get(c).map(|c| c.kind.point_ids()).unwrap_or_default());
                h.radii.push(c);
            }
        }
    }
    if !radius {
        h.radii.clear();
    }
    h
}

/// Did a solve shrink a line or an arc to nothing (a degenerate way to satisfy tangency)?
fn collapsed(before: &Sketch, after: &Sketch) -> bool {
    let size = |sk: &Sketch, i: usize| match sk.curves.get(i).map(|c| &c.kind) {
        Some(CurveKind::Line { .. }) => line_pts(sk, i).map(|(a, b)| a.dist(b)),
        Some(CurveKind::Arc { .. } | CurveKind::Circle { .. }) => sk.radius(i),
        _ => None,
    };
    (0..before.curves.len()).any(|i| match (size(before, i), size(after, i)) {
        (Some(b), Some(a)) => b > 1e-6 && a < b * 1e-3,
        _ => false,
    })
}

/// Keep an entity's size while it moves: a line's length (circles keep their radius by holding
/// it).
fn keep_size(sk: &Sketch, e: Ent) -> Vec<ConstraintKind> {
    match (
        e,
        line_pts(
            sk,
            match e {
                Ent::Curve(c) => c,
                Ent::Point(_) => usize::MAX,
            },
        ),
    ) {
        (Ent::Curve(l), Some((a, b))) => vec![ConstraintKind::Length { l, value: a.dist(b) }],
        _ => Vec::new(),
    }
}

/// Satisfy a just-added constraint the way a dimensional-constraint tool applies it: the first-picked entities stay
/// put and only the second moves (first keeping its size, so it turns and slides, then also
/// resizing).
/// If the second can't move (fixed, projected or fully determined), the first moves instead;
/// if neither can, the constraint conflicts and nothing changes. Everything else moves as
/// little as it can. Later solves are ordinary ones (nothing is held).
/// `new` are the ids of the constraints the command just added: the refusal names the
/// constraints the new one conflicts with, never the new one's own id (#9735).
pub(super) fn apply_in_order(sk: &mut Sketch, rep: &SolveReport, first: &[Ent], second: Ent, cmd: &str, new: &[String]) -> Result<()> {
    let second_locked = ent_locked(sk, rep, second);
    // (held geometry, temporary size constraints) to try in turn.
    let mut tries: Vec<(Hold, Vec<ConstraintKind>)> = Vec::new();
    let mut ordered = |stay: &[Ent], mover: Ent| {
        let mut h = hold_of(sk, stay, true);
        h.radii.extend(hold_of(sk, &[mover], true).radii);
        tries.push((h.clone(), keep_size(sk, mover)));
        tries.push((hold_of(sk, stay, true), Vec::new()));
        // An end point the two share may slide (a line tangent to an arc it ends on, whose
        // centre is pinned, has to turn about its other end).
        let shared = hold_of(sk, &[mover], false).points;
        if h.points.iter().any(|p| shared.contains(p)) {
            h.points.retain(|p| !shared.contains(p));
            tries.push((h.clone(), keep_size(sk, mover)));
            tries.push((h, Vec::new()));
        }
    };
    if !second_locked {
        ordered(first, second);
    }
    // Each first entity that can move may (a free symmetry line between two fixed lines, #1796),
    // even when another of them can't.
    for f in first.iter().filter(|f| !ent_locked(sk, rep, **f)) {
        ordered(&[second], *f);
    }
    const TEMP: &str = "__keep_size";
    for (h, extra) in tries {
        let mut trial = sk.clone();
        trial.constraints.extend(extra.into_iter().map(|kind| solidcraft_sketch::Constraint {
            id: TEMP.into(),
            kind,
            param: None,
            driven: false,
            text: None,
        }));
        let ok = solve_with(&mut trial, &h, None, Analysis::StatusOnly, None).ok();
        trial.constraints.retain(|c| c.id != TEMP);
        if ok && !collapsed(sk, &trial) && !flung(sk, &trial) {
            *sk = trial;
            return Ok(());
        }
    }
    // Both could move but holding either one leads nowhere: solve as usual. One movable first
    // entity is enough: a symmetry's axis never moves, and with a width fixed between the mirrored
    // sides neither side moves alone, but the two together slide onto the axis (#4030).
    let first_movable = first.iter().any(|f| !ent_locked(sk, rep, *f));
    if first_movable && !second_locked {
        let mut trial = sk.clone();
        if solve_with(&mut trial, &Hold::default(), None, Analysis::StatusOnly, None).ok() {
            if flung(sk, &trial) {
                return Err(bad(cmd, "that would throw points of the sketch far away: its constraints have no solution near the sketch as it is"));
            }
            if let Some(curve) = shrunk(sk, &trial) {
                return Err(bad(cmd, format!("that would shrink {curve} to a point: its constraints can only be met without it")));
            }
            *sk = trial;
            return Ok(());
        }
    }
    // Neither holding one side nor a plain solve places it. Name the constraints the new one
    // conflicts with (the sketch may well have free degrees of freedom: the conflict is with
    // its own constraints, not the entities being fixed or fully constrained, #9735). The
    // violated solve names only what it gave up on; the conflict is the sketch's own set of
    // constraints that leaves the new one unsatisfiable.
    let conflicts: Vec<String> = sk
        .constraints
        .iter()
        .position(|c| new.contains(&c.id))
        .map(|ci| solidcraft_sketch::determined_by(sk, ci))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|k| sk.constraints.get(k))
        .filter(|c| !new.contains(&c.id))
        .map(|c| c.id.clone())
        .collect();
    let conflicts = if conflicts.is_empty() { conflict_probe(sk, new) } else { conflicts };
    let conflicts = if conflicts.is_empty() { direction_conflicts(sk, new) } else { conflicts };
    if !conflicts.is_empty() {
        return Err(bad(cmd, format!("that conflicts with the sketch: it conflicts with {}", conflicts.join(", "))));
    }
    Err(bad(cmd, "that conflicts with the sketch: neither entity can move to satisfy it (they are fixed or fully constrained)"))
}

/// A sketch this large is not probed constraint by constraint for a conflict message.
const CONFLICT_PROBE_MAX: usize = 64;

/// The sketch's own constraints (ids, in creation order) that keep the just-added constraints
/// `new` unsatisfiable: each constraint whose removal lets the sketch solve again is dropped,
/// so what is left is a set the new constraint genuinely conflicts with (a constraint the
/// failure does not need is not named). A solve that collapses or flings the sketch is not a
/// solution. Empty when the sketch is too large to probe, already solves, or fails whatever
/// the sketch's own constraints — the new one asks the impossible of the geometry itself.
fn conflict_probe(sk: &Sketch, new: &[String]) -> Vec<String> {
    let is_new = |c: &solidcraft_sketch::Constraint| new.contains(&c.id);
    let holds = |s: &Sketch| {
        let mut t = s.clone();
        let rep = solve_with(&mut t, &Hold::default(), None, Analysis::StatusOnly, None);
        rep.ok() && shrunk(s, &t).is_none() && !flung(s, &t)
    };
    if sk.constraints.len() > CONFLICT_PROBE_MAX || holds(sk) {
        return Vec::new();
    }
    let mut keep: std::collections::HashSet<String> = sk.constraints.iter().filter(|c| !is_new(c)).map(|c| c.id.clone()).collect();
    for id in sk.constraints.iter().filter(|c| !is_new(c)).map(|c| c.id.clone()).collect::<Vec<_>>() {
        if !keep.contains(&id) {
            continue;
        }
        let mut t = sk.clone();
        t.constraints.retain(|c| is_new(c) || (c.id != id && keep.contains(&c.id)));
        if !holds(&t) {
            keep.remove(&id);
        }
    }
    sk.constraints.iter().filter(|c| keep.contains(&c.id)).map(|c| c.id.clone()).collect()
}

/// The existing direction constraints (horizontal, vertical, angle, parallel …) that read one
/// of the two lines of a just-added Parallel or Perpendicular: the solver cannot turn a line
/// off a direction another constraint already fixed (and reads no gradient to turn with at a
/// line pair already at the forbidden angle), so those constraints are what it conflicts with.
/// Empty for any other kind of new constraint.
fn direction_conflicts(sk: &Sketch, new: &[String]) -> Vec<String> {
    let pair = sk.constraints.iter().find(|c| new.contains(&c.id)).and_then(|c| match c.kind {
        ConstraintKind::Parallel { a, b } | ConstraintKind::Perpendicular { a, b } => Some((a, b)),
        _ => None,
    });
    let Some((a, b)) = pair else { return Vec::new() };
    sk.constraints
        .iter()
        .filter(|c| !new.contains(&c.id))
        .filter(|c| {
            matches!(c.kind,
                ConstraintKind::Horizontal { l } | ConstraintKind::Vertical { l } | ConstraintKind::LineAngle { l, .. }
                    if l == a || l == b
            ) || matches!(c.kind,
                ConstraintKind::Parallel { a: x, b: y } | ConstraintKind::Perpendicular { a: x, b: y } | ConstraintKind::Collinear { a: x, b: y } | ConstraintKind::Angle { a: x, b: y, .. }
                    if x == a || x == b || y == a || y == b
            )
        })
        .map(|c| c.id.clone())
        .collect()
}

/// Degrees of freedom of a sketch as it is.
pub(super) fn dof_now(sk: &Sketch) -> usize {
    solve_with(&mut sk.clone(), &Hold::default(), None, Analysis::Freedom, None).dof
}

/// A constraint just added that removes no degree of freedom (`before` → the edit's `info`) is
/// redundant or a conflict. Redundant when it held where the sketch was before it was added
/// (`residual`, see [`solidcraft_sketch::constraint_residual`]): what it asks for is already
/// implied by other constraints, and the solver satisfies it with them. Returns the
/// constraints that determine it (as "id (Name)"), or `None` when it removes a degree of
/// freedom. A conflict is refused; the caller's edit is rolled back by returning the error.
pub(super) fn redundancy(
    s: &Session,
    p: &Value,
    cmd: &str,
    before: Option<usize>,
    info: &Value,
    residual: Option<f64>,
    cid: &str,
) -> Result<Option<Vec<String>>> {
    let after = info.get("dof").and_then(Value::as_u64).map(|d| d as usize);
    let (Some(b), Some(a)) = (before, after) else { return Ok(None) };
    if a < b {
        return Ok(None);
    }
    if !residual.is_some_and(|r| r <= solidcraft_sketch::CONSISTENT) {
        return Err(bad(cmd, "that would over-constrain the sketch (it conflicts with what the other constraints already determine)"));
    }
    Ok(Some(determined_by(s, p, cmd, cid)))
}

/// The constraints that already determine constraint `cid` of the target sketch, as
/// "id (Name)".
fn determined_by(s: &Session, p: &Value, cmd: &str, cid: &str) -> Vec<String> {
    let Some(sk) = sketch_store::target(s, p, cmd).ok().and_then(|id| sketch_store::read(&s.doc, id).ok()) else { return Vec::new() };
    let Some(ci) = sk.constraints.iter().position(|c| c.id == cid) else { return Vec::new() };
    solidcraft_sketch::determined_by(sk, ci)
        .into_iter()
        .filter_map(|k| sk.constraints.get(k))
        .map(|c| format!("{} ({})", c.id, c.kind.name()))
        .collect()
}

/// "k3 (Vertical), k9 (Vertical)", or "other constraints" when they are not known.
pub(super) fn listed(by: &[String]) -> String {
    if by.is_empty() { "other constraints".to_string() } else { by.join(", ") }
}

/// A redundant constraint is kept (and shown as redundant) unless the caller asked for
/// `if_redundant: "refuse"` (drawing-time inferences, which only add what is not already there).
pub(super) fn keep_redundant(p: &Value, cmd: &str, by: &[String]) -> Result<()> {
    if str_(p, "if_redundant").is_some_and(|m| m.eq_ignore_ascii_case("refuse")) {
        return Err(bad(cmd, format!("that would over-constrain the sketch (it is already determined by {})", listed(by))));
    }
    Ok(())
}

/// The result of a constraint command: the constraint, and whether it is redundant (and what
/// determines it).
pub(super) fn constraint_result(p: &Value, cmd: &str, id: String, info: Value, by: Option<Vec<String>>, extra: Value) -> Result<Value> {
    let mut out = json!({"constraint": id, "sketch": info});
    if let Some(by) = by {
        keep_redundant(p, cmd, &by)?;
        out["redundant"] = json!(true);
        out["determined_by"] = json!(by);
    }
    if let (Some(o), Value::Object(e)) = (out.as_object_mut(), extra) {
        o.extend(e);
    }
    Ok(out)
}

/// The residual of the constraint `id` where the sketch is now (see
/// [`solidcraft_sketch::constraint_residual`]).
pub(super) fn residual_of(sk: &Sketch, id: &str) -> Option<f64> {
    sk.constraints.iter().position(|c| c.id == id).and_then(|ci| solidcraft_sketch::constraint_residual(sk, ci))
}

/// The point already tied to the middle of line `l`, including one created by drawing.
/// Coincident coordinates alone are not enough: the Midpoint relationship must exist.
fn existing_midpoint(sk: &Sketch, l: usize) -> Option<usize> {
    sk.constraints.iter().find_map(|c| match c.kind {
        ConstraintKind::Midpoint { p, l: line } if line == l && !c.driven => Some(p),
        _ => None,
    })
}

/// Temporary pick intent. Saved sketches use ordinary points and constraints only.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AlignmentTarget {
    Point(usize),
    Midpoint(usize),
}

impl AlignmentTarget {
    fn materialize(self, sk: &mut Sketch, cmd: &str) -> Result<usize> {
        match self {
            Self::Point(p) => Ok(p),
            Self::Midpoint(l) => {
                if let Some(p) = existing_midpoint(sk, l) {
                    return Ok(p);
                }
                let (a, b) = line_pts(sk, l).ok_or_else(|| bad(cmd, "a midpoint reference needs a line"))?;
                let p = sk.add_point((a + b) * 0.5, None)?;
                add_c(sk, ConstraintKind::Midpoint { p, l })?;
                Ok(p)
            }
        }
    }
}

fn c_horizontal_vertical(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.horizontal_vertical";
    let mode = str_(p, "mode").map(str::to_ascii_lowercase);
    constrain(s, p, cmd, |sk| {
        if let Some(pts) = p.get("points").and_then(Value::as_array) {
            if pts.len() != 2 {
                return Err(bad(cmd, "`points` needs exactly two point references"));
            }
            let target = |v: &Value| -> Result<AlignmentTarget> {
                if let Some(line) = v.get("midpoint") {
                    let l = curve_ref(sk, Some(line), cmd, "midpoint")?;
                    if !matches!(sk.curves.get(l).map(|c| &c.kind), Some(CurveKind::Line { .. })) {
                        return Err(bad(cmd, "a midpoint reference needs a line"));
                    }
                    Ok(AlignmentTarget::Midpoint(l))
                } else {
                    Ok(AlignmentTarget::Point(point_ref(sk, Some(v), cmd, "points")?))
                }
            };
            // Validate both intents before materializing either. Hover and a pending first
            // pick never change the sketch; this command accepts both in one undo step.
            let a = target(pts.first().ok_or_else(|| bad(cmd, "missing first point"))?)?;
            let b = target(pts.get(1).ok_or_else(|| bad(cmd, "missing second point"))?)?;
            if a == b {
                return Err(bad(cmd, "alignment needs two distinct point references"));
            }
            let a = a.materialize(sk, cmd)?;
            let b = b.materialize(sk, cmd)?;
            if a == b {
                return Err(bad(cmd, "alignment needs two distinct points"));
            }
            let d = sk.point(b).ok_or_else(|| bad(cmd, "invalid second point"))? - sk.point(a).ok_or_else(|| bad(cmd, "invalid first point"))?;
            let horiz = match mode.as_deref() {
                Some("horizontal") => true,
                Some("vertical") => false,
                _ => d.x.abs() >= d.y.abs(),
            };
            return Ok(if horiz { ConstraintKind::HorizontalPoints { p: a, q: b } } else { ConstraintKind::VerticalPoints { p: a, q: b } });
        }
        let l = curve_ref(sk, p.get("line"), cmd, "line")?;
        let (a, b) = match sk.curves.get(l).map(|c| &c.kind) {
            Some(CurveKind::Line { a, b }) => (*a, *b),
            _ => return Err(bad(cmd, "`line` must be a line")),
        };
        let d = sk.point(b).unwrap_or_default() - sk.point(a).unwrap_or_default();
        let horiz = match mode.as_deref() {
            Some("horizontal") => true,
            Some("vertical") => false,
            _ => d.x.abs() >= d.y.abs(),
        };
        Ok(if horiz { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } })
    })
}

fn c_coincident(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.coincident";
    let p = &super::sketch_project::project_operands(s, p, cmd, &["a", "b"])?;
    // Already tied (e.g. connected when it was drawn there): nothing to add.
    let sk = sketch_store::read(&s.doc, sketch_store::target(s, p, cmd)?)?;
    if let Some((a, b)) = two_round(sk, p) {
        return coincident_circles(s, p, cmd, a, b);
    }
    let pt = |k: &str| p.get(k).and_then(Value::as_str).and_then(|r| sk.resolve_point(r));
    let line_points = coincident_line_points(sk, p, cmd)?;
    if let Some((a, b)) = line_points.or_else(|| pt("a").zip(pt("b")))
        && let Some(c) = sk.constraints.iter().find(|c| matches!(c.kind, ConstraintKind::Coincident { p, q } if (p, q) == (a, b) || (p, q) == (b, a)))
    {
        let rep = solve(&mut sk.clone());
        return Ok(json!({"constraint": c.id, "existing": true, "sketch": report_json(&rep)}));
    }
    let pick = |sk: &mut Sketch| -> Result<ConstraintKind> {
        if let Some((a, b)) = line_points {
            return Ok(ConstraintKind::Coincident { p: a, q: b });
        }
        let aref = p.get("a").and_then(Value::as_str).unwrap_or_default();
        let bref = p.get("b").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`b` must be a point or curve"))?;
        // A curve (or sketch axis) picked first and a point second: the point goes onto it.
        if sk.resolve_point(aref).is_none()
            && let Some(q) = sk.resolve_point(bref)
            && let Some(c) = curve_or_axis(sk, aref)?
        {
            return Ok(ConstraintKind::PointOnCurve { p: q, c });
        }
        let a = point_ref(sk, p.get("a"), cmd, "a")?;
        if let Some(b) = sk.resolve_point(bref) {
            return Ok(ConstraintKind::Coincident { p: a, q: b });
        }
        let c = curve_or_axis(sk, bref)?.ok_or_else(|| bad(cmd, format!("unknown entity `{bref}`")))?;
        Ok(ConstraintKind::PointOnCurve { p: a, c })
    };
    constrain(s, p, cmd, |sk| {
        let k = pick(sk)?;
        // A line's two ends on each other shrink it to a point, as a line's own end at its
        // middle does (see `c_midpoint`).
        if let ConstraintKind::Coincident { p: a, q: b } = k
            && sk.curves.iter().any(|c| matches!(c.kind, CurveKind::Line { a: x, b: y } if (x, y) == (a, b) || (x, y) == (b, a)))
        {
            return Err(bad(cmd, "a line's two ends can't be coincident: the line would shrink to a point"));
        }
        Ok(k)
    })
}

/// `a` and `b` both circles or arcs (two different ones).
fn two_round(sk: &Sketch, p: &Value) -> Option<(usize, usize)> {
    let round = |k: &str| {
        let c = sk.curve_index(p.get(k)?.as_str()?)?;
        matches!(sk.curves.get(c)?.kind, CurveKind::Circle { .. } | CurveKind::Arc { .. }).then_some(c)
    };
    let (a, b) = (round("a")?, round("b")?);
    (a != b).then_some((a, b))
}

/// Two circles or arcs made coincident lie on each other: one centre and one radius (Concentric
/// and Equal), the second pick moving onto the first, in one step. A half already there is
/// not added again.
fn coincident_circles(s: &mut Session, p: &Value, cmd: &str, a: usize, b: usize) -> Result<Value> {
    let (ids, info) = edit(s, p, cmd, true, |sk, _| {
        let rep = solve(&mut sk.clone());
        let pair = |x: usize, y: usize| (x, y) == (a, b) || (x, y) == (b, a);
        let concentric = sk.constraints.iter().any(|c| !c.driven && matches!(c.kind, ConstraintKind::Concentric { a: x, b: y } if pair(x, y)));
        let equal = sk.constraints.iter().any(|c| !c.driven && matches!(c.kind, ConstraintKind::Equal { a: x, b: y } if pair(x, y)));
        let mut ids = Vec::new();
        if !concentric {
            ids.push(add_c(sk, ConstraintKind::Concentric { a, b })?);
        }
        if !equal {
            ids.push(add_c(sk, ConstraintKind::Equal { a, b })?);
        }
        if ids.is_empty() {
            return Err(bad(cmd, "these circles already lie on each other"));
        }
        apply_in_order(sk, &rep, &[Ent::Curve(a)], Ent::Curve(b), cmd, &ids)?;
        Ok(ids)
    })?;
    Ok(json!({"constraint": ids[0], "constraints": ids, "sketch": info}))
}

/// Four endpoint pairs, ordered independently of pick order when distances tie.
fn coincident_line_points(sk: &Sketch, p: &Value, cmd: &str) -> Result<Option<(usize, usize)>> {
    let curve = |key: &str| p.get(key).and_then(Value::as_str).and_then(|id| sk.curve_index(id));
    let (Some(a), Some(b)) = (curve("a"), curve("b")) else { return Ok(None) };
    if a == b {
        return Err(bad(cmd, "choose two distinct lines"));
    }
    let ends = |i: usize| match sk.curves.get(i).map(|curve| &curve.kind) {
        Some(CurveKind::Line { a, b }) => Some([*a, *b]),
        _ => None,
    };
    let (Some(first), Some(second)) = (ends(a), ends(b)) else {
        return Err(bad(cmd, "two curves must both be straight lines; otherwise choose a point and a curve"));
    };
    let mut best: Option<(f64, usize, usize)> = None;
    for a in first {
        for b in second {
            let (Some(pa), Some(pb)) = (sk.point(a), sk.point(b)) else { return Err(bad(cmd, "line endpoint is missing")) };
            let candidate = (pa.dist(pb), a.min(b), a.max(b));
            if best.is_none_or(|old| candidate < old) {
                best = Some(candidate);
            }
        }
    }
    let (_, a, b) = best.ok_or_else(|| bad(cmd, "line endpoints are missing"))?;
    if a == b {
        return Err(bad(cmd, "the lines already share an endpoint"));
    }
    Ok(Some((a, b)))
}

fn two_curves(sk: &Sketch, p: &Value, cmd: &str) -> Result<(usize, usize)> {
    Ok((curve_ref(sk, p.get("a"), cmd, "a")?, curve_ref(sk, p.get("b"), cmd, "b")?))
}

/// The two picks of a two-curve constraint, refused when they are one curve: parallel,
/// collinear, equal or concentric with itself always holds (it was added as a constraint that
/// does nothing), and perpendicular or tangent to itself can't, which the solve reported as
/// geometry that can't move. Coincident already refuses a point with itself.
fn two_different<T: PartialEq>(cmd: &str, (a, b): (T, T)) -> Result<(T, T)> {
    if a == b {
        return Err(bad(cmd, "`a` and `b` are the same curve: pick two different ones"));
    }
    Ok((a, b))
}

/// Two curves where either may be a sketch axis (`x` / `y`, see [`line_or_axis_ref`]).
fn two_curves_or_axes(sk: &mut Sketch, p: &Value, cmd: &str) -> Result<(usize, usize)> {
    Ok((line_or_axis_ref(sk, p.get("a"), cmd, "a")?, line_or_axis_ref(sk, p.get("b"), cmd, "b")?))
}

fn c_tangent(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.tangent", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.tangent", |sk| {
        two_curves_or_axes(sk, p, "sketch.constraint.tangent")
            .and_then(|ab| two_different("sketch.constraint.tangent", ab))
            .map(|(a, b)| ConstraintKind::Tangent { a, b })
    })
}
fn c_equal(s: &mut Session, p: &Value) -> Result<Value> {
    if p.get("curves").is_some() {
        return equal_curves(s, p);
    }
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.equal", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.equal", |sk| {
        two_curves(sk, p, "sketch.constraint.equal")
            .and_then(|ab| two_different("sketch.constraint.equal", ab))
            .map(|(a, b)| ConstraintKind::Equal { a, b })
    })
}

/// Equal over a list of curves (#4985): each one is made equal to the first, in one edit (one
/// undo step). A pair already made equal is not constrained twice. The solve moves as little as it
/// can, so a dimensioned curve keeps its size and the others take it, whatever the order.
fn equal_curves(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.equal";
    let ids = string_list(cmd, p, "curves")?;
    if !(2..=200).contains(&ids.len()) {
        return Err(bad(cmd, "`curves` must list 2 to 200 lines or circles/arcs"));
    }
    let (out, info) = edit(s, p, cmd, true, |sk, _| {
        let mut curves: Vec<usize> = Vec::with_capacity(ids.len());
        for id in &ids {
            let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            if curves.contains(&c) {
                return Err(bad(cmd, format!("`{id}` is listed twice")));
            }
            curves.push(c);
        }
        let Some((&first, rest)) = curves.split_first() else { return Err(bad(cmd, "`curves` is empty")) };
        let mut out = Vec::new();
        for &c in rest {
            let already =
                sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::Equal { a, b } if (a == first && b == c) || (a == c && b == first)));
            if !already {
                out.push(add_c(sk, ConstraintKind::Equal { a: first, b: c })?);
            }
        }
        Ok(out)
    })?;
    Ok(json!({"constraints": out, "sketch": info}))
}
fn c_parallel(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.parallel", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.parallel", |sk| {
        two_curves_or_axes(sk, p, "sketch.constraint.parallel")
            .and_then(|ab| two_different("sketch.constraint.parallel", ab))
            .map(|(a, b)| ConstraintKind::Parallel { a, b })
    })
}
fn c_perpendicular(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.perpendicular", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.perpendicular", |sk| {
        two_curves_or_axes(sk, p, "sketch.constraint.perpendicular")
            .and_then(|ab| two_different("sketch.constraint.perpendicular", ab))
            .map(|(a, b)| ConstraintKind::Perpendicular { a, b })
    })
}
fn c_concentric(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.concentric", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.concentric", |sk| {
        two_curves(sk, p, "sketch.constraint.concentric")
            .and_then(|ab| two_different("sketch.constraint.concentric", ab))
            .map(|(a, b)| ConstraintKind::Concentric { a, b })
    })
}
fn c_collinear(s: &mut Session, p: &Value) -> Result<Value> {
    let p = &super::sketch_project::project_operands(s, p, "sketch.constraint.collinear", &["a", "b"])?;
    constrain(s, p, "sketch.constraint.collinear", |sk| {
        two_curves_or_axes(sk, p, "sketch.constraint.collinear")
            .and_then(|ab| two_different("sketch.constraint.collinear", ab))
            .map(|(a, b)| ConstraintKind::Collinear { a, b })
    })
}
fn c_midpoint(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.midpoint";
    let p = &super::sketch_project::project_operands(s, p, cmd, &["a", "b", "point", "line"])?;
    let p = &line_stays(s, p, cmd);
    let pick = |sk: &mut Sketch| -> Result<ConstraintKind> {
        if p.get("a").is_none() && p.get("b").is_none() {
            return Ok(ConstraintKind::Midpoint { p: point_ref(sk, p.get("point"), cmd, "point")?, l: curve_ref(sk, p.get("line"), cmd, "line")? });
        }
        // `a` and `b` in pick order: a point and a line either way round, or two lines.
        let entity = |k: &str| -> Result<Ent> {
            let r = p.get(k).and_then(Value::as_str).ok_or_else(|| bad(cmd, format!("`{k}` must be a point or line id")))?;
            match sk.resolve_point(r) {
                Some(i) => Ok(Ent::Point(i)),
                None => sk.curve_index(r).map(Ent::Curve).ok_or_else(|| bad(cmd, format!("unknown sketch point or curve `{r}`"))),
            }
        };
        let is_line = |sk: &Sketch, c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Line { .. }));
        match (entity("a")?, entity("b")?) {
            (Ent::Point(q), Ent::Curve(l)) | (Ent::Curve(l), Ent::Point(q)) if is_line(sk, l) => Ok(ConstraintKind::Midpoint { p: q, l }),
            // Two lines: their midpoints meet. One point sits at the middle of both, reusing
            // a midpoint either line already has, so nothing new is saved in the design.
            (Ent::Curve(a), Ent::Curve(b)) if is_line(sk, a) && is_line(sk, b) => {
                if a == b {
                    return Err(bad(cmd, "pick two different lines"));
                }
                let (q, l) = match (existing_midpoint(sk, a), existing_midpoint(sk, b)) {
                    (Some(q), _) => (q, b),
                    (None, Some(q)) => (q, a),
                    (None, None) => (AlignmentTarget::Midpoint(a).materialize(sk, cmd)?, b),
                };
                if existing_midpoint_of(sk, q, l) {
                    return Err(bad(cmd, "these lines already share their midpoint"));
                }
                Ok(ConstraintKind::Midpoint { p: q, l })
            }
            _ => Err(bad(cmd, "MidPoint needs a point and a line, or two lines")),
        }
    };
    constrain(s, p, cmd, |sk| {
        let k = pick(sk)?;
        // A line's own end at its middle shrinks the line to a point; an angle, tangent or
        // parallel on it afterwards had no direction to hold and threw points millions of
        // millimetres away.
        if let ConstraintKind::Midpoint { p: q, l } = k
            && matches!(sk.curves.get(l).map(|c| &c.kind), Some(CurveKind::Line { a, b }) if *a == q || *b == q)
        {
            return Err(bad(cmd, "a line's own end can't be its midpoint: the line would shrink to a point"));
        }
        Ok(k)
    })
}

/// A point and a line picked for MidPoint, the point first: the line first instead, so the point
/// goes to the line's middle and the line stays (the first pick stays, see `apply_in_order`).
/// Picked the other way, the line turned about its far end to bring its middle to the point (an
/// end of another line clicked, then the line it should sit in the middle of). The line still
/// moves when the point can't.
fn line_stays(s: &Session, p: &Value, cmd: &str) -> Value {
    let swapped = (|| {
        let sk = sketch_store::read(&s.doc, sketch_store::target(s, p, cmd).ok()?).ok()?;
        let (a, b) = (p.get("a")?.as_str()?, p.get("b")?.as_str()?);
        let line = sk.curve_index(b).filter(|c| matches!(sk.curves.get(*c).map(|c| &c.kind), Some(CurveKind::Line { .. })));
        (sk.curve_index(a).is_none() && sk.resolve_point(a).is_some() && line.is_some()).then(|| {
            let mut q = p.clone();
            q["a"] = json!(b);
            q["b"] = json!(a);
            q
        })
    })();
    swapped.unwrap_or_else(|| p.clone())
}

/// Point `q` is already tied to the middle of line `l`.
fn existing_midpoint_of(sk: &Sketch, q: usize, l: usize) -> bool {
    sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Midpoint { p, l: line } if p == q && line == l) && !c.driven)
}

/// The centre of a symmetry about a point (#1075): `point`, or `line` naming a point rather than
/// a curve or a sketch axis (the origin picked as the third entity).
fn symmetry_center(sk: &Sketch, p: &Value, cmd: &str) -> Result<Option<usize>> {
    if let Some(r) = p.get("point") {
        return point_ref(sk, Some(r), cmd, "point").map(Some);
    }
    let Some(id) = p.get("line").and_then(Value::as_str) else { return Ok(None) };
    if sk.curve_index(id).is_some() || matches!(id, "x" | "y") {
        return Ok(None);
    }
    Ok(sk.resolve_point(id))
}

/// Points `a` and `b` symmetric about point `c`: a construction line from `a` to `b` with `c`
/// at its midpoint.
fn symmetric_about_point(sk: &mut Sketch, a: usize, b: usize, c: usize, cmd: &str) -> Result<String> {
    if a == b || a == c || b == c {
        return Err(bad(cmd, "symmetry about a point needs two other points"));
    }
    let l = sk.add_line_pts(a, b, None)?;
    if let Some(cu) = sk.curves.get_mut(l) {
        cu.construction = true;
    }
    add_c(sk, ConstraintKind::Midpoint { p: c, l })
}

/// Is point `q` an end of arc `c`: one of its end points, or a point where one of them is?
fn arc_end(sk: &Sketch, c: usize, q: usize) -> bool {
    let Some(CurveKind::Arc { a, b, .. }) = sk.curves.get(c).map(|x| &x.kind) else { return false };
    let at = |i: usize| sk.point(i);
    *a == q || *b == q || at(q).is_some_and(|pq| [*a, *b].iter().any(|e| at(*e).is_some_and(|pe| pe.dist(pq) < 1e-9)))
}

fn c_symmetry(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.symmetry";
    let p = &super::sketch_project::project_operands(s, p, cmd, &["a", "b", "line", "point"])?;
    let (ids, info) = edit(s, p, cmd, true, |sk, _| {
        if let Some(c) = symmetry_center(sk, p, cmd)? {
            return symmetry_about_point(sk, p, c, cmd);
        }
        let l = line_or_axis_ref(sk, p.get("line"), cmd, "line")?;
        let ra = p.get("a").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`a` must be a point or curve"))?;
        let rb = p.get("b").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`b` must be a point or curve"))?;
        let rep = solve(&mut sk.clone());
        let order = ent(sk, ra).zip(ent(sk, rb));
        let mut out = Vec::new();
        match (sk.resolve_point(ra), sk.resolve_point(rb)) {
            (Some(a), Some(b)) => out.push(add_c(sk, ConstraintKind::Symmetric { p: a, q: b, l })?),
            _ => {
                // Two curves: circles/arcs mirror their centres and keep equal radii; lines mirror
                // their end points.
                let ca = sk.curve_index(ra).ok_or_else(|| bad(cmd, format!("unknown entity `{ra}`")))?;
                let cb = sk.curve_index(rb).ok_or_else(|| bad(cmd, format!("unknown entity `{rb}`")))?;
                match (sk.curves.get(ca).map(|c| c.kind.clone()), sk.curves.get(cb).map(|c| c.kind.clone())) {
                    (Some(CurveKind::Line { a: a0, b: a1 }), Some(CurveKind::Line { a: b0, b: b1 })) => {
                        // Opposite sides of a rectangle run in opposite directions: pair each end
                        // with the end of the other line nearest its mirror image (#2961, #2940).
                        let (p0, p1) = if mirror_pairing_is_crossed(sk, l, [a0, a1], [b0, b1]) { (b1, b0) } else { (b0, b1) };
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: a0, q: p0, l })?);
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: a1, q: p1, l })?);
                    }
                    (Some(_), Some(_)) => {
                        let pa = round_center(sk, ca).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                        let pb = round_center(sk, cb).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                        out.push(add_c(sk, ConstraintKind::Symmetric { p: pa, q: pb, l })?);
                        out.push(add_c(sk, ConstraintKind::Equal { a: ca, b: cb })?);
                    }
                    _ => return Err(bad(cmd, "symmetry needs two points or two curves")),
                }
            }
        }
        // The first entity and the symmetry line stay; the second mirrors onto it.
        if let Some((a, b)) = order {
            apply_in_order(sk, &rep, &[a, Ent::Curve(l)], b, cmd, &out)?;
        }
        Ok(out)
    })?;
    Ok(json!({"constraints": ids, "sketch": info}))
}

/// True when the ends of line `a` mirror (about the line `l`) closer onto the ends of `b` crossed
/// (`a0` with `b1`, `a1` with `b0`) than in order. A degenerate symmetry line keeps the order.
fn mirror_pairing_is_crossed(sk: &Sketch, l: usize, a: [usize; 2], b: [usize; 2]) -> bool {
    let Some((o, e)) = line_pts(sk, l) else { return false };
    let d = e - o;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 < 1e-18 {
        return false;
    }
    let pt = |i: usize| sk.point(i).unwrap_or_default();
    let near = |i: usize, j: usize| {
        let v = pt(i) - o;
        let t = (v.x * d.x + v.y * d.y) / len2;
        let foot = o + d * t;
        (foot * 2.0 - pt(i)).dist(pt(j))
    };
    near(a[0], b[1]) + near(a[1], b[0]) < near(a[0], b[0]) + near(a[1], b[1])
}

/// Symmetry about point `c`: two points, two circles/arcs (their centres, and equal radii) or two
/// lines (each end with the end of the other line nearest its mirror image). The constraints
/// (the construction lines show in the response's `created`).
fn symmetry_about_point(sk: &mut Sketch, p: &Value, c: usize, cmd: &str) -> Result<Vec<String>> {
    let ra = p.get("a").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`a` must be a point or curve"))?;
    let rb = p.get("b").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`b` must be a point or curve"))?;
    let rep = solve(&mut sk.clone());
    let order = ent(sk, ra).zip(ent(sk, rb));
    let pairs: Vec<(usize, usize)> = match (sk.resolve_point(ra), sk.resolve_point(rb)) {
        (Some(a), Some(b)) => vec![(a, b)],
        _ => {
            let ca = sk.curve_index(ra).ok_or_else(|| bad(cmd, format!("unknown entity `{ra}`")))?;
            let cb = sk.curve_index(rb).ok_or_else(|| bad(cmd, format!("unknown entity `{rb}`")))?;
            match (sk.curves.get(ca).map(|x| x.kind.clone()), sk.curves.get(cb).map(|x| x.kind.clone())) {
                (Some(CurveKind::Line { a: a0, b: a1 }), Some(CurveKind::Line { a: b0, b: b1 })) => {
                    let pc = sk.point(c).unwrap_or_default();
                    let mirror = |i: usize| pc * 2.0 - sk.point(i).unwrap_or_default();
                    let near = |i: usize, j: usize| mirror(i).dist(sk.point(j).unwrap_or_default());
                    if near(a0, b0) + near(a1, b1) <= near(a0, b1) + near(a1, b0) { vec![(a0, b0), (a1, b1)] } else { vec![(a0, b1), (a1, b0)] }
                }
                (Some(_), Some(_)) => {
                    let pa = round_center(sk, ca).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                    let pb = round_center(sk, cb).ok_or_else(|| bad(cmd, "symmetry needs two lines or two circles/arcs"))?;
                    let mut out = symmetry_pairs(sk, &[(pa, pb)], c, cmd)?;
                    out.push(add_c(sk, ConstraintKind::Equal { a: ca, b: cb })?);
                    if let Some((a, b)) = order {
                        apply_in_order(sk, &rep, &[a, Ent::Point(c)], b, cmd, &out)?;
                    }
                    return Ok(out);
                }
                _ => return Err(bad(cmd, "symmetry needs two points or two curves")),
            }
        }
    };
    let out = symmetry_pairs(sk, &pairs, c, cmd)?;
    // The first entity and the centre stay; the second mirrors onto it.
    if let Some((a, b)) = order {
        apply_in_order(sk, &rep, &[a, Ent::Point(c)], b, cmd, &out)?;
    }
    Ok(out)
}

fn symmetry_pairs(sk: &mut Sketch, pairs: &[(usize, usize)], c: usize, cmd: &str) -> Result<Vec<String>> {
    pairs.iter().map(|(a, b)| symmetric_about_point(sk, *a, *b, c, cmd)).collect()
}

fn round_center(sk: &Sketch, c: usize) -> Option<usize> {
    match sk.curves.get(c)?.kind {
        CurveKind::Circle { c, .. } | CurveKind::Arc { c, .. } => Some(c),
        _ => None,
    }
}

fn c_fix(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.constraint.fix";
    let want = bool_(p, "fixed");
    let mut refs = string_list(cmd, p, "entities")?;
    if let Some(e) = str_(p, "entity") {
        refs.insert(0, e.to_string());
    }
    if refs.is_empty() {
        return Err(bad(cmd, "`entity` or `entities` must name sketch points or curves"));
    }
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        // Points and curves; a point is fixed by a Fix constraint, a curve by its flag (which
        // also holds its radius). Projected geometry is always fixed.
        let mut pts = Vec::new();
        let mut curves = Vec::new();
        for r in &refs {
            if let Some(i) = sk.resolve_point(r) {
                pts.push(i);
            } else {
                curves.push(sk.curve_index(r).ok_or_else(|| bad(cmd, format!("unknown entity `{r}`")))?);
            }
        }
        let all_fixed = pts.iter().all(|q| *q == 0 || sk.point_locked(*q)) && curves.iter().all(|c| sk.curve_locked(*c));
        let fix = want.unwrap_or(!all_fixed);
        let mut n = 0;
        for q in pts {
            let own = sk.points.get(q).is_some_and(|x| x.fixed || x.link.is_some());
            if q == 0 || own {
                continue;
            }
            let has = sk.constraints.iter().any(|c| c.kind == ConstraintKind::Fix { p: q });
            if fix && !has {
                add_c(sk, ConstraintKind::Fix { p: q })?;
                n += 1;
            } else if !fix && has {
                sk.constraints.retain(|c| c.kind != ConstraintKind::Fix { p: q });
                n += 1;
            }
        }
        for c in curves {
            let Some(cu) = sk.curves.get_mut(c) else { continue };
            if cu.link.is_some() {
                continue;
            }
            cu.fixed = fix;
            n += 1;
            if !fix {
                let ids = cu.kind.point_ids();
                sk.constraints.retain(|k| !matches!(k.kind, ConstraintKind::Fix { p } if ids.contains(&p)));
            }
        }
        Ok(json!({"fixed": fix, "changed": n}))
    })?;
    Ok(json!({"result": n, "sketch": info}))
}

// ---------------------------------------------------------------------------------------------
// Dimensions

pub(super) fn line_pts(sk: &Sketch, l: usize) -> Option<(Vec2, Vec2)> {
    match sk.curves.get(l)?.kind {
        CurveKind::Line { a, b } => Some((sk.point(a)?, sk.point(b)?)),
        _ => None,
    }
}

fn dimension(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.dimension";
    // A dimension may measure model geometry (#7912): an `{edge: [x,y,z], body?}` or a
    // `{vertex: [x,y,z], body?}` among its entities is projected into the sketch first, as
    // `sketch.project` does, and stands for the projected curve (or point) there.
    let p = &super::sketch_project::project_entities(s, p, cmd)?;
    let ents = string_list(cmd, p, "entities")?;
    if ents.is_empty() || ents.len() > 2 {
        return Err(bad(cmd, "`entities` must list one or two sketch entities"));
    }
    let ty = str_(p, "type").unwrap_or("auto").to_ascii_lowercase();
    let ty = match (ty.as_str(), str_(p, "orientation").map(str::to_ascii_lowercase).as_deref()) {
        ("distance" | "auto", Some("horizontal")) => "horizontal".to_string(),
        ("distance" | "auto", Some("vertical")) => "vertical".to_string(),
        ("distance_x", _) => "horizontal".to_string(),
        ("distance_y", _) => "vertical".to_string(),
        ("offset", _) => "distance".to_string(),
        _ => ty,
    };
    let value = expr(p, "value");
    let text_at = p.get("text_at").and_then(vec2);
    let driven = bool_(p, "driven").unwrap_or(false);
    // `name = expression` defines the user parameter `name` and dimensions with it (#2916).
    let (defines, value) = match value.as_ref().and_then(|v| solidcraft_doc::expr::definition(v.as_str())) {
        Some((name, rhs)) => {
            if rhs.is_empty() {
                return Err(bad(cmd, format!("value: `{name} =` needs a value after the `=`")));
            }
            if driven {
                return Err(bad(cmd, format!("value: a driven dimension has no value, so it cannot define `{name}`")));
            }
            (Some((name.to_string(), rhs.to_string())), Some(solidcraft_doc::expr::Expr::new(name)))
        }
        None => (None, value),
    };
    let scale_sketch = bool_(p, "scale_sketch").unwrap_or(false);
    let mut scaled = None;
    let mut before = None;
    let (mut residual, mut new_id) = (None, String::new());
    let start = s.doc.clone();
    let ((param, kind_name, current), info) = edit(s, p, cmd, true, |sk, doc| {
        before = Some(dof_now(sk));
        let mut ent = |r: &str| -> Result<(Option<usize>, Option<usize>)> {
            if let Some(c) = sk.curve_index(r) {
                return Ok((None, Some(c)));
            }
            if let Some(q) = sk.resolve_point(r) {
                return Ok((Some(q), None));
            }
            if let Some(line) = r.strip_prefix("mid:") {
                let l = sk.curve_index(line).ok_or_else(|| bad(cmd, format!("unknown sketch line `{line}`")))?;
                if line_pts(sk, l).is_none() {
                    return Err(bad(cmd, "a midpoint reference needs a line"));
                }
                return Ok((Some(AlignmentTarget::Midpoint(l).materialize(sk, cmd)?), None));
            }
            Err(bad(cmd, format!("unknown sketch entity `{r}`")))
        };
        let e0 = ent(ents.first().map(String::as_str).unwrap_or(""))?;
        let e1 = match ents.get(1) {
            Some(r) => Some(ent(r)?),
            None => None,
        };
        let is_line = |c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Line { .. }));
        let is_circle = |c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Circle { .. }));
        let is_round = |c: usize| matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Circle { .. } | CurveKind::Arc { .. }));
        // The other reading of an angle between two lines (see below).
        let mut alt: Option<(ConstraintKind, f64)> = None;
        let (mut k, mut cur): (ConstraintKind, f64) = match (e0, e1) {
            ((None, Some(c)), None) if ty == "sweep" => {
                let angle = arc_len(sk, c).zip(sk.radius(c)).map(|(len, r)| len / r).ok_or_else(|| bad(cmd, "sweep needs an arc"))?;
                let angle = if sk.curves.get(c).is_some_and(|c| c.reversed) { -angle } else { angle };
                (ConstraintKind::ArcSweep { c, value: angle }, angle)
            }
            ((None, Some(c)), None) if is_line(c) => {
                let (a, b) = line_pts(sk, c).unwrap_or_default();
                match auto_orientation(&ty, a, b, text_at) {
                    "angle" => {
                        let angle = (b - a).angle();
                        (ConstraintKind::LineAngle { l: c, value: angle }, angle)
                    }
                    "horizontal" => {
                        (ConstraintKind::DistanceX { p: line_ends(sk, c).0, q: line_ends(sk, c).1, value: (b.x - a.x).abs() }, (b.x - a.x).abs())
                    }
                    "vertical" => {
                        (ConstraintKind::DistanceY { p: line_ends(sk, c).0, q: line_ends(sk, c).1, value: (b.y - a.y).abs() }, (b.y - a.y).abs())
                    }
                    _ => (ConstraintKind::Length { l: c, value: a.dist(b) }, a.dist(b)),
                }
            }
            ((None, Some(c)), None) if ty == "arc_length" || ty == "arclength" => {
                let len = arc_len(sk, c).ok_or_else(|| bad(cmd, "arc length needs an arc"))?;
                (ConstraintKind::ArcLength { c, value: len }, len)
            }
            ((None, Some(c)), None) if matches!(sk.curves.get(c).map(|x| &x.kind), Some(CurveKind::Ellipse { .. })) => {
                // Ellipse: the major radius is the centre–axis-end distance, the minor its own.
                let Some(CurveKind::Ellipse { c: cp, m, r }) = sk.curves.get(c).map(|x| x.kind.clone()) else {
                    return Err(bad(cmd, "ellipse"));
                };
                if ty == "major" {
                    let d = sk.point(cp).zip(sk.point(m)).map(|(a, b)| a.dist(b)).unwrap_or(0.0);
                    (ConstraintKind::Distance { p: cp, q: m, value: d }, d)
                } else {
                    (ConstraintKind::Radius { c, value: r }, r)
                }
            }
            ((None, Some(c)), None) => {
                let r = sk.radius(c).ok_or_else(|| bad(cmd, "cannot dimension that curve"))?;
                let diameter = ty == "diameter" || (ty != "radius" && is_circle(c));
                if diameter { (ConstraintKind::Diameter { c, value: 2.0 * r }, 2.0 * r) } else { (ConstraintKind::Radius { c, value: r }, r) }
            }
            ((Some(a), None), Some((Some(b), None))) => {
                let (pa, pb) = (sk.point(a).unwrap_or_default(), sk.point(b).unwrap_or_default());
                match auto_orientation(&ty, pa, pb, text_at) {
                    "horizontal" => (ConstraintKind::DistanceX { p: a, q: b, value: (pb.x - pa.x).abs() }, (pb.x - pa.x).abs()),
                    "vertical" => (ConstraintKind::DistanceY { p: a, q: b, value: (pb.y - pa.y).abs() }, (pb.y - pa.y).abs()),
                    _ => (ConstraintKind::Distance { p: a, q: b, value: pa.dist(pb) }, pa.dist(pb)),
                }
            }
            ((Some(q), None), Some((None, Some(l)))) | ((None, Some(l)), Some((Some(q), None))) if is_line(l) => {
                let (a, b) = line_pts(sk, l).unwrap_or_default();
                let pq = sk.point(q).unwrap_or_default();
                let d = (b - a).normalized().map(|d| d.cross(pq - a).abs()).unwrap_or(0.0);
                if diameter_about(sk, l, &ty) {
                    (ConstraintKind::LinearDiameter { p: q, l, value: 2.0 * d }, 2.0 * d)
                } else {
                    (ConstraintKind::PointLineDistance { p: q, l, value: d }, d)
                }
            }
            // An arc and one of its own end points (or a point on that end): its length (#754).
            ((Some(q), None), Some((None, Some(c)))) | ((None, Some(c)), Some((Some(q), None))) if arc_end(sk, c, q) => {
                let len = arc_len(sk, c).ok_or_else(|| bad(cmd, "arc length needs an arc"))?;
                (ConstraintKind::ArcLength { c, value: len }, len)
            }
            ((None, Some(a)), Some((None, Some(b)))) if ty == "overall_length" => {
                let current = sk
                    .center(a)
                    .zip(sk.center(b))
                    .zip(sk.radius(a).zip(sk.radius(b)))
                    .map(|((ca, cb), (ra, rb))| ca.dist(cb) + ra + rb)
                    .ok_or_else(|| bad(cmd, "overall slot length needs two arc caps"))?;
                (ConstraintKind::SlotOverallLength { a, b, value: current }, current)
            }
            ((None, Some(a)), Some((None, Some(b)))) if is_line(a) && is_line(b) => {
                let (a0, a1) = line_pts(sk, a).unwrap_or_default();
                let (b0, b1) = line_pts(sk, b).unwrap_or_default();
                let (da, db) = (a1 - a0, b1 - b0);
                if da.cross(db).abs() < 1e-9 * da.len() * db.len() && ty != "angle" {
                    // Parallel lines: distance between them (a diameter about a centerline).
                    let (a, b, b0) = if sk.curves.get(a).is_some_and(|c| c.centerline) { (a, b, b0) } else { (b, a, a0) };
                    let (bq, _) = line_ends(sk, b);
                    let (a0, a1) = line_pts(sk, a).unwrap_or_default();
                    let d = (a1 - a0).normalized().map(|d| d.cross(b0 - a0).abs()).unwrap_or(0.0);
                    if diameter_about(sk, a, &ty) {
                        (ConstraintKind::LinearDiameter { p: bq, l: a, value: 2.0 * d }, 2.0 * d)
                    } else {
                        (ConstraintKind::PointLineDistance { p: bq, l: a, value: d }, d)
                    }
                } else {
                    match (text_at, angle_from_shared_end(a, b, (a0, a1), (b0, b1))) {
                        (Some(t), _) => angle_by_sector(a, b, (a0, a1), (b0, b1), t).ok_or_else(|| bad(cmd, "cannot place that angle"))?,
                        // Two lines from one point (two radii, a corner) measure the angle
                        // between them, from that point towards their other ends (#2724).
                        (None, Some((between, supplement))) => {
                            alt = Some(supplement);
                            between
                        }
                        (None, None) => {
                            let mut ang = da.cross(db).atan2(da.dot(db));
                            let (mut ka, mut kb) = (a, b);
                            if ang < 0.0 {
                                ang = -ang;
                                std::mem::swap(&mut ka, &mut kb);
                            }
                            // Two lines make two angles, θ and 180° − θ, whichever way they were
                            // drawn: the acute one unless the value asks for the other.
                            let direct = (ConstraintKind::Angle { a: ka, b: kb, value: ang, flip: false }, ang);
                            let supplement =
                                (ConstraintKind::Angle { a: kb, b: ka, value: std::f64::consts::PI - ang, flip: true }, std::f64::consts::PI - ang);
                            let (acute, obtuse) = if ang <= std::f64::consts::FRAC_PI_2 { (direct, supplement) } else { (supplement, direct) };
                            alt = Some(obtuse);
                            acute
                        }
                    }
                }
            }
            // A line and a circle or arc: the distance from the line to its edge (#1803).
            ((None, Some(l)), Some((None, Some(c)))) | ((None, Some(c)), Some((None, Some(l)))) if is_line(l) && is_round(c) => {
                let (a, b) = line_pts(sk, l).unwrap_or_default();
                let (centre, r) = sk.center(c).zip(sk.radius(c)).ok_or_else(|| bad(cmd, "cannot dimension that curve"))?;
                let d = (b - a).normalized().map(|u| u.cross(centre - a).abs()).ok_or_else(|| bad(cmd, "that line has no length"))?;
                // A line that cuts the circle measures how far its edge reaches past the line.
                let inside = d < r;
                let gap = (d - r).abs();
                (ConstraintKind::LineCircleDistance { l, c, value: gap, inside }, gap)
            }
            // A point and a circle or arc: the distance from the point to its edge, along the
            // radius through the point (#4064).
            ((Some(q), None), Some((None, Some(c)))) | ((None, Some(c)), Some((Some(q), None))) if is_round(c) => {
                let own_centre = matches!(sk.curves.get(c).map(|x| &x.kind), Some(CurveKind::Circle { c: centre, .. } | CurveKind::Arc { c: centre, .. }) if *centre == q);
                if own_centre {
                    return Err(bad(cmd, "that point is the circle's own centre: dimension its radius or diameter instead"));
                }
                let (centre, r) = sk.center(c).zip(sk.radius(c)).ok_or_else(|| bad(cmd, "cannot dimension that curve"))?;
                let d = sk.point(q).unwrap_or_default().dist(centre);
                // A point within the circle measures how far inside its edge it lies.
                let inside = d < r;
                let gap = (d - r).abs();
                (ConstraintKind::PointCircleDistance { p: q, c, value: gap, inside }, gap)
            }
            // Two circles or arcs: the gap between their edges, how far they overlap, or the
            // narrowest gap between the inner one and the outer one, as they lie now (#4064).
            ((None, Some(a)), Some((None, Some(b)))) if is_round(a) && is_round(b) => {
                if a == b {
                    return Err(bad(cmd, "pick two different circles or arcs"));
                }
                let round = |c: usize| sk.center(c).zip(sk.radius(c)).ok_or_else(|| bad(cmd, "cannot dimension that curve"));
                let ((ca, ra), (cb, rb)) = (round(a)?, round(b)?);
                let d = ca.dist(cb);
                // A slot's two arc caps: the centre-to-centre distance (the slot's length), not
                // the gap between the inner edges (#7464).
                if (ty == "auto" || ty == "distance") && slot_caps(sk, a, b) {
                    let centre = |c: usize| match sk.curves.get(c).map(|x| &x.kind) {
                        Some(CurveKind::Arc { c, .. }) => Some(*c),
                        _ => None,
                    };
                    let (pa, pb) = centre(a).zip(centre(b)).ok_or_else(|| bad(cmd, "cannot dimension that curve"))?;
                    (ConstraintKind::Distance { p: pa, q: pb, value: d }, d)
                } else {
                    let circles = TwoCircles::of(d, ra, rb);
                    let gap = circles.gap(d, ra, rb).max(0.0);
                    (ConstraintKind::CircleCircleDistance { a, b, value: gap, circles }, gap)
                }
            }
            _ => return Err(bad(cmd, "cannot dimension that combination of entities")),
        };
        let is_angle = k.is_angle();
        if driven {
            // A reference dimension: measures, no parameter.
            let name = k.name();
            let id = sk.add_constraint(k, None)?;
            place_text(sk, &id, text_at);
            if let Some(c) = sk.constraints.iter_mut().find(|c| c.id == id) {
                c.driven = true;
            }
            before = None;
            return Ok((String::new(), name, measured(is_angle, cur)));
        }
        // With no value, the dimension restates what it measures, in the design's units (#3812).
        let (unit, default_expr) = if is_angle { ("deg".into(), format!("{} deg", round6(cur.to_degrees()))) } else { doc.design_length(cur) };
        let kind = if is_angle { Kind::Angle } else { Kind::Length };
        if let Some((name, rhs)) = &defines {
            // The new user parameter reads a bare number as the dimension would (the design's
            // unit for a length, degrees for an angle), and carries that unit.
            if let Some(problem) = solidcraft_doc::Document::param_name_problem(name) {
                return Err(bad(cmd, format!("value: {problem}")));
            }
            if doc.param(name).is_some() {
                return Err(bad(cmd, format!("value: there is already a parameter `{name}`: type `{name}` to use it, or change it in Parameters")));
            }
            let rhs = doc.with_design_unit(rhs, kind);
            let unit = if is_angle { "deg".to_string() } else { solidcraft_doc::expr::length_unit_or_mm(&doc.units).to_string() };
            doc.add_user_param(name, &rhs, Some(&unit), None).map_err(|e| bad(cmd, format!("value: {e}")))?;
        }
        let e = value.as_ref().map(|e| doc.with_design_unit(e, kind)).unwrap_or(default_expr);
        let pname = doc.new_model_param(&e, &unit);
        // The value must evaluate.
        let (vals, errs) = doc.param_values();
        if let Some(err) = errs.get(&pname) {
            return Err(bad(cmd, format!("value: {err}")));
        }
        // An angle between two lines dimensions whichever of its two readings is nearer the value.
        if let (Some((ka, ca)), Some(want)) = (alt, vals.get(&pname).and_then(|v| v.to_kind(Kind::Angle).ok()))
            && (want - ca).abs() < (want - cur).abs()
        {
            (k, cur) = (ka, ca);
        }
        // How far the value is from what the geometry measures now: zero for a dimension that
        // only restates what the sketch already has.
        let want = vals.get(&pname).and_then(|v| v.to_kind(if is_angle { Kind::Angle } else { Kind::Length }).ok());
        residual = want.map(|w| (w - cur).abs()).filter(|r| r.is_finite());
        // The first length dimension of a sketch with nothing sized yet scales it (#1365).
        if scale_sketch
            && !is_angle
            && let Some(factor) = want.map(|w| w / cur).filter(|f| f.is_finite() && (f - 1.0).abs() > 1e-9)
            && let Some(big) = sk.scaled_for_first_dimension(factor)
        {
            *sk = big;
            scaled = Some(factor);
            residual = Some(0.0);
        }
        let name = k.name();
        let id = sk.add_constraint(k, Some(pname.clone()))?;
        new_id = id.clone();
        place_text(sk, &id, text_at);
        Ok((pname, name, measured(is_angle, cur)))
    })?;
    // Report what the dimension measures once the sketch is solved, not the geometry before.
    let current = solved_measure(s, p, cmd).unwrap_or(current);
    if driven {
        return Ok(json!({"param": Value::Null, "driven": true, "type": kind_name, "measured": current.0, "unit": current.1, "sketch": info}));
    }
    // A dimension the sketch already determines cannot drive it: it is refused, or with
    // `if_redundant: "driven"` placed as a driven (reference) dimension instead.
    let by = redundancy(s, p, cmd, before, &info, residual, &new_id)
        .map_err(|_| bad(cmd, "that dimension conflicts with the sketch: other constraints already determine it with a different value"))?;
    if let Some(by) = by {
        if str_(p, "if_redundant").is_some_and(|m| m.eq_ignore_ascii_case("driven")) {
            s.doc = start;
            let mut q = p.clone();
            q["driven"] = json!(true);
            if let Some(o) = q.as_object_mut() {
                o.remove("if_redundant");
                o.remove("value");
            }
            let mut out = dimension(s, &q)?;
            out["redundant"] = json!(true);
            out["determined_by"] = json!(by);
            return Ok(out);
        }
        return Err(bad(
            cmd,
            format!("that dimension is already determined by {}; add it as a driven (reference) dimension with driven: true", listed(&by)),
        ));
    }
    let v = s.doc.param(&param).map(|p| p.expr.clone()).unwrap_or_default();
    let mut out = json!({"param": param, "type": kind_name, "expression": v, "measured": current.0, "unit": current.1, "sketch": info});
    if let Some(factor) = scaled {
        out["scaled"] = json!(round6(factor));
    }
    if let Some((name, _)) = defines {
        out["defined"] = json!(name);
    }
    Ok(out)
}

/// A dimension's measured value as reported, in the unit its expression is written in.
fn measured(is_angle: bool, v: f64) -> (f64, &'static str) {
    if is_angle { (round6(v.to_degrees()), "deg") } else { (round6(v), "mm") }
}

/// What the dimension just added (the sketch's last constraint) measures on the solved sketch.
fn solved_measure(s: &Session, p: &Value, cmd: &str) -> Option<(f64, &'static str)> {
    let sk = sketch_store::read(&s.doc, sketch_store::target(s, p, cmd).ok()?).ok()?;
    let k = &sk.constraints.last()?.kind;
    Some(measured(k.is_angle(), solidcraft_sketch::measure_dimension(sk, k)?))
}

/// The type of a dimension between `a` and `b` (two points, a line's ends or two line
/// midpoints). `auto` takes it from where the text is placed relative to the box the two span
/// as opposite corners: inside the box the distance between them; above or below it the
/// horizontal distance, beside it the vertical one; in the corners outside it the distance
/// again (#7295). Every type keeps a zone the size of the box's side whatever the slope, and
/// the box's edges are the thresholds. With no text, or when the two already line up with an
/// axis, `auto` stays the distance between them; other types are returned unchanged.
fn auto_orientation(ty: &str, a: Vec2, b: Vec2, text_at: Option<Vec2>) -> &str {
    let Some(t) = text_at.filter(|_| ty == "auto") else { return ty };
    let d = b - a;
    let len = d.len();
    if !t.x.is_finite() || !t.y.is_finite() || d.x.abs() <= 1e-9 * len || d.y.abs() <= 1e-9 * len {
        return ty;
    }
    let between = |v: f64, p: f64, q: f64| p.min(q) <= v && v <= p.max(q);
    match (between(t.x, a.x, b.x), between(t.y, a.y, b.y)) {
        (true, false) => "horizontal",
        (false, true) => "vertical",
        _ => ty,
    }
}

/// Keep a dimension's text where it was placed (`at`, sketch coordinates), relative to the
/// dimension so it follows the geometry.
fn place_text(sk: &mut Sketch, id: &str, at: Option<Vec2>) {
    let Some(i) = sk.constraints.iter().position(|c| c.id == id) else { return };
    let text = match (at, sk.constraints.get(i).and_then(|c| solidcraft_sketch::dim_frame(sk, &c.kind))) {
        (Some(at), Some(f)) => Some(solidcraft_sketch::encode_text(&f, at)),
        _ => None,
    };
    if let Some(c) = sk.constraints.get_mut(i) {
        c.text = text.filter(|t| t.is_finite() && t.x.abs() < 1e9 && t.y.abs() < 1e9);
    }
}

fn dimension_text(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.dimension_text";
    let key = str_(p, "dimension").ok_or_else(|| bad(cmd, "`dimension` must be a dimension id or parameter name"))?.to_string();
    let at = if bool_(p, "reset").unwrap_or(false) { None } else { Some(req_vec2(cmd, p, "at")?) };
    let (id, info) = edit(s, p, cmd, false, |sk, _| {
        let c = sk
            .constraints
            .iter()
            .find(|c| c.kind.is_dimension() && (c.id == key || c.param.as_deref() == Some(key.as_str())))
            .ok_or_else(|| bad(cmd, format!("no dimension `{key}` in this sketch")))?;
        let id = c.id.clone();
        place_text(sk, &id, at);
        Ok(id)
    })?;
    Ok(json!({"dimension": id, "sketch": info}))
}

/// Dimension a point (or a parallel line) to line `l` as a diameter: when `l` is a centerline
/// (unless a plain distance was asked for) or when a diameter was asked for.
fn diameter_about(sk: &Sketch, l: usize, ty: &str) -> bool {
    ty == "diameter" || (ty == "auto" && sk.curves.get(l).is_some_and(|c| c.centerline))
}

fn arc_len(sk: &Sketch, c: usize) -> Option<f64> {
    match sk.segs(c).first()? {
        solidcraft_geom::Seg2::Arc { radius, sweep, .. } if matches!(sk.curves.get(c)?.kind, CurveKind::Arc { .. }) => Some(radius * sweep),
        _ => None,
    }
}

/// Whether two arcs are the two caps of a straight slot: each shares its end points with the same
/// two lines (the slot's sides), and those sides are tangent to both caps. The distance between
/// the caps is then their centre-to-centre distance, not the gap between their inner edges (#7464).
fn slot_caps(sk: &Sketch, a: usize, b: usize) -> bool {
    if a == b {
        return false;
    }
    let arc = |c: usize| match sk.curves.get(c).map(|x| &x.kind) {
        Some(CurveKind::Arc { c, a, b }) => sk.point(*c).map(|centre| (centre, *a, *b)),
        _ => None,
    };
    let (Some((ca, a0, a1)), Some((cb, b0, b1))) = (arc(a), arc(b)) else {
        return false;
    };
    // Count the sides that run between the two caps and are tangent to both. Tangency keeps two
    // arcs joined by two lines but bulging inward out of it: for those, centre distance plus both
    // radii is not the overall length.
    let mut sides = 0;
    for c in &sk.curves {
        let CurveKind::Line { a: la, b: lb } = &c.kind else {
            continue;
        };
        let on = |p: &usize, (x, y): (usize, usize)| *p == x || *p == y;
        let (pa, pb) = if on(la, (a0, a1)) && on(lb, (b0, b1)) {
            (*la, *lb)
        } else if on(lb, (a0, a1)) && on(la, (b0, b1)) {
            (*lb, *la)
        } else {
            continue;
        };
        if tangent_at(sk, pa, pb, ca) && tangent_at(sk, pb, pa, cb) {
            sides += 1;
        }
    }
    sides >= 2
}

/// Whether the line from `p` to `q` is tangent to the circle about `centre` at `p`: its direction
/// is perpendicular to the radius from `centre` to `p`.
fn tangent_at(sk: &Sketch, p: usize, q: usize, centre: Vec2) -> bool {
    let (Some(p), Some(q)) = (sk.point(p), sk.point(q)) else {
        return false;
    };
    match ((q - p).normalized(), (p - centre).normalized()) {
        (Some(d), Some(r)) => d.dot(r).abs() < 1e-9,
        _ => false,
    }
}

/// The angle dimension between two lines whose sector contains `t` (the text position): returns
/// the constraint and the current value of that sector angle.
fn angle_by_sector(a: usize, b: usize, (a0, a1): (Vec2, Vec2), (b0, b1): (Vec2, Vec2), t: Vec2) -> Option<(ConstraintKind, f64)> {
    let (da, db) = ((a1 - a0).normalized()?, (b1 - b0).normalized()?);
    let den = da.cross(db);
    if den.abs() < 1e-12 {
        return None;
    }
    let s = (b0 - a0).cross(db) / den;
    let x = a0 + da * s;
    let ta = (t - x).angle();
    // The four rays from the intersection, with their line and direction sign.
    let mut rays: Vec<(f64, usize, bool)> = vec![(da.angle(), a, true), ((-da).angle(), a, false), (db.angle(), b, true), ((-db).angle(), b, false)];
    rays.sort_by(|p, q| p.0.total_cmp(&q.0));
    let tau = std::f64::consts::TAU;
    for k in 0..4 {
        let (s0, l0, p0) = rays[k];
        let (s1, l1, p1) = rays[(k + 1) % 4];
        let span = (s1 - s0).rem_euclid(tau);
        let off = (ta - s0).rem_euclid(tau);
        if off <= span && l0 != l1 {
            return Some((ConstraintKind::Angle { a: l0, b: l1, value: span, flip: p0 != p1 }, span));
        }
    }
    None
}

/// The angle dimension between two lines that run from one shared end, measured from that end
/// towards their other ends (#2724), and its supplement; None when they share no end or are
/// collinear.
fn angle_from_shared_end(
    a: usize,
    b: usize,
    (a0, a1): (Vec2, Vec2),
    (b0, b1): (Vec2, Vec2),
) -> Option<((ConstraintKind, f64), (ConstraintKind, f64))> {
    let tol = 1e-9 * a0.dist(a1).max(b0.dist(b1)).max(1.0);
    // The sign that turns each line's direction into its ray away from the shared end.
    let (sa, sb) = [(a0, b0, 1.0, 1.0), (a0, b1, 1.0, -1.0), (a1, b0, -1.0, 1.0), (a1, b1, -1.0, -1.0)]
        .into_iter()
        .find(|(p, q, ..)| p.dist(*q) <= tol)
        .map(|(_, _, sa, sb)| (sa, sb))?;
    let (ra, rb) = ((a1 - a0) * sa, (b1 - b0) * sb);
    let mut ang = ra.cross(rb).atan2(ra.dot(rb));
    let (mut first, mut second) = ((a, sa), (b, sb));
    if ang < 0.0 {
        ang = -ang;
        std::mem::swap(&mut first, &mut second);
    }
    if !(ang > 1e-9 && ang < std::f64::consts::PI - 1e-9) {
        return None;
    }
    // The constraint turns the first line's direction onto the second's, negated when the
    // two rays run against their lines' directions differently.
    let flip = first.1 * second.1 < 0.0;
    let between = (ConstraintKind::Angle { a: first.0, b: second.0, value: ang, flip }, ang);
    let supplement = (ConstraintKind::Angle { a: second.0, b: first.0, value: std::f64::consts::PI - ang, flip: !flip }, std::f64::consts::PI - ang);
    Some((between, supplement))
}

pub(super) fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

fn line_ends(sk: &Sketch, l: usize) -> (usize, usize) {
    match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => (*a, *b),
        _ => (0, 0),
    }
}

// ---------------------------------------------------------------------------------------------
// Editing

fn construction(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.construction";
    let ids = string_list(cmd, p, "curves")?;
    if ids.is_empty() {
        return Err(bad(cmd, "`curves` must list curve ids"));
    }
    let want = bool_(p, "value");
    let (n, info) = edit(s, p, cmd, false, |sk, _| {
        let cs: Vec<usize> =
            ids.iter().map(|id| sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))).collect::<Result<_>>()?;
        // The selection switches together (a slot or polygon as a whole): to construction
        // unless all of it already is. Projected curves may switch too. A centerline that
        // becomes construction stops being a centerline (one line type at a time).
        let to = want.unwrap_or_else(|| !cs.iter().all(|c| sk.curves.get(*c).is_some_and(|c| c.construction)));
        let mut n = 0;
        for c in cs {
            if let Some(cu) = sk.curves.get_mut(c)
                && (cu.construction != to || to && cu.centerline || !to && cu.infinite)
            {
                cu.construction = to;
                if to {
                    cu.centerline = false;
                } else {
                    // A normal line is a finite one.
                    cu.infinite = false;
                }
                n += 1;
            }
        }
        Ok((n, to))
    })?;
    Ok(json!({"changed": n.0, "construction": n.1, "sketch": info}))
}

fn sketch_delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.delete";
    let ids = string_list(cmd, p, "entities")?;
    if ids.is_empty() {
        return Err(bad(cmd, "`entities` must list sketch entity ids"));
    }
    let (n, info) = edit(s, p, cmd, false, |sk, doc| {
        let mut curves = Vec::new();
        let mut points = Vec::new();
        let mut cons = Vec::new();
        for id in &ids {
            if let Some(c) = sk.curve_index(id) {
                curves.push(c);
            } else if let Some(q) = sk.point_index(id) {
                if q == 0 {
                    return Err(bad(cmd, "the sketch origin cannot be deleted"));
                }
                points.push(q);
            } else if sk.constraints.iter().any(|c| &c.id == id) {
                cons.push(id.clone());
            } else if sk.wire_index(id).is_some() {
                sk.remove_wires(std::slice::from_ref(id));
            } else if sk.link(id).is_some() {
                curves.extend(sk.link_curves(id));
                points.extend(sk.link_points(id));
            } else {
                return Err(bad(cmd, format!("unknown sketch entity `{id}`")));
            }
        }
        let selected_curves = curves.iter().copied().collect::<std::collections::BTreeSet<_>>();
        let selected_points = points.iter().copied().collect::<std::collections::BTreeSet<_>>();
        let removed = sk
            .curves
            .iter()
            .enumerate()
            .filter(|(index, curve)| selected_curves.contains(index) || curve.kind.point_ids().iter().any(|point| selected_points.contains(point)))
            .map(|(_, curve)| curve.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        for offset in &sk.offsets {
            let Some(topology) = &offset.topology else { continue };
            if offset.outputs.iter().any(|id| removed.contains(id.as_str()))
                && topology
                    .roles
                    .iter()
                    .any(|role| !role.retired && role.kind == solidcraft_sketch::OffsetRoleKind::Arc && !removed.contains(role.output.as_str()))
            {
                return Err(bad(cmd, "break the offset association before deleting only part of its curved boundary"));
            }
        }
        if sk.constraints.iter().any(|c| cons.contains(&c.id) && matches!(c.kind, ConstraintKind::OffsetDistance { .. })) {
            let report = solve_current(sk, doc)?;
            if !report.keeps_orientation() {
                return Err(bad(cmd, "cannot remove an offset dimension from a conflicting sketch"));
            }
            sk.refresh_offsets()?;
        }
        sk.constraints.retain(|c| !cons.contains(&c.id));
        let pids: Vec<String> = points.iter().filter_map(|q| sk.points.get(*q).map(|x| x.id.clone())).collect();
        // Linked geometry from the model, by its entity count before the delete (#5098).
        let size = |sk: &Sketch, id: &str| sk.link_curves(id).len() + sk.link_points(id).len();
        let linked: Vec<(String, usize)> = sk
            .links
            .iter()
            .filter(|l| !matches!(l.source, solidcraft_sketch::LinkSource::Text { .. }))
            .map(|l| (l.id.clone(), size(sk, &l.id)))
            .collect();
        sk.remove_curves(&curves);
        let pidx: Vec<usize> = pids.iter().filter_map(|id| sk.point_index(id)).collect();
        sk.remove_points(&pidx);
        // Part of a link deleted: the rest is broken off where it is. Still linked, the next
        // refresh would rebuild the whole link, deleted curves and all, as normal geometry.
        for (id, before) in &linked {
            if sk.link(id).is_some() && size(sk, id) < *before {
                sk.break_link(id)?;
            }
        }
        doc.prune_model_params();
        Ok(ids.len())
    })?;
    // Dimension parameters may have been dropped; prune again with the stored sketch.
    s.doc_mut().prune_model_params();
    Ok(json!({"deleted": n, "sketch": info}))
}

fn move_point(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.move_point";
    let to = req_vec2(cmd, p, "to")?;
    let id = sketch_store::target(s, p, cmd)?;
    let state = s.model.result(id).map(|r| r.state.clone());
    let last = state.as_ref().and_then(|state| state.sketch(id));
    let (_, rep) = edit_solved(s, id, false, Analysis::Freedom, |sk, _| {
        let q = point_ref(sk, p.get("point"), cmd, "point")?;
        Ok(((), drag_entity(sk, Ent::Point(q), &[], to, to, false, last).1.solve_again()))
    })?;
    Ok(json!({"sketch": rep.as_ref().map(report_json)}))
}

fn drag(s: &mut Session, p: &Value) -> Result<Value> {
    drag_frame(s, p, false).map(|(v, _)| v)
}

fn drag_live(s: &mut Session, p: &Value) -> Result<(Value, Option<crate::LiveEdit>)> {
    drag_frame(s, p, true)
}

/// `sketch.drag`; `live`: one frame of an interactive drag, returned with the sketch to show.
fn drag_frame(s: &mut Session, p: &Value, live: bool) -> Result<(Value, Option<crate::LiveEdit>)> {
    let cmd = "sketch.drag";
    let to = req_vec2(cmd, p, "to")?;
    let from = match p.get("from") {
        None | Some(Value::Null) => to,
        Some(v) => vec2(v).ok_or_else(|| bad(cmd, "`from` must be a point [x, y]"))?,
    };
    let id = sketch_store::target(s, p, cmd)?;
    // The sketch's report as last evaluated: a live frame keeps its degrees of freedom.
    let state = s.model.result(id).map(|r| r.state.clone());
    let last = state.as_ref().and_then(|state| state.sketch(id));
    let analysis = if live { Analysis::Full } else { Analysis::Freedom };
    let (d, rep) = edit_solved(s, id, false, analysis, |sk, doc| {
        let r = p.get("entity").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`entity` must be a sketch point or curve"))?;
        let e = ent(sk, r).ok_or_else(|| bad(cmd, format!("unknown sketch entity `{r}`")))?;
        let with = match p.get("with") {
            None | Some(Value::Null) => Vec::new(),
            Some(v) => {
                let ids = v.as_array().ok_or_else(|| bad(cmd, "`with` must be a list of sketch point or curve ids"))?;
                if ids.len() > sk.points.len() + sk.curves.len() {
                    return Err(bad(cmd, "`with` lists more entities than the sketch has"));
                }
                ids.iter()
                    .map(|v| {
                        let r = v.as_str().ok_or_else(|| bad(cmd, "`with` must be a list of sketch point or curve ids"))?;
                        ent(sk, r).ok_or_else(|| bad(cmd, format!("unknown sketch entity `{r}`")))
                    })
                    .collect::<Result<Vec<_>>>()?
            }
        };
        if let Some(link) = super::sketch_create::text_link(sk, r).map(str::to_owned) {
            let moving = sk.link_points(&link).into_iter().filter_map(|i| sk.point(i).map(|p| (i, p))).collect();
            super::sketch_create::move_text(sk, &link, to - from, 0.0, Vec2::ZERO, false)?;
            return Ok((drag::Dragged { reached: 1.0, moving, ..Default::default() }, Left::Unsolved));
        }
        // The held solves work on the dimensions' current values (a solved step applies them).
        doc.apply_dimension_values(&doc.param_values().0, sk)?;
        let (d, left) = drag_entity(sk, e, &with, from, to, live, last);
        // A live frame's held solve is its only one; otherwise the sketch's own solve reports
        // its degrees of freedom.
        Ok((d, if live && last.is_some() { left } else { left.solve_again() }))
    })?;
    // Lines retain the grabbed material point through rotation. Other entities report the
    // average displacement of their moving points, as before.
    let sk = sketch_store::read(&s.doc, id)?;
    let at = d.at(sk, from);
    let rep = rep.or_else(|| last.map(|ss| SolveReport { status: solidcraft_sketch::SolveStatus::Solved, failing: Vec::new(), ..ss.report.clone() }));
    let out = json!({"sketch": rep.as_ref().map(report_json), "reached": d.reached, "at": [at.x, at.y], "retry": d.retry});
    Ok((out, rep.filter(|_| live).map(|report| crate::LiveEdit { sketch: id, report })))
}

/// Ends a live drag: the snap constraint it let go on, if any, then one evaluation of the model.
/// It reads the model only before the sketch it edits (never stale), so `execute` does not
/// evaluate first.
fn drag_end(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.drag_end";
    let id = sketch_store::target(s, p, cmd)?;
    let start = s.doc.clone();
    let mut constraint = Value::Null;
    if let Some(c) = p.get("constraint").filter(|c| !c.is_null()) {
        let name = c.get("command").and_then(Value::as_str).unwrap_or_default();
        let spec = super::find_command(name)
            .filter(|spec| spec.id.starts_with("sketch.constraint."))
            .ok_or_else(|| bad(cmd, "`constraint.command` must be a sketch.constraint.* command"))?;
        let params = c.get("params").cloned().unwrap_or(Value::Null);
        // A constraint that cannot be added (it would over-constrain the sketch) is not kept.
        constraint = spec.run_nested(s, &params).unwrap_or_else(|e| {
            s.doc = start.clone();
            json!({"error": e.to_string()})
        });
    }
    // A new constraint changed the document: `execute` evaluates the model after this.
    if !std::sync::Arc::ptr_eq(&start, &s.doc) {
        return Ok(json!({"constraint": constraint, "sketch": constraint.get("sketch")}));
    }
    if s.model.is_stale() {
        s.refresh();
    }
    let report = s.model.result(id).and_then(|r| r.state.sketch(id)).map(|ss| report_json(&ss.report));
    Ok(json!({"constraint": constraint, "sketch": report}))
}

/// The Sketch Palette options of a sketch, by their palette names (true = shown / on).
fn options_json(v: &solidcraft_sketch::SketchView) -> Value {
    json!({
        "show_profile": !v.hide_profile,
        "show_points": !v.hide_points,
        "show_dimensions": !v.hide_dimensions,
        "show_constraints": !v.hide_constraints,
        "show_projected": !v.hide_projected,
        "show_construction": !v.hide_construction,
        "grid": !v.hide_grid,
        "snap": !v.no_snap,
        "slice": v.slice,
        "sketch_3d": v.three_d,
    })
}

fn sketch_options(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.options";
    let keys = [
        "show_profile",
        "show_points",
        "show_dimensions",
        "show_constraints",
        "show_projected",
        "show_construction",
        "grid",
        "snap",
        "slice",
        "sketch_3d",
    ];
    if let Some(o) = p.as_object()
        && let Some(k) = o.keys().find(|k| *k != "sketch" && !keys.contains(&k.as_str()))
    {
        return Err(bad(cmd, format!("unknown option `{k}`")));
    }
    let set = |k: &str| bool_(p, k);
    if keys.iter().all(|k| set(k).is_none()) {
        let id = sketch_store::target(s, p, cmd)?;
        return Ok(json!({"options": options_json(&sketch_store::read(&s.doc, id)?.view)}));
    }
    let (v, info) = edit(s, p, cmd, false, |sk, _| {
        let v = &mut sk.view;
        let put = |k: &str, flag: &mut bool, shown_means_off: bool| {
            if let Some(b) = set(k) {
                *flag = if shown_means_off { !b } else { b };
            }
        };
        put("show_profile", &mut v.hide_profile, true);
        put("show_points", &mut v.hide_points, true);
        put("show_dimensions", &mut v.hide_dimensions, true);
        put("show_constraints", &mut v.hide_constraints, true);
        put("show_projected", &mut v.hide_projected, true);
        put("show_construction", &mut v.hide_construction, true);
        put("grid", &mut v.hide_grid, true);
        put("snap", &mut v.no_snap, true);
        put("slice", &mut v.slice, false);
        put("sketch_3d", &mut v.three_d, false);
        Ok(options_json(v))
    })?;
    Ok(json!({"options": v, "sketch": info}))
}

fn chain_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.chain";
    let id = sketch_store::target(s, p, cmd)?;
    let sk = sketch_store::read(&s.doc, id)?;
    let c = curve_ref(sk, p.get("curve"), cmd, "curve")?;
    Ok(json!({"curves": ids_of(sk, &sk.chain(c))}))
}

fn solve_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let id = sketch_store::target(s, p, "sketch.solve")?;
    let st = s.model.state();
    let ss = st.sketch(id).ok_or_else(|| EngineError::Other("the sketch has not been evaluated".into()))?;
    Ok(json!({"status": ss.report.status, "dof": ss.report.dof, "failing": ss.report.failing, "profiles": ss.profiles.len()}))
}

#[cfg(test)]
#[path = "sketch_flip_tests.rs"]
mod flip_tests;

#[cfg(test)]
#[path = "sketch_rect_tests.rs"]
mod rect_tests;

#[cfg(test)]
#[path = "sketch_rounded_rect_tests.rs"]
mod rounded_rect_tests;

#[cfg(test)]
#[path = "sketch_ux_tests.rs"]
mod ux_tests;

#[cfg(test)]
#[path = "sketch_axis_ref_tests.rs"]
mod axis_ref_tests;
#[cfg(test)]
#[path = "sketch_centre_align_tests.rs"]
mod centre_align_tests;
#[cfg(test)]
#[path = "sketch_circle_edge_tests.rs"]
mod circle_edge_tests;
#[cfg(test)]
#[path = "sketch_drag_live_tests.rs"]
mod drag_live_tests;
#[cfg(test)]
#[path = "sketch_ids_tests.rs"]
mod ids_tests;
#[cfg(test)]
#[path = "sketch_infinite_tests.rs"]
mod infinite_tests;
#[cfg(test)]
#[path = "sketch_line_circle_tests.rs"]
mod line_circle_tests;
#[cfg(test)]
#[path = "sketch_point_expr_tests.rs"]
mod point_expr_tests;
#[cfg(test)]
#[path = "sketch_redundant_tests.rs"]
mod redundant_tests;
#[cfg(test)]
#[path = "sketch_symmetry_point_tests.rs"]
mod symmetry_point_tests;
