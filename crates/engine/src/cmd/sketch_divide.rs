//! Divide Curve (#8969): points that divide sketch curves into equal lengths.
//!
//! Each point is held by a [`ConstraintKind::PointAlong`] at its share k/N of the curve's length,
//! so it stays on the curve and keeps its share when the curve is edited, dragged or
//! dimensioned. The length is measured along the exact curve (arc length), not its parameter:
//! a spline's points are evenly spaced along it however its fit or control points are spread.
//! The curve itself is left as it is. Curves of 3D sketches and model edges are not divided yet.

use serde_json::{Value, json};
use solidcraft_sketch::{ConstraintKind, CurveKind, Sketch, point_along};

use super::sketch::{add_c, edit};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, num, string_list};
use crate::{Result, Session};

/// Most segments one curve is divided into.
pub(crate) const MAX_SEGMENTS: usize = 500;
/// Most curves one command divides.
const MAX_CURVES: usize = 200;
/// Most points one command adds.
const MAX_POINTS: usize = 2_000;

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("sketch.point.divide", "Divide Curve", divide)
    .at("SKETCH", "CREATE")
    .icon("point")
    .enabled(in_sketch)
    .params(concat!(
        "curve: id | curves: [ids] (each divided on its own); segments: whole number from 2 to 500. Places points that divide each curve into `segments` pieces of equal length along it (arc length): segments − 1 points on an open curve (line, arc, spline, conic), `segments` points on a closed one (a circle from its point on sketch +X, an ellipse from its major-axis end, a closed spline from its first point). Each point is constrained to its share of the curve's length, so it stays on the curve and keeps equal spacing as the curve changes; the curve is not changed. Infinite lines are refused. Returns points: [ids], per curve in order, and constraints: [ids]",
        created_doc!()
    ))];

/// Whether curve `c` is closed: its points go all the way round, from its start.
fn closed(sk: &Sketch, c: usize) -> bool {
    matches!(sk.curves.get(c).map(|c| &c.kind), Some(CurveKind::Circle { .. } | CurveKind::Ellipse { .. } | CurveKind::Spline { periodic: true, .. }))
}

/// The shares of the length where the points of `segments` equal pieces go.
pub(crate) fn shares(segments: usize, closed: bool) -> Vec<f64> {
    let first = usize::from(!closed);
    (first..segments).map(|k| k as f64 / segments as f64).collect()
}

fn divide(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "sketch.point.divide";
    let mut ids = string_list(cmd, p, "curves")?;
    ids.extend(string_list(cmd, p, "curve")?);
    if ids.is_empty() {
        return Err(bad(cmd, "`curve` (or `curves`) must name the curves to divide"));
    }
    if ids.len() > MAX_CURVES {
        return Err(bad(cmd, format!("at most {MAX_CURVES} curves at once")));
    }
    let n = num(p, "segments")
        .filter(|n| n.fract() == 0.0 && *n >= 2.0 && *n <= MAX_SEGMENTS as f64)
        .ok_or_else(|| bad(cmd, format!("`segments` must be a whole number from 2 to {MAX_SEGMENTS}")))? as usize;
    let ((points, constraints), info) = edit(s, p, cmd, true, |sk, _| {
        let mut curves = Vec::new();
        for id in &ids {
            let c = sk.curve_index(id).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            let curve = sk.curves.get(c).ok_or_else(|| bad(cmd, format!("unknown curve `{id}`")))?;
            if curve.infinite {
                return Err(bad(cmd, format!("`{id}` is an infinite line: it has no length to divide")));
            }
            if !curves.contains(&c) {
                curves.push(c);
            }
        }
        let total: usize = curves.iter().map(|c| shares(n, closed(sk, *c)).len()).sum();
        if total > MAX_POINTS {
            return Err(bad(cmd, format!("that would add {total} points; at most {MAX_POINTS} at once")));
        }
        let (mut points, mut constraints) = (Vec::new(), Vec::new());
        for c in curves {
            for at in shares(n, closed(sk, c)) {
                let id = sk.curves.get(c).map(|x| x.id.clone()).unwrap_or_default();
                let pos = point_along(sk, c, at).ok_or_else(|| bad(cmd, format!("`{id}` has no length to divide")))?;
                let q = sk.add_point(pos, None)?;
                constraints.push(add_c(sk, ConstraintKind::PointAlong { p: q, c, at })?);
                points.push(sk.points.get(q).map(|x| x.id.clone()).unwrap_or_default());
            }
        }
        Ok((points, constraints))
    })?;
    Ok(json!({"points": points, "constraints": constraints, "sketch": info}))
}

#[cfg(test)]
#[path = "sketch_divide_tests.rs"]
mod tests;
