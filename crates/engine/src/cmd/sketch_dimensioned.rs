//! Shape creation and its driving dimensions form one command and one undo step: the typed
//! values, with `as_drawn` every other size of the shape at its size as drawn, and with `locate`
//! its place from the sketch origin until nothing of it is left free.
//!
//! [`dimension_roles`] is the one list of the sizes each creator has. The UI asks it which
//! creators can be dimensioned on creation, so a creator added here (with its entities in
//! `draw`) needs nothing else to take part. `locate` needs nothing per creator: it works on the
//! points any creator reports, along the directions of an [`OriginFrame`].
//!
//! 3D sketches: their curves (`sketch.line3d`…) take no constraints yet. Once they do, a 3D
//! creator joins [`dimension_roles`] with its roles and entities, and `locate` gets a frame with a
//! Z direction; the UI routes the 3D tools through `tools::draw`, which asks `dimension_roles`.
use serde_json::{Map, Value, json};
use solidcraft_doc::expr::Kind;
use solidcraft_geom::Vec3;

use crate::{
    Result, Session,
    params::{bad, bool_, expr, str_},
};

/// The dimension roles of a sketch creator, in the order `as_drawn` adds them, or None when
/// `sketch.draw.dimensioned` cannot dimension it. Roles of one [`ALTERNATIVES`] pair are two
/// readings of one size: only the first given (or listed) of them is placed.
pub fn dimension_roles(creator: &str) -> Option<&'static [&'static str]> {
    Some(match creator {
        "sketch.line" => &["length", "angle"],
        "sketch.rectangle.two_point" | "sketch.rectangle.center" | "sketch.rectangle.three_point" | "sketch.rectangle.rounded" => {
            &["width", "height"]
        }
        // A circle is dimensioned by its diameter unless a radius is given.
        "sketch.circle.center" | "sketch.circle.two_point" | "sketch.circle.three_point" => &["diameter", "radius"],
        "sketch.arc.center_point" => &["radius", "sweep"],
        "sketch.slot.center_to_center" | "sketch.slot.overall" | "sketch.slot.center_point" => &["length", "width"],
        "sketch.slot.arc_center" => &["radius", "sweep", "width"],
        "sketch.slot.arc_three_point" => &["width"],
        "sketch.polygon.inscribed" | "sketch.polygon.circumscribed" | "sketch.polygon.edge" => &["radius"],
        _ => return None,
    })
}

/// Roles that measure the same size two ways: a shape takes one of each pair.
const ALTERNATIVES: &[[&str; 2]] = &[["radius", "diameter"]];

/// The roles to place: the given ones, then (`as_drawn`) each other role of `roles` that is not
/// an alternative of one already placed. None stands for the size as drawn.
fn wanted<'a>(roles: &[&'a str], given: &'a Map<String, Value>, as_drawn: bool) -> Vec<(&'a str, Option<&'a Value>)> {
    let mut wanted: Vec<(&str, Option<&Value>)> = given.iter().map(|(role, value)| (role.as_str(), Some(value))).collect();
    if as_drawn {
        for role in roles {
            let placed = |r: &str| wanted.iter().any(|(w, _)| *w == r);
            let alternative_placed = ALTERNATIVES.iter().any(|pair| pair.contains(role) && pair.iter().any(|other| other != role && placed(other)));
            if !placed(role) && !alternative_placed {
                wanted.push((role, None));
            }
        }
    }
    wanted
}

pub(super) fn draw(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.draw.dimensioned";
    let create = str_(p, "command").ok_or_else(|| bad(cmd, "`command` must name a supported sketch creator"))?;
    let roles = dimension_roles(create).ok_or_else(|| bad(cmd, "unsupported sketch creator"))?;
    let as_drawn = bool_(p, "as_drawn").unwrap_or(false);
    let locate = bool_(p, "locate").unwrap_or(false);
    let mut params = p.get("params").filter(|v| v.is_object()).cloned().ok_or_else(|| bad(cmd, "`params` must be an object"))?;
    let none = Map::new();
    let dimensions = match p.get("dimensions") {
        None if as_drawn || locate => &none,
        given => given.and_then(Value::as_object).ok_or_else(|| bad(cmd, "`dimensions` must be an object"))?,
    };
    if dimensions.len() > roles.len() || dimensions.keys().any(|key| !roles.contains(&key.as_str())) {
        return Err(bad(cmd, "unsupported dimension role for this creator"));
    }
    if dimensions.contains_key("radius") && dimensions.contains_key("diameter") {
        return Err(bad(cmd, "choose radius or diameter, not both"));
    }
    let mut overall_length = None;
    let mut slot_width = None;
    for (role, value) in dimensions {
        let e = expr(&json!({"value": value}), "value")
            .filter(|e| e.len() <= 4096)
            .ok_or_else(|| bad(cmd, "dimension values must be bounded expressions or finite numbers"))?;
        let kind = if role == "angle" || role == "sweep" { Kind::Angle } else { Kind::Length };
        let evaluated = s.doc.eval(&e, kind)?;
        if role == "length" {
            overall_length = Some(evaluated);
        }
        if role == "width" {
            slot_width = Some(evaluated);
        }
        if !evaluated.is_finite() || evaluated.abs() > 1e9 || kind == Kind::Length && evaluated <= 1e-9 {
            return Err(bad(cmd, format!("invalid {role} dimension")));
        }
        if role == "sweep" && !(evaluated.abs() > 1e-9 && evaluated.abs() < std::f64::consts::TAU - 1e-9) {
            return Err(bad(cmd, "sweep must be nonzero and less than a full turn"));
        }
        if role == "sweep" && create == "sketch.slot.arc_center" && evaluated <= 0.0 {
            return Err(bad(cmd, "arc slot sweep must be greater than zero and less than a full turn"));
        }
        if role == "sweep" {
            params["sweep"] = json!(evaluated.to_degrees());
            if let Some(o) = params.as_object_mut() {
                o.remove("end");
            }
        }
    }
    if create == "sketch.slot.overall"
        && let Some(length) = overall_length
        && let Some(width) = slot_width.or_else(|| params.get("width").and_then(Value::as_f64))
        && length <= width + 1e-9
    {
        return Err(bad(cmd, "overall slot length must exceed its width"));
    }
    if create == "sketch.line" {
        if params.get("closed").and_then(Value::as_bool).unwrap_or(false)
            || params.get("points").and_then(Value::as_array).is_none_or(|points| points.len() != 2)
        {
            return Err(bad(cmd, "dimensioned line creation needs exactly two points"));
        }
        if dimensions.contains_key("angle") {
            params["infer"] = json!(false);
        }
    }
    let mut scratch = s.scratch();
    let mut out = scratch.execute(create, &params)?;
    let curves = out.get("curves").and_then(Value::as_array).cloned().unwrap_or_default();
    let curve = |index: usize| -> Result<Value> {
        curves.get(index).filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "creator did not return the expected curve"))
    };
    let mut added = serde_json::Map::new();
    for (role, value) in wanted(roles, dimensions, as_drawn) {
        let (entities, ty) = match (create, role) {
            ("sketch.line", "length") => (vec![curve(0)?], "length"),
            ("sketch.line", "angle") => (vec![curve(0)?], "angle"),
            // A rounded rectangle's sides stop at the arcs: its size is between opposite sides.
            ("sketch.rectangle.rounded", "width" | "height") => {
                let [a, b] = if role == "width" { ["left", "right"] } else { ["bottom", "top"] };
                let side =
                    |k: &str| out["sides"].get(k).filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "creator did not return its sides"));
                (vec![side(a)?, side(b)?], "distance")
            }
            (c, "width" | "height") if c.starts_with("sketch.rectangle.") => (vec![curve(usize::from(role == "height"))?], "length"),
            (c, "radius" | "diameter") if c.starts_with("sketch.circle.") => (vec![curve(0)?], role),
            ("sketch.arc.center_point", "radius" | "sweep") => (vec![curve(0)?], role),
            ("sketch.slot.center_to_center" | "sketch.slot.center_point", "length") => (vec![curve(0)?], "length"),
            ("sketch.slot.overall", "length") => (vec![curve(1)?, curve(3)?], "overall_length"),
            (c, "width") if c.starts_with("sketch.slot.") => (vec![curve(1)?], "diameter"),
            ("sketch.slot.arc_center", "radius" | "sweep") => {
                let centre = out.get("centre_arc").filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "slot centre arc missing"))?;
                (vec![centre], role)
            }
            (c, "radius") if c.starts_with("sketch.polygon.") => {
                let support =
                    out.get("support_circle").filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "polygon support circle missing"))?;
                (vec![support], "radius")
            }
            _ => return Err(bad(cmd, "unsupported dimension role")),
        };
        let mut dimension = json!({"entities": entities, "type": ty});
        if let Some(sketch) = params.get("sketch") {
            dimension["sketch"] = sketch.clone();
        }
        match value {
            // A typed size the sketch already has (a rectangle drawn onto another's side, the
            // same height) is placed as a driven (reference) dimension rather than refused.
            Some(value) => {
                dimension["value"] = value.clone();
                dimension["if_redundant"] = json!("driven");
                added.insert(role.to_string(), scratch.execute("sketch.dimension", &dimension)?);
            }
            // A size as drawn (no value: the current one) that the sketch already determines, such
            // as the angle of a line that snapped horizontal, is left out; a failed command leaves
            // the sketch as it was.
            None => {
                if let Ok(result) = scratch.execute("sketch.dimension", &dimension) {
                    added.insert(role.to_string(), result);
                }
            }
        }
    }
    let located = if locate {
        // The freedom left once the shape and its sizes are in: the last reply that reports it.
        let dof = added.values().rev().chain([&out]).find_map(dof_of);
        place_from_origin(&mut scratch, &out, &params, dof)
    } else {
        Vec::new()
    };
    // Session::execute records the wrapper's before snapshot. Nested history must not leak.
    scratch.undo.clear();
    s.accept_computed(scratch);
    out["dimensions"] = Value::Object(added);
    if locate {
        out["located"] = Value::Array(located);
    }
    Ok(out)
}

/// The sketch's degrees of freedom as a command reported them.
fn dof_of(reply: &Value) -> Option<u64> {
    reply.get("sketch").and_then(|s| s.get("dof")).and_then(Value::as_u64)
}

/// More points than any creator makes: a cap on the work `locate` does.
const MAX_LOCATED_POINTS: usize = 64;

/// What `locate` places a shape with: the directions it measures from the origin, and the
/// commands that fix a distance along one of them or put a point on one of its axes. The loop in
/// [`place_from_origin`] only reads this, so a 3D sketch joins with a frame of its own (a third
/// direction, Z, and its 3D commands) once its geometry can take constraints.
struct OriginFrame {
    directions: &'static [OriginDirection],
    /// Fixes a point's distance from the origin along a direction: `{entities: ["origin", point], type}`.
    distance_command: &'static str,
    /// Puts a point on an axis: `{a: point, b: axis}`.
    onto_command: &'static str,
}

/// One direction of an [`OriginFrame`].
struct OriginDirection {
    /// The point's coordinate along this direction.
    along: fn(Vec3) -> f64,
    /// The distance dimension's `type` along it.
    distance: &'static str,
    /// The axis a point at 0 along this direction lies on (a distance of 0 keeps no side).
    onto: &'static str,
}

fn x_of(p: Vec3) -> f64 {
    p.x
}
fn y_of(p: Vec3) -> f64 {
    p.y
}

/// A sketch on a plane: horizontal and vertical from its origin; x = 0 is its own y axis, y = 0 its
/// x axis.
const PLANE: OriginFrame = OriginFrame {
    directions: &[
        OriginDirection { along: x_of, distance: "horizontal", onto: "y" },
        OriginDirection { along: y_of, distance: "vertical", onto: "x" },
    ],
    distance_command: "sketch.dimension",
    onto_command: "sketch.constraint.coincident",
};

/// `locate`: distances from the sketch origin to the shape's points along each direction of the
/// frame, centres first (a circle, arc or polygon is placed by its centre), each kept only where it
/// takes freedom away, so a shape anchored to the origin or to placed geometry gets none and the
/// rest end fully constrained. A point at 0 along a direction goes onto that axis instead. Nothing
/// here is per creator (it reads the points the creator reports) or per number of directions (it
/// reads the frame).
fn place_from_origin(scratch: &mut Session, out: &Value, params: &Value, mut dof: Option<u64>) -> Vec<Value> {
    let frame = &PLANE;
    let mut points: Vec<String> = out["created"]["points"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_string)).collect();
    points.sort_by_key(|p| !p.ends_with(".center"));
    let mut placed = Vec::new();
    for point in points.iter().take(MAX_LOCATED_POINTS) {
        for direction in frame.directions {
            if dof == Some(0) {
                return placed;
            }
            let Some(at) = coordinate(scratch, params, point) else { break };
            let (command, mut request) = if (direction.along)(at).abs() < 1e-6 {
                (frame.onto_command, json!({"a": point, "b": direction.onto}))
            } else {
                (frame.distance_command, json!({"entities": ["origin", point], "type": direction.distance}))
            };
            if let Some(sketch) = params.get("sketch") {
                request["sketch"] = sketch.clone();
            }
            // Refused (the sketch already determines it): nothing changed.
            let Ok(result) = scratch.execute(command, &request) else { continue };
            match (dof, dof_of(&result)) {
                (Some(before), Some(after)) if after >= before => {
                    // Accepted but redundant (a constraint the sketch already implies): take it back.
                    let _ = scratch.execute("edit.undo", &json!({}));
                }
                (_, after) => {
                    dof = after.or(dof);
                    placed.push(result);
                }
            }
        }
    }
    placed
}

/// Where a point of the target sketch is now, in the frame's coordinates: a plane sketch's
/// points have z = 0 (a 3D frame would read its points' own z).
fn coordinate(s: &Session, params: &Value, point: &str) -> Option<Vec3> {
    let id = super::target_sketch(s, params, "sketch.draw.dimensioned").ok()?;
    let sketch = s.doc.sketch(id).ok()?;
    let at = sketch.point(sketch.resolve_point(point)?)?;
    Some(Vec3::new(at.x, at.y, 0.0))
}

#[cfg(test)]
#[path = "sketch_dimensioned_tests.rs"]
mod tests;
