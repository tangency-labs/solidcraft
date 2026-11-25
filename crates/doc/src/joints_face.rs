//! Joint origins on named faces: a face found by its persistent name gives its own plane or
//! cylinder (axis, radius, extent) directly, instead of searching again for the face nearest a
//! point (on a thin plate the face nearest a point on a hole's wall is often the plate's face).
//! A round origin's end is kept by the name of the face across the rim at that end, so it is the
//! same end after the design changes; an x reference keeps the origin's turn about its axis.

use solidcraft_geom::prelude::*;
use std::collections::BTreeMap;

use solidcore_record::{Loop2, Plane, Vec2, Vec3};
use solidcraft_geom::Ring;
use solidcraft_kernel::CylinderFace;

use super::{Snap, XRef, component_bodies};
use crate::history_names::Match;
use crate::{DocError, Document, IDENTITY, Mat, ModelBody, ModelState, Result, apply_point, apply_vector, mat_inverse};

/// Rim points sampled to find the face across a cylinder's rim.
const RIM_SAMPLES: usize = 16;

/// A face found by its persistent name: its body, its index there, the point of it nearest the
/// pick (the component's coordinates) and the body's offset (a body moved in from another
/// component keeps its geometry where it was made; the offset places it).
pub(super) struct NamedFace<'a> {
    pub body: &'a ModelBody,
    pub face: usize,
    pub point: Vec3,
    pub offset: Mat,
}

fn gone(name: &str) -> DocError {
    DocError::Invalid(format!("joint origin: the face `{name}` no longer exists (edit the joint to pick again)"))
}

/// The face called `name` on the bodies of `component` nearest `pick` (an error when it is gone:
/// a joint must not quietly attach elsewhere). Each body is looked at in its own coordinates, and
/// the nearest of the faces the name finds is taken. Only the component's own bodies: the parts of one STEP import share their face names, and another
/// part's face of that name can lie nearer the pick than this part's own.
pub(super) fn named_face<'a>(doc: &Document, st: &'a ModelState, component: u64, pick: Vec3, name: &str) -> Result<NamedFace<'a>> {
    let own = component_bodies(doc, st, component).into_iter().filter_map(|body| {
        let offset = doc.model_body_offset(body).copied().unwrap_or(IDENTITY);
        let (face, at) = crate::naming::face_on_body(body, name, apply_point(&mat_inverse(&offset)?, pick))?;
        let point = apply_point(&offset, at);
        Some((point.dist(pick), NamedFace { body, face, point, offset }))
    });
    if let Some((_, nf)) = own.min_by(|(a, _), (b, _)| a.total_cmp(b)) {
        return Ok(nf);
    }
    // On none of them (the face's body has gone to another component since): wherever the name
    // is now.
    let hit = crate::name_lookup::locate_face(st, name, pick).ok_or_else(|| gone(name))?;
    let body = st.bodies.get(hit.body).ok_or_else(|| gone(name))?;
    let Some(offset) = doc.model_body_offset(body).copied() else {
        return Ok(NamedFace { body, face: hit.face, point: hit.point, offset: IDENTITY });
    };
    let inv = mat_inverse(&offset).ok_or_else(|| gone(name))?;
    let (face, at) = crate::naming::face_on_body(body, name, apply_point(&inv, pick)).ok_or_else(|| gone(name))?;
    Ok(NamedFace { body, face, point: apply_point(&offset, at), offset })
}

impl NamedFace<'_> {
    /// A point of the component's coordinates in the body's own.
    fn local(&self, p: Vec3) -> Vec3 {
        if self.offset == IDENTITY {
            return p;
        }
        mat_inverse(&self.offset).map_or(p, |inv| apply_point(&inv, p))
    }

    /// The face as a cylinder, in the body's own coordinates (`None`: not a cylinder).
    fn own_cylinder(&self) -> Option<CylinderFace> {
        solidcraft_kernel::cylinder_face(&self.body.body, &self.body.mesh(), self.face)
    }

    /// The face as a cylindrical or conical round face, in the body's own coordinates.
    fn own_round(&self) -> Option<RoundFace> {
        if let Some(cyl) = self.own_cylinder() {
            return Some(RoundFace { cyl, cone_half_angle: None });
        }
        cone_round_in(&self.body.body, self.face)
    }

    /// The face as a cylinder, in the component's coordinates.
    pub fn cylinder(&self) -> Option<CylinderFace> {
        self.own_cylinder().map(|c| placed(&self.offset, c))
    }

    /// The face's centre (the middle of its outline, `outline_centre`) and outward normal (the
    /// component's coordinates), when it is planar.
    pub fn plane(&self) -> Result<Option<(Vec3, Vec3)>> {
        let faces = self.body.joint_faces()?;
        Ok(faces.get(self.face).and_then(|f| {
            let n = f.plane_normal?;
            let c = outline_centre(&self.body.body, self.face, f.centroid, n).unwrap_or(f.centroid);
            Some((apply_point(&self.offset, c), apply_vector(&self.offset, n)))
        }))
    }
}

/// The middle of the area the outer boundary of planar face `face` encloses (the body's own
/// coordinates; `centroid` and `n` are the face's), for a face with holes: a hole cut into the
/// face, moved or suppressed changes the face's centroid but not its outline, so a joint at the
/// face's centre stays where it is. `None` for a face without holes (its centroid is that middle
/// already) and for one whose boundary gives no clean outline (the caller keeps the centroid).
pub(super) fn outline_centre(body: &solidcraft_kernel::Body, face: usize, centroid: Vec3, n: Vec3) -> Option<Vec3> {
    if body.is_mesh() {
        return None;
    }
    let plane = Plane::from_normal(centroid, n)?;
    let rec = body.record();
    let f = rec.faces.get(face)?;
    if f.loops.len() < 2 {
        return None;
    }
    let mut loops: Vec<Ring<Vec2>> = Vec::new();
    for lp in &f.loops {
        let solidcore_record::brep::Loop::Edges { edges } = lp else { continue };
        let mut ring = Vec::new();
        for u in edges {
            let edge = rec.edges.get(u.edge as usize)?;
            let eval = solidcraft_geom::brep::eval::CurveEval::new(edge.parameters()).ok()?;
            let [t0, t1] = edge.interval;
            let n_samples = match &eval {
                solidcraft_geom::brep::eval::CurveEval::Line { .. } => 1,
                _ => 16,
            };
            for k in 0..n_samples {
                let frac = k as f64 / n_samples as f64;
                let t = if u.forward { t0 + (t1 - t0) * frac } else { t1 - (t1 - t0) * frac };
                let p = eval.point(t)?;
                ring.push(plane.to_local(p));
            }
        }
        if !ring.is_empty() {
            loops.push(Ring::new(ring));
        }
    }
    if loops.len() < 2 {
        return None;
    }
    let size = body.size().max(1e-9);
    let q = (size * 1e-9).max(1e-12);
    let outer = loops.iter().max_by(|a, b| a.signed_area().abs().total_cmp(&b.signed_area().abs()))?;
    (outer.signed_area().abs() > q * q).then(|| plane.to_world(Loop2::polygon(outer).centroid()))
}

/// A cylinder of a body's own coordinates placed by the body's (rigid) offset.
pub(super) fn placed(m: &Mat, c: CylinderFace) -> CylinderFace {
    CylinderFace { axis_point: apply_point(m, c.axis_point), axis: apply_vector(m, *c.axis).unit().unwrap_or(c.axis), ..c }
}

/// A cylindrical or conical face of a body (in the body's own coordinates).
/// For a cone, `cyl.axis_point` is the apex, `cyl.axis` points into the cone, and
/// `cone_half_angle` is `Some(half_angle)`, so the radius at axial coordinate `s` is
/// `s * half_angle.tan()`.
#[derive(Clone, Copy)]
struct RoundFace {
    cyl: CylinderFace,
    cone_half_angle: Option<f64>,
}

impl RoundFace {
    fn is_cone(&self) -> bool {
        self.cone_half_angle.is_some()
    }

    fn radius_at(&self, at: f64) -> f64 {
        match self.cone_half_angle {
            Some(h) => (at * h.tan()).abs(),
            None => self.cyl.radius,
        }
    }

    fn at_end(&self, chosen: (f64, f64), offset: &Mat) -> Round {
        let mut c = self.cyl;
        c.radius = self.radius_at(chosen.0);
        Round::at(placed(offset, c), chosen)
    }

    /// Valid circular rim ends `(at, dir)` along `cyl.axis`. A pointed cone reaching its apex has
    /// only its larger end; a cylinder or frustum has both.
    fn valid_ends(&self, body_size: f64) -> Vec<(f64, f64)> {
        let Some(h) = self.cone_half_angle else {
            return ends(&self.cyl).to_vec();
        };
        let tol = (body_size * 1e-6).max(1e-6);
        let tan_h = h.tan();
        let mut out = Vec::new();
        if self.cyl.start * tan_h > tol && (self.cyl.end - self.cyl.start).abs() > tol {
            out.push((self.cyl.start, -1.0));
        }
        if self.cyl.end * tan_h > tol {
            out.push((self.cyl.end, 1.0));
        }
        out
    }
}

/// Face `face` of `body` as a conical round face (exact B-rep, no display mesh).
fn cone_round_in(body: &solidcraft_kernel::Body, face: usize) -> Option<RoundFace> {
    let (cyl, half_angle) = solidcraft_kernel::cone_face(body, face)?;
    let rf = RoundFace { cyl, cone_half_angle: Some(half_angle) };
    (!rf.valid_ends(body.size()).is_empty()).then_some(rf)
}

/// The face across the rim at the `at` end (along the axis) of round face `c`, face `face` of `b`
/// (the body's own coordinates): of the body's other faces, the one nearest the rim's points.
fn rim_neighbour(b: &ModelBody, face: usize, c: &CylinderFace, at: f64) -> Option<usize> {
    let mesh = b.mesh();
    let span = (c.end - c.start).abs();
    let slack = if span > 1e-9 { span * 1e-3 + 1e-9 } else { c.radius * 1e-3 + 1e-9 };
    let mut rim: Vec<Vec3> = Vec::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        if *f as usize != face {
            continue;
        }
        for k in t {
            if let Some(q) = mesh.positions.get(*k as usize)
                && ((*q - c.axis_point).dot(c.axis) - at).abs() <= slack
                && !rim.iter().any(|r| r.dist(*q) <= slack)
            {
                rim.push(*q);
            }
        }
    }
    // Evenly through the rim's points, at most RIM_SAMPLES of them.
    let step = rim.len().div_ceil(RIM_SAMPLES).max(1);
    let rim: Vec<Vec3> = rim.into_iter().step_by(step).collect();
    if rim.is_empty() {
        return None;
    }
    let mut near: BTreeMap<usize, Vec<f64>> = BTreeMap::new();
    for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
        let f = *f as usize;
        if f == face {
            continue;
        }
        let Some([p, q, r]) = mesh.tri(t) else { continue };
        let slot = near.entry(f).or_insert_with(|| vec![f64::INFINITY; rim.len()]);
        for (d, x) in slot.iter_mut().zip(&rim) {
            *d = d.min(crate::naming::closest(*x, p, q, r).dist(*x));
        }
    }
    // The face that meets the whole rim (the other half of a cylinder split in two meets only
    // the rim's points at its seams).
    near.into_iter().map(|(f, d)| (d.iter().sum::<f64>(), f)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, f)| f)
}

/// The ends of a cylinder: (place along the axis, which way out of the face).
fn ends(c: &CylinderFace) -> [(f64, f64); 2] {
    [(c.start, -1.0), (c.end, 1.0)]
}

/// The end of `c` nearer `pick` along the axis. On a tie, the end older builds took: the start
/// of the axis as the kernel turns it (toward +x, else +y, else +z), whichever way the face's
/// own surface runs.
fn nearer_end(c: &CylinderFace, pick: Vec3) -> (f64, f64) {
    let s = (pick - c.axis_point).dot(c.axis);
    let [start, end] = ends(c);
    let (to_start, to_end) = ((s - c.start).abs(), (c.end - s).abs());
    let reversed = c.axis.x + c.axis.y * 1e-3 + c.axis.z * 1e-6 < 0.0;
    if to_start < to_end || (to_start == to_end && !reversed) { start } else { end }
}

/// The end a named round face takes without an `end` (a design saved before origins named
/// it): the one nearer the pick along the axis, decided as those builds decided it for cylinders
/// (by the kernel's reading of the cylinder at the pick, so a pick midway along the wall takes
/// the same end as it did then). For a cone, uses `raw_pick` directly so a centre pick on a
/// chamfer's wide rim isn't pulled axially toward its narrower rim.
fn end_by_pick(nf: &NamedFace, own: &RoundFace, raw_pick: Vec3) -> (f64, f64) {
    if own.is_cone() {
        let candidates = own.valid_ends(nf.body.body.size());
        return match candidates.as_slice() {
            [only] => *only,
            _ => nearer_end(&own.cyl, nf.local(raw_pick)),
        };
    }
    let own = &own.cyl;
    let pick = nf.local(nf.point);
    let [start, end] = ends(own);
    match solidcraft_kernel::cylinder_face_at(&nf.body.body, pick).filter(|k| k.axis.dot(own.axis).abs() > 0.5) {
        Some(k) => {
            let s = (pick - k.axis_point).dot(k.axis);
            let out = if (s - k.start).abs() <= (k.end - s).abs() { -k.axis } else { k.axis };
            if out.dot(own.axis) < 0.0 { start } else { end }
        }
        None => nearer_end(own, pick),
    }
}

/// A round joint origin worked out: its cylinder, and the centre and outward axis of the end
/// it takes (the component's coordinates).
pub(super) struct Round {
    pub cylinder: CylinderFace,
    pub centre: Vec3,
    pub z: Vec3,
}

impl Round {
    fn at(c: CylinderFace, (at, dir): (f64, f64)) -> Round {
        Round { cylinder: c, centre: c.axis_point + c.axis * at, z: c.axis * dir }
    }
}

/// How stored face name `name` matches face `f` of `b`.
fn face_name_match(b: &ModelBody, names: &crate::history_names::Names, f: usize, name: &str) -> Option<Match> {
    if crate::name_lookup::legacy_name(name).is_some() {
        return crate::name_lookup::face_has_name(b, f, name).then_some(Match::Exact);
    }
    crate::history_names::matches(names, f, name)
}

/// Faces of `b` sharing a B-rep edge with `face` (other than `face` itself), as `(other_face, edge)`.
fn adjacent_faces(b: &ModelBody, face: usize) -> Vec<(usize, usize)> {
    let record = b.body.record();
    let mut out = Vec::new();
    for edge in solidcraft_kernel::face_edge_indices(&b.body, face) {
        let Some(e) = record.edges.get(edge) else { continue };
        for u in &e.uses {
            let other = u.face as usize;
            if other != face {
                out.push((other, edge));
            }
        }
    }
    out
}

/// Whether `face` on `b` is a coaxial cone (chamfer/countersink) or torus (fillet) on `own`'s axis.
fn is_coaxial_bevel(b: &ModelBody, face: usize, own: &CylinderFace) -> bool {
    let tol = (b.body.size() * 1e-5).max(1e-3);
    let (pt, axis) = match solidcraft_kernel::face_surface(&b.body, face) {
        Some(solidcraft_kernel::FaceSurface::Cone { apex, axis, .. }) => (apex, axis),
        Some(solidcraft_kernel::FaceSurface::Torus { center, axis, .. }) => (center, axis),
        _ => return false,
    };
    if (*axis - *own.axis).len() > 1e-3 && (*axis + *own.axis).len() > 1e-3 {
        return false;
    }
    Plane::from_normal(own.axis_point, *own.axis).is_some_and(|pl| pl.to_local(pt).len() <= tol)
}

/// Coaxial bevel faces (chamfers, countersinks, fillets) meeting cylinder `face` at its `at` rim.
fn rim_bevels(b: &ModelBody, face: usize, own: &CylinderFace, at: f64) -> Vec<usize> {
    let tol = (b.body.size() * 1e-5).max(1e-3);
    let at_point = own.axis_point + own.axis * at;
    let mut out = Vec::new();
    for (other, edge) in adjacent_faces(b, face) {
        if !is_coaxial_bevel(b, other, own) {
            continue;
        }
        if let Some(ec) = solidcraft_kernel::edge_circle(&b.body, edge)
            && ec.center.dist(at_point) <= tol
            && !out.contains(&other)
        {
            out.push(other);
        }
    }
    if out.is_empty()
        && let Some(other) = rim_neighbour(b, face, own, at)
        && is_coaxial_bevel(b, other, own)
    {
        out.push(other);
    }
    out
}

/// The axial coordinate along `own` where `own`'s axis meets face `adj` across `bevel` (moving
/// outward from `at` in direction `dir`), if `adj` lies on that side of `at`.
fn axial_at_end_face(b: &ModelBody, own: &CylinderFace, (at, dir): (f64, f64), bevel: usize, adj: usize) -> Option<f64> {
    let tol = (b.body.size() * 1e-5).max(1e-3);
    let at_point = own.axis_point + own.axis * at;
    let out = *own.axis * dir;
    for (other, edge) in adjacent_faces(b, bevel) {
        if other != adj {
            continue;
        }
        if let Some(ec) = solidcraft_kernel::edge_circle(&b.body, edge)
            && ((*ec.axis - *own.axis).len() <= 1e-3 || (*ec.axis + *own.axis).len() <= 1e-3)
        {
            let shift = ec.center.dist(at_point);
            if (at_point + out * shift).dist(ec.center) <= tol {
                return Some(at + dir * shift);
            }
        }
    }
    let plane @ solidcraft_kernel::FaceSurface::Plane { .. } = solidcraft_kernel::face_surface(&b.body, adj)? else {
        return None;
    };
    let q0 = plane.nearest(at_point)?;
    let s0 = at_point.dist(q0);
    if s0 <= tol {
        return Some(at);
    }
    let p1 = at_point + out;
    let q1 = plane.nearest(p1)?;
    let (v0, v1) = (q0 - at_point, q1 - p1);
    let s1 = v1.len();
    let same_side = (v1 - v0).len() < (v1 + v0).len();
    let rate = if same_side { s0 - s1 } else { s0 + s1 };
    if rate <= 1e-6 {
        return None;
    }
    let shift = s0 / rate;
    let hit = at_point + out * shift;
    (shift >= -tol && shift <= b.body.size() && plane.nearest(hit).is_some_and(|q| q.dist(hit) <= tol)).then_some(at + dir * shift)
}

/// Faces reachable from cylinder `face` at end `(at, dir)` across one or two coaxial bevels
/// (chamfers, countersinks, fillets), with the axial coordinate where `own`'s axis meets each
/// face.
fn bevel_end_faces(b: &ModelBody, face: usize, own: &CylinderFace, (at, dir): (f64, f64)) -> Vec<(usize, f64)> {
    if b.body.is_mesh() {
        return Vec::new();
    }
    let mut seen = vec![face];
    let mut frontier = rim_bevels(b, face, own, at);
    seen.extend(frontier.iter().copied());
    let mut out: Vec<(usize, f64)> = Vec::new();
    for _ in 0..2 {
        let mut next = Vec::new();
        for bevel in frontier {
            for (adj, _) in adjacent_faces(b, bevel) {
                if seen.contains(&adj) {
                    continue;
                }
                if is_coaxial_bevel(b, adj, own) {
                    seen.push(adj);
                    next.push(adj);
                } else if let Some(t) = axial_at_end_face(b, own, (at, dir), bevel, adj)
                    && !out.iter().any(|&(f, _)| f == adj)
                {
                    out.push((adj, t));
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    out
}

/// When a chamfer or fillet is added to a cylinder's rim, the cylinder no longer directly meets
/// the face `name` across that rim: a coaxial cone or torus now sits between them. Walk across up
/// to two coaxial bevel faces from each cylinder end to find `name`, keeping the origin on `name`
/// instead of shifting inward to the recessed cylinder end.
fn end_across_bevel(b: &ModelBody, face: usize, own: &CylinderFace, name: &str) -> Option<(f64, f64)> {
    if b.body.is_mesh() {
        return None;
    }
    let names = crate::naming::names(b);
    let find = |end: (f64, f64)| -> Option<(Match, f64)> {
        let mut best: Option<(Match, f64)> = None;
        for (adj, t) in bevel_end_faces(b, face, own, end) {
            if let Some(m) = face_name_match(b, &names, adj, name)
                && best.is_none_or(|(bm, _)| m > bm)
            {
                best = Some((m, t));
            }
        }
        best
    };
    let [start, end] = ends(own);
    match (find(start), find(end)) {
        (Some((ms, ts)), Some((me, te))) => {
            if ms > me {
                Some((ts, start.1))
            } else if me > ms {
                Some((te, end.1))
            } else {
                None
            }
        }
        (Some((_, ts)), None) => Some((ts, start.1)),
        (None, Some((_, te))) => Some((te, end.1)),
        (None, None) => None,
    }
}

/// A circle-centre snap's cylinder or cone found through its named face (`None`: the snap names no
/// face, or the face is not round any more; the caller looks by point, as older builds did). The
/// end is the one whose rim meets the face called `end` (directly or across an added chamfer or
/// fillet), else the one nearer `pick`.
pub(super) fn named_round(doc: &Document, st: &ModelState, component: u64, pick: Vec3, face: &str, end: Option<&str>) -> Result<Option<Round>> {
    let nf = named_face(doc, st, component, pick, face)?;
    let Some(own) = nf.own_round() else { return Ok(None) };
    let chosen = end
        .and_then(|name| {
            end_named(nf.body, nf.face, &own, name).or_else(|| (!own.is_cone()).then(|| end_across_bevel(nf.body, nf.face, &own.cyl, name)).flatten())
        })
        .unwrap_or_else(|| end_by_pick(&nf, &own, pick));
    Ok(Some(own.at_end(chosen, &nf.offset)))
}

/// A conical round face at `pick` (component coordinates) and its circular rim end nearer `pick`,
/// for an unnamed circle-centre snap on a chamfer or countersink rim.
pub(super) fn cone_round_at(doc: &Document, st: &ModelState, component: u64, pick: Vec3) -> Option<Round> {
    let mut best: Option<(f64, Round)> = None;
    for b in component_bodies(doc, st, component) {
        if b.body.is_mesh() {
            continue;
        }
        let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
        let p = apply_point(&mat_inverse(&offset)?, pick);
        let size = b.body.size();
        let tol = (size * 1e-3).max(1e-3);
        for f in 0..b.body.face_count() {
            let Some(surf @ solidcraft_kernel::FaceSurface::Cone { .. }) = solidcraft_kernel::face_surface(&b.body, f) else {
                continue;
            };
            let Some((cyl, half_angle)) = solidcraft_kernel::cone_face(&b.body, f) else { continue };
            let rf = RoundFace { cyl, cone_half_angle: Some(half_angle) };
            let candidates = rf.valid_ends(size);
            if candidates.is_empty() {
                continue;
            }
            let Some(q) = surf.nearest(p) else { continue };
            let sq = q.dist(cyl.axis_point) * half_angle.cos();
            let off = if sq >= cyl.start && sq <= cyl.end {
                q.dist(p)
            } else {
                let sc = sq.clamp(cyl.start, cyl.end);
                let rim_pt = if sq > 1e-12 { cyl.axis_point + (q - cyl.axis_point) * (sc / sq) } else { cyl.axis_point + cyl.axis * sc };
                rim_pt.dist(p)
            };
            if off <= tol && best.as_ref().is_none_or(|(bo, _)| off < *bo) {
                let chosen = candidates.into_iter().min_by(|(a, _), (b, _)| (sq - *a).abs().total_cmp(&(sq - *b).abs()))?;
                best = Some((off, rf.at_end(chosen, &offset)));
            }
        }
    }
    best.map(|(_, r)| r)
}

/// The end of round face `own` (face `face` of `b`) whose rim meets the face called `name` better
/// than the other end's rim does: the very face, or one merged into it, else a piece of it whose
/// neighbours have changed (the face round a hole in a sheet is told apart from its other pieces
/// by the faces around it, and those change when the hole moves). `None` when neither rim, or
/// both, meet it as well.
fn end_named(b: &ModelBody, face: usize, own: &RoundFace, name: &str) -> Option<(f64, f64)> {
    let names = crate::naming::names(b);
    let candidates = own.valid_ends(b.body.size());
    match candidates.as_slice() {
        [only] => rim_neighbour(b, face, &own.cyl, only.0).and_then(|f| face_name_match(b, &names, f, name)).map(|_| *only),
        [start, end] => {
            let how = |at: f64| rim_neighbour(b, face, &own.cyl, at).and_then(|f| face_name_match(b, &names, f, name));
            match (how(start.0), how(end.0)) {
                (s, e) if s > e => Some(*start),
                (s, e) if e > s => Some(*end),
                _ => None,
            }
        }
        _ => None,
    }
}

/// One exact named face on the permitted component; no proximity tie-break.
fn alignment_face<'a>(doc: &Document, st: &'a ModelState, component: u64, name: &str) -> Result<(&'a ModelBody, usize)> {
    let mut found = None;
    for body in component_bodies(doc, st, component) {
        for index in 0..body.body.record().faces.len() {
            if crate::naming::face_has_name(body, index, name) {
                if !crate::naming::face_name_exact(body, index) {
                    return Err(DocError::Invalid("alignment needs an exact persistent face identity".into()));
                }
                if found.replace((body, index)).is_some() {
                    return Err(DocError::Invalid("alignment face identity is ambiguous".into()));
                }
            }
        }
    }
    found.ok_or_else(|| DocError::Invalid("alignment face no longer exists on the origin's component".into()))
}

/// A named edge is the unique exact intersection of its named incident faces.
fn alignment_edge<'a>(doc: &Document, st: &'a ModelState, component: u64, name: &str) -> Result<(&'a ModelBody, usize)> {
    let parsed = crate::persistent_name::EdgeName::parse(name).map_err(|_| DocError::Invalid("alignment needs an exact edge identity".into()))?;
    if parsed.tie.is_some() || parsed.faces.is_empty() {
        return Err(DocError::Invalid("alignment edge identity is ambiguous".into()));
    }
    let mut owning: Option<&ModelBody> = None;
    let mut faces = Vec::new();
    for name in &parsed.faces {
        let (body, face) = alignment_face(doc, st, component, &name.to_string())?;
        if owning.is_some_and(|b| !std::ptr::eq(b, body)) {
            return Err(DocError::Invalid("alignment edge faces are on different bodies".into()));
        }
        if faces.contains(&face) {
            return Err(DocError::Invalid("alignment edge identity is ambiguous".into()));
        }
        owning = Some(body);
        faces.push(face);
    }
    let body = owning.ok_or_else(|| DocError::Invalid("alignment edge has no incident face".into()))?;
    let mut found = body.body.record().edges.iter().enumerate().filter(|(_, e)| {
        let mut uses: Vec<_> = e.uses.iter().map(|u| u.face as usize).collect();
        uses.sort_unstable();
        uses.dedup();
        uses.len() == faces.len() && faces.iter().all(|f| uses.contains(f))
    });
    let (edge, _) = found.next().ok_or_else(|| DocError::Invalid("alignment edge no longer exists".into()))?;
    if found.next().is_some() {
        return Err(DocError::Invalid("alignment edge identity is ambiguous".into()));
    }
    Ok((body, edge))
}

pub(super) fn alignment_direction(doc: &Document, st: &ModelState, reference: &super::AlignmentRef, component: u64, origin: Vec3) -> Result<Vec3> {
    use super::AlignmentRef;
    let bad = |message: &str| DocError::Invalid(message.into());
    let owns = |feature| -> Result<()> {
        let f = doc.feature(feature).ok_or_else(|| bad("alignment feature no longer exists"))?;
        if f.component != component {
            return Err(bad("alignment reference is not on the origin's component"));
        }
        Ok(())
    };
    match reference {
        AlignmentRef::CircleCenter(rim) => {
            let (body, face) = alignment_face(doc, st, component, &rim.name)?;
            let (end_body, end) = alignment_face(doc, st, component, &rim.end)?;
            if !std::ptr::eq(body, end_body)
                || !matches!(solidcraft_kernel::face_surface(&body.body, face), Some(solidcraft_kernel::FaceSurface::Cylinder { .. }))
            {
                return Err(bad("alignment center must remain a cylindrical rim"));
            }
            let ends = solidcraft_kernel::face_edge_indices(&body.body, end);
            let mut shared = solidcraft_kernel::face_edge_indices(&body.body, face).into_iter().filter(|e| {
                ends.contains(e) && body.body.record().edges.get(*e).is_some_and(|e| matches!(e.curve, solidcore_record::brep::Curve::Circle { .. }))
            });
            let edge = shared.next().ok_or_else(|| bad("alignment circle rim no longer exists"))?;
            if shared.next().is_some() {
                return Err(bad("alignment circle rim is ambiguous"));
            }
            let circle = solidcraft_kernel::edge_circle(&body.body, edge).ok_or_else(|| bad("alignment circle is unavailable"))?;
            let center = doc.model_body_offset(body).map_or(circle.center, |m| apply_point(m, circle.center));
            Ok(center - origin)
        }
        AlignmentRef::Edge { name } => {
            let (body, edge) = alignment_edge(doc, st, component, name)?;
            let direction = match body.body.record().edges.get(edge).map(|e| &e.curve) {
                Some(solidcore_record::brep::Curve::Line { start, end }) => Vec3::from(*end) - Vec3::from(*start),
                Some(solidcore_record::brep::Curve::Circle { .. }) => {
                    let c = solidcraft_kernel::edge_circle(&body.body, edge).ok_or_else(|| bad("alignment circle is unavailable"))?;
                    *c.axis
                }
                _ => return Err(bad("alignment edge must remain an exact line or circle")),
            };
            Ok(doc.model_body_offset(body).map_or(direction, |m| apply_vector(m, direction)))
        }
        AlignmentRef::Cylinder { name } => {
            let (body, face) = alignment_face(doc, st, component, name)?;
            match solidcraft_kernel::face_surface(&body.body, face) {
                Some(solidcraft_kernel::FaceSurface::Cylinder { axis, .. }) => {
                    let direction = *axis;
                    Ok(doc.model_body_offset(body).map_or(direction, |m| apply_vector(m, direction)))
                }
                _ => Err(bad("alignment reference must remain cylindrical")),
            }
        }
        AlignmentRef::Construction { feature } | AlignmentRef::ConstructionPlane { feature } => {
            owns(*feature)?;
            let plane = matches!(reference, AlignmentRef::ConstructionPlane { .. });
            let f = doc.feature(*feature).ok_or_else(|| bad("alignment feature no longer exists"))?;
            if if plane {
                !matches!(f.kind, crate::FeatureKind::ConstructionPlane { .. })
            } else {
                !matches!(f.kind, crate::FeatureKind::ConstructionAxis { .. })
            } {
                return Err(bad("alignment construction has the wrong kind"));
            }
            let mut found = st.construct.iter().filter(|c| c.feature == *feature);
            let c = found.next().ok_or_else(|| bad("alignment construction is not evaluated"))?;
            if found.next().is_some() {
                return Err(bad("alignment construction identity is ambiguous"));
            }
            match (&c.geom, plane) {
                (crate::construct::ConstructGeom::Axis { dir, .. }, false) => Ok(*dir),
                (crate::construct::ConstructGeom::Plane(p), true) => Ok(p.normal()),
                _ => Err(bad("alignment evaluated construction has the wrong kind")),
            }
        }
        AlignmentRef::SketchLine { sketch, curve } => {
            owns(*sketch)?;
            let ss = st.sketch(*sketch).ok_or_else(|| bad("alignment sketch no longer exists"))?;
            let index = ss.sketch.curve_index(curve).ok_or_else(|| bad("alignment line no longer exists"))?;
            if !matches!(ss.sketch.curves[index].kind, solidcraft_sketch::CurveKind::Line { .. }) {
                return Err(bad("alignment sketch curve must remain a line"));
            }
            let solidcraft_sketch::CurveKind::Line { a, b } = ss.sketch.curves[index].kind else {
                return Err(bad("alignment sketch curve must remain a line"));
            };
            let (a, b) = (
                ss.sketch.point(a).ok_or_else(|| bad("alignment endpoint is missing"))?,
                ss.sketch.point(b).ok_or_else(|| bad("alignment endpoint is missing"))?,
            );
            Ok(ss.plane.to_world(b) - ss.plane.to_world(a))
        }
        AlignmentRef::OriginAxis { axis } => match axis.as_str() {
            "X" => Ok(Vec3::X),
            "Y" => Ok(Vec3::Y),
            "Z" => Ok(Vec3::Z),
            _ => Err(bad("unknown alignment axis")),
        },
        AlignmentRef::OriginPlane { plane } => match plane.as_str() {
            "XY" => Ok(Vec3::Z),
            "XZ" => Ok(-Vec3::Y),
            "YZ" => Ok(Vec3::X),
            _ => Err(bad("unknown alignment plane")),
        },
    }
}

/// The unprojected direction of a secondary reference. The existing frame resolver performs
/// the one strict projection; legacy references retain their fallback policy.
pub(super) fn x_axis(doc: &Document, st: &ModelState, component: u64, x: Option<&XRef>, o: Vec3) -> Result<Option<Vec3>> {
    Ok(match x {
        None => None,
        Some(XRef::Dir(d)) => Some(*d),
        Some(XRef::Reference { reference, reverse }) => {
            let direction = alignment_direction(doc, st, reference, component, o)?;
            Some(if *reverse { -direction } else { direction })
        }
        Some(XRef::Planar { planar_face, reverse }) => {
            let (body, face) = alignment_face(doc, st, component, planar_face)?;
            let Some(solidcraft_kernel::FaceSurface::Plane { normal, .. }) = solidcraft_kernel::face_surface(&body.body, face) else {
                return Err(DocError::Invalid("alignment reference must remain planar".into()));
            };
            let n = doc.model_body_offset(body).map_or(*normal, |m| apply_vector(m, *normal));
            Some(if *reverse { -n } else { n })
        }
        Some(XRef::Face { face, pick }) => {
            let nf = named_face(doc, st, component, pick.unwrap_or(o), face)?;
            if let Some((_, n)) = nf.plane()? {
                Some(n)
            } else if let Some(c) = nf.cylinder() {
                let v = c.axis_point - o;
                Some(v - c.axis * v.dot(c.axis))
            } else {
                Some(nf.point - o)
            }
        }
    })
}

/// The persistent name of the face of a component's bodies at `pick` (the component's
/// coordinates), measured to each face's triangles and, among the faces as near as the mesh
/// lets them be told apart, to the face's own surface (a point on a thin plate's hole wall lies
/// as near the facets of the plate's face round the rim). `round` prefers a cylinder on a tie
/// (a pick on a hole's rim names its wall). `None`: no named face there.
pub fn face_name_at(doc: &Document, st: &ModelState, component: u64, pick: Vec3, round: bool) -> Option<String> {
    let mut best: Option<(f64, &ModelBody, usize)> = None;
    for b in component_bodies(doc, st, component) {
        if b.body.is_mesh() {
            continue;
        }
        let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
        let p = apply_point(&mat_inverse(&offset)?, pick);
        let mesh = b.mesh();
        let tol = (b.body.size() * 1e-3).max(1e-3).max(b.display_tol());
        let mut near: BTreeMap<usize, f64> = BTreeMap::new();
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            let Some([x, y, z]) = mesh.tri(t) else { continue };
            let d = crate::naming::closest(p, x, y, z).dist(p);
            near.entry(*f as usize).and_modify(|n| *n = n.min(d)).or_insert(d);
        }
        let nearest = near.values().copied().fold(f64::INFINITY, f64::min);
        if nearest > tol * 5.0 {
            continue;
        }
        for (f, d) in near.into_iter().filter(|(_, d)| *d <= nearest + tol) {
            let (off, round_bonus) = match solidcraft_kernel::face_surface(&b.body, f) {
                Some(solidcraft_kernel::FaceSurface::Plane { normal, point }) => (d.min((p - point).dot(normal).abs()), 0.0),
                Some(solidcraft_kernel::FaceSurface::Cylinder { .. }) => {
                    let off = solidcraft_kernel::cylinder_face(&b.body, &mesh, f).map_or(d, |c| d.min(c.off(p)));
                    (off, if round { tol + 1e-6 * tol } else { 0.0 })
                }
                Some(surf @ solidcraft_kernel::FaceSurface::Cone { .. }) => match solidcraft_kernel::cone_face(&b.body, f) {
                    Some((c, half_angle)) => {
                        let v = p - c.axis_point;
                        let along = v.dot(c.axis);
                        let radial = (v - c.axis * along).len();
                        let exact = match surf.nearest(p) {
                            Some(q) => {
                                let sq = (q - c.axis_point).dot(c.axis);
                                if sq >= c.start && sq <= c.end {
                                    q.dist(p)
                                } else {
                                    let clamped = sq.clamp(c.start, c.end);
                                    let dr = radial - clamped * half_angle.tan();
                                    let ds = along - clamped;
                                    (dr * dr + ds * ds).sqrt()
                                }
                            }
                            None => d,
                        };
                        (d.min(exact), if round { tol } else { 0.0 })
                    }
                    None => (d, 0.0),
                },
                _ => (d, 0.0),
            };
            let score = off - round_bonus;
            if best.as_ref().is_none_or(|(s, ..)| score < *s) {
                best = Some((score, b, f));
            }
        }
    }
    let (_, b, f) = best?;
    crate::naming::face_names(b).get(f).filter(|n| !n.is_empty()).cloned()
}

/// Whether the face called `name` nearest `pick` (the component's coordinates) on the bodies of
/// `component` is round: a cylinder or cone, as its exact surface says. `false` when no face has
/// that name.
pub fn face_is_round(doc: &Document, st: &ModelState, component: u64, pick: Vec3, name: &str) -> bool {
    named_face(doc, st, component, pick, name).is_ok_and(|f| {
        matches!(
            solidcraft_kernel::face_surface(&f.body.body, f.face),
            Some(solidcraft_kernel::FaceSurface::Cylinder { .. } | solidcraft_kernel::FaceSurface::Cone { .. })
        )
    })
}

/// A circle-centre snap for the round end of a component's cylindrical or conical face centred at
/// `centre` (the component's coordinates; a circle given by its centre, where no face lies),
/// with the face's name and its end's: `None` when no named cylinder or cone ends there.
pub fn round_end_at(doc: &Document, st: &ModelState, component: u64, centre: Vec3) -> Option<Snap> {
    let mut best: Option<(f64, bool, &ModelBody, usize, CylinderFace, f64)> = None;
    for b in component_bodies(doc, st, component) {
        if b.body.is_mesh() {
            continue;
        }
        let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
        let p = apply_point(&mat_inverse(&offset)?, centre);
        let mesh = b.mesh();
        let size = b.body.size();
        let tol = (size * 1e-3).max(1e-3);
        for f in 0..b.body.face_count() {
            let Some(rf) = solidcraft_kernel::cylinder_face(&b.body, &mesh, f)
                .map(|cyl| RoundFace { cyl, cone_half_angle: None })
                .or_else(|| cone_round_in(&b.body, f))
            else {
                continue;
            };
            let c = rf.cyl;
            let v = p - c.axis_point;
            let along = v.dot(c.axis);
            for (at, _) in rf.valid_ends(size) {
                let off = (v - c.axis * along).len() + (along - at).abs();
                let better = best.as_ref().is_none_or(|(bo, b_cone, ..)| off < *bo - 1e-9 || ((off - *bo).abs() <= 1e-9 && !rf.is_cone() && *b_cone));
                if off <= tol && better {
                    best = Some((off, rf.is_cone(), b, f, c, at));
                }
            }
        }
    }
    let (_, _, b, f, c, at) = best?;
    let names = crate::naming::face_names(b);
    let face = names.get(f).filter(|n| !n.is_empty()).cloned()?;
    let end = rim_neighbour(b, f, &c, at).and_then(|n| names.get(n)).filter(|n| !n.is_empty()).cloned();
    Some(Snap::CircleCenter { pick: centre, face: Some(face), end, x: None })
}

/// Name the end a named circle-centre snap of `component` takes now (the face across the rim
/// there), so it keeps that end when the design changes. Other snaps, and one whose end is named
/// already, are left as they are.
pub fn pin_end(doc: &Document, st: &ModelState, component: u64, snap: &mut Snap) {
    let Snap::CircleCenter { pick, face: Some(face), end: end @ None, .. } = snap else { return };
    let Ok(nf) = named_face(doc, st, component, *pick, face) else { return };
    let Some(own) = nf.own_round() else { return };
    let (at, _) = end_by_pick(&nf, &own, *pick);
    *end = rim_neighbour(nf.body, nf.face, &own.cyl, at).and_then(|n| crate::naming::face_names(nf.body).get(n).cloned()).filter(|n| !n.is_empty());
}

/// The names of the faces across the rims at the ends of the cylindrical or conical face a named
/// circle-centre snap of `component` names (what its `end` can be, including planar ends
/// reachable across an added chamfer or fillet); empty when the snap names no round face.
pub fn end_faces(doc: &Document, st: &ModelState, component: u64, snap: &Snap) -> Vec<String> {
    let Snap::CircleCenter { pick, face: Some(face), .. } = snap else { return Vec::new() };
    let Ok(nf) = named_face(doc, st, component, *pick, face) else { return Vec::new() };
    let Some(own) = nf.own_round() else { return Vec::new() };
    let names = crate::naming::face_names(nf.body);
    let mut out = Vec::new();
    let mut push = |f: usize| {
        if let Some(n) = names.get(f)
            && !n.is_empty()
            && !out.contains(n)
        {
            out.push(n.clone());
        }
    };
    for end in own.valid_ends(nf.body.body.size()) {
        if let Some(direct) = rim_neighbour(nf.body, nf.face, &own.cyl, end.0) {
            push(direct);
        }
        if !own.is_cone() {
            for (across, _) in bevel_end_faces(nf.body, nf.face, &own.cyl, end) {
                push(across);
            }
        }
    }
    out
}

/// The face `model.faces` calls `name` (on the body called `body`, or any body): the component
/// it belongs to and a point on it in that component's coordinates, for a joint origin given by
/// a face's name. `Err` says why there is no one such face.
pub fn named_face_spot(doc: &Document, st: &ModelState, body: Option<&str>, name: &str) -> std::result::Result<(u64, Vec3), String> {
    let hit = crate::name_lookup::locate_one(st, body, name)?;
    let b = st.bodies.get(hit.body).ok_or_else(|| format!("no face is named `{name}`"))?;
    let spot = crate::naming::face_spot(b, hit.face).ok_or_else(|| format!("face `{name}` has no surface to pick"))?;
    let offset = doc.model_body_offset(b).copied().unwrap_or(IDENTITY);
    Ok((doc.model_body_component(b), apply_point(&offset, spot.point)))
}
