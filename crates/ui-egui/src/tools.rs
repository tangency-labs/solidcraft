//! Interactive sketch tools: click points on the sketch plane (or pick sketch entities) and the
//! tool runs the matching command. Tools are UI state only; the work is done by commands.

use egui::{Pos2, Stroke};
use serde_json::{Value, json};
use solidcore_record::Vec2;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::tool_modifiers::{Audit, Binding, Match, ModAction, ModifierCombo, Scope, ToolModifiers, Trigger};
use crate::viewport::{Hit, Proj, pick_for_tool as pick, sketch_point_at};
use solidcraft_ui_text::tr;

const DRAW_SCOPES: &[Scope] = &[Scope::SketchDraw, Scope::SketchDrag];

/// Rectangles: Shift forces a square (`SquareRectangle`). Shift does not snap to a midpoint here
/// (an empty binding overrides the sketch scopes' one).
const SQUARE_BINDINGS: &[Binding] = &[
    Binding {
        action: ModAction::SquareRectangle,
        defaults: &[ModifierCombo::SHIFT],
        matching: Match::AtLeast,
        trigger: Trigger::Live,
        rebindable: true,
    },
    Binding { action: ModAction::TargetMidpoint, defaults: &[], matching: Match::AtLeast, trigger: Trigger::Live, rebindable: true },
];

/// Per-tool modifier descriptors owned by this host file (coordinator seeds / audits these).
pub const MODIFIERS: &[ToolModifiers] = &[
    ToolModifiers { tool: "sketch.line", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.rectangle.two_point", scopes: DRAW_SCOPES, bindings: SQUARE_BINDINGS, audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.rectangle.center", scopes: DRAW_SCOPES, bindings: SQUARE_BINDINGS, audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.rectangle.rounded", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.rectangle.three_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.rectangle.edge_center", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.circle.center", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.circle.two_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.circle.three_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.arc.three_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.arc.center_point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.polygon.inscribed", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.polygon.circumscribed", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.polygon.edge", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.slot.center_to_center", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.slot.overall", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.point", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    ToolModifiers { tool: "sketch.dimension", scopes: DRAW_SCOPES, bindings: &[], audit: Audit::Implemented },
    // Constraint tools: pick-only; no tool-specific modifier chords.
    ToolModifiers { tool: "sketch.constraint.horizontal_vertical", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.horizontal", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.vertical", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.fix", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.coincident", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.tangent", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.equal", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.parallel", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.perpendicular", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.concentric", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.collinear", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.midpoint", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
    ToolModifiers { tool: "sketch.constraint.symmetry", scopes: &[], bindings: &[], audit: Audit::VerifiedNoOp },
];

#[cfg(test)]
#[path = "sketch_display_preview_tests.rs"]
mod display_preview_tests;

#[cfg(test)]
#[path = "sketch_neutral_click_tests.rs"]
mod neutral_click_tests;

#[cfg(test)]
#[path = "symmetry_pick_tests.rs"]
mod symmetry_pick_tests;

#[cfg(test)]
#[path = "sketch_centre_align_ui_tests.rs"]
mod centre_align_ui_tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Click points; `n` points complete the shape (0 = chain until finished).
    Draw(usize),
    /// Pick sketch entities; `n` picks run the command.
    Pick(usize),
    /// Pick one straight line, or two distinct points to align: Horizontal and Vertical (and the
    /// combined command, which only agents and scripts start).
    HorizontalVertical,
    Dimension,
    /// A tool from `sketch_tools` (it handles its own clicks).
    Ext,
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub cmd: &'static str,
    pub kind: Kind,
    /// Clicked points (sketch coordinates) and the sketch point they snapped to.
    pub pts: Vec<(Vec2, Option<String>)>,
    pub hover: Option<(Vec2, Option<String>)>,
    /// Picked entity ids.
    pub picks: Vec<String>,
    /// Horizontal/Vertical point references, or the Dimension tool's midpoint snaps.
    pub point_picks: Vec<Value>,
    /// Inline dimension boxes for the shape being drawn, the point count they were made for,
    /// and whether the first box should take the keyboard.
    pub dims: Vec<crate::sketch_dims::DimBox>,
    /// Locked inputs from earlier stages of this shape (arc radius, slot length).
    pub dim_history: Vec<crate::sketch_dims::DimBox>,
    pub dims_stage: usize,
    pub dims_focus: bool,
    /// How far (radians, signed) the cursor has gone round a centre-point arc's centre since its
    /// start was placed: its sign is the way the arc runs (#6645).
    pub arc_turn: f64,
    /// Where each point was placed and the sketch points the snap lined it up with (#7828):
    /// constrained to them once the shape is made.
    pub lined: Vec<(Vec2, Vec<crate::sketch_tools::Lined>)>,
    /// The Line tool's chain, which a rest on its last point switches to tangent arcs and back
    /// (#9489).
    pub chain: crate::sketch_line_arc::LineChain,
    /// A construction line tool's line type (#10258, #15066), numbered as the Sketch Palette's
    /// Line Type icons: 0 normal, 1 construction, 2 centerline, 3 infinite. Infinite draws an
    /// infinite construction line in one click, or a construction segment once a length is
    /// typed; the others always draw a segment. A tool starts at the type last chosen, so
    /// switching between the construction line methods keeps it.
    pub line_type: u8,
    /// A construction line tool works as before #15066 (`construction_line_classic`): the
    /// Construction/Normal switch under the Length box sets `line_type`, and only a typed length
    /// makes a segment. Kept in step with the preference while the tool runs.
    pub classic: bool,
    /// The 3-Point Arc's tangent mode is on (`sketch_arc_tangent_snap`, #20719): its third point
    /// has Radius and Sweep boxes and a centre cross. Kept in step with the preference while the
    /// tool runs; off, the tool shows only the pointer's coordinates, as before.
    pub arc_modes: bool,
    /// True when the constraint/dimension tool was entered from a valid preselection: once that
    /// single operation commits, the tool unselects itself (`app.tool = None`).
    pub preselected: bool,
    /// Signals `on_click` not to re-store the tool (used by `Kind::Ext` entity constraints).
    pub finished: bool,
}

impl Tool {
    pub fn for_command(id: &str) -> Option<Tool> {
        let (cmd, kind): (&'static str, Kind) = match id {
            "sketch.line" => ("sketch.line", Kind::Draw(0)),
            "sketch.rectangle.two_point" => ("sketch.rectangle.two_point", Kind::Draw(2)),
            "sketch.rectangle.center" => ("sketch.rectangle.center", Kind::Draw(2)),
            "sketch.rectangle.rounded" => ("sketch.rectangle.rounded", Kind::Draw(2)),
            "sketch.rectangle.three_point" => ("sketch.rectangle.three_point", Kind::Draw(3)),
            "sketch.rectangle.edge_center" => ("sketch.rectangle.edge_center", Kind::Draw(3)),
            "sketch.circle.center" => ("sketch.circle.center", Kind::Draw(2)),
            "sketch.circle.two_point" => ("sketch.circle.two_point", Kind::Draw(2)),
            "sketch.circle.three_point" => ("sketch.circle.three_point", Kind::Draw(3)),
            "sketch.arc.three_point" => ("sketch.arc.three_point", Kind::Draw(3)),
            "sketch.arc3d" => ("sketch.arc3d", Kind::Draw(3)),
            "sketch.line3d" => ("sketch.line3d", Kind::Draw(0)),
            "sketch.spline3d" => ("sketch.spline3d", Kind::Draw(0)),
            "sketch.spline3d.control_point" => ("sketch.spline3d.control_point", Kind::Draw(0)),
            "sketch.point3d" => ("sketch.point3d", Kind::Draw(1)),
            "sketch.arc.center_point" => ("sketch.arc.center_point", Kind::Draw(3)),
            "sketch.polygon.inscribed" => ("sketch.polygon.inscribed", Kind::Draw(2)),
            "sketch.polygon.circumscribed" => ("sketch.polygon.circumscribed", Kind::Draw(2)),
            "sketch.polygon.edge" => ("sketch.polygon.edge", Kind::Draw(2)),
            "sketch.slot.center_to_center" => ("sketch.slot.center_to_center", Kind::Draw(3)),
            "sketch.slot.overall" => ("sketch.slot.overall", Kind::Draw(3)),
            "sketch.point" => ("sketch.point", Kind::Draw(1)),
            "sketch.dimension" => ("sketch.dimension", Kind::Dimension),
            "sketch.constraint.horizontal_vertical" => ("sketch.constraint.horizontal_vertical", Kind::HorizontalVertical),
            "sketch.constraint.horizontal" => ("sketch.constraint.horizontal", Kind::HorizontalVertical),
            "sketch.constraint.vertical" => ("sketch.constraint.vertical", Kind::HorizontalVertical),
            "sketch.constraint.fix" => ("sketch.constraint.fix", Kind::Pick(1)),
            "sketch.constraint.coincident" => ("sketch.constraint.coincident", Kind::Pick(2)),
            "sketch.constraint.tangent" => ("sketch.constraint.tangent", Kind::Pick(2)),
            "sketch.constraint.equal" => ("sketch.constraint.equal", Kind::Pick(2)),
            "sketch.constraint.parallel" => ("sketch.constraint.parallel", Kind::Pick(2)),
            "sketch.constraint.perpendicular" => ("sketch.constraint.perpendicular", Kind::Pick(2)),
            "sketch.constraint.concentric" => ("sketch.constraint.concentric", Kind::Pick(2)),
            "sketch.constraint.collinear" => ("sketch.constraint.collinear", Kind::Pick(2)),
            "sketch.constraint.midpoint" => ("sketch.constraint.midpoint", Kind::Pick(2)),
            "sketch.constraint.symmetry" => ("sketch.constraint.symmetry", Kind::Pick(3)),
            _ => return crate::sketch_tools::tool_for(id),
        };
        Some(Tool {
            cmd,
            kind,
            pts: Vec::new(),
            hover: None,
            picks: Vec::new(),
            point_picks: Vec::new(),
            dims: Vec::new(),
            dim_history: Vec::new(),
            dims_stage: usize::MAX,
            dims_focus: false,
            arc_turn: 0.0,
            lined: Vec::new(),
            chain: Default::default(),
            line_type: 3,
            classic: false,
            arc_modes: false,
            preselected: false,
            finished: false,
        })
    }
}

pub fn hint(id: &str) -> String {
    match id {
        "sketch.line" => tr("sketch.hint.line").into(),
        "sketch.rectangle.two_point" => tr("sketch.hint.rectangle.two_point").into(),
        "sketch.rectangle.center" => tr("sketch.hint.rectangle.center").into(),
        "sketch.rectangle.rounded" => tr("sketch.hint.rectangle.rounded").into(),
        "sketch.rectangle.three_point" => tr("sketch.hint.rectangle.three_point").into(),
        "sketch.rectangle.edge_center" => tr("sketch.hint.rectangle.edge_center").into(),
        "sketch.circle.center" => tr("sketch.hint.circle.center").into(),
        "sketch.arc.three_point" => tr("sketch.hint.arc.three_point").into(),
        "sketch.arc3d" => tr("sketch.hint.arc3d").into(),
        "sketch.line3d" => tr("sketch.hint.line3d").into(),
        "sketch.spline3d" => tr("sketch.hint.spline3d").into(),
        "sketch.spline3d.control_point" => tr("sketch.hint.spline3d.control_point").into(),
        "sketch.point3d" => tr("sketch.hint.point3d").into(),
        "sketch.arc.center_point" => tr("sketch.hint.arc.center_point").into(),
        "sketch.dimension" => tr("sketch.hint.dimension").into(),
        "sketch.constraint.horizontal_vertical" => tr("sketch.hint.constraint.horizontal_vertical").into(),
        "sketch.constraint.horizontal" => tr("sketch.hint.constraint.horizontal").into(),
        "sketch.constraint.vertical" => tr("sketch.hint.constraint.vertical").into(),
        "sketch.slot.center_to_center" | "sketch.slot.overall" => tr("sketch.hint.slot").into(),
        "sketch.constraint.midpoint" => tr("sketch.hint.constraint.midpoint").into(),
        _ if id.starts_with("sketch.constraint.") => tr("sketch.hint.constraint.generic").into(),
        _ => crate::sketch_tools::hint(id).unwrap_or_else(|| tr("sketch.hint.fallback").into()),
    }
}

fn arg(p: &(Vec2, Option<String>)) -> Value {
    match &p.1 {
        Some(id) => json!(id),
        None => json!([p.0.x, p.0.y]),
    }
}

fn xy(p: &(Vec2, Option<String>)) -> Value {
    json!([p.0.x, p.0.y])
}

/// A clicked point that is held where it snapped: a midpoint, on-curve, crossing or body-vertex
/// snap goes as itself, so the command keeps the point there. A snap to an existing point goes as
/// its place, where the command's `connect` joins the two with a Coincident the sketch shows.
/// So does every snap in a drawing sheet's sketch, where the commands take no snap yet. A snap
/// whose curve the Undo button took away does not reach this function as a snap: `run_shape`
/// passes every stored click through `standing` first, which turns it into the place clicked.
fn held(app: &SolveApp, p: &(Vec2, Option<String>)) -> Value {
    match &p.1 {
        Some(id) if solidcraft_engine::cmd::SNAP_PREFIXES.iter().any(|k| id.starts_with(k)) && snap_stands(app, id, p.0) == Some(true) => json!(id),
        _ => xy(p),
    }
}

/// Whether the snap `id` of a click at `at` still names a point, curve or crossing of the sketch
/// being edited. None when there is no such sketch to look in, as in a drawing sheet's sketch.
fn snap_stands(app: &SolveApp, id: &str, at: Vec2) -> Option<bool> {
    if app.session.sheet_sketching() {
        return None;
    }
    let st = crate::open_meshing::session_world(&app.session);
    let ss = app.session.active_sketch.and_then(|sid| st.sketch(sid))?;
    Some(crate::sketch_line_arc::chain_point(&ss.sketch, id, at).is_some())
}

/// A stored click as a shape sends it: a snap whose point or curve the Undo button took away
/// after the click (it leaves the tool on) goes as the place clicked, so the command does not
/// refuse click after click (#21835), as the Line's chain re-checks its point (#10016). It is
/// checked when the shape is sent, so a Redo that brings the curve back brings the snap back.
fn standing(app: &SolveApp, p: &(Vec2, Option<String>)) -> (Vec2, Option<String>) {
    match &p.1 {
        Some(id) if snap_stands(app, id, p.0) == Some(false) => (p.0, None),
        _ => p.clone(),
    }
}

/// Shift exposes a line midpoint as a point target, only in the alignment tool.
/// How near (screen pixels) a Coincident pick on a model edge must be to the edge's middle to
/// take that middle, as near as a sketch line's middle is offered.
const EDGE_MIDDLE_PX: f32 = 8.0;

/// How near (screen pixels) the cursor must be to a sketch line's middle for the alignment tool
/// to take that middle as the point it aligns, as near as the idle viewport offers that middle
/// (#20442).
const LINE_MIDDLE_PX: f32 = 8.0;

/// The middle of the straight line among `hits` whose middle is within `radius` (screen pixels)
/// of the cursor.
fn midpoint_of(app: &SolveApp, proj: &Proj, hits: &[Hit], pos: Pos2, radius: f32) -> Option<(Vec2, String)> {
    let sid = app.session.active_sketch?;
    let state = crate::open_meshing::session_world(&app.session);
    let ss = state.sketch(sid)?;
    hits.iter().find_map(|hit| {
        let Hit::SketchCurve { sketch, id, straight: true, .. } = hit else { return None };
        if *sketch != sid {
            return None;
        }
        let l = ss.sketch.curve_index(id)?;
        let solidcraft_engine::sketch::Shape::Line { a, b } = ss.sketch.shape(l)? else { return None };
        let mid = (a + b) * 0.5;
        let screen = proj.to_screen(ss.plane.to_world(mid))?;
        (screen.distance(pos) <= radius).then_some((mid, id.clone()))
    })
}

/// The active sketch's closest point or curve under the cursor.
fn active_hit<'a>(hits: &'a [Hit], app: &SolveApp) -> Option<&'a Hit> {
    hits.iter().find(|hit| match hit {
        Hit::SketchPoint { sketch, .. } | Hit::SketchCurve { sketch, .. } => Some(*sketch) == app.session.active_sketch,
        _ => false,
    })
}

/// Whether the rectangle `t` is about to be drawn is held to a square.
fn squared_rectangle(t: &Tool) -> bool {
    crate::tool_modifiers::square_rectangle() && matches!(t.cmd, "sketch.rectangle.two_point" | "sketch.rectangle.center")
}

/// The corner of a rectangle (two-point or centre) held to a square while the square chord is
/// down: the side is the larger of the two distances from the first point, in the direction the
/// cursor went. A corner the square moved loses its snap, which was for where the pointer was. A
/// corner already square to the first point stays where it was clicked and keeps its snap.
fn squared(t: &Tool, p: (Vec2, Option<String>)) -> (Vec2, Option<String>) {
    let rectangle = matches!(t.cmd, "sketch.rectangle.two_point" | "sketch.rectangle.center");
    let (true, true, [first]) = (rectangle, crate::tool_modifiers::square_rectangle(), t.pts.as_slice()) else { return p };
    let d = p.0 - first.0;
    let side = d.x.abs().max(d.y.abs());
    let corner =
        Vec2::new(first.0.x + side.copysign(if d.x == 0.0 { 1.0 } else { d.x }), first.0.y + side.copysign(if d.y == 0.0 { 1.0 } else { d.y }));
    if corner == p.0 { p } else { (corner, None) }
}

/// Whether `t` is an interactive constraint-picking or dimensioning tool (suppresses
/// drawing-time auto-snaps, green cursor rings, and readouts while picking existing geometry).
pub(crate) fn is_constraint_pick_tool(t: &Tool) -> bool {
    matches!(t.kind, Kind::Pick(_) | Kind::HorizontalVertical | Kind::Dimension)
        || (t.kind == Kind::Ext && crate::sketch_tools::is_entity_constraint_tool(t.cmd))
}

/// The constraint preview icon to show beside the cursor while a constraint or dimension tool runs.
/// Returns `None` while the inline Dimension value editor owns the cursor.
pub(crate) fn constraint_cursor_icon(app: &SolveApp, tool: &Tool, proj: &Proj, pos: Pos2, shift: bool) -> Option<&'static str> {
    if !is_constraint_pick_tool(tool) {
        return None;
    }
    if tool.kind == Kind::Dimension && crate::dim_view::editing() {
        return None;
    }
    // Horizontal and Vertical go one way only and show their own icon.
    if tool.kind == Kind::HorizontalVertical && axis_of_tool(tool.cmd).is_none() {
        return Some(horizontal_vertical_cursor_icon(app, tool, proj, pos, shift));
    }
    solidcraft_engine::find_command(tool.cmd).map(|spec| spec.icon).filter(|icon| !icon.is_empty())
}

fn horizontal_vertical_cursor_icon(app: &SolveApp, tool: &Tool, proj: &Proj, pos: Pos2, shift: bool) -> &'static str {
    let Some(sid) = app.session.active_sketch else { return "c_hv" };
    let state = crate::open_meshing::session_world(&app.session);
    let Some(ss) = state.sketch(sid) else { return "c_hv" };
    let hits = pick(app, proj, pos);
    let hit = active_hit(&hits, app);
    let picked = !tool.point_picks.is_empty();
    let midpoint = if shift {
        midpoint_of(app, proj, &hits, pos, 12.0)
    } else if picked && matches!(hit, Some(Hit::SketchCurve { straight: true, .. })) {
        midpoint_of(app, proj, &hits, pos, LINE_MIDDLE_PX)
    } else {
        None
    };
    let axis_icon = |mode: &str| if mode == "horizontal" { "c_horizontal" } else { "c_vertical" };
    if let Some(first) = tool.point_picks.first() {
        let first_pos = first
            .get("midpoint")
            .and_then(Value::as_str)
            .and_then(|line| ss.sketch.curve_index(line))
            .and_then(|ci| crate::selection::line_midpoint(&ss.sketch, ci))
            .or_else(|| first.as_str().and_then(|id| ss.sketch.resolve_point(id)).and_then(|pi| ss.sketch.point(pi)));
        let hovered_point = midpoint.as_ref().map(|(p, id)| (json!({"midpoint": id}), *p)).or_else(|| match hit {
            Some(Hit::SketchPoint { id, .. }) => ss.sketch.resolve_point(id).and_then(|pi| ss.sketch.point(pi)).map(|p| (json!(id), p)),
            _ => None,
        });
        if let (Some(first_pos), Some((hover_ref, hover_pos))) = (first_pos, hovered_point)
            && &hover_ref != first
        {
            return axis_icon(solidcraft_engine::sketch::axis_mode_between(first_pos, hover_pos));
        }
        if !shift
            && let Some(Hit::SketchCurve { id, straight: true, .. }) = hit
            && let Some(mode) = line_axis_mode(&ss.sketch, id)
        {
            return axis_icon(mode);
        }
    } else if !shift
        && midpoint.is_none()
        && let Some(Hit::SketchCurve { id, straight: true, .. }) = hit
        && let Some(mode) = line_axis_mode(&ss.sketch, id)
    {
        return axis_icon(mode);
    }
    "c_hv"
}

pub fn on_hover(app: &mut SolveApp, proj: &Proj, pos: Pos2, shift: bool) {
    crate::sketch3d::on_hover(app, proj, pos);
    if let Some(mut tool) = app.tool.take() {
        crate::sketch_line_arc::rejoin(app, &mut tool);
        app.tool = Some(tool);
    }
    // The alignment tool takes a line's middle as a point: with Shift held, from anywhere on the
    // line; with a point already picked, where the cursor is on the middle (#20442). With no
    // point picked the cursor takes the line, so a line picked on its own is still made
    // horizontal or vertical.
    let alignment = app.tool.as_ref().is_some_and(|t| t.kind == Kind::HorizontalVertical);
    let picked = app.tool.as_ref().is_some_and(|t| t.kind == Kind::HorizontalVertical && !t.point_picks.is_empty());
    let want_middle = alignment && (shift || picked);
    let hits = if want_middle { pick(app, proj, pos) } else { Vec::new() };
    let midpoint = if !want_middle {
        None
    } else if shift {
        midpoint_of(app, proj, &hits, pos, f32::INFINITY)
    } else if matches!(active_hit(&hits, app), Some(Hit::SketchCurve { straight: true, .. })) {
        midpoint_of(app, proj, &hits, pos, LINE_MIDDLE_PX)
    } else {
        None
    };
    let constraint_pick = app.tool.as_ref().is_some_and(is_constraint_pick_tool);
    let dimension_tool = app.tool.as_ref().is_some_and(|t| t.kind == Kind::Dimension);
    let axis_tool = app.tool.as_ref().is_some_and(|t| axis_of_tool(t.cmd).is_some());
    let h = if let Some((point, _)) = midpoint {
        crate::sketch_tools::clear_snap();
        Some((point, Some("Midpoint".into())))
    } else if constraint_pick {
        crate::sketch_tools::clear_snap();
        // Dimension still tracks the raw plane point so live previews/drags follow the pointer, and
        // so do Horizontal and Vertical, for their guide.
        (dimension_tool || axis_tool).then(|| crate::viewport::sketch_plane_cursor(app, proj, pos).map(|p| (p, None))).flatten()
    } else if let Some(point) = app.tool.as_ref().and_then(|tool| crate::sketch_tools::periodic_start_snap(app, tool, proj, pos)) {
        Some((point, Some("Close smooth loop".into())))
    } else {
        crate::sketch_tools::tool_point_at(app, proj, pos, app.tool.as_ref().map(|t| t.cmd))
    };
    let follow = app.preferences.center_arc_follows_cursor;
    if let Some(t) = app.tool.as_mut() {
        t.hover = h.map(|h| squared(t, h));
        if follow {
            follow_arc_turn(t);
        } else {
            t.arc_turn = 0.0;
        }
    }
}

/// Add the cursor's latest step round a centre-point arc's centre to the turn it has made since
/// the start was placed, a step never more than half a turn. A full turn back to the start wraps.
fn follow_arc_turn(t: &mut Tool) {
    let (Some(c), Some(a), Some(h)) = (t.pts.first(), t.pts.get(1), t.hover.as_ref()) else {
        t.arc_turn = 0.0;
        return;
    };
    if t.cmd != "sketch.arc.center_point" || t.pts.len() != 2 {
        t.arc_turn = 0.0;
        return;
    }
    let (c, a, h) = (c.0, a.0, h.0);
    if h.dist(c) <= 1e-9 || a.dist(c) <= 1e-9 {
        return;
    }
    let at = (a - c).angle() + t.arc_turn;
    let step = ((h - c).angle() - at + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
    if step.is_finite() {
        t.arc_turn = (t.arc_turn + step) % std::f64::consts::TAU;
    }
}

/// A centre-point arc's sweep from its start to `end` (radians): counter-clockwise, unless the
/// cursor went round the centre clockwise from the start (#6645).
pub(crate) fn center_arc_sweep(t: &Tool, end: Vec2) -> f64 {
    let (Some(c), Some(a)) = (t.pts.first(), t.pts.get(1)) else { return 0.0 };
    let ccw = ((end - c.0).angle() - (a.0 - c.0).angle()).rem_euclid(std::f64::consts::TAU);
    if t.arc_turn < 0.0 && ccw > 0.0 { ccw - std::f64::consts::TAU } else { ccw }
}

pub fn on_click(app: &mut SolveApp, proj: &Proj, pos: Pos2, shift: bool) {
    // A click away dismisses the inline editor; it must not also pick the first entity of
    // the next dimension before that cancellation has been processed. The type-first value
    // of the Dimension tool takes clicks itself (a second pick, or where its text goes).
    if crate::dim_view::editing() && crate::dim_view::live().is_none() {
        return;
    }
    let Some(mut tool) = app.tool.take() else { return };
    // In a 3D sketch the drawing tools take points off the plane.
    if crate::sketch3d::on_click(app, &mut tool, proj, pos) {
        app.tool = Some(tool);
        return;
    }
    match tool.kind {
        Kind::Draw(n) => {
            if let Some(p) = sketch_point_at(app, proj, pos) {
                // Typed values win over the cursor; lines snap to horizontal/vertical.
                let p = crate::sketch_dims::placed(app, &tool, &squared(&tool, p));
                // A repeated initial point supplies no extent. Keep waiting rather than
                // attempting a degenerate shape; explicit typed values still validate.
                if n != 1 && tool.pts.len() == 1 && tool.pts.first().is_some_and(|first| first.0 == p.0) && !tool.dims.iter().any(|b| b.locked) {
                    app.tool = Some(tool);
                    return;
                }
                app.tool = Some(tool);
                place(app, p);
                return;
            }
        }
        Kind::HorizontalVertical => {
            let hits = pick(app, proj, pos);
            let hit = active_hit(&hits, app);
            // A line's middle is a point the tool takes, the way it takes the line's ends: with
            // Shift held, from anywhere on the line; with a point already picked, where the
            // cursor is on the middle (#20442). With no point picked the click takes the line,
            // so a line picked on its own is still made horizontal or vertical.
            let picked = !tool.point_picks.is_empty();
            let midpoint = if shift {
                midpoint_of(app, proj, &hits, pos, 12.0)
            } else if picked && matches!(hit, Some(Hit::SketchCurve { straight: true, .. })) {
                midpoint_of(app, proj, &hits, pos, LINE_MIDDLE_PX)
            } else {
                None
            };
            let point = midpoint.map(|(_, id)| json!({"midpoint": id})).or_else(|| match hit {
                Some(Hit::SketchPoint { id, .. }) => Some(json!(id)),
                _ => None,
            });
            if let Some(point) = point {
                if let Some(first) = tool.point_picks.first() {
                    if first != &point && app.run(tool.cmd, json!({"points": [first, point]})).is_ok() {
                        tool.point_picks.clear();
                        if tool.preselected {
                            return;
                        }
                    }
                } else {
                    tool.point_picks.push(point);
                }
            } else if !shift && let Some(Hit::SketchCurve { id, straight: true, .. }) = hit {
                // A line picked on its own is made horizontal/vertical; a line picked after a
                // point aligns that point with the line (a circle's centre with a square's
                // edge, #617).
                let first = tool.point_picks.first().cloned();
                let params = match first {
                    None => Some(json!({"line": id})),
                    // Horizontal and Vertical on their own say which way: no `mode` is sent.
                    Some(point) if axis_of_tool(tool.cmd).is_some() => {
                        let state = crate::open_meshing::session_world(&app.session);
                        app.session
                            .active_sketch
                            .and_then(|sid| state.sketch(sid))
                            .and_then(|ss| point_line_points(&ss.sketch, &point, id))
                            .map(|points| json!({"points": points}))
                    }
                    Some(point) => {
                        let state = crate::open_meshing::session_world(&app.session);
                        app.session.active_sketch.and_then(|sid| state.sketch(sid)).and_then(|ss| point_line_alignment(&ss.sketch, &point, id))
                    }
                };
                if let Some(params) = params
                    && app.run(tool.cmd, params).is_ok()
                {
                    tool.point_picks.clear();
                    if tool.preselected {
                        return;
                    }
                }
            }
        }
        Kind::Pick(n) => {
            let hits = curve_operands(tool.cmd, pick(app, proj, pos));
            let entity = if app.ui.project_link_tools {
                active_entity(app, &hits).map(|(id, _)| id).or_else(|| {
                    hits.iter().find_map(|h| match h {
                        Hit::Axis { name } => axis_pick(app, &tool, name),
                        _ => None,
                    })
                })
            } else {
                // Keep main's hit order unless the enhanced projection workflow is enabled.
                hits.iter().find_map(|h| match h {
                    Hit::SketchPoint { id, .. } | Hit::SketchCurve { id, .. } => Some(id.clone()),
                    Hit::Axis { name } => axis_pick(app, &tool, name),
                    _ => None,
                })
            };
            // Fix never projects model or foreign-sketch geometry.
            let projects = tool.cmd != "sketch.constraint.fix";
            // With nothing of the sketch there, a body's edge or vertex is picked (kept as its
            // reference): the command projects it, so any body can be constrained to, a mirrored
            // copy too (#6348). Another sketch's curve is projected here first ([`project_pick`]).
            // Coincident at an edge's middle takes that middle (#17920), as at a sketch line's.
            let model = || {
                hits.iter()
                    .filter(|h| projects && matches!(h, Hit::Edge { .. } | Hit::Vertex { .. }))
                    .find_map(|h| {
                        let r = crate::sketch_tools::model_ref(app, h)?;
                        let middle = matches!(h, Hit::Edge { mid, .. } if tool.cmd == "sketch.constraint.coincident"
                            && proj.to_screen(*mid).is_some_and(|at| at.distance(pos) <= EDGE_MIDDLE_PX));
                        Some(if middle { json!({"midpoint": r}) } else { r })
                    })
                    .map(|r| r.to_string())
            };
            if let Some(id) = entity
                .or_else(model)
                .or_else(|| if projects && app.ui.project_link_tools { project_pick(app, &hits).map(|(id, _)| id) } else { None })
                && !tool.picks.contains(&id)
            {
                tool.picks.push(id);
            }
            if tool.picks.len() >= n {
                let applied = run_pick(app, &tool);
                if applied && tool.preselected {
                    return;
                }
                if !applied && tool.preselected {
                    tool.picks.pop();
                } else {
                    tool.picks.clear();
                }
            }
        }
        Kind::Ext => {
            crate::sketch_tools::on_click(app, &mut tool, proj, pos);
            if crate::sketch_tools::take_ended() {
                return;
            }
        }
        Kind::Dimension => {
            let hits = pick(app, proj, pos);
            // The snap identifies the intended line even when another (projected) edge
            // overlaps it and wins the ordinary curve hit. Both picks can be midpoints.
            let snapped = sketch_point_at(app, proj, pos);
            crate::sketch_tools::clear_snap();
            let midpoint = snapped.and_then(|(_, snap)| snap).and_then(|id| {
                if id.starts_with("mid:") { return Some(id); }
                // An earlier dimension may already have materialized this midpoint.
                // Picking that point retains the same midpoint intent on subsequent dimensions.
                let state = crate::open_meshing::session_world(&app.session);
                let ss = app.session.active_sketch.and_then(|sid| state.sketch(sid))?;
                let point = ss.sketch.resolve_point(&id)?;
                ss.sketch.constraints.iter().find_map(|c| match c.kind {
                    solidcraft_engine::sketch::ConstraintKind::Midpoint { p, l } if !c.driven && p == point => {
                        ss.sketch.curves.get(l).map(|c| format!("mid:{}", c.id))
                    }
                    _ => None,
                })
            }).filter(|id| {
                let Some(line) = id.strip_prefix("mid:") else { return false };
                let state = crate::open_meshing::session_world(&app.session);
                app.session
                    .active_sketch
                    .and_then(|sid| state.sketch(sid))
                    .is_some_and(|ss| {
                        ss.sketch.curve_index(line).is_some_and(|l| solidcraft_engine::view::sketch_curve_visible(&ss.sketch, l))
                            // A circle/arc and a line still measure their edge-to-edge gap.
                            && tool.picks.first().is_none_or(|first| {
                                ss.sketch.curve_index(first).is_none_or(|l| matches!(ss.sketch.shape(l), Some(solidcraft_engine::sketch::Shape::Line { .. })))
                            })
                    })
            });
            let hit = midpoint
                .as_deref()
                .and_then(|id| id.strip_prefix("mid:"))
                .map(|id| (id.to_string(), true, true))
                .or_else(|| {
                    hits.iter().find_map(|h| match h {
                        Hit::SketchPoint { sketch, id, .. } if Some(*sketch) == app.session.active_sketch => Some((id.clone(), false, true)),
                        Hit::SketchCurve { sketch, id, straight, .. } if Some(*sketch) == app.session.active_sketch => {
                            Some((id.clone(), true, *straight))
                        }
                        Hit::Axis { name } => sketch_axis_of(app, name).map(|a| (a.to_string(), true, true)),
                        _ => None,
                    })
                })
                .or_else(|| {
                    // With nothing of the sketch under the cursor, a body's edge or vertex may be
                    // measured too (#7912): the command projects it into the sketch. An edge is a
                    // length of its own; a vertex waits for the entity it is measured against.
                    hits.iter().find_map(|h| match h {
                        Hit::Edge { .. } => crate::sketch_tools::model_ref(app, h).map(|r| (r.to_string(), true, false)),
                        Hit::Vertex { .. } => crate::sketch_tools::model_ref(app, h).map(|r| (r.to_string(), false, false)),
                        // An origin axis is projected too; it has no length of its own, so it
                        // waits for the entity measured to it (a line's distance to the axis).
                        // Only while the dimension takes another pick: once it has two, a click
                        // on an axis places the dimension as a click on empty space does.
                        Hit::Axis { name } if matches!(name.as_str(), "X" | "Y" | "Z") && tool.picks.len() < 2 => {
                            Some((json!({"axis": name}).to_string(), false, false))
                        }
                        _ => None,
                    })
                });
            let fresh = |tool: &Tool, id: &String| !tool.picks.contains(id) && tool.picks.len() < 2;
            // Before a new dimension is under way, a click on an existing one edits its value;
            // the tool stays on for the next dimension.
            if crate::dim_view::live().is_none()
                && tool.picks.is_empty()
                && !app.ui.pick_off.iter().any(|k| k == "dimensions")
                && crate::dim_view::tool_click(app, pos, hit.is_some())
            {
                app.tool = Some(tool);
                return;
            }
            let typed = crate::dim_view::live();
            // A model edge or vertex (or another sketch's curve) joins as its projection, but a
            // click that only places the text of a dimension being typed projects nothing.
            let joins = app.ui.project_link_tools && typed.is_none_or(|typed| !typed) && tool.picks.len() < 2;
            let hit = hit.or_else(|| {
                let (id, curve) = if joins { project_pick(app, &hits)? } else { return None };
                let sketch = app.session.active_sketch.and_then(|sid| app.session.doc.sketch(sid).ok());
                let line = sketch
                    .and_then(|sk| sk.curve_index(&id).and_then(|l| sk.shape(l)))
                    .is_some_and(|shape| matches!(shape, solidcraft_engine::sketch::Shape::Line { .. }));
                Some((id, curve, !curve || line))
            });
            if let Some(typed) = typed {
                match hit {
                    // Before anything is typed, another entity joins the dimension, as in the
                    // place-first flow (a distance, an angle between lines).
                    Some((id, _, midpoint_target)) if !typed && fresh(&tool, &id) => {
                        let picks = dimension_picks(&tool, id, midpoint.as_deref(), midpoint_target);
                        match crate::dim_view::begin_live(app, picks.clone(), Some(pos)) {
                            Ok(live) => {
                                if !live {
                                    crate::dim_view::cancel();
                                }
                                tool.picks = picks;
                                if let Some(midpoint) = midpoint {
                                    tool.point_picks.push(json!(midpoint));
                                }
                            }
                            Err(error) => app.set_status(error, true),
                        }
                    }
                    // Otherwise the click places the text; the value keeps the keyboard.
                    _ => {
                        if let Some((at, _)) = sketch_point_at(app, proj, pos) {
                            crate::sketch_tools::clear_snap();
                            crate::dim_view::pin(app, at);
                        } else {
                            crate::sketch_tools::clear_snap();
                        }
                    }
                }
            } else if let Some((id, curve, midpoint_target)) = hit {
                if fresh(&tool, &id) {
                    tool.picks = dimension_picks(&tool, id, midpoint.as_deref(), midpoint_target);
                    if let Some(midpoint) = midpoint {
                        tool.point_picks.push(json!(midpoint));
                    }
                    // As soon as the picks determine a dimension (one line, circle or arc, or a
                    // second entity), its value takes the keyboard: D, click, type. A pick that
                    // doesn't keeps the place-first flow; placing reports why.
                    if curve || tool.picks.len() == 2 {
                        let _ = crate::dim_view::begin_live(app, tool.picks.clone(), Some(pos));
                    }
                }
            } else if !tool.picks.is_empty()
                && let Some((at, _)) = sketch_point_at(app, proj, pos)
            {
                crate::sketch_tools::clear_snap();
                match crate::dim_view::begin_dimension(app, tool.picks.clone(), at) {
                    Ok(()) => {
                        tool.picks.clear();
                        tool.point_picks.clear();
                        if tool.preselected && !crate::dim_view::editing() {
                            return;
                        }
                    }
                    Err(error) => app.set_status(error, true),
                }
            }
        }
    }
    if !tool.finished && !crate::sketch_text::editing(app) && !crate::sketch_tools::image_placement_finished(&tool) {
        app.tool = Some(tool);
    }
}

/// A single edge still measures its length; a second pick uses the snapped midpoints.
fn dimension_picks(tool: &Tool, id: String, midpoint: Option<&str>, midpoint_target: bool) -> Vec<String> {
    let mut picks = tool.picks.clone();
    if midpoint_target
        && picks.len() == 1
        && let Some(midpoint) = tool.point_picks.first().and_then(Value::as_str)
        && let Some(first) = picks.first_mut()
    {
        *first = midpoint.to_string();
    }
    picks.push(if picks.len() == 1 { midpoint.map(str::to_string).unwrap_or(id) } else { id });
    picks
}

/// Put down the next point of the shape being drawn; the last point runs the command (with
/// the typed values as dimension constraints).
pub fn place(app: &mut SolveApp, p: (Vec2, Option<String>)) {
    let Some(mut tool) = app.tool.take() else { return };
    crate::sketch_line_arc::rejoin(app, &mut tool);
    if tool.kind == Kind::Ext {
        crate::sketch_tools::place_point(app, &mut tool, p);
        // A placed text opens Edit Text, which ends the tool.
        if !crate::sketch_text::editing(app) {
            app.tool = Some(tool);
        }
        return;
    }
    let Kind::Draw(n) = tool.kind else {
        app.tool = Some(tool);
        return;
    };
    if let Err(error) = crate::sketch_dims::validate(app, &tool) {
        app.set_status(error, true);
        app.tool = Some(tool);
        return;
    }
    tool.lined.push((p.0, crate::sketch_tools::lined_at(p.0)));
    tool.pts.push(p);
    let before = crate::sketch_tools::point_ids(app);
    if tool.cmd == "sketch.line" {
        // A new chain's first point: nothing drawn yet to go on tangent from.
        if tool.pts.len() == 1 {
            tool.chain = Default::default();
        }
        if tool.pts.len() >= 2 {
            let (a, b) = (tool.pts[tool.pts.len() - 2].clone(), tool.pts[tool.pts.len() - 1].clone());
            let angled = crate::sketch_dims::has_locked(&tool, "Angle");
            let dimensioned = tool.dims.iter().any(|dimension| dimension.locked);
            if let Ok(v) = draw(app, &tool, json!({"points": [arg(&a), arg(&b)], "infer": !angled})) {
                let curves: Vec<String> = v["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect();
                // Chain: the next segment starts at this one's end point.
                if let Some(id) = curves.first() {
                    if angled {
                        crate::inference::clear();
                        tool.lined.clear();
                    } else {
                        // Endpoint inference is part of this drawing. Preserve its original
                        // undo history even when the history is already at the engine's cap.
                        let history = dimensioned.then(|| std::mem::take(&mut app.session.undo));
                        crate::inference::commit(app, id, b.0, b.1.as_deref());
                        crate::sketch_tools::constrain_lined(app, &mut tool, &[a.0, b.0], &before);
                        if let Some(history) = history {
                            app.session.undo = history;
                        }
                    }
                    let last = tool.pts.len() - 1;
                    tool.pts[last].1 = Some(format!("{id}.end"));
                    // The chain needs only its current endpoint, even after many segments.
                    tool.pts = vec![tool.pts[last].clone()];
                    tool.chain.placed(id, a.clone());
                }
                crate::sketch_dims::reset(&mut tool);
            } else {
                tool.pts.pop();
                tool.lined.pop();
            }
        }
    } else if n > 0 && tool.pts.len() >= n {
        // Typed sizes win over the square: with both a width and a height typed they define the
        // rectangle, and an Equal on its sides would fight them.
        let square = squared_rectangle(&tool) && !(crate::sketch_dims::has_locked(&tool, "Width") && crate::sketch_dims::has_locked(&tool, "Height"));
        match run_shape(app, &tool) {
            Ok(curves) => {
                // One side equal to the next: a single dimension then defines the square.
                if square && let [a, b, ..] = curves.as_slice() {
                    let history = std::mem::take(&mut app.session.undo);
                    let made = app.run("sketch.constraint.equal", json!({"curves": [a, b]}));
                    app.session.undo = history;
                    if let Err(error) = made {
                        app.set_status(error, true);
                    }
                }
                let placed: Vec<Vec2> = tool.pts.iter().map(|p| p.0).collect();
                // A 3-Point Arc whose third point was the tangent snap is tangent to the curves it
                // starts and ends on (#20719).
                if tool.cmd == "sketch.arc.three_point"
                    && let [start, end, through] = placed[..]
                    && let Some(arc) = curves.first()
                {
                    crate::inference::commit_arc(app, arc, start, end, through);
                }
                crate::sketch_tools::constrain_lined(app, &mut tool, &placed, &before);
                tool.pts.clear();
                crate::sketch_dims::reset(&mut tool);
            }
            Err(error) => {
                app.set_status(error, true);
                tool.pts.pop();
                tool.lined.pop();
            }
        }
    }
    app.tool = Some(tool);
}

/// Geometry and typed constraints are committed atomically, as one undo step. With "Dimension
/// new shapes" on (Preferences › Sketch), every other size of a shape the engine can dimension
/// joins them at its size as drawn, and with "Also place them from the origin" the shape is then
/// placed until fully constrained; the engine's `dimension_roles` decides which shapes those are.
/// This is the one place drawing tools commit through: the 3D tools (`sketch3d`) take part by
/// committing here once their creators are listed there.
pub(crate) fn draw(app: &mut SolveApp, t: &Tool, mut params: Value) -> Result<Value, String> {
    if matches!(t.cmd, "sketch.spline.control_point" | "sketch.spline.control_point_5") {
        params["control_polygon"] = json!(true);
    }
    let dimensions = crate::sketch_dims::dimensions(t);
    let as_drawn = app.preferences.sketch_auto_dimensions && solidcraft_engine::dimension_roles(t.cmd).is_some();
    let locate = as_drawn && app.preferences.sketch_auto_locate;
    if !as_drawn && dimensions.as_object().is_none_or(|values| values.is_empty()) {
        app.run(t.cmd, params)
    } else {
        let request = json!({"command": t.cmd, "params": params, "dimensions": dimensions, "as_drawn": as_drawn, "locate": locate});
        let result = app.run("sketch.draw.dimensioned", request)?;
        // The internal wrapper has no toolbar category; apply the original creator's line
        // type while retaining the draw's undo history, including at the history limit.
        let history = std::mem::take(&mut app.session.undo);
        crate::sketch_palette::apply_mode(app, t.cmd, &result);
        app.session.undo = history;
        // A box typed `name = expression` defined the parameter `name` (#7567).
        let defined: Vec<_> = result["defined"].as_array().into_iter().flatten().filter_map(Value::as_str).map(|name| format!("`{name}`")).collect();
        match defined.as_slice() {
            [] => {}
            [name] => app.set_status(format!("Parameter {name} defined"), false),
            names => app.set_status(format!("Parameters {} defined", names.join(", ")), false),
        }
        Ok(result)
    }
}

fn run_shape(app: &mut SolveApp, t: &Tool) -> Result<Vec<String>, String> {
    let pts: Vec<_> = t.pts.iter().map(|p| standing(app, p)).collect();
    let p = &pts;
    let d = |a: usize, b: usize| p[a].0.dist(p[b].0);
    let sides = crate::sketch_dims::locked_value(app, t, "Sides").map_or(6, |v| v as usize);
    let params = match t.cmd {
        // A corner or centre clicked on a curve, a midpoint or a crossing stays there, as a
        // line's end does (#18756).
        "sketch.rectangle.two_point" => json!({"p0": held(app, &p[0]), "p1": held(app, &p[1])}),
        "sketch.rectangle.center" => json!({"center": held(app, &p[0]), "corner": held(app, &p[1])}),
        "sketch.rectangle.rounded" => {
            json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "radius": crate::sketch_dims::rounded_radius_value(app, t, p[0].0, p[1].0)})
        }
        "sketch.rectangle.three_point" => json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "p2": xy(&p[2])}),
        "sketch.rectangle.edge_center" => {
            // The second click gives the edge's direction and half-length, the third its width.
            // The middle goes as the circle's centre does: a snap to a line's midpoint, a curve or
            // a crossing as itself, so it is held there; a snap to a point as its place (#7299).
            // The edge's end is read from the click as it was made, as the preview reads it: a
            // snap whose curve Undo took away still keeps the edge off the axis nudge.
            let (a, p1) = (p[0].0, edge_end(p[0].0, &t.pts[1]));
            let d = (p1 - a).normalized().ok_or("the edge's end must differ from its middle")?;
            let corner = a + d * (p1 - a).len() + d.perp() * (p[2].0 - a).dot(d.perp());
            let anchor = match &p[0].1 {
                Some(id) if ["mid:", "on:", "vertex:", "x:"].iter().any(|k| id.starts_with(k)) => json!(id),
                _ => xy(&p[0]),
            };
            json!({"anchor": anchor, "direction": [d.x, d.y], "corner": xy(&(corner, None))})
        }
        "sketch.circle.center" => {
            // A centre snapped to an existing point goes as its place, where `connect` ties
            // them with a Coincident the sketch shows (#7299); a midpoint, on-curve, crossing or
            // body-vertex snap goes as itself, so the centre is held there. A second click
            // snapped to a point keeps the circle on that point (#3099).
            let center = match &p[0].1 {
                Some(id) if ["mid:", "on:", "vertex:", "x:"].iter().any(|k| id.starts_with(k)) => json!(id),
                _ => xy(&p[0]),
            };
            let mut v = json!({"center": center, "radius": d(0, 1)});
            if p[1].1.is_some() {
                v["through"] = arg(&p[1]);
            }
            v
        }
        "sketch.circle.two_point" => json!({"p0": xy(&p[0]), "p1": xy(&p[1])}),
        // Each click goes as its snap, as the 3-Point Arc's do: a click snapped to a point or
        // curve keeps the circle on it (#9858).
        "sketch.circle.three_point" => json!({"p0": arg(&p[0]), "p1": arg(&p[1]), "p2": arg(&p[2])}),
        "sketch.arc.three_point" => json!({"start": arg(&p[0]), "end": arg(&p[1]), "through": arg(&p[2])}),
        "sketch.arc.center_point" => {
            // A centre snapped to an existing point goes as its place, where `connect` ties
            // them with a Coincident the sketch shows (#7299); a midpoint, on-curve, crossing or
            // body-vertex snap goes as itself, so the centre is held there.
            let center = match &p[0].1 {
                Some(id) if ["mid:", "on:", "vertex:", "x:"].iter().any(|k| id.starts_with(k)) => json!(id),
                _ => xy(&p[0]),
            };
            match crate::sketch_dims::locked_value(app, t, "Sweep") {
                Some(sweep) => json!({"center": center, "start": arg(&p[1]), "sweep": sweep.to_degrees()}),
                None if center_arc_sweep(t, p[2].0) < 0.0 => {
                    json!({"center": center, "start": arg(&p[1]), "end": xy(&p[2]), "clockwise": true})
                }
                None => json!({"center": center, "start": arg(&p[1]), "end": xy(&p[2])}),
            }
        }
        "sketch.polygon.inscribed" | "sketch.polygon.circumscribed" => {
            let a = (p[1].0 - p[0].0).angle().to_degrees();
            json!({"center": xy(&p[0]), "radius": d(0, 1), "sides": sides, "angle": a})
        }
        "sketch.polygon.edge" => json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "sides": sides}),
        "sketch.slot.center_to_center" | "sketch.slot.overall" => {
            let dir = (p[1].0 - p[0].0).normalized().unwrap_or(Vec2::X);
            // Seed the creator with the typed width too: an Overall slot places its cap
            // centres from this width before the driving dimension is applied.
            let width = crate::sketch_dims::locked_value(app, t, "Width").unwrap_or_else(|| (2.0 * dir.cross(p[2].0 - p[0].0).abs()).max(1e-3));
            json!({"p0": xy(&p[0]), "p1": xy(&p[1]), "width": width})
        }
        // A midpoint, on-curve, crossing or body-vertex snap goes as itself, so the point is held
        // there (#4691, #15387); a snap to an existing point goes as its place, where `connect`
        // joins them.
        "sketch.point" => match &p[0].1 {
            Some(id) if ["mid:", "on:", "vertex:", "x:"].iter().any(|k| id.starts_with(k)) => json!({"point": id}),
            _ => json!({"point": xy(&p[0])}),
        },
        _ => return Err(format!("drawing {} is not supported", t.cmd)),
    };
    let result = draw(app, t, params)?;
    Ok(result["curves"].as_array().into_iter().flatten().filter_map(|c| c.as_str().map(str::to_string)).collect())
}

/// #617: Aligning a point with a straight line (a circle's centre with a square's edge) aligns
/// the point with the line's midpoint, in the mode the line's dominant axis suggests. None when
/// the curve is not a line.
pub(crate) fn point_line_alignment(sk: &solidcraft_engine::sketch::Sketch, point: &Value, line: &str) -> Option<Value> {
    solidcraft_engine::sketch_selection::point_line_alignment(sk, point, line)
}

/// The `points` pair aligning `point` with the midpoint of `line`; None when `line` is not a line.
pub(crate) fn point_line_points(sk: &solidcraft_engine::sketch::Sketch, point: &Value, line: &str) -> Option<[Value; 2]> {
    solidcraft_engine::sketch_selection::point_line_points(sk, point, line)
}

/// The axis a straight line is closest to: the alignment mode when a point is aligned with it (#617).
fn line_axis_mode(sk: &solidcraft_engine::sketch::Sketch, line: &str) -> Option<&'static str> {
    let solidcraft_engine::sketch::Shape::Line { a, b } = sk.shape(sk.curve_index(line)?)? else { return None };
    Some(solidcraft_engine::sketch::axis_mode_between(a, b))
}

fn is_line_curve(sketch: Option<&solidcraft_engine::sketch::Sketch>, id: &str) -> bool {
    sketch.and_then(|sk| sk.curve_index(id).and_then(|ci| sk.shape(ci))).is_some_and(|sh| matches!(sh, solidcraft_engine::sketch::Shape::Line { .. }))
}

pub(crate) fn curve_compatible_with_constraint(cmd: &str, sketch: Option<&solidcraft_engine::sketch::Sketch>, id: &str) -> bool {
    match cmd {
        "sketch.constraint.horizontal"
        | "sketch.constraint.vertical"
        | "sketch.constraint.horizontal_vertical"
        | "sketch.constraint.parallel"
        | "sketch.constraint.perpendicular"
        | "sketch.constraint.collinear"
        | "sketch.constraint.midpoint"
        | "sketch.constraint.polygon" => is_line_curve(sketch, id),
        "sketch.constraint.concentric" => sketch
            .and_then(|sk| sk.curve_index(id).and_then(|ci| sk.shape(ci)))
            .is_some_and(|sh| !matches!(sh, solidcraft_engine::sketch::Shape::Line { .. })),
        _ => true,
    }
}

/// Only consume a complete, unambiguous selection; command validation owns geometry. The rules
/// are the commands' own (`CommandSpec::for_selection`).
#[cfg(test)]
pub(crate) fn constraint_preselection(
    command: &str,
    selection: &[solidcraft_engine::Sel],
    sketch: Option<&solidcraft_engine::sketch::Sketch>,
) -> Option<Value> {
    solidcraft_engine::sketch_selection::constraint_params(command, selection, sketch)
}

/// Move a compatible partial selection into a constraint tool's pick list so the user can finish
/// by picking only the remaining operands. Returns `true` when `tool` was seeded.
pub(crate) fn seed_constraint_preselection(app: &SolveApp, tool: &mut Tool) -> bool {
    use solidcraft_engine::Sel::{Axis, SketchCurve, SketchMidpoint, SketchPoint};
    let selection = &app.session.selection;
    if selection.is_empty() {
        return false;
    }
    let active = app.session.active_sketch;
    let state = crate::open_meshing::session_world(&app.session);
    let sketch = active.and_then(|s| state.sketch(s)).map(|ss| &ss.sketch);
    if tool.kind == Kind::HorizontalVertical {
        if selection.len() != 1 {
            return false;
        }
        let picked = match &selection[0] {
            SketchPoint { id } => Some(json!(id)),
            SketchMidpoint { id } if is_line_curve(sketch, id) => Some(json!({"midpoint": id})),
            _ => None,
        };
        if let Some(point) = picked {
            tool.point_picks.push(point);
            return true;
        }
        return false;
    }
    let (max_picks, takes_points): (usize, bool) = match tool.cmd {
        "sketch.constraint.parallel"
        | "sketch.constraint.perpendicular"
        | "sketch.constraint.equal"
        | "sketch.constraint.collinear"
        | "sketch.constraint.curvature" => (1, false),
        "sketch.constraint.tangent" | "sketch.constraint.coincident" | "sketch.constraint.concentric" | "sketch.constraint.midpoint" => (1, true),
        "sketch.constraint.symmetry" => (2, true),
        "sketch.constraint.polygon" => (2, false),
        "sketch.constrainer" => (200, true),
        _ => return false,
    };
    if selection.len() > max_picks {
        return false;
    }
    let mut picks = Vec::with_capacity(selection.len());
    for sel in selection {
        let Some(id) = (match sel {
            SketchCurve { id, sketch: s } if s.is_none_or(|sid| Some(sid) == active) && curve_compatible_with_constraint(tool.cmd, sketch, id) => {
                Some(id.clone())
            }
            SketchPoint { id } if takes_points => Some(id.clone()),
            Axis { name } if tool.cmd == "sketch.constraint.symmetry" => own_axis(app, name).map(str::to_string),
            Axis { name } => axis_pick(app, tool, name),
            _ => None,
        }) else {
            return false;
        };
        if picks.contains(&id) {
            return false;
        }
        picks.push(id);
    }
    tool.picks = picks;
    true
}

/// Move 1 or 2 valid dimension operands from the current selection into `tool.picks`.
pub(crate) fn seed_dimension_preselection(app: &SolveApp, tool: &mut Tool) -> bool {
    use solidcraft_engine::Sel::{Axis, SketchCurve, SketchPoint};
    let selection = &app.session.selection;
    if !(1..=2).contains(&selection.len()) {
        return false;
    }
    let active = app.session.active_sketch;
    let mut picks = Vec::with_capacity(selection.len());
    for sel in selection {
        let Some(id) = (match sel {
            SketchCurve { id, sketch: s } if s.is_none_or(|sid| Some(sid) == active) => Some(id.clone()),
            SketchPoint { id } => Some(id.clone()),
            Axis { name } => sketch_axis_of(app, name).map(str::to_string),
            _ => None,
        }) else {
            return false;
        };
        if picks.contains(&id) {
            return false;
        }
        picks.push(id);
    }
    tool.picks = picks;
    true
}

pub(crate) fn projection_preselection(selection: &[solidcraft_engine::Sel]) -> Option<Vec<Value>> {
    use solidcraft_engine::Sel;
    // The projection command processes at most 1000 references; never consume a subset.
    if selection.is_empty() || selection.len() > 1000 {
        return None;
    }
    selection
        .iter()
        .map(|item| {
            Some(match item {
                Sel::Body { name } => json!({"body":name}),
                Sel::ExternalSketchCurve { sketch, id } => json!({"sketch":sketch,"curve":id}),
                // A straight line of another sketch, picked into the dialog, names its sketch
                // (#3726): the same reference the external pick stands for (#10291).
                Sel::SketchCurve { id, sketch: Some(sketch) } => json!({"sketch":sketch,"curve":id}),
                Sel::ExternalSketchPoint { sketch, id } => json!({"sketch":sketch,"point":id}),
                Sel::Vertex { body, point } => json!({"body":body,"vertex":[point.x,point.y,point.z]}),
                Sel::Edge { body, point, .. } => json!({"body":body,"edge":[point.x,point.y,point.z]}),
                Sel::Face { body, point, .. } => json!({"body":body,"face":[point.x,point.y,point.z]}),
                Sel::Axis { name } => json!({"axis":name}),
                Sel::Plane { name } => json!({"plane":name}),
                Sel::ConstructionPoint { name } => json!({"point":name}),
                _ => return None,
            })
        })
        .collect()
}

/// `x` or `y` when the origin axis `name` is the active sketch's own x or y axis: along it, and
/// through the sketch's origin.
pub(crate) fn sketch_axis_of(app: &SolveApp, name: &str) -> Option<&'static str> {
    use solidcore_record::Vec3;
    let dir = match name {
        "X" => Vec3::X,
        "Y" => Vec3::Y,
        "Z" => Vec3::Z,
        _ => return None,
    };
    let st = crate::open_meshing::session_world(&app.session);
    let plane = st.sketch(app.session.active_sketch?)?.plane;
    let o = plane.origin;
    if (o - dir * o.dot(dir)).len() > 1e-6 {
        return None;
    }
    if plane.x.dot(dir).abs() > 1.0 - 1e-9 {
        Some("x")
    } else if plane.y.dot(dir).abs() > 1.0 - 1e-9 {
        Some("y")
    } else {
        None
    }
}

fn symmetry_args(picks: &[Value], sketch: Option<&solidcraft_engine::sketch::Sketch>) -> Option<Value> {
    solidcraft_engine::sketch_selection::symmetry_args(picks, sketch)
}

/// An origin axis as a pick of `tool`, or None when the tool does not take it. Symmetry's third
/// pick may be an axis along the sketch's own x or y (#1075), which the command names `x` / `y`;
/// either pick of Coincident, Collinear and Tangent (#15461) may be any origin axis: the sketch's
/// own `x` / `y`, otherwise its reference, which the command projects.
fn axis_pick(app: &SolveApp, tool: &Tool, name: &str) -> Option<String> {
    match tool.cmd {
        // Symmetry takes no axis reference, so the axis isn't taken when a curve or point of the
        // sketch has its `x` / `y` as id: the command would read that entity instead.
        "sketch.constraint.symmetry" if tool.picks.len() == 2 => own_axis(app, name).map(str::to_string),
        cmd if cmd == "sketch.constraint.coincident" || takes_axis_curve(cmd) => axis_operand(app, name).map(|v| match v {
            Value::String(id) => id,
            other => other.to_string(),
        }),
        _ => None,
    }
}

/// The curve-pair constraints whose `a` / `b` may be an origin axis: a line on it, a curve
/// touching it (#15461).
pub(crate) fn takes_axis_curve(command: &str) -> bool {
    matches!(command, "sketch.constraint.tangent" | "sketch.constraint.collinear")
}

/// Origin axis `name` as a constraint operand: the sketch's own `x` / `y`, unless a curve or point
/// of the sketch is called that; then the axis reference, which can't be mistaken for it.
pub(crate) fn axis_operand(app: &SolveApp, name: &str) -> Option<Value> {
    own_axis(app, name).map(|id| json!(id)).or_else(|| matches!(name, "X" | "Y" | "Z").then(|| json!({"axis": name})))
}

/// The sketch's own `x` / `y` that origin axis `name` lies along, unless a curve or point of the
/// active sketch has that id.
fn own_axis(app: &SolveApp, name: &str) -> Option<&'static str> {
    sketch_axis_of(app, name).filter(|id| {
        let Ok(st) = app.session.world_state() else { return true };
        !app.session
            .active_sketch
            .and_then(|s| st.sketch(s))
            .is_some_and(|ss| ss.sketch.curve_index(id).is_some() || ss.sketch.point_index(id).is_some())
    })
}

/// An origin axis picked in the browser while a pick tool that takes it runs: the same as
/// clicking it in the view. False when there is no such tool.
pub(crate) fn pick_axis_from_browser(app: &mut SolveApp, name: &str) -> bool {
    let Some(mut tool) = app.tool.take() else { return false };
    // A dimension measures the axis's projection against the other pick, as a click in the view does.
    if matches!(tool.kind, Kind::Dimension) && matches!(name, "X" | "Y" | "Z") && tool.picks.len() < 2 {
        let id = json!({"axis": name}).to_string();
        // A value already typed into the open dimension is kept: the axis joins only before
        // anything is typed, as a second pick in the view does.
        if !tool.picks.contains(&id) && crate::dim_view::live() != Some(true) {
            tool.picks.push(id);
            if tool.picks.len() == 2 {
                match crate::dim_view::begin_live(app, tool.picks.clone(), None) {
                    Ok(true) => {}
                    Ok(false) => crate::dim_view::cancel(),
                    Err(error) => app.set_status(error, true),
                }
            }
        }
        app.tool = Some(tool);
        return true;
    }
    let picked = match tool.kind {
        Kind::Pick(n) => axis_pick(app, &tool, name).map(|id| (n, id)),
        _ => None,
    };
    let Some((n, id)) = picked else {
        app.tool = Some(tool);
        return false;
    };
    tool.picks.push(id);
    if tool.picks.len() >= n {
        let applied = run_pick(app, &tool);
        if applied && tool.preselected {
            return true;
        }
        if !applied && tool.preselected {
            tool.picks.pop();
        } else {
            tool.picks.clear();
        }
    }
    app.tool = Some(tool);
    true
}

/// The active sketch's point (`false`) or curve (`true`) under the cursor. Other sketches'
/// entities are not this sketch's: they are projected first ([`project_pick`]).
fn active_entity(app: &SolveApp, hits: &[Hit]) -> Option<(String, bool)> {
    let active = app.session.active_sketch?;
    hits.iter().find_map(|h| match h {
        Hit::SketchPoint { sketch, id, .. } if *sketch == active => Some((id.clone(), false)),
        Hit::SketchCurve { sketch, id, .. } if *sketch == active => Some((id.clone(), true)),
        _ => None,
    })
}

/// A model vertex or edge, or another sketch's curve, under the cursor, projected into the
/// active sketch so a constraint or dimension can use it: the projected point (`false`) or
/// curve (`true`). The status line tells what was projected (or why it could not be).
fn project_pick(app: &mut SolveApp, hits: &[Hit]) -> Option<(String, bool)> {
    let active = app.session.active_sketch?;
    let (r, what) = hits.iter().find_map(|h| match h {
        Hit::Vertex { body, point } => Some((json!({"vertex": [point.x, point.y, point.z], "body": body}), "vertex")),
        Hit::Edge { body, mid, .. } => Some((json!({"edge": [mid.x, mid.y, mid.z], "body": body}), "edge")),
        Hit::SketchCurve { sketch, id, .. } if *sketch != active => Some((json!({"sketch": sketch, "curve": id}), "curve")),
        _ => None,
    })?;
    let v = app.run("sketch.project", json!({"refs": [r]})).ok()?;
    let link = v["links"].get(0)?;
    let first = |k: &str| link[k].get(0).and_then(Value::as_str).map(str::to_string);
    // A vertex projects to a point, and so does an edge seen end-on.
    let curve = if what == "vertex" { None } else { first("curves") };
    let picked = curve.map(|c| (c, true)).or_else(|| first("points").map(|p| (p, false)));
    if picked.is_some() {
        app.set_status(solidcraft_ui_text::tr(&format!("sketch.projected_{what}")), false);
    }
    picked
}

fn run_pick(app: &mut SolveApp, t: &Tool) -> bool {
    // An entity id, or a model edge's or vertex's reference.
    let k: Vec<Value> = t.picks.iter().map(|pick| serde_json::from_str(pick).ok().filter(Value::is_object).unwrap_or_else(|| json!(pick))).collect();
    let params = match t.cmd {
        "sketch.constraint.fix" => json!({"entity": k[0]}),
        "sketch.constraint.coincident" => json!({"a": k[0], "b": k[1]}),
        // In pick order: a point and a line either way round, or two lines to centre.
        "sketch.constraint.midpoint" => json!({"a": k[0], "b": k[1]}),
        "sketch.constraint.symmetry" => {
            let params = {
                let state = crate::open_meshing::session_world(&app.session);
                symmetry_args(&k, app.session.active_sketch.and_then(|id| state.sketch(id)).map(|ss| &ss.sketch))
            };
            let Some(params) = params else {
                app.set_status("Symmetry: pick two points or two curves and the line between them", true);
                return false;
            };
            params
        }
        _ => json!({"a": k[0], "b": k[1]}),
    };
    // A command that changed nothing says why (Fix on projected geometry).
    let ran = app.run(t.cmd, params);
    if let Ok(v) = &ran
        && let Some(note) = v["result"]["note"].as_str()
    {
        app.set_status(note, false);
    }
    ran.is_ok()
}

/// Parallel, Perpendicular, Equal and Collinear constrain two curves, and their commands take no
/// point. So a click on a curve's end point picks the curve under it, not the point, and a body's
/// edge is taken rather than its vertex (#12092). Tangent also takes a curve's end point: that
/// end then lies on the other curve. A body's vertex ends no sketch curve, so Tangent takes the
/// edge there too.
fn curve_operands(cmd: &str, hits: Vec<Hit>) -> Vec<Hit> {
    let points = match cmd {
        "sketch.constraint.collinear" | "sketch.constraint.equal" | "sketch.constraint.parallel" | "sketch.constraint.perpendicular" => false,
        "sketch.constraint.tangent" => true,
        _ => return hits,
    };
    hits.into_iter().filter(|h| !matches!(h, Hit::Vertex { .. }) && (points || !matches!(h, Hit::SketchPoint { .. }))).collect()
}

/// Take back a pending spline point before undo or Backspace reaches the sketch.
pub fn undo_spline_point(app: &mut SolveApp) -> bool {
    let Some(t) = app.tool.as_mut() else { return false };
    if !matches!(t.cmd, "sketch.spline.fit_point" | "sketch.spline.control_point" | "sketch.spline.control_point_5") || t.pts.is_empty() {
        return false;
    }
    t.pts.pop();
    crate::sketch_dims::reset(t);
    true
}

/// Right click / Esc: end the current chain (line) or cancel the tool.
pub fn finish(app: &mut SolveApp) {
    crate::dim_view::cancel();
    if crate::sketch3d::finish(app) {
        return;
    }
    if crate::sketch_line_arc::finish(app) {
        return;
    }
    if let Some(mut t) = app.tool.take_if(|t| t.kind == Kind::Ext) {
        if crate::sketch_tools::finish(app, &mut t) {
            app.tool = Some(t);
        }
        return;
    }
    if let Some(t) = app.tool.as_mut()
        && !t.pts.is_empty()
    {
        t.pts.clear();
        t.picks.clear();
        crate::sketch_dims::reset(t);
        return;
    }
    app.tool = None;
}

/// Rubber-band preview of the shape being drawn.
pub(crate) fn paint_preview_curve(
    mut sketch: solidcraft_engine::sketch::Sketch,
    plane: &solidcore_record::Plane,
    painter: &egui::Painter,
    proj: &Proj,
    stroke: Stroke,
    mode: u8,
) {
    for curve in &mut sketch.curves {
        let line = matches!(curve.kind, solidcraft_engine::sketch::CurveKind::Line { .. });
        curve.centerline = mode == 2 && line;
        curve.infinite = mode == 3 && line;
        curve.construction |= mode != 0 && !curve.centerline;
    }
    let mut budget = solidcraft_engine::render::SamplingBudget::new(4096, 65_536);
    for index in 0..sketch.curves.len() {
        paint_curve(&sketch, index, plane, painter, proj, stroke, &mut budget);
    }
    crate::scenario::publish_count("tool_preview_curves", sketch.curves.len() as f64);
}

pub(crate) fn paint_sketch_path(points: &[solidcore_record::Vec3], mode: u8, painter: &egui::Painter, proj: &Proj, stroke: Stroke) {
    if mode == 0 {
        let screen: Vec<_> = points.iter().filter_map(|point| proj.to_screen(*point)).collect();
        if screen.len() >= 2 {
            painter.add(egui::Shape::line(screen, stroke));
        }
        return;
    }
    let unit = solidcraft_engine::view::grid_step(proj.cam.half_height()).0;
    for (a, b) in crate::viewport::sketch_segments(points, mode, unit) {
        if let (Some(a), Some(b)) = (proj.to_screen(a), proj.to_screen(b)) {
            painter.line_segment([a, b], stroke);
        }
    }
}

pub(crate) fn paint_curve(
    sketch: &solidcraft_engine::sketch::Sketch,
    index: usize,
    plane: &solidcore_record::Plane,
    painter: &egui::Painter,
    proj: &Proj,
    stroke: Stroke,
    budget: &mut solidcraft_engine::render::SamplingBudget,
) {
    paint_curve_placed(sketch, index, plane, painter, proj, stroke, budget, None);
}

/// [`paint_curve`], with each piece moved by `placed` first (the Insert SVG/DXF window's
/// placement, drawn without importing the drawing again).
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_curve_placed(
    sketch: &solidcraft_engine::sketch::Sketch,
    index: usize,
    plane: &solidcore_record::Plane,
    painter: &egui::Painter,
    proj: &Proj,
    stroke: Stroke,
    budget: &mut solidcraft_engine::render::SamplingBudget,
    placed: Option<&dyn Fn(solidcore_record::Seg2) -> solidcore_record::Seg2>,
) {
    for part in crate::sketch_display::curve_with_budget_placed(sketch, index, plane, proj, painter.ctx().pixels_per_point(), budget, placed).parts {
        let mode = sketch.curves.get(index).map_or(0, |c| if c.centerline { 2 } else { u8::from(c.construction) });
        paint_sketch_path(&part, mode, painter, proj, stroke);
    }
}

/// Temporary analytic geometry uses the same exact span extraction as committed curves.
/// It never solves constraints or modifies the document.
pub(crate) fn preview_curve(
    t: &Tool,
    hover: Vec2,
    sweep: Option<f64>,
    rho: Option<f64>,
    slot_width: Option<f64>,
) -> Option<solidcraft_engine::sketch::Sketch> {
    use solidcraft_engine::sketch::{CurveKind, Sketch};
    if !hover.is_finite() || t.pts.len() >= solidcraft_engine::sketch::MAX_SPLINE_POINTS || t.pts.iter().any(|p| !p.0.is_finite()) {
        return None;
    }
    let mut sk = Sketch::new();
    let pts: Vec<_> = t.pts.iter().map(|p| p.0).collect();
    match (t.cmd, pts.as_slice()) {
        ("sketch.circle.center" | "sketch.polygon.inscribed" | "sketch.polygon.circumscribed", [c]) => {
            sk.add_circle(*c, c.dist(hover), None, None).ok()?;
        }
        ("sketch.circle.two_point", [a]) => {
            sk.add_circle((*a + hover) * 0.5, a.dist(hover) * 0.5, None, None).ok()?;
        }
        // The first click is the middle: the line runs from the cursor's mirror image through
        // it to the cursor (#1358), not from the middle to the cursor.
        ("sketch.line.midpoint", [m]) => {
            if m.dist(hover) <= 1e-9 {
                return None;
            }
            sk.add_line(*m * 2.0 - hover, hover, None, None, None).ok()?;
        }
        ("sketch.circle.three_point" | "sketch.arc.three_point", [a, b]) => {
            let centre = preview_circumcentre(*a, *b, hover)?;
            if t.cmd == "sketch.circle.three_point" {
                sk.add_circle(centre, centre.dist(*a), None, None).ok()?;
            } else {
                let angle = |p: Vec2| (p - centre).angle();
                let span = |p: Vec2| (angle(p) - angle(*a)).rem_euclid(std::f64::consts::TAU);
                let ccw = span(hover) < span(*b);
                sk.add_arc(centre, if ccw { *a } else { *b }, if ccw { *b } else { *a }, [None; 3], None).ok()?;
                if let Some(curve) = sk.curves.first_mut() {
                    curve.reversed = !ccw;
                }
            }
        }
        ("sketch.arc.center_point", [c, a]) => {
            let start = (*a - *c).angle();
            let sweep = sweep.unwrap_or_else(|| center_arc_sweep(t, hover));
            if !sweep.is_finite() || sweep.abs() <= 1e-9 || sweep.abs() >= std::f64::consts::TAU {
                return None;
            }
            let b = *c + Vec2::from_angle(start + sweep) * c.dist(*a);
            sk.add_arc(*c, if sweep > 0.0 { *a } else { b }, if sweep > 0.0 { b } else { *a }, [None; 3], None).ok()?;
            if let Some(curve) = sk.curves.first_mut() {
                curve.reversed = sweep < 0.0;
            }
        }
        ("sketch.slot.center_to_center" | "sketch.slot.overall" | "sketch.slot.center_point", [a, b]) => {
            let dir = (*b - *a).normalized()?;
            // Typed widths can be smaller than the final click's pointer minimum.
            let width = slot_width.unwrap_or_else(|| (2.0 * dir.cross(hover - *a).abs()).max(1e-3));
            if !width.is_finite() || width <= 1e-9 {
                return None;
            }
            let radius = width * 0.5;
            let (c0, c1) = match t.cmd {
                "sketch.slot.overall" if a.dist(*b) > width => (*a + dir * radius, *b - dir * radius),
                "sketch.slot.overall" => return None,
                "sketch.slot.center_point" => (*a * 2.0 - *b, *b),
                _ => (*a, *b),
            };
            let n = dir.perp() * radius;
            sk.add_line(c0 - n, c1 - n, None, None, None).ok()?;
            sk.add_arc(c1, c1 - n, c1 + n, [None; 3], None).ok()?;
            sk.add_line(c1 + n, c0 + n, None, None, None).ok()?;
            sk.add_arc(c0, c0 + n, c0 - n, [None; 3], None).ok()?;
            // The construction line between the arc centres the slot's middle sits on (#7908).
            let axis = sk.add_line(c0, c1, None, None, None).ok()?;
            sk.curves.get_mut(axis)?.construction = true;
            sk.add_point((c0 + c1) * 0.5, None).ok()?;
        }
        ("sketch.slot.arc_center" | "sketch.slot.arc_three_point", [a, b, c]) => {
            let (centre, start, end, radius, width) = if t.cmd == "sketch.slot.arc_center" {
                (*a, *b, *c, a.dist(*b), (2.0 * (hover.dist(*a) - a.dist(*b)).abs()).max(1e-3))
            } else {
                let centre = preview_circumcentre(*a, *c, *b)?;
                let span = |p: Vec2| ((p - centre).angle() - (*a - centre).angle()).rem_euclid(std::f64::consts::TAU);
                let (start, end) = if span(*b) < span(*c) { (*a, *c) } else { (*c, *a) };
                (centre, start, end, centre.dist(*a), (2.0 * hover.dist(*c)).max(1e-3))
            };
            let width = slot_width.unwrap_or(width);
            let t0 = (start - centre).angle();
            let sweep = ((end - centre).angle() - t0).rem_euclid(std::f64::consts::TAU);
            let half = width * 0.5;
            if !radius.is_finite()
                || !half.is_finite()
                || half >= radius
                || end.dist(centre) < 1e-9
                || !(sweep > 1e-6 && sweep < std::f64::consts::TAU - 1e-6)
            {
                return None;
            }
            let at = |angle: f64, r: f64| centre + Vec2::from_angle(angle) * r;
            let t1 = t0 + sweep;
            sk.add_arc(centre, at(t0, radius + half), at(t1, radius + half), [None; 3], None).ok()?;
            sk.add_arc(centre, at(t0, radius - half), at(t1, radius - half), [None; 3], None).ok()?;
            sk.add_arc(at(t1, radius), at(t1, radius + half), at(t1, radius - half), [None; 3], None).ok()?;
            sk.add_arc(at(t0, radius), at(t0, radius - half), at(t0, radius + half), [None; 3], None).ok()?;
            let guide = sk.add_arc(centre, at(t0, radius), at(t1, radius), [None; 3], None).ok()?;
            sk.curves.get_mut(guide)?.construction = true;
            // The middle, halfway along the centre arc, and its construction chords (#7908).
            let mid = at(t0 + sweep * 0.5, radius);
            for end in [at(t0, radius), at(t1, radius)] {
                let chord = sk.add_line(mid, end, None, None, None).ok()?;
                sk.curves.get_mut(chord)?.construction = true;
            }
        }
        ("sketch.ellipse", [c, m]) => {
            let u = (*m - *c).normalized()?;
            let d = hover - *c;
            let k = 1.0 - (d.dot(u) / c.dist(*m)).powi(2);
            if !k.is_finite() || k <= 1e-12 {
                return None;
            }
            let r = d.cross(u).abs() / k.sqrt();
            let c = sk.add_point(*c, None).ok()?;
            let m = sk.add_point(*m, None).ok()?;
            sk.add_curve(CurveKind::Ellipse { c, m, r }, None).ok()?;
        }
        ("sketch.conic", [a, b]) => {
            // Positioning the apex (the third click): the live conic through the cursor at the
            // typed `rho` (or the default parabola's 0.5), so the curve it is about to become is
            // on screen while it is being placed, and a typed `rho` reshapes it before the
            // click. Without this the tool showed only the straight start-to-end, end-to-cursor
            // rubber band every tool draws while it has points still to place.
            let rho = rho.filter(|r| r.is_finite() && *r > 1e-6 && *r < 1.0 - 1e-6).unwrap_or(0.5);
            let a = sk.add_point(*a, None).ok()?;
            let b = sk.add_point(*b, None).ok()?;
            let apex = sk.add_point(hover, None).ok()?;
            sk.add_curve(CurveKind::Conic { a, b, apex, rho }, None).ok()?;
        }
        ("sketch.spline.fit_point" | "sketch.spline.control_point" | "sketch.spline.control_point_5", points) if !points.is_empty() => {
            let periodic = t.cmd == "sketch.spline.fit_point" && points.len() >= 3 && points.first() == Some(&hover);
            let mut indices = points.iter().map(|p| sk.add_point(*p, None).ok()).collect::<Option<Vec<_>>>()?;
            if periodic {
                indices.push(*indices.first()?);
            } else if points.last() != Some(&hover) {
                indices.push(sk.add_point(hover, None).ok()?);
            }
            sk.add_curve(
                CurveKind::Spline {
                    pts: indices,
                    control: t.cmd != "sketch.spline.fit_point",
                    degree: if t.cmd == "sketch.spline.control_point_5" { 5 } else { 3 },
                    periodic,
                    nurbs: None,
                    handles: Vec::new(),
                },
                None,
            )
            .ok()?;
        }
        _ => return None,
    }
    Some(sk)
}

pub(crate) fn preview_circumcentre(a: Vec2, b: Vec2, c: Vec2) -> Option<Vec2> {
    solidcraft_kernel::services::sketch_arc::circumcentre(a.into(), b.into(), c.into()).map(Vec2::from)
}

/// The polygon's actual straight edges, in the same orientation as the eventual command.
/// Keep preview work bounded even while a side-count expression is being edited.
pub(crate) fn polygon_outline(t: &Tool, hover: Vec2, sides: f64) -> Option<Vec<Vec2>> {
    if !hover.is_finite() || !(3.0..=200.0).contains(&sides) || sides.fract() != 0.0 || t.pts.len() != 1 {
        return None;
    }
    let a = t.pts.first()?.0;
    let n = sides as usize;
    let half = std::f64::consts::PI / sides;
    let distance = a.dist(hover);
    if !a.is_finite() || !distance.is_finite() || distance <= 1e-9 {
        return None;
    }
    let (centre, radius, start) = match t.cmd {
        "sketch.polygon.inscribed" => (a, distance, (hover - a).angle()),
        "sketch.polygon.circumscribed" => (a, distance / half.cos(), (hover - a).angle() + half),
        "sketch.polygon.edge" => {
            let radius = distance / (2.0 * half.sin());
            let centre = (a + hover) * 0.5 + (hover - a).normalized()?.perp() * (radius * half.cos());
            (centre, radius, (a - centre).angle())
        }
        _ => return None,
    };
    let mut corners: Vec<_> = (0..n).map(|i| centre + Vec2::from_angle(start + std::f64::consts::TAU * i as f64 / sides) * radius).collect();
    if corners.iter().any(|p| !p.is_finite()) {
        return None;
    }
    corners.push(*corners.first()?);
    Some(corners)
}

/// The pointer ring is left out where it would touch a dimension's value being placed: drawn
/// after the value, it would sit on the number (#14231). Its largest radius and half its stroke
/// count, so a ring centred just outside the value is left out too.
pub(crate) fn pointer_ring_shown(t: &Tool, at: Pos2) -> bool {
    const REACH: f32 = 6.0 + 1.5 / 2.0;
    !(t.kind == Kind::Dimension && crate::dim_view::draft_text().is_some_and(|r| r.expand(REACH).contains(at)))
}

/// Highlight an affected sketch curve with the viewport's normal selection style.
pub(crate) fn highlight_curve(
    sk: &solidcraft_engine::sketch::Sketch,
    ci: usize,
    plane: &solidcore_record::Plane,
    painter: &egui::Painter,
    proj: &Proj,
    budget: &mut solidcraft_engine::render::SamplingBudget,
) {
    let tk = solidcraft_theme::Tokens::view();
    paint_curve(sk, ci, plane, painter, proj, Stroke::new(3.5, tk.sel_edge), budget);
}

/// Highlight an affected sketch point or center with the viewport's normal selection style.
pub(crate) fn highlight_point(painter: &egui::Painter, sp: Pos2) {
    let tk = solidcraft_theme::Tokens::view();
    painter.circle_filled(sp, 4.0, tk.sel_vertex);
    painter.circle_stroke(sp, 6.0, Stroke::new(2.0, tk.sel_edge));
}

/// Which way the Horizontal (`Some(true)`) or Vertical (`Some(false)`) tool aligns; `None` for any
/// other tool, the combined Horizontal/Vertical among them, which picks the nearer way itself.
pub(crate) fn axis_of_tool(cmd: &str) -> Option<bool> {
    match cmd {
        "sketch.constraint.horizontal" => Some(true),
        "sketch.constraint.vertical" => Some(false),
        _ => None,
    }
}

/// The screen ends of the guide through `anchor` (sketch coordinates) along the sketch's X axis
/// (`horizontal`) or Y axis, reaching past both sides of `view`: `None` when the direction is
/// seen end on or the anchor is off the screen side of the camera.
pub(crate) fn axis_guide_ends(anchor: Vec2, horizontal: bool, to: impl Fn(Vec2) -> Option<Pos2>, view: egui::Rect) -> Option<[Pos2; 2]> {
    let step = if horizontal { Vec2::new(1.0, 0.0) } else { Vec2::new(0.0, 1.0) };
    let a = to(anchor)?;
    // Both ways from the anchor, so a point behind the camera on one side still gives the line.
    let along = to(anchor + step).map(|b| b - a).or_else(|| to(anchor - step).map(|b| a - b))?;
    let length = along.length();
    if !a.is_finite() || !length.is_finite() || length < 1e-3 {
        return None;
    }
    let reach = 2.0 * (view.width() + view.height() + (a - view.center()).length());
    let dir = along / length;
    Some([a - dir * reach, a + dir * reach])
}

/// The thin dashed guide the Horizontal and Vertical tools show along their direction.
fn paint_axis_guide(anchor: Vec2, horizontal: bool, plane: &solidcore_record::Plane, painter: &egui::Painter, proj: &Proj, stroke: Stroke) {
    let Some(ends) = axis_guide_ends(anchor, horizontal, |p| proj.to_screen(plane.to_world(p)), proj.rect) else { return };
    painter.extend(crate::ui_kit::dashed_line(&ends, painter.clip_rect(), stroke, 6.0, 4.0));
    // Where it crosses the view, for checks: flat for a horizontal guide in a top view.
    crate::scenario::publish_rect("viewport:axis_guide", egui::Rect::from_two_pos(ends[0], ends[1]).intersect(proj.rect));
}

pub fn preview(app: &SolveApp, t: &Tool, painter: &egui::Painter, proj: &Proj) {
    if crate::sketch3d::preview(app, t, painter, proj) {
        return;
    }
    if t.kind == Kind::Ext {
        return crate::sketch_tools::preview(app, t, painter, proj);
    }
    let Some(sid) = app.session.active_sketch else { return };
    let st = crate::open_meshing::session_world(&app.session);
    let Some(ss) = st.sketch(sid) else { return };
    let to = |p: Vec2| proj.to_screen(ss.plane.to_world(p));
    // Drawn straight onto the view: its side's colours (#4146).
    let tk = solidcraft_theme::Tokens::view();
    let stroke = Stroke::new(1.5, tk.sketch_accent);
    if let Some((hp, snap)) = &t.hover
        && let Some(sp) = to(*hp)
    {
        if !is_constraint_pick_tool(t) && pointer_ring_shown(t, sp) {
            painter.circle_stroke(sp, if snap.is_some() { 6.0 } else { 3.0 }, stroke);
        }
        if snap.as_deref() == Some("Midpoint") && t.kind == Kind::HorizontalVertical {
            painter.circle_stroke(sp, 8.0, Stroke::new(2.0, tk.accent));
        }
        // The design's coordinates: not while a value is typed and the sketch is drawn at it.
        if (t.kind == Kind::Dimension || !is_constraint_pick_tool(t)) && crate::dim_view::preview_world(app).is_none() {
            let readout = painter.text(
                crate::tool_cursor::readout_at(painter, proj.rect, sp),
                egui::Align2::LEFT_TOP,
                crate::sketch_cursor_readout::text(app, *hp),
                egui::FontId::proportional(solidcraft_theme::Tokens::get().font_small),
                tk.text_dim,
            );
            crate::scenario::publish_rect("viewport:readout", readout);
        }
    }
    if matches!(t.kind, Kind::Pick(_) | Kind::Dimension) {
        let mut display_budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
        let curve_stroke = Stroke::new(3.5, tk.sel_edge);
        for id in &t.picks {
            if let Some(ci) = ss.sketch.curve_index(id) {
                highlight_curve(&ss.sketch, ci, &ss.plane, painter, proj, &mut display_budget);
            } else if let Some(sp) = ss.sketch.resolve_point(id).and_then(|pi| ss.sketch.point(pi)).and_then(to) {
                highlight_point(painter, sp);
            } else if let Ok(v) = serde_json::from_str::<Value>(id) {
                if let Some(axis) = v.get("axis").and_then(Value::as_str) {
                    let dir = match axis {
                        "X" => Some(solidcore_record::Vec3::X),
                        "Y" => Some(solidcore_record::Vec3::Y),
                        "Z" => Some(solidcore_record::Vec3::Z),
                        _ => None,
                    };
                    if let Some(dir) = dir {
                        let span = app.cam.half_height() * 2.0;
                        paint_sketch_path(&[-dir * span, dir * span], 0, painter, proj, curve_stroke);
                    }
                } else if let Some(pt) = v.get("vertex").or_else(|| v.get("edge")).and_then(Value::as_array)
                    && let (Some(x), Some(y), Some(z)) =
                        (pt.first().and_then(Value::as_f64), pt.get(1).and_then(Value::as_f64), pt.get(2).and_then(Value::as_f64))
                    && let Some(sp) = proj.to_screen(solidcore_record::Vec3::new(x, y, z))
                {
                    highlight_point(painter, sp);
                }
            }
        }
    }
    if matches!(t.kind, Kind::HorizontalVertical | Kind::Dimension) {
        for target in &t.point_picks {
            let midpoint = target.get("midpoint").and_then(Value::as_str).or_else(|| target.as_str().and_then(|id| id.strip_prefix("mid:")));
            let point = if let Some(id) = midpoint {
                ss.sketch.curve_index(id).and_then(|l| match ss.sketch.shape(l)? {
                    solidcraft_engine::sketch::Shape::Line { a, b } => Some((a + b) * 0.5),
                    _ => None,
                })
            } else {
                target.as_str().and_then(|id| ss.sketch.resolve_point(id)).and_then(|p| ss.sketch.point(p))
            };
            if let Some(sp) = point.and_then(to) {
                highlight_point(painter, sp);
            }
        }
    }
    // Horizontal or Vertical: one thin dashed guide along that direction through the cursor for as
    // long as the tool is on, after the first pick too (the picked point has its own ring).
    if let Some(horizontal) = axis_of_tool(t.cmd)
        && let Some((cursor, _)) = &t.hover
    {
        paint_axis_guide(*cursor, horizontal, &ss.plane, painter, proj, Stroke::new(1.0, tk.sketch_accent.gamma_multiply(0.7)));
    }
    let Some(hover) = t.hover.as_ref() else { return };
    let h = crate::sketch_dims::shown(app, t, hover);
    if let Some(corners) = polygon_outline(t, h, crate::sketch_dims::locked_value(app, t, "Sides").unwrap_or(6.0)) {
        let outline: Vec<_> = corners.iter().map(|c| ss.plane.to_world(*c)).collect();
        paint_sketch_path(&outline, crate::sketch_palette::mode(app), painter, proj, stroke);
        crate::scenario::publish_count("polygon_preview_edges", (corners.len() - 1) as f64);
        if t.cmd == "sketch.polygon.edge" {
            return;
        }
    }

    if let Some(curve) = preview_curve(
        t,
        h,
        crate::sketch_dims::locked_value(app, t, "Sweep"),
        crate::sketch_dims::locked_value(app, t, "Rho"),
        crate::sketch_dims::locked_value(app, t, "Width"),
    ) {
        paint_preview_curve(curve, &ss.plane, painter, proj, stroke, crate::sketch_palette::mode(app));
        return;
    }
    let pts: Vec<Vec2> = t.pts.iter().map(|p| p.0).collect();
    let line = |a: Vec2, b: Vec2| {
        paint_sketch_path(&[ss.plane.to_world(a), ss.plane.to_world(b)], crate::sketch_palette::mode(app), painter, proj, stroke);
    };
    match (t.cmd, pts.as_slice()) {
        ("sketch.line", [.., last]) => {
            line(*last, h);
        }
        ("sketch.rectangle.two_point", [a]) => {
            let (b, c) = (Vec2::new(h.x, a.y), Vec2::new(a.x, h.y));
            line(*a, b);
            line(b, h);
            line(h, c);
            line(c, *a);
        }
        ("sketch.rectangle.rounded", [a]) => {
            let r = crate::sketch_dims::rounded_radius(app, t, *a, h);
            let outline: Vec<_> = rounded_outline(*a, h, r).into_iter().map(|q| ss.plane.to_world(q)).collect();
            paint_sketch_path(&outline, crate::sketch_palette::mode(app), painter, proj, stroke);
        }
        ("sketch.rectangle.edge_center", [a]) => {
            let e = edge_end(*a, &(h, t.hover.as_ref().and_then(|x| x.1.clone())));
            line(*a * 2.0 - e, e);
        }
        ("sketch.rectangle.edge_center", [a, b]) => {
            let e = edge_end(*a, &(*b, t.pts.get(1).and_then(|x| x.1.clone())));
            if let Some(k) = edge_center_outline(*a, e, h) {
                for i in 0..4 {
                    line(k[i], k[(i + 1) % 4]);
                }
            }
        }
        ("sketch.rectangle.center", [c]) => {
            let d = h - *c;
            let k =
                [Vec2::new(c.x - d.x, c.y - d.y), Vec2::new(c.x + d.x, c.y - d.y), Vec2::new(c.x + d.x, c.y + d.y), Vec2::new(c.x - d.x, c.y + d.y)];
            for i in 0..4 {
                line(k[i], k[(i + 1) % 4]);
            }
        }
        (_, [a]) => line(*a, h),
        (_, [a, b]) => {
            line(*a, *b);
            line(*b, h);
        }
        _ => {}
    }
}

/// The end of the anchored edge for the second click `p`: within a couple of degrees of a sketch
/// axis it is nudged onto it (as for a line), unless it snapped to existing geometry.
pub(crate) fn edge_end(a: Vec2, p: &(Vec2, Option<String>)) -> Vec2 {
    let v = p.0 - a;
    if p.1.is_some() || v.len() < 1e-12 {
        return p.0;
    }
    a + Vec2::from_angle(crate::sketch_dims::snap_axis(v.angle())) * v.len()
}

/// The corners of the edge-centre rectangle whose edge is centred at `a` and ends at `e`, with
/// its width taken from the cursor `c` (the geometry of the `sketch.rectangle.edge_center`
/// command). None until the edge has a direction.
fn edge_center_outline(a: Vec2, e: Vec2, c: Vec2) -> Option<[Vec2; 4]> {
    let d = (e - a).normalized()?;
    let n = d.perp();
    let w = (c - a).dot(n);
    let (c0, c1) = (a * 2.0 - e, e);
    Some([c0, c1, c1 + n * w, c0 + n * w])
}

/// The closed outline of the rectangle with corners `a`, `b` and corner radius `r` (clamped to
/// fit), as points for a preview: counter-clockwise, arcs sampled, first point repeated last.
pub(crate) fn rounded_outline(a: Vec2, b: Vec2, r: f64) -> Vec<Vec2> {
    let (lo, hi) = (Vec2::new(a.x.min(b.x), a.y.min(b.y)), Vec2::new(a.x.max(b.x), a.y.max(b.y)));
    let r = if r.is_finite() { r.clamp(0.0, 0.5 * (hi.x - lo.x).min(hi.y - lo.y)) } else { 0.0 };
    let q = std::f64::consts::FRAC_PI_2;
    // Corner centres and the angle each arc starts at, CCW from the bottom-right corner.
    let corners = [
        (Vec2::new(hi.x - r, lo.y + r), -q),
        (Vec2::new(hi.x - r, hi.y - r), 0.0),
        (Vec2::new(lo.x + r, hi.y - r), q),
        (Vec2::new(lo.x + r, lo.y + r), 2.0 * q),
    ];
    let mut pts = Vec::with_capacity(4 * 9 + 1);
    for (c, a0) in corners {
        for i in 0..=8 {
            pts.push(c + Vec2::from_angle(a0 + q * f64::from(i) / 8.0) * r);
        }
    }
    if let Some(first) = pts.first().copied() {
        pts.push(first);
    }
    pts
}

#[cfg(test)]
mod constraint_cursor_tests {
    use super::*;
    use crate::scenario::Harness;
    use solidcore_record::Vec3;

    #[test]
    fn constraint_cursor_icon_follows_tool_and_dynamic_hv() {
        let mut h = Harness::new();
        h.run(&json!([
            {"call": "engine.execute", "params": {"command": "sketch.create", "params": {"plane": "XY"}}},
            // l1: horizontal-ish (0,0)-(30,2); l2: vertical-ish (-20,0)-(-18,30).
            {"call": "engine.execute", "params": {"command": "sketch.line", "params": {"points": [[0, 0], [30, 2]], "infer": false}}},
            {"call": "engine.execute", "params": {"command": "sketch.line", "params": {"points": [[-20, 0], [-18, 30]], "infer": false}}},
            {"call": "ui.view", "params": {"view": "top"}}
        ]))
        .expect("setup sketch");
        h.settle();

        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let proj = crate::viewport::projection(&h.app, rect);
        let screen = |x: f64, y: f64| proj.to_screen(Vec3::new(x, y, 0.0)).expect("on screen");

        // Static command icons across constraint pick tools.
        for (cmd, expected) in [
            ("sketch.constraint.horizontal", "c_horizontal"),
            ("sketch.constraint.vertical", "c_vertical"),
            ("sketch.constraint.parallel", "c_parallel"),
            ("sketch.constraint.curvature", "c_smooth"),
            ("sketch.constraint.polygon", "c_polygon"),
            ("sketch.constrainer", "constrainer"),
            ("sketch.dimension", "dimension"),
        ] {
            let tool = Tool::for_command(cmd).expect(cmd);
            assert_eq!(constraint_cursor_icon(&h.app, &tool, &proj, screen(0.0, -30.0), false), Some(expected), "{cmd}");
        }

        // Horizontal/Vertical dynamically resolves line orientation and empty-space fallback.
        let mut hv = Tool::for_command("sketch.constraint.horizontal_vertical").expect("hv tool");
        assert_eq!(constraint_cursor_icon(&h.app, &hv, &proj, screen(0.0, -30.0), false), Some("c_hv"));
        assert_eq!(constraint_cursor_icon(&h.app, &hv, &proj, screen(15.0, 1.0), false), Some("c_horizontal"));
        assert_eq!(constraint_cursor_icon(&h.app, &hv, &proj, screen(-19.0, 15.0), false), Some("c_vertical"));

        // With 1st point picked at `l1.start` (0, 0), hovering `l2.end` (-18, 30) resolves vertical.
        hv.point_picks.push(json!("l1.start"));
        assert_eq!(constraint_cursor_icon(&h.app, &hv, &proj, screen(-18.0, 30.0), false), Some("c_vertical"));
    }
}
