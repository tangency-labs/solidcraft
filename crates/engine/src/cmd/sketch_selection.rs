//! How sketch constraint commands take the current selection (#5501): the parameters a
//! complete, unambiguous selection of the sketch being edited gives each of them. A toolbar start
//! runs the constraint at once with them, and menus offer the constraints a selection fits from
//! the same rules ([`super::CommandSpec::for_selection`]). The commands still validate the
//! geometry; these rules only read which ids were picked, in pick order.

use serde_json::{Value, json};

use crate::Sel;
use crate::Session;
use crate::sketch::{Shape, Sketch};

/// Most entities a selection-wide constraint (Fix, Polygon, Equal) takes from the selection.
const MOST: usize = 200;

/// The parameters `command` takes from `selection` in `sketch` (the sketch being edited), or
/// `None` when the selection is not exactly what it takes.
pub fn constraint_params(command: &str, selection: &[Sel], sketch: Option<&Sketch>) -> Option<Value> {
    use Sel::{SketchCurve, SketchPoint};
    let selected_ids = |curves_only: bool, minimum: usize| -> Option<Vec<&str>> {
        if !(minimum..=MOST).contains(&selection.len()) {
            return None;
        }
        let ids: Vec<&str> = selection
            .iter()
            .map(|s| match s {
                SketchCurve { id, .. } => Some(id.as_str()),
                SketchPoint { id } if !curves_only => Some(id.as_str()),
                _ => None,
            })
            .collect::<Option<_>>()?;
        let unique: std::collections::BTreeSet<_> = ids.iter().copied().collect();
        (unique.len() == ids.len()).then_some(ids)
    };
    let rule = match (command, selection) {
        ("sketch.constraint.fix", _) => selected_ids(false, 1).map(|ids| json!({"entities":ids})),
        ("sketch.constraint.polygon", _) => selected_ids(true, 3).map(|ids| json!({"lines":ids})),
        // Three or more curves: all of them equal, in pick order (#4985). Two keep the `a, b` form.
        ("sketch.constraint.equal", _) if selection.len() >= 3 => selected_ids(true, 3).map(|ids| json!({"curves":ids})),
        ("sketch.constraint.curvature", [SketchCurve { id: a, .. }, SketchCurve { id: b, .. }]) if a != b => Some(json!({"a":a,"b":b})),
        ("sketch.constraint.horizontal_vertical", [SketchCurve { id, .. }]) => Some(json!({"line":id})),
        // A point and a straight line (a circle's centre and a square's edge, #617): align the
        // point with the line, in the mode the line's own axis suggests.
        ("sketch.constraint.horizontal_vertical", [SketchPoint { id }, SketchCurve { id: line, .. }])
        | ("sketch.constraint.horizontal_vertical", [SketchCurve { id: line, .. }, SketchPoint { id }]) => {
            point_line_alignment(sketch?, &json!(id), line)
        }
        ("sketch.constraint.horizontal_vertical", _) => alignment_points(selection).map(|points| json!({"points": points})),
        ("sketch.constraint.coincident", [_, _]) => coincident_operands(selection),
        ("sketch.constraint.concentric", [SketchCurve { id: a, .. }, SketchPoint { id: b }] | [SketchPoint { id: a }, SketchCurve { id: b, .. }])
            if a != b =>
        {
            Some(json!({"a":a,"b":b}))
        }
        (
            "sketch.constraint.midpoint",
            [SketchPoint { id: point }, SketchCurve { id: line, .. }] | [SketchCurve { id: line, .. }, SketchPoint { id: point }],
        ) => Some(json!({"point":point,"line":line})),
        ("sketch.constraint.midpoint", [SketchCurve { id: a, .. }, SketchCurve { id: b, .. }]) if a != b => Some(json!({"a":a,"b":b})),
        ("sketch.constraint.symmetry", [a, b, c]) => {
            let picks = [a, b, c]
                .into_iter()
                .map(|entity| match entity {
                    SketchPoint { id } | SketchCurve { id, .. } => Some(json!(id)),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            symmetry_args(&picks, sketch)
        }
        _ => None,
    };
    rule.or_else(|| curve_pair(command, selection))
}

/// Curve-pair constraints take exactly two distinct curves, as `a, b` in pick order.
fn curve_pair(command: &str, selection: &[Sel]) -> Option<Value> {
    let pairs = [
        "sketch.constraint.parallel",
        "sketch.constraint.perpendicular",
        "sketch.constraint.equal",
        "sketch.constraint.tangent",
        "sketch.constraint.concentric",
        "sketch.constraint.collinear",
    ];
    if !pairs.contains(&command) {
        return None;
    }
    let [Sel::SketchCurve { id: a, .. }, Sel::SketchCurve { id: b, .. }] = selection else { return None };
    (a != b).then(|| json!({"a": a, "b": b}))
}

/// [`constraint_params`] for the session's selection in the sketch being edited.
fn from_session(command: &str, s: &Session) -> Option<Value> {
    let state = s.world_state();
    let sketch = super::sketch_store::active(s).and_then(|id| state.sketch(id)).map(|ss| &ss.sketch);
    constraint_params(command, &s.selection, sketch)
}

macro_rules! takes {
    ($($name:ident => $id:literal),* $(,)?) => {
        $(pub(super) fn $name(s: &Session) -> Option<Value> {
            from_session($id, s)
        })*
    };
}

takes! {
    coincident => "sketch.constraint.coincident",
    collinear => "sketch.constraint.collinear",
    concentric => "sketch.constraint.concentric",
    curvature => "sketch.constraint.curvature",
    equal => "sketch.constraint.equal",
    fix => "sketch.constraint.fix",
    horizontal_vertical => "sketch.constraint.horizontal_vertical",
    midpoint => "sketch.constraint.midpoint",
    parallel => "sketch.constraint.parallel",
    perpendicular => "sketch.constraint.perpendicular",
    polygon => "sketch.constraint.polygon",
    symmetry => "sketch.constraint.symmetry",
    tangent => "sketch.constraint.tangent",
}

/// Two distinct point intents for the alignment command, without creating sketch points.
pub fn alignment_points(selection: &[Sel]) -> Option<[Value; 2]> {
    let [a, b] = selection else { return None };
    let point = |s: &Sel| match s {
        Sel::SketchPoint { id } => Some(json!(id)),
        Sel::SketchMidpoint { id } => Some(json!({"midpoint": id})),
        _ => None,
    };
    let (a, b) = (point(a)?, point(b)?);
    (a != b).then_some([a, b])
}

/// Coincident's two operands for a two-item selection: points and curves by id, a line's
/// midpoint as `{midpoint: line}`, which the command ties a point to (#10311).
pub fn coincident_operands(selection: &[Sel]) -> Option<Value> {
    let [a, b] = selection else { return None };
    let operand = |s: &Sel| match s {
        Sel::SketchPoint { id } | Sel::SketchCurve { id, .. } => Some(json!(id)),
        Sel::SketchMidpoint { id } => Some(json!({"midpoint": id})),
        _ => None,
    };
    let (a, b) = (operand(a)?, operand(b)?);
    (a != b).then(|| json!({"a": a, "b": b}))
}

/// The alignment of `point` with the midpoint of `line`, in the mode the line's axis suggests.
pub fn point_line_alignment(sk: &Sketch, point: &Value, line: &str) -> Option<Value> {
    let points = point_line_points(sk, point, line)?;
    let mode = line_axis_mode(sk, line)?;
    Some(json!({"points": points, "mode": mode}))
}

/// The `points` pair aligning `point` with the midpoint of `line`; None when `line` is not a line.
pub fn point_line_points(sk: &Sketch, point: &Value, line: &str) -> Option<[Value; 2]> {
    let l = sk.curve_index(line)?;
    matches!(sk.shape(l)?, Shape::Line { .. }).then(|| [point.clone(), json!({"midpoint": line})])
}

/// The axis a straight line is closest to: the alignment mode when a point is aligned with it (#617).
fn line_axis_mode(sk: &Sketch, line: &str) -> Option<&'static str> {
    let Shape::Line { a, b } = sk.shape(sk.curve_index(line)?)? else { return None };
    let d = b - a;
    Some(if d.x.abs() >= d.y.abs() { "horizontal" } else { "vertical" })
}

/// Symmetry's arguments from three picked sketch ids in any order (#1796): with two points and
/// a line, the line; with three lines, the only construction line or centerline among them (the
/// line drawn between the two it mirrors); otherwise the last pick, as before.
pub fn symmetry_args(picks: &[Value], sketch: Option<&Sketch>) -> Option<Value> {
    let curve = |v: &Value| match v.as_str() {
        Some(id) => sketch.and_then(|sk| sk.curve_index(id).and_then(|i| sk.curves.get(i))),
        None => None,
    };
    let is_line = |v: &Value| matches!(v.as_str(), Some("x" | "y")) || curve(v).is_some();
    let is_axis = |v: &Value| matches!(v.as_str(), Some("x" | "y")) || curve(v).is_some_and(|c| c.construction || c.centerline);
    let only = |f: &dyn Fn(&Value) -> bool| {
        let hits: Vec<usize> = (0..picks.len()).filter(|i| picks.get(*i).is_some_and(f)).collect();
        hits.first().copied().filter(|_| hits.len() == 1)
    };
    if picks.len() != 3 {
        return None;
    }
    let line = only(&is_line).or_else(|| only(&is_axis)).unwrap_or(2);
    let mut rest = (0..3).filter(|i| *i != line).filter_map(|i| picks.get(i));
    let (a, b, line) = (rest.next()?, rest.next()?, picks.get(line)?);
    (a != b).then(|| json!({"a": a, "b": b, "line": line}))
}
