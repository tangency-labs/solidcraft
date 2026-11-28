//! Offset Curve (`surface.offset_curve`, #22414): a curve at a distance from an edge of a face, or
//! from a curve lying on it, measured along the face, added to the active sketch as a linked 3D
//! curve (as Isoparametric Curve and Intersection Curve add theirs). The geometry is the kernel's
//! (`solidcraft_kernel::offset_on_face`), reached through the link's resolution in the document.

use serde_json::Value;
use solidcraft_sketch::{LinkKind, LinkSource};

use super::sketch_project::{one_link, source};
use super::{CommandSpec, in_sketch};
use crate::params::{bad, num, str_, vec3};
use crate::schema::surface_offset_curve::{FROM_MESSAGE, OffsetCurveParams, OffsetCurveResult};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const OFFSET_CURVE_CONTRACT: TypedContract<OffsetCurveParams> = TypedContract::new();

pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new("surface.offset_curve", "Offset Curve", crate::checked!("surface.offset_curve", offset_curve))
    .at("SURFACE", "CREATE")
    .icon("surface_offset")
    .enabled(in_sketch)
    .contract(&OFFSET_CURVE_CONTRACT)
    .output(output_schema_for::<OffsetCurveResult>)
    .params(concat!(
        "face: [x,y,z] (a point on the face; it picks the side when the face lies on both sides of the source), body?; from: {edge: [x,y,z]} (an edge of the face: the curve goes into the face) | {sketch, curve} (a curve or 3D curve of another sketch lying on the face); distance: mm (> 0), measured along the face: each point that far along the straightest path on the face leaving the source square to it (a plane: the ordinary offset; a cylinder, cone or extrusion: the offset in the face unrolled); tolerance?: mm (default 0.01: how far the fitted cubic spline through the offset's points may be from the true offset, measured between them). Where the offset would leave the face it stops at the face's edge; an offset that leaves the face's surface, one that folds over itself (the distance past the source's radius of curvature on the face), a source not on the face and a pole on the way are refused. Needs an active sketch: a linked 3D curve that follows the face and the source; the reply gives the deviation and whether the face's edge cut it",
        created_doc!()
    ))];

fn offset_curve(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "surface.offset_curve";
    let at = p.get("face").and_then(vec3).ok_or_else(|| bad(cmd, "`face` must be a point [x,y,z] on the face"))?;
    let body = str_(p, "body").unwrap_or("").to_string();
    let from = match source(s, p.get("from").ok_or_else(|| bad(cmd, FROM_MESSAGE))?, cmd)? {
        x @ (LinkSource::Edge { .. } | LinkSource::SketchCurve { .. }) => x,
        _ => return Err(bad(cmd, FROM_MESSAGE)),
    };
    let distance =
        num(p, "distance").filter(|d| d.is_finite() && *d > 0.0).ok_or_else(|| bad(cmd, "`distance` must be a number of mm more than 0"))?;
    let tolerance = match p.get("tolerance") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_f64().filter(|t| t.is_finite() && *t > 0.0).ok_or_else(|| bad(cmd, "`tolerance` must be a number of mm more than 0"))?),
    };
    let src = LinkSource::FaceOffset { body, at, from: Box::new(from), distance, tolerance };
    // The kernel's report (deviation, cut by the boundary), and its refusal word for word, from
    // the model the link resolves against: the one before the active sketch.
    let st = match super::sketch_store::active(s) {
        Some(id) => s.model.state_before(id),
        None => s.model.state(),
    };
    let (_, _, report) = solidcraft_doc::project::face_offset(&st, &src).map_err(|e| bad(cmd, e.to_string()))?;
    let made = one_link(s, cmd, LinkKind::Include, src)?;
    serde_json::to_value(OffsetCurveResult { made, deviation: report.deviation, clipped: report.clipped, created: None, param_warnings: Vec::new() })
        .map_err(|e| bad(cmd, e.to_string()))
}

#[cfg(test)]
#[path = "surface_offset_curve_tests.rs"]
mod tests;
