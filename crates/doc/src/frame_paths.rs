//! Exact selected line/arc paths beside the ordinary Auto frame evaluator.
//! Plain straight frames, pairs and corner processing stay in the parent module.

use solidcraft_geom::prelude::*;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

use solidcore_record::frame::{PieceData, Segment};
use solidcore_record::{Plane, Region2, Vec2, Vec3};
use solidcraft_kernel as kernel;
use solidcraft_kernel::{PathFailure, PathRefusal};
use solidcraft_sketch::CurveKind;

use super::{ModelBody, ModelState, SolvedSketch, new_name, path_in_frame, sketch_in_feature_frame};
use crate::expr::Value;
use crate::frame::{self, MAX_MEMBERS};
use crate::{DocError, Document, Feature, FeatureKind, Result};

/// Route only continuous stock or explicitly selected arcs here. All ordinary
/// straight lines and point pairs retain main's Auto/corner/fit pipeline.
pub(super) fn uses(st: &ModelState, kind: &FeatureKind) -> Result<bool> {
    let FeatureKind::FrameMember { path_sketch, lines, continuous, .. } = kind else { return Ok(false) };
    if *continuous {
        return Ok(true);
    }
    if lines.is_empty() {
        return Ok(false);
    }
    let Some(sketch) = path_sketch else { return Ok(false) };
    let ps = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    Ok(lines.iter().any(|id| {
        ps.sketch.wire_index(id).and_then(|i| ps.sketch.wires.get(i)).is_some_and(|w| w.arc.is_some())
            || ps.sketch.curve_index(id).and_then(|i| ps.sketch.curves.get(i)).is_some_and(|c| matches!(c.kind, CurveKind::Arc { .. }))
    }))
}

/// Path stock has no straight corner/cut model yet. Refuse saved settings too;
/// none of the ordinary finishing options may silently disappear on this route.
fn check_options(kind: &FeatureKind) -> Result<()> {
    let FeatureKind::FrameMember { corner, corners, fit_ends, coped, flip, gap, continuous, pairs, groups, up, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    if !up.is_z() {
        return Err(DocError::Invalid(
            "not supported yet: curved or continuous members take world Z as up; use separate Frame Members for straight lines with another up axis"
                .into(),
        ));
    }
    if !groups.is_empty() {
        return Err(DocError::Invalid(
            "not supported yet: curved or continuous members cannot be combined with groups; use separate Frame Members".into(),
        ));
    }
    if !corner.is_none() || !corners.is_empty() || *fit_ends || *coped || *flip || gap.is_some() {
        return Err(DocError::Invalid(
            "not supported yet: curved or continuous frame members require corner None, no overrides, Fit Ends, Coped, Flip or gap".into(),
        ));
    }
    if *continuous && !pairs.is_empty() {
        return Err(DocError::Invalid(
            "not supported yet: a continuous member cannot include point pairs; select its sketch lines and arcs in path order".into(),
        ));
    }
    Ok(())
}

pub(super) fn evaluate(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &mut ModelState) -> Result<()> {
    let FeatureKind::FrameMember { body_name, names, continuous, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { paths, regions, description, at, turn, turns, centre } = layout(doc, vals, f, st)?;
    let ids = path_keys(&paths, *continuous);
    let mut made = Vec::with_capacity(paths.len());
    for (id, path) in ids.iter().zip(&paths) {
        let first = path.first().ok_or_else(|| DocError::Invalid("a member path is empty".into()))?;
        let body = if let [piece] = path.as_slice()
            && let Some(length) = kernel::stock_line_length(&piece.geometry).map_err(|e| path_error(path, e))?
        {
            let data = piece.geometry;
            let plane = member_plane(data.a, data.b, turn, at, centre, turns.get(id).copied())
                .ok_or_else(|| DocError::Invalid(format!("line `{id}`: no plane square to it")))?;
            let mut bodies = kernel::extrude(&plane, &regions, 0.0, length)?;
            match (bodies.pop(), bodies.is_empty()) {
                (Some(body), true) => body,
                _ => return Err(DocError::Invalid("the member is not one solid".into())),
            }
        } else {
            let plane = kernel::stock_section_plane(&first.geometry, at, turn).map_err(|e| path_error(path, e))?;
            let region = regions.first().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
            let geometry: Vec<_> = path.iter().map(|p| p.geometry).collect();
            kernel::check_bend_clearance(&geometry, region, at, turn).map_err(|e| path_error(path, e))?;
            kernel::sweep(&plane, region, &path.iter().map(path_segment).collect::<Vec<_>>())?
        };
        made.push(crate::lineage::numbered_tool(body));
    }
    if !f.body_names.is_empty() {
        // A copy (pasted, or in a copied component) names its members by position, as any
        // feature's bodies.
        for (k, t) in made.into_iter().enumerate() {
            let name = new_name(st, f, k);
            st.bodies.push(ModelBody::traced_new(name, t, f.id));
        }
        return Ok(());
    }
    let base = body_name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map_or(description, str::to_string);
    for (name, t) in super::member_names(st, &ids, &[], names, &base).into_iter().zip(made) {
        st.bodies.push(ModelBody::traced_new(name, t, f.id));
    }
    Ok(())
}

struct Layout {
    paths: Vec<Vec<Piece>>,
    regions: Vec<Region2>,
    description: String,
    at: Vec2,
    turn: f64,
    turns: BTreeMap<String, f64>,
    centre: Vec2,
}

fn layout(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState) -> Result<Layout> {
    check_options(&f.kind)?;
    let FeatureKind::FrameMember { path_sketch, lines, continuous, pairs, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let path_sketch = path_sketch.ok_or_else(|| DocError::Invalid("curved or continuous members need a path sketch".into()))?;
    let ps = st.sketch(path_sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {path_sketch} (it must come earlier in the timeline)")))?;
    let mapped =
        if ps.sketch.wires.is_empty() { Cow::Owned(sketch_in_feature_frame(doc, ps, f.component)?) } else { path_in_frame(doc, ps, f.component)? };
    let mut paths = member_paths(mapped.as_ref(), lines, *continuous)?;
    for (id, a, b) in super::pair_lines(doc, vals, f, st, pairs)? {
        let geometry = kernel::stock_line(a, b).map_err(|e| DocError::Invalid(format!("pair `{id}`: {e}")))?;
        paths.push(vec![Piece { id, geometry }]);
    }
    let ids = path_keys(&paths, *continuous);
    let mut seen = HashSet::new();
    if ids.iter().any(|id| !seen.insert(id)) || paths.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} distinct members in one feature")));
    }
    let super::Section { regions, description, at, turn, turns, centre, .. } = super::section(doc, vals, f, st)?;
    Ok(Layout { paths, regions, description, at, turn, turns, centre })
}

fn member_plane(a: Vec3, b: Vec3, turn: f64, at: Vec2, centre: Vec2, own: Option<f64>) -> Option<Plane> {
    let (x, y) = frame::member_axes(b - a, turn, frame::UpAxis::Z)?;
    let origin = a - x * at.x - y * at.y;
    let Some(t) = own else { return Plane::new(origin, x, y) };
    let pivot = origin + x * centre.x + y * centre.y;
    let (x, y) = frame::member_axes(b - a, turn + t.rem_euclid(std::f64::consts::TAU), frame::UpAxis::Z)?;
    Plane::new(pivot - x * centre.x - y * centre.y, x, y)
}

pub(super) fn member_section(
    doc: &Document,
    vals: &BTreeMap<String, Value>,
    f: &Feature,
    st: &ModelState,
    line: &str,
) -> Result<(Plane, f64, Region2, [bool; 2])> {
    let FeatureKind::FrameMember { continuous, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { paths, regions, at, turn, turns, centre, .. } = layout(doc, vals, f, st)?;
    let keys = path_keys(&paths, *continuous);
    let path = keys
        .iter()
        .zip(&paths)
        .find_map(|(key, path)| (key == line).then_some(path))
        .ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let [piece] = path.as_slice() else {
        return Err(DocError::Invalid("not supported yet: end caps on curved frame members".into()));
    };
    let length = kernel::stock_line_length(&piece.geometry)
        .map_err(|e| path_error(path, e))?
        .ok_or_else(|| DocError::Invalid("not supported yet: end caps on curved frame members".into()))?;
    let data = piece.geometry;
    let plane = member_plane(data.a, data.b, turn, at, centre, turns.get(line).copied())
        .ok_or_else(|| DocError::Invalid(format!("line `{line}`: no plane square to it")))?;
    let region = regions.into_iter().next().ok_or_else(|| DocError::Invalid("the member profile has no area".into()))?;
    Ok((plane, length, region, [true, true]))
}

pub(super) fn member_pivot(doc: &Document, vals: &BTreeMap<String, Value>, f: &Feature, st: &ModelState, line: &str) -> Result<Vec3> {
    let FeatureKind::FrameMember { continuous, .. } = &f.kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let Layout { paths, at, turn, centre, .. } = layout(doc, vals, f, st)?;
    let keys = path_keys(&paths, *continuous);
    let path = keys
        .iter()
        .zip(&paths)
        .find_map(|(key, path)| (key == line).then_some(path))
        .ok_or_else(|| DocError::Unknown(format!("line `{line}` of the member feature")))?;
    let [piece] = path.as_slice() else {
        return Err(DocError::Invalid("not supported yet: rotation on curved frame members".into()));
    };
    if kernel::stock_line_length(&piece.geometry).map_err(|e| path_error(path, e))?.is_none() {
        return Err(DocError::Invalid("not supported yet: rotation on curved frame members".into()));
    }
    let data = piece.geometry;
    let plane =
        member_plane(data.a, data.b, turn, at, centre, None).ok_or_else(|| DocError::Invalid(format!("line `{line}`: no plane square to it")))?;
    Ok(plane.origin + plane.x * centre.x + plane.y * centre.y + (data.b - data.a) * 0.5)
}

/// Path identities in the same order evaluate emits them; no later cut pieces.
pub(super) fn member_parts(st: &ModelState, kind: &FeatureKind) -> Result<(Vec<String>, frame::LaterPieces)> {
    check_options(kind)?;
    let FeatureKind::FrameMember { path_sketch, lines, continuous, pairs, .. } = kind else {
        return Err(DocError::Invalid("not a frame member feature".into()));
    };
    let sketch = path_sketch.ok_or_else(|| DocError::Invalid("curved or continuous members need a path sketch".into()))?;
    let ps = st.sketch(sketch.0).ok_or_else(|| DocError::Unknown(format!("sketch {sketch}")))?;
    let mut ids = path_keys(&member_paths(ps, lines, *continuous)?, *continuous);
    ids.extend(pairs.iter().map(|p| p.id.clone()));
    let mut seen = HashSet::new();
    if ids.len() > MAX_MEMBERS || ids.iter().any(|id| !seen.insert(id)) {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} distinct members in one feature")));
    }
    let later = vec![None; ids.len()];
    Ok((ids, later))
}

/// Display centrelines in world coordinates; endpoints never certify a curved body.
pub(super) fn member_segments(doc: &Document, st: &ModelState, f: &Feature) -> Result<Vec<(String, Vec3, Vec3)>> {
    let FeatureKind::FrameMember { continuous, .. } = &f.kind else { return Ok(Vec::new()) };
    let Layout { paths, .. } = layout(doc, &doc.evaluation_values(), f, st)?;
    let ids = path_keys(&paths, *continuous);
    let transform = doc.component_transform(f.component);
    Ok(ids
        .into_iter()
        .zip(paths)
        .filter_map(|(id, path)| {
            let (a, b) = (path.first()?.geometry.a, path.last()?.geometry.b);
            Some((id, crate::apply_point(&transform, a), crate::apply_point(&transform, b)))
        })
        .collect())
}

/// A continuous member belongs to its feature, independently of traversal order or which
/// end segment is selected first. Keep using per-curve keys for separate members.
fn path_keys(paths: &[Vec<Piece>], continuous: bool) -> Vec<String> {
    if continuous { vec!["$path".into()] } else { paths.iter().filter_map(|p| p.first().map(|p| p.id.clone())).collect() }
}

/// A sketch identity paired with exact geometry validated and prepared by solidcore.
struct Piece {
    id: String,
    geometry: PieceData,
}

/// Convert the prepared segment to the existing sweep boundary, field for field.
fn path_segment(piece: &Piece) -> kernel::PathSeg {
    match piece.geometry.segment {
        Segment::Line { a, b } => kernel::PathSeg::Line { a, b },
        Segment::Arc { a, center, axis, angle } => kernel::PathSeg::Arc { a, center, axis, angle },
    }
}

/// Name the sketch curve a path refusal points at, so the user can find the bad pick.
fn path_error(path: &[Piece], e: PathFailure) -> DocError {
    let id = |i: usize| path.get(i).map_or("?", |p| p.id.as_str());
    let message = match e {
        PathFailure::Refused { why: PathRefusal::Connection(i), .. } => {
            format!("continuous member: `{}` is not connected end to end in path order; pick the curves in order along the path", id(i))
        }
        PathFailure::Refused { why: PathRefusal::Tangency(i), .. } => {
            format!("continuous member: `{}` is not tangent to the previous curve; draw a bend arc", id(i))
        }
        PathFailure::Refused { why: PathRefusal::Bend(i), .. } => format!("arc `{}`: the bend radius is too small for the anchored profile", id(i)),
        e => e.to_string(),
    };
    DocError::Invalid(message)
}

/// The exact geometry of sketch curve or 3D wire `id`: a line, or an open circular arc.
/// Splines and other curves are refused; display polylines never decide the kind.
fn curve_piece(ss: &SolvedSketch, id: &str) -> Result<Piece> {
    let sk = &ss.sketch;
    let geometry = if let Some(w) = sk.wire_index(id).and_then(|i| sk.wires.get(i)) {
        if let Some(arc) = w.arc {
            kernel::stock_spatial_arc(arc.start, arc.end, arc.center, arc.normal, arc.radius)
                .map_err(|e| DocError::Invalid(format!("arc `{id}`: {e}")))?
        } else if let Some(line) = wire_line(w)? {
            line
        } else {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an exact circular arc")));
        }
    } else if let Some(ci) = sk.curve_index(id) {
        if !sk.curves.get(ci).is_some_and(|c| matches!(c.kind, CurveKind::Line { .. } | CurveKind::Arc { .. })) {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc")));
        }
        let segments = sk.segs(ci);
        let [segment] = segments.as_slice() else {
            return Err(DocError::Invalid(format!("not supported yet: `{id}` must be a straight line or an open circular arc")));
        };
        kernel::stock_planar(&ss.plane, segment).map_err(|e| DocError::Invalid(format!("curve `{id}`: {e}")))?
    } else {
        return Err(DocError::Unknown(format!("path curve `{id}` in {}", ss.name)));
    };
    Ok(Piece { id: id.into(), geometry })
}

/// The member paths of a feature: one single-curve path per member, or one oriented,
/// connected and tangent path when `continuous` is set (which needs explicit `ids`).
fn member_paths(ss: &SolvedSketch, ids: &[String], continuous: bool) -> Result<Vec<Vec<Piece>>> {
    if ids.is_empty() {
        if continuous {
            return Err(DocError::Invalid("a continuous member needs explicit `lines` in path order".into()));
        }
        return Ok(default_member_lines(ss)?.into_iter().map(|p| vec![p]).collect());
    }
    if ids.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} curves in one member feature")));
    }
    let mut seen = HashSet::new();
    let mut pieces = Vec::with_capacity(ids.len());
    for id in ids {
        if !seen.insert(id.as_str()) {
            return Err(DocError::Invalid(format!("curve `{id}` is listed twice")));
        }
        pieces.push(curve_piece(ss, id)?);
    }
    if !continuous {
        return Ok(pieces.into_iter().map(|p| vec![p]).collect());
    }
    let mut geometry: Vec<_> = pieces.iter().map(|p| p.geometry).collect();
    kernel::orient_stock(&mut geometry).map_err(|e| path_error(&pieces, e))?;
    for (p, geometry) in pieces.iter_mut().zip(geometry) {
        p.geometry = geometry;
    }
    Ok(vec![pieces])
}

/// Drawn straight lines with a length. Splines are never classified from display samples.
fn default_member_lines(ss: &SolvedSketch) -> Result<Vec<Piece>> {
    let sk = &ss.sketch;
    let mut out = Vec::new();
    for (ci, c) in sk.curves.iter().enumerate() {
        if !c.construction && !c.centerline && c.link.is_none() && matches!(c.kind, CurveKind::Line { .. }) {
            let segments = sk.segs(ci);
            if let [segment] = segments.as_slice() {
                match kernel::stock_planar(&ss.plane, segment) {
                    Ok(geometry) => out.push(Piece { id: c.id.clone(), geometry }),
                    Err(PathFailure::Refused { why: PathRefusal::Line, .. }) => {}
                    Err(e) => return Err(path_error(&[], e)),
                }
            }
        }
    }
    for w in sk.wires.iter().filter(|w| w.link.is_none()) {
        if let Some(geometry) = wire_line(w)? {
            out.push(Piece { id: w.id.clone(), geometry });
        }
    }
    if out.is_empty() {
        return Err(DocError::Invalid(format!("{} has no lines for members", ss.name)));
    }
    if out.len() > MAX_MEMBERS {
        return Err(DocError::Invalid(format!("at most {MAX_MEMBERS} members in one feature (pick the lines)")));
    }
    Ok(out)
}

/// A drawn line's defining points, or one linked exact line on its retained interval.
pub(super) fn wire_line(w: &solidcraft_sketch::Wire) -> Result<Option<PieceData>> {
    if w.arc.is_some() {
        return Ok(None);
    }
    let line = if w.link.is_none()
        && w.exact.is_empty()
        && let [a, b] = w.fit.as_slice()
    {
        kernel::stock_line(*a, *b)
    } else if let [solidcraft_sketch::WireCurve { curve: solidcore_record::brep::Curve::Line { start, end }, interval }] = w.exact.as_slice() {
        kernel::stock_retained_line(Vec3::from(*start), Vec3::from(*end), *interval)
    } else {
        return Ok(None);
    };
    match line {
        Ok(line) => Ok(Some(line)),
        Err(PathFailure::Refused { why: PathRefusal::Line, .. }) => Ok(None),
        Err(e) => Err(path_error(&[], e)),
    }
}
