//! `component.set_origin`: move a component's origin (and, if asked, turn its axes) to a new
//! place while everything it holds stays where it is in the world (#13136).
//!
//! A component's contents are written in its own frame. With `F` the new origin's pose in the
//! component's current coordinates, every placement of the component becomes `T·F`, and what
//! the component holds is re-expressed by `F⁻¹`: its features' points, planes and directions
//! (references to its own origin planes, axes and point are written out first, so they stay
//! put), imported meshes, Move features (conjugated: they turn about the origin), joint snaps on
//! its placements, its child placements, the offsets of bodies it hands to or takes from other
//! components, face attachments and canvases. A STEP import, whose text can't be rewritten, gets
//! a Move feature right after it instead.
//!
//! The rewrite happens on a scratch copy, which is evaluated and compared with the design as it
//! was: every body, sketch plane, construction plane, axis and point and every other placement
//! must be where it was, and no feature may gain an error. Otherwise nothing changes. One undo
//! step; nothing new is saved, so the design opens in any release.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_doc::origin_rebase::{conjugate_move, explicit_origin_refs, explicit_plane, transform_meshes, transform_plane};
use solidcraft_doc::{FeatureKind, IDENTITY, Mat, ModelState, apply_point, apply_vector, is_identity, mat_inverse, mat_mul, point_expr};
use solidcraft_geom::prelude::*;

use super::{component, occurrence_path};
use crate::cmd::clipboard::explicit_planes;
use crate::params::{bad, bool_, vec3};
use crate::{Result, Session};

const CMD: &str = "component.set_origin";

pub(super) fn set_origin(s: &mut Session, p: &Value) -> Result<Value> {
    let comp = component(s, p, "component", CMD)?;
    if comp == 0 {
        return Err(bad(CMD, "the design's own origin can't move: pick a component"));
    }
    let name = s.doc.components.iter().find(|c| c.id == comp).map(|c| c.name.clone()).ok_or_else(|| bad(CMD, "no such component"))?;
    if s.doc.marker.is_some() {
        return Err(bad(CMD, "finish timeline rollback before moving an origin"));
    }
    if !s.pending_moves.is_empty() {
        return Err(bad(CMD, "capture or revert the component moves in progress first (component.capture_position)"));
    }
    if s.doc.links.contains_key(&comp) {
        return Err(bad(CMD, format!("{name} is linked to another design: its contents come from there (Break Link first)")));
    }
    if s.doc.parts.contains_key(&comp) {
        return Err(bad(CMD, format!("{name} is a standard part, which is rebuilt when its size changes")));
    }
    let path = placement_path(s, p, comp)?;
    let placement = s.occurrence_placement(&path).map_err(|e| bad(CMD, e.to_string()))?;
    let world = placement.world;
    let to_local = mat_inverse(&world).ok_or_else(|| bad(CMD, "the component's frame is singular"))?;
    let space = match p.get("space") {
        None => "world",
        Some(Value::String(v)) if v == "world" || v == "component" => v.as_str(),
        _ => return Err(bad(CMD, "`space` must be world or component")),
    };
    let frame = new_frame(s, p, &path, &world, space == "component")?;
    // The new origin's pose in the component's current coordinates.
    let f = mat_mul(&to_local, &frame);
    if !super::valid_position(&f) {
        return Err(bad(CMD, "the new origin is out of the supported range"));
    }
    let d = mat_inverse(&f).ok_or_else(|| bad(CMD, "the new frame is singular"))?;
    let keep_world = bool_(p, "keep_world").unwrap_or(true);
    let placements: Vec<u64> = s.doc.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect();
    let reply = |inserted: Vec<String>, unchanged: bool| {
        let o = apply_point(&frame, Vec3::ZERO);
        let axis = |v: Vec3| {
            let w = apply_vector(&frame, v);
            json!([w.x, w.y, w.z])
        };
        json!({
            "component": comp,
            "path": path,
            "origin": [o.x, o.y, o.z],
            "x": axis(Vec3::X),
            "y": axis(Vec3::Y),
            "z": axis(Vec3::Z),
            "offset": f,
            "placements": placements,
            "kept_world": keep_world,
            "inserted": inserted,
            "unchanged": unchanged,
        })
    };
    if is_identity(&f) {
        return Ok(reply(Vec::new(), true));
    }

    let mut scratch = s.scratch();
    if !keep_world {
        // The contents keep their coordinates and go with the origin: only the placements move.
        for o in scratch.doc_mut().occurrences.iter_mut().filter(|o| o.component == comp) {
            o.transform = mat_mul(&o.transform, &f);
        }
        scratch.refresh();
        s.accept_computed(scratch);
        return Ok(reply(Vec::new(), false));
    }

    let before = Snapshot::take(s)?;
    let inserted = rebase(s, &mut scratch, comp, &f, &d)?;
    scratch.refresh();
    if let Some((feature, error)) = new_error(s, &scratch) {
        return Err(bad(CMD, format!("moving the origin would break {feature}: {error}")));
    }
    let after = Snapshot::take(&scratch)?;
    if let Some(what) = before.moved(&after, comp, &f, &scratch) {
        return Err(bad(CMD, format!("moving the origin of {name} would move {what}; nothing was changed")));
    }
    s.accept_computed(scratch);
    Ok(reply(inserted, false))
}

/// The placement whose frame world inputs are read in: `path`, else the component's first.
fn placement_path(s: &Session, p: &Value, comp: u64) -> Result<Vec<u64>> {
    if p.get("path").is_some() {
        let leaf = p
            .get("path")
            .and_then(Value::as_array)
            .and_then(|a| a.last())
            .and_then(Value::as_u64)
            .ok_or_else(|| bad(CMD, "`path` must list 1..64 occurrence ids"))?;
        let path = occurrence_path(s, p, leaf, CMD)?;
        if s.doc.occurrences.iter().find(|o| o.id == leaf).map(|o| o.component) != Some(comp) {
            return Err(bad(CMD, "`path` leads to a placement of another component"));
        }
        return Ok(path);
    }
    let path = s.doc.first_path(comp);
    if path.is_empty() {
        return Err(bad(CMD, "that component has no placement"));
    }
    Ok(path)
}

/// The new frame in the world: its origin from `point` or `frame.origin` (a point, or `center`
/// / `bottom` of the placement's bodies), its axes from `frame.x` / `frame.z` (default: the
/// component's own axes, as they are now). In `component` space the inputs are in the
/// component's current coordinates.
fn new_frame(s: &Session, p: &Value, path: &[u64], world: &Mat, local: bool) -> Result<Mat> {
    let frame = p.get("frame");
    if let Some(v) = frame
        && !v.is_object()
    {
        return Err(bad(CMD, "`frame` must be {origin?, x?, z?}"));
    }
    if let Some(keys) = frame.and_then(Value::as_object)
        && let Some(k) = keys.keys().find(|k| !matches!(k.as_str(), "origin" | "x" | "z"))
    {
        return Err(bad(CMD, format!("`frame` takes origin, x and z, not `{k}`")));
    }
    let point_value = match (p.get("point"), frame.and_then(|f| f.get("origin"))) {
        (Some(_), Some(_)) => return Err(bad(CMD, "give `point` or `frame.origin`, not both")),
        (a, b) => a.or(b),
    };
    if point_value.is_none() && frame.is_none() {
        return Err(bad(CMD, "give `point` (where the origin goes) or `frame` ({origin?, x?, z?})"));
    }
    let to_world = |v: Vec3| if local { apply_point(world, v) } else { v };
    let dir_to_world = |v: Vec3| if local { apply_vector(world, v) } else { v };
    let origin = match point_value {
        None => apply_point(world, Vec3::ZERO),
        Some(Value::String(k)) => bounds_point(s, path, k)?,
        Some(v) => to_world(vec3(v).ok_or_else(|| bad(CMD, "`point` must be [x, y, z] (finite numbers), `center` or `bottom`"))?),
    };
    let axis = |k: &str| -> Result<Option<Vec3>> {
        match frame.and_then(|f| f.get(k)) {
            None => Ok(None),
            Some(v) => {
                let d = vec3(v).ok_or_else(|| bad(CMD, format!("`frame.{k}` must be [x, y, z] (finite numbers)")))?;
                let d = dir_to_world(d).normalized().ok_or_else(|| bad(CMD, format!("`frame.{k}` must not be zero")))?;
                Ok(Some(d))
            }
        }
    };
    let (x_given, z_given) = (axis("x")?, axis("z")?);
    let own = |v: Vec3| apply_vector(world, v).normalized().unwrap_or(v);
    let z = z_given.unwrap_or_else(|| own(Vec3::Z));
    let square = |x: Vec3| (x - z * x.dot(z)).normalized();
    let x = match x_given {
        Some(x) => square(x).ok_or_else(|| bad(CMD, "`frame.x` and `frame.z` are parallel"))?,
        // The component's own x, laid square to the new z (its y when z is along it).
        None => square(own(Vec3::X)).or_else(|| square(own(Vec3::Y))).unwrap_or_else(|| z.any_perp()),
    };
    let y = z.cross(x);
    Ok([[x.x, x.y, x.z, 0.0], [y.x, y.y, y.z, 0.0], [z.x, z.y, z.z, 0.0], [origin.x, origin.y, origin.z, 1.0]])
}

/// The centre of the box around the placement's bodies (with those of its child placements),
/// or the centre of its bottom (lowest world Z): where a part bought as a mesh usually wants
/// its origin.
fn bounds_point(s: &Session, path: &[u64], which: &str) -> Result<Vec3> {
    if !matches!(which, "center" | "bottom") {
        return Err(bad(CMD, format!("`point` keyword `{which}` is not center or bottom")));
    }
    let names: BTreeSet<String> = s.occurrence_bodies(path, true).map_err(|e| bad(CMD, e.to_string()))?.into_iter().collect();
    let st = s.world_state()?;
    let mut bb = solidcraft_geom::Aabb3::EMPTY;
    for b in st.bodies.iter().filter(|b| names.contains(&b.name)) {
        bb = bb.union(&b.body.outline_bounds());
    }
    if bb.is_empty() {
        return Err(bad(CMD, "the component has no bodies to measure"));
    }
    let c = (bb.min + bb.max) * 0.5;
    Ok(if which == "bottom" { Vec3::new(c.x, c.y, bb.min.z) } else { c })
}

/// Rewrite the scratch design: `comp`'s placements by `f`, everything it holds by `d = f⁻¹`.
/// Returns the names of the Move features added after STEP imports.
fn rebase(s: &Session, scratch: &mut Session, comp: u64, f: &Mat, d: &Mat) -> Result<Vec<String>> {
    let origins = s.body_origins();
    let feature_component: BTreeMap<u64, u64> = s.doc.features.iter().map(|x| (x.id, x.component)).collect();
    let made_in = |body: &str| origins.get(body).and_then(|id| feature_component.get(id)).copied();
    let constant_angle = |e: &solidcraft_doc::Expr| solidcraft_doc::Document::default().eval(e.as_str(), solidcraft_doc::expr::Kind::Angle).is_ok();
    // Features whose bodies can't be re-expressed: a Move right after each puts them in place.
    let mut follow: Vec<(u64, Vec<String>)> = Vec::new();
    let bodies_of = |id: u64| origins.iter().filter(|(_, f)| **f == id).map(|(b, _)| b.clone()).collect::<Vec<String>>();
    let turns = (0..3).any(|c| (0..3).any(|r| (d[c][r] - IDENTITY[c][r]).abs() > 1e-12));
    let doc = scratch.doc_mut();

    for o in doc.occurrences.iter_mut() {
        if o.component == comp {
            o.transform = mat_mul(&o.transform, f);
        } else if o.parent == comp {
            o.transform = mat_mul(d, &o.transform);
        }
    }

    for feat in doc.features.iter_mut().filter(|x| x.component == comp) {
        match &mut feat.kind {
            FeatureKind::BodyTransfer { .. } => {}
            FeatureKind::MeshImport { meshes, .. } => transform_meshes(d, meshes),
            FeatureKind::Import { .. } => follow.push((feat.id, bodies_of(feat.id))),
            // A box is square to its component's axes: turned, it is made as it was and moved.
            FeatureKind::Box { operation, .. } if turns => {
                if !matches!(operation, solidcraft_doc::Operation::NewBody) {
                    return Err(bad(
                        CMD,
                        format!("{} adds to or cuts a body square to the component's axes: move the origin without turning them", feat.name),
                    ));
                }
                follow.push((feat.id, bodies_of(feat.id)));
            }
            FeatureKind::Move { translate, rotate_axis, angle, .. } => {
                let value = match angle {
                    Some(e) => s.doc.eval(e.as_str(), solidcraft_doc::expr::Kind::Angle).map_err(|e| bad(CMD, e.to_string()))?,
                    None => 0.0,
                };
                let constant = angle.as_ref().is_none_or(constant_angle);
                if !conjugate_move(d, translate, rotate_axis, value, constant) {
                    return Err(bad(CMD, format!("{} turns by a parameter about the origin, so it can't keep its place", feat.name)));
                }
            }
            kind => {
                if let FeatureKind::Sketch { plane, .. } | FeatureKind::ConstructionPlane { plane } = kind {
                    explicit_planes(plane);
                }
                if !explicit_origin_refs(kind) {
                    return Err(bad(CMD, format!("{} refers to the component's origin in a way that can't be kept in place", feat.name)));
                }
                kind.transform_geometry(d);
            }
        }
    }

    // Bodies made in one component and shown in another carry the transform between the two.
    for feat in doc.features.iter_mut() {
        if let FeatureKind::BodyTransfer { from, to, transform, .. } = &mut feat.kind {
            if *from == comp {
                *transform = mat_mul(transform, f);
            }
            if *to == comp {
                *transform = mat_mul(d, transform);
            }
        }
    }
    let moved: Vec<String> = doc.body_components.keys().chain(doc.body_offsets.keys()).cloned().collect::<BTreeSet<_>>().into_iter().collect();
    for body in moved {
        let owner = doc.body_components.get(&body).copied();
        let maker = made_in(&body);
        if owner != Some(comp) && maker != Some(comp) {
            continue;
        }
        let mut offset = doc.body_offsets.get(&body).copied().unwrap_or(IDENTITY);
        if owner == Some(comp) {
            offset = mat_mul(d, &offset);
        }
        if maker == Some(comp) {
            offset = mat_mul(&offset, f);
        }
        if is_identity(&offset) {
            doc.body_offsets.remove(&body);
        } else {
            doc.body_offsets.insert(body, offset);
        }
    }

    // Attached sketches remember the frames of the component they were drawn in and of the one
    // whose face they sit on.
    for feat in doc.features.iter_mut() {
        let author = if feat.component == comp { *d } else { IDENTITY };
        if let FeatureKind::Sketch { plane, .. } = &mut feat.kind
            && let Some(anchor) = crate::cmd::sketch::face::anchor_mut(plane)
        {
            let source = if anchor.component == comp { *d } else { IDENTITY };
            anchor.reframe_placement(&author, &source)?;
        }
    }

    // Joint origins on the component's placements are in its coordinates.
    let on_comp: BTreeSet<u64> = doc.occurrences.iter().filter(|o| o.component == comp).map(|o| o.id).collect();
    let assembly = &mut doc.assembly;
    let snaps = assembly
        .joints
        .iter_mut()
        .flat_map(|j| [&mut j.a, &mut j.b])
        .chain(assembly.origins.iter_mut().map(|o| &mut o.origin))
        .chain(assembly.traces.iter_mut().map(|t| &mut t.point));
    for origin in snaps.filter(|o| on_comp.contains(&o.occurrence)) {
        origin.snap.transform(d);
    }

    for canvas in doc.canvases.iter_mut().filter(|c| c.component == comp) {
        explicit_plane(&mut canvas.plane);
        transform_plane(d, &mut canvas.plane);
    }

    // A STEP import's bodies are its text, and a box is square to the axes it was made in: a Move
    // right after each puts its bodies where the rest of the component, re-expressed, expects them.
    let mut inserted = Vec::new();
    for (import, bodies) in follow {
        if bodies.is_empty() {
            continue;
        }
        let rows = [0, 1, 2].map(|r| [d[0][r], d[1][r], d[2][r]]);
        let (axis, angle) = axis_angle(&rows);
        let kind = FeatureKind::Move {
            bodies: bodies.into_iter().map(Into::into).collect(),
            translate: point_expr(Vec3::new(d[3][0], d[3][1], d[3][2])),
            rotate_axis: (angle.abs() > 1e-12).then_some(solidcraft_doc::Dir(axis)),
            angle: (angle.abs() > 1e-12).then(|| solidcraft_doc::Expr::new(format!("{} deg", angle.to_degrees()))),
            copy: false,
        };
        let id = doc.add_feature(kind, Some("Origin Moved")).map_err(|e| bad(CMD, e.to_string()))?;
        let at = doc.features.iter().position(|x| x.id == import).map_or(0, |i| i + 1);
        if let Some(from) = doc.features.iter().position(|x| x.id == id) {
            let mut moved = doc.features.remove(from);
            moved.component = comp;
            inserted.push(moved.name.clone());
            doc.features.insert(at.min(doc.features.len()), moved);
        }
    }
    Ok(inserted)
}

/// A rotation's axis and angle (radians), from its rows.
fn axis_angle(r: &[[f64; 3]; 3]) -> (Vec3, f64) {
    let c = ((r[0][0] + r[1][1] + r[2][2] - 1.0) * 0.5).clamp(-1.0, 1.0);
    let angle = c.acos();
    if angle.abs() < 1e-12 {
        return (Vec3::Z, 0.0);
    }
    let v = Vec3::new(r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1]);
    if let Some(axis) = v.normalized() {
        return (axis, angle);
    }
    // A half turn: the axis is the column of R + I that is longest.
    let cols = [0, 1, 2].map(|k| Vec3::new(r[0][k], r[1][k], r[2][k]) + [Vec3::X, Vec3::Y, Vec3::Z][k]);
    let axis = cols.into_iter().max_by(|a, b| a.len().total_cmp(&b.len())).and_then(Vec3::normalized).unwrap_or(Vec3::Z);
    (axis, angle)
}

/// The first feature that has an error in `after` and had none before, and its error.
fn new_error(s: &Session, after: &Session) -> Option<(String, String)> {
    after.doc.features.iter().find_map(|feat| {
        let had = s.model.result(feat.id).is_some_and(|r| r.error.is_some());
        if had {
            return None;
        }
        after.model.result(feat.id).and_then(|r| r.error.clone()).map(|e| (feat.name.clone(), e))
    })
}

/// Where everything is in the world, to check that moving an origin moved nothing.
struct Snapshot {
    bodies: BTreeMap<String, (Vec3, Vec3, f64)>,
    sketches: BTreeMap<u64, [Vec3; 3]>,
    constructs: BTreeMap<u64, solidcraft_doc::construct::ConstructGeom>,
    placements: BTreeMap<Vec<u64>, Mat>,
}

impl Snapshot {
    fn take(s: &Session) -> Result<Snapshot> {
        let st: std::sync::Arc<ModelState> = s.world_state()?;
        let mut bodies = BTreeMap::new();
        for b in &st.bodies {
            let bb = if b.body.is_mesh() { b.body.outline_bounds() } else { b.body.exact_bounds().unwrap_or_else(|_| b.body.outline_bounds()) };
            bodies.insert(b.name.clone(), (bb.min, bb.max, b.body.size()));
        }
        let sketches = st.sketches.iter().map(|k| (k.feature, [k.plane.origin, k.plane.x, k.plane.y])).collect();
        let constructs = st.construct.iter().map(|c| (c.feature, c.geom.clone())).collect();
        let placements = s.doc.placements().into_iter().map(|(_, path, m)| (path, m)).collect();
        Ok(Snapshot { bodies, sketches, constructs, placements })
    }

    /// What moved between `self` and `after` (`comp`'s own placements are expected to have
    /// turned by `f`), if anything.
    fn moved(&self, after: &Snapshot, comp: u64, f: &Mat, scratch: &Session) -> Option<String> {
        let near = |a: Vec3, b: Vec3, size: f64| a.dist(b) <= 1e-6 * (1.0 + size.abs()) + 1e-7;
        for (name, (lo, hi, size)) in &self.bodies {
            match after.bodies.get(name) {
                None => return Some(format!("body {name} (it is gone)")),
                Some((lo2, hi2, _)) if !(near(*lo, *lo2, *size) && near(*hi, *hi2, *size)) => return Some(format!("body {name}")),
                _ => {}
            }
        }
        if let Some(name) = after.bodies.keys().find(|n| !self.bodies.contains_key(*n)) {
            return Some(format!("body {name} (it is new)"));
        }
        let feature_name = |id: &u64| scratch.doc.feature(*id).map_or_else(|| id.to_string(), |x| x.name.clone());
        for (id, v) in &self.sketches {
            let scale = v[0].len();
            if after.sketches.get(id).is_none_or(|w| v.iter().zip(w).any(|(a, b)| !near(*a, *b, scale))) {
                return Some(format!("sketch {}", feature_name(id)));
            }
        }
        for (id, g) in &self.constructs {
            if after.constructs.get(id).is_none_or(|h| !same_construct(g, h)) {
                return Some(format!("construction {}", feature_name(id)));
            }
        }
        for (path, m) in &self.placements {
            let ends_here = path.last().and_then(|id| scratch.doc.occurrences.iter().find(|o| o.id == *id)).is_some_and(|o| o.component == comp);
            let want = if ends_here { mat_mul(m, f) } else { *m };
            let Some(got) = after.placements.get(path) else { return Some(format!("placement {path:?} (it is gone)")) };
            let scale = Vec3::new(want[3][0], want[3][1], want[3][2]).len();
            if want.iter().flatten().zip(got.iter().flatten()).any(|(a, b)| (a - b).abs() > 1e-6 * (1.0 + scale)) {
                let name =
                    path.last().and_then(|id| scratch.doc.occurrences.iter().find(|o| o.id == *id)).map(|o| o.name.clone()).unwrap_or_default();
                return Some(format!("placement {name}"));
            }
        }
        None
    }
}

/// Whether two construction items are the same in the world: a point the same point, an axis
/// the same line and a plane the same plane (where on it its origin is reported may differ).
fn same_construct(a: &solidcraft_doc::construct::ConstructGeom, b: &solidcraft_doc::construct::ConstructGeom) -> bool {
    use solidcraft_doc::construct::ConstructGeom as G;
    let tol = |p: Vec3| 1e-6 * (1.0 + p.len());
    let parallel = |u: Vec3, v: Vec3| match (u.normalized(), v.normalized()) {
        (Some(u), Some(v)) => u.cross(v).len() < 1e-7,
        _ => false,
    };
    match (a, b) {
        (G::Point(p), G::Point(q)) => p.dist(*q) <= tol(*p),
        (G::Axis { origin: o1, dir: d1 }, G::Axis { origin: o2, dir: d2 }) => {
            let off = *o2 - *o1;
            parallel(*d1, *d2) && d1.normalized().is_some_and(|d| (off - d * off.dot(d)).len() <= tol(*o1))
        }
        (G::Plane(p), G::Plane(q)) => {
            let (n1, n2) = (p.x.cross(p.y), q.x.cross(q.y));
            parallel(n1, n2) && n1.normalized().is_some_and(|n| (q.origin - p.origin).dot(n).abs() <= tol(p.origin))
        }
        _ => false,
    }
}

#[cfg(test)]
#[path = "component_set_origin_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests;
