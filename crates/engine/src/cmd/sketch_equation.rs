//! Equation curves (#8352): a sketch curve drawn from formulas, x(t) and y(t) over a range of t
//! or y = f(x) over a range of x, in the design's expression language and parameters. The curve
//! is a fit point spline through points worked out from the formulas, as many as keep it within
//! a tolerance of them; it keeps its formulas, so it follows the parameters and can be edited.
//! A `z(t)` formula (#12662) makes it a 3D curve instead, drawn as a 3D sketch curve (a wire) of
//! x(t), y(t) and z(t) in a 3D sketch, usable as a sweep or pipe path.

use serde_json::{Value, json};
use solidcraft_doc::sketch_equation::{self, DEFAULT_TOLERANCE, EquationFit, EquationFit3};
use solidcraft_sketch::{EquationCurve, Sketch, VariableUnit};

use super::sketch::{edit, mark_construction};
use super::{CommandSpec, in_sketch};
use crate::params::bad;
use crate::schema::sketch_equation::{EquationCurveParams, EquationCurveResult, EquationEditParams, EquationEditResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

#[cfg(test)]
#[path = "sketch_equation_tests.rs"]
mod tests;

const CREATE_CONTRACT: TypedContract<EquationCurveParams> = TypedContract::new();
const EDIT_CONTRACT: TypedContract<EquationEditParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("sketch.equation_curve", "Equation Curve", crate::checked!("sketch.equation_curve", create))
        .at("SKETCH", "CREATE")
        .icon("equation_curve")
        .enabled(in_sketch)
        .contract(&CREATE_CONTRACT)
        .output(output_schema_for::<EquationCurveResult>)
        .params(concat!(
            "x: x(t), y: y(t): formulas of t in the design's expression language and parameters (\"r * (cos(t) + t * sin(t))\"); or explicit: true and y: f(x) alone (no x); \
             z?: z(t): a formula of t too, lifting the curve off the sketch plane into a 3D curve through x(t), y(t) and z(t) in the sketch's own coordinates (as a 3D spline's points), which needs 3D Sketch on (sketch.options sketch_3d: true); the explicit form takes no z; \
             start, end: the range of t (or of x): a number or an expression (\"2 * pi\", \"360 deg\" is 2·pi, a length is in the design's unit); \
             tolerance?: mm (default 0.01): how far the curve may stray from the formulas (0.0001 to 10); construction?: bool (a plane curve only); \
             variable_unit?: \"number\" (default) or \"range\". \
             The variable is a plain number: sin(t) reads it in radians; a result without a unit is in the design's length unit. \
             With variable_unit \"range\" it is a length, so a ^ 2 / (4 * t) is a length when a is one: start and end must both be lengths, or one a length and the other a plain number, which is then in the design's unit. \
             Makes a fit point spline through fixed points, or (with z) a drawn 3D curve through fit points, as many as the tolerance needs (up to 500), closed smoothly when it ends where it starts; \
             the points are placed again from the formulas whenever the parameters change. Returns curve, points (how many), closed, deviation (mm)",
            created_doc!()
        )),
    CommandSpec::new("sketch.equation_curve.edit", "Edit Equation Curve", crate::checked!("sketch.equation_curve.edit", edit_curve))
        .icon("equation_curve")
        .enabled(in_sketch)
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EquationEditResult>)
        .params(
            "curve: an equation curve's id; x?, y?, z?, explicit?, start?, end?, tolerance?, variable_unit?: as in sketch.equation_curve, each kept when left out (explicit: true drops x and z; false needs x). \
             Giving z makes a 3D curve; z: null (or \"\") drops z and makes the curve a plane one, and explicit: true turns a 3D curve back into a plane curve; such a change makes a new curve, whose id the reply names. \
             The points are chosen again for the new formulas; the curve's start point (and the end point of an open curve) keeps its id, so geometry joined there stays joined. Returns curve, points, closed, deviation",
        ),
];

/// A formula or range parameter: a string (an expression) or a number.
fn text(p: &Value, cmd: &str, key: &str) -> Result<Option<String>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_string())),
        Some(Value::Number(n)) => n
            .as_f64()
            .and_then(crate::params::finite)
            .map(|v| Some(format!("{v}")))
            .ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number"))),
        Some(_) => Err(bad(cmd, format!("`{key}` must be a formula (a string) or a number"))),
    }
}

/// Whether a `z` parameter is there but empty: null, or a blank string.
fn absent(v: &Value) -> bool {
    matches!(v, Value::Null) || v.as_str().is_some_and(|s| s.trim().is_empty())
}

fn tolerance(p: &Value, cmd: &str) -> Result<Option<f64>> {
    match p.get("tolerance") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|t| t.is_finite() && (sketch_equation::MIN_TOLERANCE..=sketch_equation::MAX_TOLERANCE).contains(t))
            .map(Some)
            .ok_or_else(|| bad(cmd, "`tolerance` must be a number of mm from 0.0001 to 10")),
    }
}

fn explicit(p: &Value, cmd: &str) -> Result<Option<bool>> {
    match p.get("explicit") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad(cmd, "`explicit` must be true or false")),
    }
}

fn variable_unit(p: &Value, cmd: &str) -> Result<Option<VariableUnit>> {
    match p.get("variable_unit") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().and_then(VariableUnit::parse).map(Some).ok_or_else(|| bad(cmd, "`variable_unit` must be \"number\" or \"range\"")),
    }
}

/// The equation a command's parameters give, over `old` (each left out kept).
fn equation(p: &Value, cmd: &str, old: Option<&EquationCurve>) -> Result<EquationCurve> {
    let x = text(p, cmd, "x")?;
    let explicit = explicit(p, cmd)?;
    let x = match (explicit, x) {
        (Some(true), Some(_)) => return Err(bad(cmd, "an explicit curve takes y = f(x) alone: leave out `x`")),
        (Some(true), None) => None,
        (_, Some(x)) => Some(x),
        (Some(false), None) => match old.and_then(|o| o.x.clone()) {
            Some(x) => Some(x),
            None => return Err(bad(cmd, "a parametric curve needs `x`, a formula of t")),
        },
        (None, None) => match old {
            Some(o) => o.x.clone(),
            None => return Err(bad(cmd, "needs `x` and `y` (formulas of t), or explicit: true and `y` (a formula of x)")),
        },
    };
    // The explicit form is a plane curve: it has no z to keep or take (#12662). On an edit an
    // empty `z` (or null) is the window's empty row: it takes the formula away and the curve is a
    // plane curve again, while leaving `z` out keeps it.
    let z = match (explicit, p.get("z")) {
        (Some(true), Some(v)) if !absent(v) => return Err(bad(cmd, "an explicit curve y = f(x) is a plane curve: leave out `z`")),
        (Some(true), _) => None,
        (_, None) => old.and_then(|o| o.z.clone()),
        (_, Some(v)) if absent(v) => None,
        (_, Some(_)) => text(p, cmd, "z")?,
    };
    let pick = |key: &str, kept: Option<&String>| -> Result<String> {
        match text(p, cmd, key)? {
            Some(v) => Ok(v),
            None => kept.cloned().ok_or_else(|| bad(cmd, format!("missing `{key}`"))),
        }
    };
    let eq = EquationCurve {
        curve: old.map(|o| o.curve.clone()).unwrap_or_default(),
        x,
        y: pick("y", old.map(|o| &o.y))?,
        z,
        start: pick("start", old.map(|o| &o.start))?,
        end: pick("end", old.map(|o| &o.end))?,
        tolerance: tolerance(p, cmd)?.or(old.map(|o| o.tolerance)).unwrap_or(DEFAULT_TOLERANCE),
        variable_unit: variable_unit(p, cmd)?.or(old.map(|o| o.variable_unit)).unwrap_or_default(),
    };
    sketch_equation::check(&eq).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(eq)
}

/// The points of an equation, over the design's parameters as features are evaluated with them.
fn fit(doc: &solidcraft_doc::Document, eq: &EquationCurve, cmd: &str) -> Result<EquationFit> {
    sketch_equation::fit(&doc.evaluation_values(), eq).map_err(|e| bad(cmd, e.to_string()))
}

/// The 3D points of an equation with a `z` formula (#12662).
fn fit3(doc: &solidcraft_doc::Document, eq: &EquationCurve, cmd: &str) -> Result<EquationFit3> {
    sketch_equation::fit3(&doc.evaluation_values(), eq).map_err(|e| bad(cmd, e.to_string()))
}

/// A curve the command made or edited, as its reply names it.
struct Made {
    curve: String,
    points: usize,
    closed: bool,
    deviation: f64,
}

impl Made {
    fn planar(curve: String, fit: &EquationFit) -> Made {
        // A closed curve's last point is its first one.
        Made { curve, points: fit.points.len().saturating_sub(usize::from(fit.closed)), closed: fit.closed, deviation: fit.deviation }
    }

    fn spatial(curve: String, fit: &EquationFit3) -> Made {
        Made { curve, points: fit.points.len().saturating_sub(usize::from(fit.closed)), closed: fit.closed, deviation: fit.deviation }
    }

    /// The reply's fields, with the sketch's solve.
    fn reply(&self, info: Value) -> Value {
        json!({"curve": self.curve, "points": self.points, "closed": self.closed, "deviation": self.deviation, "sketch": info})
    }
}

/// Draw `eq` in the sketch: a fit point spline of its plane, or (with a `z` formula, #12662) a
/// drawn 3D curve of the sketch's own 3D space. `old` is the equation of the curve being edited:
/// it keeps its id, unless the edit changes the curve between the plane and 3D kinds, which the
/// sketch keeps as different geometry (`curve` then names the new one).
fn place(sk: &mut Sketch, doc: &solidcraft_doc::Document, p: &Value, cmd: &str, eq: &EquationCurve, old: Option<&EquationCurve>) -> Result<Made> {
    let err = |e: solidcraft_sketch::SketchError| bad(cmd, e.to_string());
    if eq.is_3d() {
        if !sk.view.three_d {
            return Err(bad(cmd, "a curve with z(t) leaves the sketch plane: turn on 3D Sketch (sketch.options sketch_3d: true)"));
        }
        let fit = fit3(doc, eq, cmd)?;
        if let Some(o) = old.filter(|o| o.is_3d()) {
            sk.replace_equation_wire(&o.curve, &fit.points, eq.clone()).map_err(err)?;
            return Ok(Made::spatial(o.curve.clone(), &fit));
        }
        remove_old(sk, old);
        let id = sk.add_equation_wire(&fit.points, eq.clone()).map_err(err)?;
        return Ok(Made::spatial(id, &fit));
    }
    let fit = fit(doc, eq, cmd)?;
    match old.map(|o| (&o.curve, o.is_3d())) {
        Some((id, false)) => {
            let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            sk.replace_equation_curve(c, &fit.points, fit.closed, eq.clone()).map_err(err)?;
            Ok(Made::planar(id.clone(), &fit))
        }
        _ => {
            remove_old(sk, old);
            let c = sk.add_equation_curve(&fit.points, fit.closed, eq.clone()).map_err(err)?;
            // The curve is new geometry, not the one that had the old curve's construction flag.
            mark_construction(sk, &[c], p);
            let id = sk.curves.get(c).map(|c| c.id.clone()).ok_or_else(|| bad(cmd, "the new curve is missing"))?;
            Ok(Made::planar(id, &fit))
        }
    }
}

/// Remove the curve an edited one leaves behind when it becomes the other kind: its geometry, the
/// points only it placed, and its formulas.
fn remove_old(sk: &mut Sketch, old: Option<&EquationCurve>) {
    let Some(old) = old else { return };
    if old.is_3d() {
        sk.remove_wires(std::slice::from_ref(&old.curve));
    } else if let Some(c) = sk.curve_index(&old.curve) {
        sk.remove_curves(&[c]);
    }
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.equation_curve";
    let eq = equation(p, cmd, None)?;
    let (made, info) = edit(s, p, cmd, false, |sk: &mut Sketch, doc| place(sk, doc, p, cmd, &eq, None))?;
    let mut out = made.reply(info);
    if let Some(o) = out.as_object_mut() {
        o.insert("curves".into(), json!([made.curve]));
    }
    Ok(out)
}

fn edit_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.equation_curve.edit";
    let id = p.get("curve").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`curve` must be an equation curve's id"))?.to_string();
    let (made, info) = edit(s, p, cmd, false, |sk: &mut Sketch, doc| {
        if sk.curve_index(&id).is_none() && sk.wire_index(&id).is_none() {
            return Err(bad(cmd, format!("unknown curve `{id}`")));
        }
        let old = sk.equation_of(&id).cloned().ok_or_else(|| bad(cmd, format!("`{id}` is not an equation curve")))?;
        let eq = equation(p, cmd, Some(&old))?;
        place(sk, doc, p, cmd, &eq, Some(&old))
    })?;
    Ok(made.reply(info))
}
