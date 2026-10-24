//! Shape creation and its typed driving dimensions form one command and one undo step.
use serde_json::{Value, json};
use solidcraft_doc::expr::Kind;

use crate::{
    Result, Session,
    params::{bad, expr, str_},
};

pub(super) fn draw(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.draw.dimensioned";
    let create = str_(p, "command").ok_or_else(|| bad(cmd, "`command` must name a supported sketch creator"))?;
    let roles: &[&str] = match create {
        "sketch.line" | "sketch.line.midpoint" => &["length", "angle"],
        "sketch.rectangle.two_point" | "sketch.rectangle.center" | "sketch.rectangle.three_point" | "sketch.rectangle.rounded" => {
            &["width", "height"]
        }
        "sketch.circle.center" | "sketch.circle.two_point" | "sketch.circle.three_point" => &["radius", "diameter"],
        "sketch.arc.center_point" => &["radius", "sweep"],
        "sketch.slot.center_to_center" | "sketch.slot.overall" | "sketch.slot.center_point" => &["length", "width"],
        "sketch.slot.arc_center" => &["radius", "sweep", "width"],
        "sketch.slot.arc_three_point" => &["width"],
        "sketch.polygon.inscribed" | "sketch.polygon.circumscribed" | "sketch.polygon.edge" => &["radius"],
        _ => return Err(bad(cmd, "unsupported sketch creator")),
    };
    let mut params = p.get("params").filter(|v| v.is_object()).cloned().ok_or_else(|| bad(cmd, "`params` must be an object"))?;
    let dimensions = p.get("dimensions").and_then(Value::as_object).ok_or_else(|| bad(cmd, "`dimensions` must be an object"))?;
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
    for (role, value) in dimensions {
        let (entities, ty) = match (create, role.as_str()) {
            ("sketch.line" | "sketch.line.midpoint", "length") => (vec![curve(0)?], "length"),
            ("sketch.line" | "sketch.line.midpoint", "angle") => (vec![curve(0)?], "angle"),
            // A rounded rectangle's sides stop at the arcs: its size is between opposite sides.
            ("sketch.rectangle.rounded", "width" | "height") => {
                let [a, b] = if role == "width" { ["left", "right"] } else { ["bottom", "top"] };
                let side =
                    |k: &str| out["sides"].get(k).filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "creator did not return its sides"));
                (vec![side(a)?, side(b)?], "distance")
            }
            (c, "width" | "height") if c.starts_with("sketch.rectangle.") => (vec![curve(usize::from(role == "height"))?], "length"),
            (c, "radius" | "diameter") if c.starts_with("sketch.circle.") => (vec![curve(0)?], role.as_str()),
            ("sketch.arc.center_point", "radius" | "sweep") => (vec![curve(0)?], role.as_str()),
            ("sketch.slot.center_to_center" | "sketch.slot.center_point", "length") => (vec![curve(0)?], "length"),
            ("sketch.slot.overall", "length") => (vec![curve(1)?, curve(3)?], "overall_length"),
            (c, "width") if c.starts_with("sketch.slot.") => (vec![curve(1)?], "diameter"),
            ("sketch.slot.arc_center", "radius" | "sweep") => {
                let centre = out.get("centre_arc").filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "slot centre arc missing"))?;
                (vec![centre], role.as_str())
            }
            (c, "radius") if c.starts_with("sketch.polygon.") => {
                let support =
                    out.get("support_circle").filter(|v| v.is_string()).cloned().ok_or_else(|| bad(cmd, "polygon support circle missing"))?;
                (vec![support], "radius")
            }
            _ => return Err(bad(cmd, "unsupported dimension role")),
        };
        // A typed size the sketch already has (a rectangle drawn onto another's side, the same
        // height) is placed as a driven (reference) dimension rather than refused.
        let mut dimension = json!({"entities": entities, "type": ty, "value": value, "if_redundant": "driven"});
        if let Some(sketch) = params.get("sketch") {
            dimension["sketch"] = sketch.clone();
        }
        let result = scratch.execute("sketch.dimension", &dimension)?;
        added.insert(role.clone(), result);
    }
    // Session::execute records the wrapper's before snapshot. Nested history must not leak.
    scratch.undo.clear();
    s.accept_computed(scratch);
    out["dimensions"] = Value::Object(added);
    Ok(out)
}

#[cfg(test)]
#[path = "sketch_dimensioned_tests.rs"]
mod tests;
