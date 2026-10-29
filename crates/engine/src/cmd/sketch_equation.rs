//! Equation curves (#8352): a sketch curve drawn from formulas, x(t) and y(t) over a range of t
//! or y = f(x) over a range of x, in the design's expression language and parameters. The curve
//! is a fit point spline through points worked out from the formulas, as many as keep it within
//! a tolerance of them; it keeps its formulas, so it follows the parameters and can be edited.

use serde_json::{Value, json};
use solidcraft_doc::sketch_equation::{self, DEFAULT_TOLERANCE, EquationFit};
use solidcraft_sketch::{EquationCurve, Sketch};

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
             start, end: the range of t (or of x): a number or an expression (\"2 * pi\", \"360 deg\" is 2·pi, a length is in the design's unit); \
             tolerance?: mm (default 0.01): how far the curve may stray from the formulas (0.0001 to 10); construction?: bool. \
             The variable is a plain number: sin(t) reads it in radians; a result without a unit is in the design's length unit. \
             Makes a fit point spline through fixed points, as many as the tolerance needs (up to 500), closed smoothly when it ends where it starts; \
             the points are placed again from the formulas whenever the parameters change. Returns curve, points (how many), closed, deviation (mm)",
            created_doc!()
        )),
    CommandSpec::new("sketch.equation_curve.edit", "Edit Equation Curve", crate::checked!("sketch.equation_curve.edit", edit_curve))
        .icon("equation_curve")
        .enabled(in_sketch)
        .contract(&EDIT_CONTRACT)
        .output(output_schema_for::<EquationEditResult>)
        .params(
            "curve: an equation curve's id; x?, y?, explicit?, start?, end?, tolerance?: as in sketch.equation_curve, each kept when left out (explicit: true drops x; false needs x). \
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
        start: pick("start", old.map(|o| &o.start))?,
        end: pick("end", old.map(|o| &o.end))?,
        tolerance: tolerance(p, cmd)?.or(old.map(|o| o.tolerance)).unwrap_or(DEFAULT_TOLERANCE),
    };
    sketch_equation::check(&eq).map_err(|e| bad(cmd, e.to_string()))?;
    Ok(eq)
}

/// The points of an equation, over the design's parameters as features are evaluated with them.
fn fit(doc: &solidcraft_doc::Document, eq: &EquationCurve, cmd: &str) -> Result<EquationFit> {
    sketch_equation::fit(&doc.evaluation_values(), eq).map_err(|e| bad(cmd, e.to_string()))
}

fn reply(curve: String, fit: &EquationFit, info: Value) -> Value {
    // A closed curve's last point is its first one.
    let points = fit.points.len().saturating_sub(usize::from(fit.closed));
    json!({"curves": [curve.clone()], "curve": curve, "points": points, "closed": fit.closed, "deviation": fit.deviation, "sketch": info})
}

fn create(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.equation_curve";
    let eq = equation(p, cmd, None)?;
    let ((curve, fit), info) = edit(s, p, cmd, false, |sk: &mut Sketch, doc| {
        let fit = fit(doc, &eq, cmd)?;
        let c = sk.add_equation_curve(&fit.points, fit.closed, eq.clone()).map_err(|e| bad(cmd, e.to_string()))?;
        mark_construction(sk, &[c], p);
        let id = sk.curves.get(c).map(|c| c.id.clone()).ok_or_else(|| bad(cmd, "the new curve is missing"))?;
        Ok((id, fit))
    })?;
    Ok(reply(curve, &fit, info))
}

fn edit_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.equation_curve.edit";
    let id = p.get("curve").and_then(Value::as_str).ok_or_else(|| bad(cmd, "`curve` must be an equation curve's id"))?.to_string();
    let (fit, info) = edit(s, p, cmd, false, |sk: &mut Sketch, doc| {
        let c = sk.curve_index(&id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
        let old = sk.equation_of(&id).cloned().ok_or_else(|| bad(cmd, format!("`{id}` is not an equation curve")))?;
        let eq = equation(p, cmd, Some(&old))?;
        let fit = fit(doc, &eq, cmd)?;
        sk.replace_equation_curve(c, &fit.points, fit.closed, eq).map_err(|e| bad(cmd, e.to_string()))?;
        Ok(fit)
    })?;
    let mut out = reply(id, &fit, info);
    if let Some(o) = out.as_object_mut() {
        o.remove("curves");
    }
    Ok(out)
}
