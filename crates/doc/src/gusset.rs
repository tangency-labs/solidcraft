//! Attached fabrication gussets, composed from existing planar extrusion operations.
use std::collections::BTreeMap;

use crate::expr::{Kind, Value};
use crate::{DocError, Document, Expr, Feature, FeatureKind, ModelState, PlaneRef, Result};
use serde::{Deserialize, Serialize};
use solidcore_ops::frame_gusset as geometry;
use solidcore_record::Plane;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub feature: crate::FeatureId,
    pub line: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(deny_unknown_fields)]
pub struct Support {
    pub plane: PlaneRef,
    pub member: Option<Member>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, solidcraft_derive::Walk)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    #[default]
    Center,
    Positive,
    Negative,
}

fn invalid(s: impl Into<String>) -> DocError {
    DocError::Invalid(s.into())
}

/// Identify the frame member by its producer and sketch line, even when the body is renamed.
pub fn member(doc: &Document, st: &ModelState, plane: &PlaneRef) -> Result<Option<Member>> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = plane else { return Err(invalid("gusset supports require direct persistent planar faces")) };
    let source = doc.feature(anchor.target.origin_feature).ok_or_else(|| invalid("gusset support producer no longer exists"))?;
    if !matches!(source.kind, FeatureKind::FrameMember { .. }) {
        return Ok(None);
    }
    let line = st
        .body(&anchor.target.body)
        .filter(|body| body.feature == source.id)
        .and_then(|body| body.frame_member_line.as_deref())
        .ok_or_else(|| invalid("gusset support is no longer the selected member"))?;
    Ok(Some(Member { feature: source.id.into(), line: line.to_owned() }))
}

/// Input budgets for the existing exact support-face reader.
const MAX_LOOPS: usize = 4096;
const MAX_PIECES: usize = 65536;

/// Resolve the certified support and reconstruct its finite material from trimmed face loops.
fn face(doc: &Document, st: &ModelState, f: &Feature, support: &Support) -> Result<geometry::Support> {
    let PlaneRef::Face { anchor: Some(anchor), .. } = &support.plane else { return Err(invalid("gusset support must be an attached face")) };
    if member(doc, st, &support.plane)? != support.member {
        return Err(invalid("gusset support's member identity changed"));
    }
    let plane = doc.resolve_face_anchor(anchor, f.component, st)?;
    let body = st.body(&anchor.target.body).ok_or_else(|| invalid("gusset support body disappeared"))?;
    if body.body.face_count() > 4096 {
        return Err(invalid("gusset support exceeds the 4096-face limit"));
    }
    let (indices, _) = doc.anchored_face_indices(st, body, &anchor.target)?;
    let to_author = anchor.source_to_author(doc, f.component, body)?;
    let to_source = crate::mat_inverse(&to_author).ok_or_else(|| invalid("gusset support placement is not invertible"))?;
    let tolerance = body.body.planar_edges_tolerance().clamp(1e-9, 1e-3);
    let record = body.body.record();
    let mut source: Option<Plane> = None;
    let mut faces = Vec::new();
    for index in indices {
        let face_plane = body.body.planar_face(index).ok_or_else(|| invalid("gusset support face must be planar"))?;
        let face_plane = geometry::support_plane(source.as_ref(), &face_plane, body.body.size(), tolerance).map_err(geometry_error)?;
        source = Some(face_plane);
        let face = record.faces.get(index).ok_or_else(|| invalid("gusset support face disappeared"))?;
        if face.loops.len() > MAX_LOOPS {
            return Err(invalid("gusset support face has too many loops"));
        }
        let mut loops = Vec::new();
        for l in &face.loops {
            // A vertex loop (a pole) bounds no area.
            let solidcore_record::brep::Loop::Edges { edges } = l else { continue };
            let ids: Vec<usize> = edges.iter().map(|e| e.edge as usize).collect();
            let mut segs = Vec::new();
            for pieces in body.body.planar_edges(&face_plane, &ids) {
                let pieces = pieces.ok_or_else(|| invalid("gusset support face has an edge that can't be read exactly"))?;
                for piece in pieces {
                    let crate::kernel::PlanarPiece::Seg(seg) = piece else {
                        return Err(invalid("not supported yet: a gusset on a face bounded by a free-form spline edge"));
                    };
                    segs.push(seg);
                }
                if segs.len() > MAX_PIECES {
                    return Err(invalid("gusset support face has too many edge pieces"));
                }
            }
            loops.push(segs);
        }
        faces.push(loops);
    }
    let source = source.ok_or_else(|| invalid("gusset support has no material"))?;
    geometry::support(&geometry::SupportFrames { plane, source, to_source, to_author, tolerance }, &faces).map_err(geometry_error)
}

/// Bound geometry inputs; signed offsets share the size limit but need no positive minimum.
fn length(v: f64, positive: bool) -> Result<f64> {
    if !v.is_finite() || v.abs() > 1e6 || (positive && v < 0.01) {
        return Err(invalid("gusset sizes must be 0.01…1e6 mm; offsets must be finite and within 1e6 mm"));
    }
    Ok(v)
}

/// Validate persistent supports and create the operation's independent plate in the author frame.
pub(crate) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameGusset { supports, leg1, leg2, thickness, relief, clip, offset, placement } = &f.kind else {
        return Err(invalid("not a gusset"));
    };
    let [a, b] = supports.as_slice() else { return Err(invalid("a gusset requires exactly two supports")) };
    if a.plane == b.plane {
        return Err(invalid("select two different support faces"));
    }
    let (a, b) = (face(doc, st, f, a)?, face(doc, st, f, b)?);
    let value = |e: &Expr, positive| length(crate::eval::val(vals, e, Kind::Length)?, positive);
    let dimensions = geometry::Dimensions {
        leg1: value(leg1, true)?,
        leg2: value(leg2, true)?,
        thickness: value(thickness, true)?,
        relief: value(relief, false)?,
        clip: value(clip, false)?,
        offset: value(offset, false)?,
    };
    let placement = match placement {
        Placement::Center => geometry::Placement::Center,
        Placement::Positive => geometry::Placement::Positive,
        Placement::Negative => geometry::Placement::Negative,
    };
    let prepared = geometry::prepare([&a, &b], dimensions, placement).map_err(geometry_error)?;
    let mut bodies = solidcraft_kernel::extrude(&prepared.plane, &[prepared.profile], prepared.start, prepared.end)?;
    let body = bodies.pop().filter(|_| bodies.is_empty()).ok_or_else(|| invalid("gusset must be one solid"))?;
    let name = gusset_name(st, f);
    st.bodies.push(crate::ModelBody::traced_new(name, crate::lineage::numbered_tool(body), f.id));
    Ok(())
}

fn geometry_error(error: solidcore_ops::Error) -> DocError {
    match error {
        solidcore_ops::Error::Refused { reason, .. } | solidcore_ops::Error::Unsupported(reason) => invalid(reason),
        solidcore_ops::Error::Argument | solidcore_ops::Error::Profile(_) => solidcraft_kernel::KernelError::Invalid(error.to_string()).into(),
        solidcore_ops::Error::Internal(message) => solidcraft_kernel::KernelError::Internal(message).into(),
        error @ solidcore_ops::Error::Invariant { defect: true, .. } => solidcraft_kernel::KernelError::Internal(error.to_string()).into(),
        other => solidcraft_kernel::KernelError::Failed(other.to_string()).into(),
    }
}

/// A free body name for the plate: its stored name, else `Gusset`, then `Gusset (2)`, `Gusset (3)`…
fn gusset_name(st: &mut ModelState, f: &Feature) -> String {
    if f.body_names.first().is_some_and(|n| !n.is_empty()) {
        return crate::eval::new_name(st, f, 0);
    }
    let base = "Gusset";
    // Not a name the design keeps for a suppressed feature or a rename (#4302, #4957).
    let tries = st.bodies.len() + st.reserved_body_names.len() + 2;
    (1..=tries)
        .map(|i| if i == 1 { base.to_string() } else { format!("{base} ({i})") })
        .find(|n| st.body(n).is_none() && !st.reserved_body_names.contains(n))
        .unwrap_or_else(|| base.to_string())
}
