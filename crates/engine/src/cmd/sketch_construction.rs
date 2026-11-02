//! Fast reference lines, infinite or with a length (#10258). Preview and creation use the same
//! analytic solutions.

use serde_json::{Value, json};
use solidcraft_geom::{Seg2, Vec2};
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch};

use super::sketch::{add_c, curve_ref, edit, ids_of, line_or_axis_ref, line_pts, req_parg, result};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, expr, num, str_, vec2};
use crate::schema::sketch_create::{ConstructionLineParams, ConstructionLineResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CONTRACT: TypedContract<ConstructionLineParams> = TypedContract::new();
const CMD: &str = "sketch.construction_line";

macro_rules! variant {
    ($id:literal, $label:literal, $mode:literal) => {
        CommandSpec::new($id, $label, |s, p| {
            crate::schema::run_checked_as(s, p, $id, |s, p| run(s, p, $mode))
        })
        .at("SKETCH", "CREATE")
        .enabled(in_sketch)
        .contract(&CONTRACT)
        .output(output_schema_for::<ConstructionLineResult>)
        .params(concat!("Construction line, ", $mode, ". at: point; through?: second point; curve?, curve2?: sketch references; angle?: degrees from sketch X; offset?: positive length (parallel: distance from the reference line; horizontal: distance from sketch X; vertical: distance from sketch Y; the side follows at) and a driving dimension at that value; near?: solution selector, and the side a line with a length runs towards; solution?: zero-based solution index; length?: a segment of this length from `at`, held by a length dimension, instead of an infinite line (horizontal, vertical, angle, parallel, perpendicular); construction?: false makes that segment a normal line. Tangents take circles/arcs, or an open spline/conic endpoint for tangent_at.", created_doc!()))
    };
}

pub static COMMANDS: &[CommandSpec] = &[
    variant!("sketch.construction_line", "Construction Line", "free").icon("construction_line"),
    variant!("sketch.construction_line.horizontal", "Horizontal Construction Line", "horizontal").icon("construction_line_horizontal"),
    variant!("sketch.construction_line.vertical", "Vertical Construction Line", "vertical").icon("construction_line_vertical"),
    variant!("sketch.construction_line.angle", "Construction Line at Angle", "angle").icon("construction_line_angle"),
    variant!("sketch.construction_line.parallel", "Parallel Construction Line", "parallel").icon("construction_line_parallel"),
    variant!("sketch.construction_line.perpendicular", "Perpendicular Construction Line", "perpendicular").icon("construction_line_perpendicular"),
    variant!("sketch.construction_line.tangent_external", "Tangent through External Point", "tangent_external")
        .icon("construction_line_tangent_external"),
    variant!("sketch.construction_line.tangent_at", "Tangent at Curve Point", "tangent_at").icon("construction_line_tangent_at"),
    variant!("sketch.construction_line.tangent_two", "Common Tangent", "tangent_two").icon("construction_line_tangent_two"),
    variant!("sketch.construction_line.bisector", "Angle Bisector", "bisector").icon("construction_line_bisector"),
];

/// One mathematical solution, before adding any sketch entities.
#[derive(Clone, Copy, Debug)]
pub struct ConstructionLine {
    pub start: Vec2,
    pub direction: Vec2,
}

impl ConstructionLine {
    fn distance(self, p: Vec2) -> f64 {
        (p - self.start).cross(self.direction).abs()
    }
}

/// The methods that take a `length`: the ones whose line starts at `at` with a known direction.
pub fn construction_line_takes_length(mode: &str) -> bool {
    matches!(mode, "horizontal" | "vertical" | "angle" | "parallel" | "perpendicular")
}

/// The two ends of a construction line cut to `length` from its start: along its direction, or
/// the other way when `near` lies behind the start. The preview draws exactly what is created.
pub fn construction_segment(line: ConstructionLine, length: f64, near: Option<Vec2>) -> (Vec2, Vec2) {
    let behind = near.is_some_and(|q| (q - line.start).dot(line.direction) < 0.0);
    let direction = if behind { -line.direction } else { line.direction };
    (line.start, line.start + direction * length)
}

/// The method belonging to an interactive command. The generic command defaults to two points.
pub fn construction_line_mode(id: &str) -> Option<&str> {
    if id == CMD { Some("free") } else { id.strip_prefix("sketch.construction_line.") }
}

fn point(sk: &Sketch, v: Option<&Value>, key: &str) -> Result<Vec2> {
    v.and_then(|v| vec2(v).or_else(|| v.as_str().and_then(|id| sk.resolve_point(id)).and_then(|i| sk.point(i))))
        .ok_or_else(|| bad(CMD, format!("`{key}` must be a finite point or sketch point reference")))
}

fn reference(sk: &Sketch, p: &Value, key: &str) -> Result<usize> {
    curve_ref(sk, p.get(key), CMD, key)
}

fn line(sk: &Sketch, p: &Value, key: &str) -> Result<(Vec2, Vec2)> {
    match str_(p, key) {
        Some("x") if sk.curve_index("x").is_none() => Ok((Vec2::ZERO, Vec2::X)),
        Some("y") if sk.curve_index("y").is_none() => Ok((Vec2::ZERO, Vec2::Y)),
        _ => line_pts(sk, reference(sk, p, key)?).ok_or_else(|| bad(CMD, "the reference must be a straight line")),
    }
}

fn round(sk: &Sketch, i: usize) -> Result<(Vec2, f64)> {
    let c = sk.center(i).ok_or_else(|| bad(CMD, "the reference must be a circle or circular arc"))?;
    let r = sk
        .radius(i)
        .filter(|r| r.is_finite() && *r > 1e-9)
        .ok_or_else(|| bad(CMD, "the reference must be a non-degenerate circle or circular arc"))?;
    Ok((c, r))
}

/// A circular arc accepts tangency on its drawn sweep, including either end.
fn on_sweep(sk: &Sketch, i: usize, q: Vec2) -> bool {
    if matches!(sk.curves.get(i).map(|c| &c.kind), Some(CurveKind::Circle { .. })) {
        return true;
    }
    sk.segs(i).iter().any(|s| match s {
        Seg2::Arc { center, start, sweep, .. } => {
            let theta = (q - *center).angle();
            let span = if *sweep >= 0.0 { theta - start } else { start - theta };
            span.rem_euclid(std::f64::consts::TAU) <= sweep.abs() + 1e-8 || span.rem_euclid(std::f64::consts::TAU) > std::f64::consts::TAU - 1e-8
        }
        _ => false,
    })
}

fn endpoint_tangent(sk: &Sketch, i: usize, at: Vec2) -> Option<(usize, Vec2)> {
    let ends = sk.curves.get(i)?.kind.ends()?;
    [ends.0, ends.1].into_iter().find_map(|pi| {
        let q = sk.point(pi)?;
        (q.dist(at) <= 1e-6).then(|| super::sketch_create::leaving_dir(sk, i, pi).map(|d| (pi, d))).flatten()
    })
}

/// A numeric offset already evaluated in millimetres. An expression is resolved by the command
/// before it asks for solutions; a missing or non-positive value means "through `at`".
fn positive_offset(p: &Value) -> Option<f64> {
    num(p, "offset").filter(|d| d.is_finite() && *d > 1e-9)
}

fn offset_side(signed: f64) -> f64 {
    if signed < 0.0 { -1.0 } else { 1.0 }
}

/// The typed offset as its driving parameter and the length in millimetres. `name = expression`
/// defines the user parameter `name`, the same way a dimension does.
fn typed_offset(doc: &mut solidcraft_doc::Document, p: &Value, mode: &str) -> Result<Option<(String, f64)>> {
    if p.get("offset").is_none() {
        return Ok(None);
    }
    if !matches!(mode, "parallel" | "horizontal" | "vertical") {
        return Err(bad(CMD, "`offset` applies to a parallel, horizontal or vertical construction line"));
    }
    let raw = expr(p, "offset").ok_or_else(|| bad(CMD, "`offset` must be a positive length or expression"))?;
    let (defines, rhs) = match solidcraft_doc::expr::definition(raw.as_str()) {
        Some((name, "")) => return Err(bad(CMD, format!("offset: `{name} =` needs a value after the `=`"))),
        Some((name, rhs)) => (Some(name), rhs),
        None => (None, raw.as_str()),
    };
    let rhs = doc.with_design_unit(rhs, solidcraft_doc::expr::Kind::Length);
    let mm = doc.eval(&rhs, solidcraft_doc::expr::Kind::Length).map_err(|e| bad(CMD, format!("offset: {e}")))?;
    if !mm.is_finite() || mm <= 1e-9 || mm >= 1e9 {
        return Err(bad(CMD, "offset must be positive and within the model's size limits"));
    }
    let unit = solidcraft_doc::expr::length_unit_or_mm(&doc.units).to_string();
    let stored = if let Some(name) = defines {
        if let Some(problem) = solidcraft_doc::Document::param_name_problem(name) {
            return Err(bad(CMD, format!("offset: {problem}")));
        }
        if doc.param(name).is_some() {
            return Err(bad(CMD, format!("offset: there is already a parameter `{name}`: type `{name}` to use it, or change it in Parameters")));
        }
        doc.add_user_param(name, &rhs, Some(&unit), None).map_err(|e| bad(CMD, format!("offset: {e}")))?;
        name.to_string()
    } else {
        rhs
    };
    Ok(Some((doc.new_model_param(&stored, &unit), mm)))
}

/// All valid solutions in deterministic order. This reads the sketch and never changes it.
pub fn construction_line_solutions(sk: &Sketch, p: &Value, mode: &str) -> Result<Vec<ConstructionLine>> {
    let at = point(sk, p.get("at"), "at")?;
    let mut out = Vec::new();
    let mut push = |start: Vec2, d: Vec2| {
        if let Some(direction) = d.normalized()
            && start.is_finite()
            && direction.is_finite()
        {
            out.push(ConstructionLine { start, direction });
        }
    };
    match mode {
        "free" => push(at, point(sk, p.get("through"), "through")? - at),
        "horizontal" => {
            let y = positive_offset(p).map_or(at.y, |d| offset_side(at.y) * d);
            push(Vec2::new(at.x, y), Vec2::X);
        }
        "vertical" => {
            let x = positive_offset(p).map_or(at.x, |d| offset_side(at.x) * d);
            push(Vec2::new(x, at.y), Vec2::Y);
        }
        "angle" => {
            let a = num(p, "angle").ok_or_else(|| bad(CMD, "`angle` must be finite degrees"))?;
            push(at, Vec2::from_angle(a.rem_euclid(360.0).to_radians()));
        }
        "parallel" | "perpendicular" => {
            let (a, b) = line(sk, p, "curve")?;
            let along = b - a;
            if mode == "perpendicular" {
                push(at, along.perp());
            } else if let Some(distance) = positive_offset(p) {
                let dir = along.normalized().ok_or_else(|| bad(CMD, "the reference has no direction"))?;
                let normal = dir.perp();
                let side = offset_side(normal.dot(at - a));
                let origin = a + normal * (side * distance);
                push(origin + dir * (at - origin).dot(dir), dir);
            } else {
                push(at, along);
            }
        }
        "bisector" => {
            let (a, b) = line(sk, p, "curve")?;
            let (c, d) = line(sk, p, "curve2")?;
            let u = (b - a).normalized().ok_or_else(|| bad(CMD, "the reference has no direction"))?;
            let v = (d - c).normalized().ok_or_else(|| bad(CMD, "the reference has no direction"))?;
            let cross = u.cross(v);
            if cross.abs() < 1e-9 {
                return Err(bad(CMD, "the two lines must intersect"));
            }
            let origin = a + u * ((c - a).cross(v) / cross);
            push(origin, u + v);
            push(origin, u - v);
        }
        "tangent_at" => {
            let i = reference(sk, p, "curve")?;
            if let Ok((c, r)) = round(sk, i) {
                let radial = (at - c).normalized().ok_or_else(|| bad(CMD, "the point must differ from the centre"))?;
                let q = c + radial * r;
                if on_sweep(sk, i, q) {
                    push(q, radial.perp());
                }
            } else if let Some((pi, d)) = endpoint_tangent(sk, i, at) {
                push(sk.point(pi).unwrap_or(at), d);
            } else {
                return Err(bad(CMD, "pick a point on a circle/arc, or an endpoint of an open spline or conic"));
            }
        }
        "tangent_external" => {
            let i = reference(sk, p, "curve")?;
            let (c, r) = round(sk, i)?;
            let delta = at - c;
            let distance = delta.len();
            if distance <= r + 1e-9 {
                return Err(bad(CMD, "the point must be outside the circle or arc"));
            }
            let u = delta / distance;
            let ratio = r / distance;
            for side in [1.0, -1.0] {
                let contact = c + (u * ratio + u.perp() * (side * (1.0 - ratio * ratio).max(0.0).sqrt())) * r;
                if on_sweep(sk, i, contact) {
                    push(at, contact - at);
                }
            }
        }
        "tangent_two" => {
            let a = reference(sk, p, "curve")?;
            let b = reference(sk, p, "curve2")?;
            if a == b {
                return Err(bad(CMD, "choose two different curves"));
            }
            let (c1, r1) = round(sk, a)?;
            let (c2, r2) = round(sk, b)?;
            let delta = c2 - c1;
            let distance = delta.len();
            let u = delta.normalized().ok_or_else(|| bad(CMD, "concentric curves have no unique common tangent"))?;
            for sign in [1.0, -1.0] {
                let along = (r1 - sign * r2) / distance;
                if along.abs() > 1.0 + 1e-12 {
                    continue;
                }
                let along = along.clamp(-1.0, 1.0);
                for side in [1.0, -1.0] {
                    let n = u * along + u.perp() * (side * (1.0 - along * along).max(0.0).sqrt());
                    let q1 = c1 + n * r1;
                    let q2 = c2 + n * (sign * r2);
                    if on_sweep(sk, a, q1) && on_sweep(sk, b, q2) {
                        push(q1, n.perp());
                    }
                }
            }
        }
        _ => return Err(bad(CMD, "unknown construction method")),
    }
    out.dedup_by(|a, b| a.direction.cross(b.direction).abs() < 1e-9 && a.distance(b.start) < 1e-8);
    if out.is_empty() {
        return Err(bad(CMD, "no construction line satisfies these inputs"));
    }
    if out.iter().any(|l| l.start.x.abs() >= 1e9 || l.start.y.abs() >= 1e9) {
        return Err(bad(CMD, "the construction is outside the supported coordinate range"));
    }
    Ok(out)
}

/// Which preview solution a click selects. Equal distances keep the deterministic first one.
pub fn construction_line_choice(lines: &[ConstructionLine], p: &Value) -> Result<usize> {
    if let Some(v) = p.get("solution") {
        let i = v.as_u64().and_then(|i| usize::try_from(i).ok()).filter(|i| *i < lines.len());
        return i.ok_or_else(|| bad(CMD, "`solution` is outside the available solutions"));
    }
    if let Some(near) = p.get("near") {
        let near = vec2(near).ok_or_else(|| bad(CMD, "`near` must be a finite point"))?;
        return lines
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| a.distance(near).total_cmp(&b.distance(near)))
            .map(|(i, _)| i)
            .ok_or_else(|| bad(CMD, "there are no solutions"));
    }
    if p.get("alternate").is_some_and(|v| !v.is_boolean()) {
        return Err(bad(CMD, "`alternate` must be a boolean"));
    }
    Ok(usize::from(p.get("alternate").and_then(Value::as_bool).unwrap_or(false)).min(lines.len().saturating_sub(1)))
}

fn run(s: &mut Session, p: &Value, default_mode: &str) -> Result<Value> {
    let mode = str_(p, "mode").unwrap_or(default_mode);
    if default_mode != "free" && mode != default_mode {
        return Err(bad(CMD, "`mode` does not match this construction command"));
    }
    let p = super::sketch_project::snap_points(s, p, CMD)?;
    let (out, info) = edit(s, &p, CMD, true, |sk, doc| {
        let first_new = sk.points.len();
        let at = req_parg(sk, doc, &p, "at", CMD)?;
        let through = if mode == "free" { Some(req_parg(sk, doc, &p, "through", CMD)?) } else { None };
        let mut params = p.clone();
        params["at"] = json!([at.pos().x, at.pos().y]);
        let angle_expression = if mode == "angle" {
            let e = expr(&p, "angle").ok_or_else(|| bad(CMD, "`angle` must be an angle or expression"))?;
            let e = doc.with_design_unit(e.as_str(), solidcraft_doc::expr::Kind::Angle);
            let radians = doc.eval(&e, solidcraft_doc::expr::Kind::Angle).map_err(|e| bad(CMD, format!("angle: {e}")))?;
            if !radians.is_finite() || radians.abs() > 1e9 {
                return Err(bad(CMD, "angle is outside the supported range"));
            }
            params["angle"] = json!(radians.to_degrees());
            Some(e)
        } else {
            None
        };
        let offset = typed_offset(doc, &p, mode)?;
        if let Some((_, mm)) = &offset {
            params["offset"] = json!(*mm);
        }
        if let Some(q) = &through {
            params["through"] = json!([q.pos().x, q.pos().y]);
        }
        let reference = if matches!(mode, "parallel" | "perpendicular" | "bisector") {
            Some(line_or_axis_ref(sk, p.get("curve"), CMD, "curve")?)
        } else if mode.starts_with("tangent_") {
            Some(curve_ref(sk, p.get("curve"), CMD, "curve")?)
        } else {
            None
        };
        let second = if mode == "bisector" {
            Some(line_or_axis_ref(sk, p.get("curve2"), CMD, "curve2")?)
        } else if mode == "tangent_two" {
            Some(curve_ref(sk, p.get("curve2"), CMD, "curve2")?)
        } else {
            None
        };
        let length = match p.get("length") {
            None | Some(Value::Null) => None,
            Some(_) if !construction_line_takes_length(mode) => {
                return Err(bad(CMD, "`length` is for the horizontal, vertical, angle, parallel and perpendicular methods"));
            }
            Some(_) => {
                let e = expr(&p, "length").ok_or_else(|| bad(CMD, "`length` must be a length or expression"))?;
                let e = doc.with_design_unit(e.as_str(), solidcraft_doc::expr::Kind::Length);
                let value = doc.eval(&e, solidcraft_doc::expr::Kind::Length).map_err(|e| bad(CMD, format!("length: {e}")))?;
                if !(value > 1e-9 && value < 1e8) {
                    return Err(bad(CMD, "length must be positive"));
                }
                Some((e, value))
            }
        };
        let construction = match p.get("construction") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err(bad(CMD, "`construction` must be a boolean")),
        };
        if !construction && length.is_none() {
            return Err(bad(CMD, "only a line with a `length` can be a normal line: an infinite line is construction geometry"));
        }
        let solutions = construction_line_solutions(sk, &params, mode)?;
        let choice = construction_line_choice(&solutions, &params)?;
        let chosen = solutions.get(choice).ok_or_else(|| bad(CMD, "no solution"))?;
        // A typed offset does not pass through the picked point, so that point is not the line's start.
        let start_ref = if offset.is_some() {
            None
        } else if mode == "tangent_at" {
            if let Some(i) = at.idx() {
                if at.pos().dist(chosen.start) > 1e-6 {
                    return Err(bad(CMD, "the referenced point must lie on the curve"));
                }
                Some(i)
            } else {
                reference.and_then(|i| endpoint_tangent(sk, i, chosen.start).map(|(pi, _)| pi))
            }
        } else if matches!(mode, "tangent_two" | "bisector") {
            None
        } else {
            at.idx()
        };
        let near = p.get("near").and_then(vec2);
        let (start, end) = match &length {
            Some((_, value)) => construction_segment(*chosen, *value, near),
            None => (chosen.start, through.as_ref().map_or(chosen.start + chosen.direction * 10.0, |q| q.pos())),
        };
        let l = sk.add_line(start, end, start_ref, through.as_ref().and_then(|q| q.idx()), None)?;
        if let Some(c) = sk.curves.get_mut(l) {
            c.infinite = length.is_none();
            c.construction = construction;
        }
        let mut cons = Vec::new();
        let kind = match (mode, reference) {
            ("horizontal", _) => Some(ConstraintKind::Horizontal { l }),
            ("vertical", _) => Some(ConstraintKind::Vertical { l }),
            ("angle", _) => Some(ConstraintKind::LineAngle { l, value: chosen.direction.angle() }),
            ("parallel", Some(b)) => Some(ConstraintKind::Parallel { a: l, b }),
            ("perpendicular", Some(b)) => Some(ConstraintKind::Perpendicular { a: l, b }),
            ("tangent_at" | "tangent_external" | "tangent_two", Some(b)) => Some(ConstraintKind::Tangent { a: l, b }),
            _ => None,
        };
        if let Some(k) = kind {
            let parameter = angle_expression.map(|e| doc.new_model_param(&e, "deg"));
            cons.push(sk.add_constraint(k, parameter)?);
        }
        if let Some((e, value)) = &length {
            let unit = solidcraft_doc::expr::length_unit_or_mm(&doc.units).to_string();
            let parameter = doc.new_model_param(e, &unit);
            cons.push(sk.add_constraint(ConstraintKind::Length { l, value: *value }, Some(parameter))?);
        }
        if let ("tangent_two", Some(b)) = (mode, second) {
            cons.push(add_c(sk, ConstraintKind::Tangent { a: l, b })?);
        }
        let start = sk.curves.get(l).and_then(|c| c.kind.ends()).map(|(a, _)| a).ok_or_else(|| bad(CMD, "no line start"))?;
        if let Some((pname, mm)) = offset {
            let distance = match mode {
                "parallel" => {
                    let reference = reference.ok_or_else(|| bad(CMD, "parallel offset needs a reference line"))?;
                    ConstraintKind::PointLineDistance { p: start, l: reference, value: mm }
                }
                "horizontal" => {
                    let origin = sk.point_index("origin").ok_or_else(|| bad(CMD, "the sketch has no origin"))?;
                    ConstraintKind::DistanceY { p: origin, q: start, value: mm }
                }
                "vertical" => {
                    let origin = sk.point_index("origin").ok_or_else(|| bad(CMD, "the sketch has no origin"))?;
                    ConstraintKind::DistanceX { p: origin, q: start, value: mm }
                }
                _ => return Err(bad(CMD, "`offset` does not apply to this construction")),
            };
            cons.push(sk.add_constraint(distance, Some(pname))?);
        }
        if let ("tangent_at", Some(c)) = (mode, reference) {
            let already_on =
                sk.constraints.iter().any(|k| matches!(k.kind, ConstraintKind::PointOnCurve { p, c: curve } if p == start && curve == c));
            let own_end = sk.curves.get(c).and_then(|c| c.kind.ends()).is_some_and(|(a, b)| a == start || b == start);
            if sk.radius(c).is_some() && !already_on && !own_end {
                cons.push(add_c(sk, ConstraintKind::PointOnCurve { p: start, c })?);
            }
        }
        if let ("bisector", Some(a), Some(b)) = (mode, reference, second) {
            let (a0, a1) = sk.curves.get(a).and_then(|c| c.kind.ends()).ok_or_else(|| bad(CMD, "the reference must be a line"))?;
            let pa = if sk.point(a1).is_some_and(|q| q.dist(chosen.start) > 1e-6) { a1 } else { a0 };
            let source = sk.point(pa).ok_or_else(|| bad(CMD, "missing reference point"))?;
            let relative = source - chosen.start;
            let mirror = chosen.start + chosen.direction * (2.0 * relative.dot(chosen.direction)) - relative;
            let pb = sk.add_point(mirror, None)?;
            for k in [
                ConstraintKind::PointOnCurve { p: start, c: a },
                ConstraintKind::PointOnCurve { p: start, c: b },
                ConstraintKind::PointOnCurve { p: pb, c: b },
                ConstraintKind::Symmetric { p: pa, q: pb, l },
            ] {
                cons.push(add_c(sk, k)?);
            }
        }
        super::sketch_create::solve_new_only(sk, first_new);
        Ok((ids_of(sk, &[l]), cons))
    })?;
    Ok(result(out, info))
}
