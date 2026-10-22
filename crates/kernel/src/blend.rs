//! Edge fillets and chamfers. SMLib blends exactly the picked edges in one call
//! ([`crate::blend_smlib`]): the blend faces, the corners where blends meet (a sphere, a patch, a
//! bevel or a mitre, as its fillet corners choose) and the end faces rebuilt round them. A blend
//! whose contact lines would leave the faces it rolls on is SMLib's refusal, reported as
//! [`KernelError::TooLarge`]. A tangent chain is the caller's to select ([`tangent_chain`]).
//!
//! The helpers here read a body's schema 4 record: planar faces' outward normals, the direction
//! into a face across an edge, an edge's tangent at its ends.

use std::collections::BTreeSet;

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::{CurveEval, SurfaceEval};
use solidcraft_geom::brep::{Brep, Curve, Loop, OrientedEdge, Shell, Surface};

use crate::body::Body;
use crate::{KernelError, Result};

pub(crate) fn unsupported(msg: &str) -> KernelError {
    KernelError::Failed(format!("not supported yet: {msg}"))
}

/// A blend too large for the faces around it.
pub(crate) fn too_large(msg: &str) -> KernelError {
    KernelError::TooLarge(msg.into())
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A body's record as the blends' checks read it: the record and each face's side in its shell.
pub(crate) struct Faces {
    pub(crate) rec: Brep,
    /// Whether each face's outward normal is its surface's natural normal (`None`: in no face shell).
    forward: Vec<Option<bool>>,
}

impl Faces {
    pub(crate) fn of(body: &Body) -> Result<Faces> {
        let rec = crate::native::brep_record(body)?;
        let mut forward = vec![None; rec.faces.len()];
        for sh in &rec.shells {
            if let Shell::Faces { faces, .. } = sh {
                for f in faces {
                    if let Some(slot) = forward.get_mut(f.face as usize) {
                        *slot = Some(f.forward);
                    }
                }
            }
        }
        Ok(Faces { rec, forward })
    }

    pub(crate) fn point(&self, v: u32) -> Option<Vec3> {
        self.rec.vertices.get(v as usize).map(|v| v3(v.point)).filter(|p| p.is_finite())
    }

    /// The points of the edge's start and end vertices.
    pub(crate) fn ends(&self, e: usize) -> Option<(Vec3, Vec3)> {
        let [a, b] = self.rec.edges.get(e)?.vertices;
        Some((self.point(a)?, self.point(b)?))
    }

    /// Whether the edge's curve is a straight line.
    pub(crate) fn straight(&self, e: usize) -> bool {
        self.rec.edges.get(e).is_some_and(|x| matches!(x.curve, Curve::Line { .. }))
    }

    /// The unit natural normal of a planar face's surface.
    fn natural_plane_normal(&self, f: usize) -> Option<Vec3> {
        let (du, dv) = match &self.rec.faces.get(f)?.surface {
            Surface::Plane { u_axis, v_axis, .. } => (v3(*u_axis), v3(*v_axis)),
            Surface::PlaneThrough { origin, u_point, v_point } => (v3(*u_point) - v3(*origin), v3(*v_point) - v3(*origin)),
            _ => return None,
        };
        du.cross(dv).normalized().filter(|n| n.is_finite())
    }

    /// The side of the face in its shell: 1 where its outward normal is its surface's natural one.
    fn side(&self, f: usize) -> Option<f64> {
        self.forward.get(f).copied().flatten().map(|fw| if fw { 1.0 } else { -1.0 })
    }

    /// The outward unit normal of a planar face, if planar.
    pub(crate) fn plane_normal(&self, f: usize) -> Option<Vec3> {
        Some(self.natural_plane_normal(f)? * self.side(f)?)
    }

    /// The outward unit normal of face `f` at `p` (a point within `accept` of its surface).
    #[cfg(test)]
    pub(crate) fn normal_at(&self, f: usize, p: Vec3, accept: f64) -> Option<Vec3> {
        let s = SurfaceEval::new(&self.rec.faces.get(f)?.surface).ok()?;
        let uv = s.project(p, None, accept)?;
        let (_, su, sv) = s.derivs(uv[0], uv[1])?;
        Some(su.cross(sv).normalized()? * self.side(f)?)
    }

    /// Every edge use in the face's loops.
    pub(crate) fn uses(&self, f: usize) -> impl Iterator<Item = &OrientedEdge> {
        self.rec.faces.get(f).into_iter().flat_map(|face| face.loops.iter()).flat_map(|l| match l {
            Loop::Edges { edges } => edges.as_slice(),
            Loop::Vertex { .. } => &[],
        })
    }

    /// The faces that use the edge, each once.
    pub(crate) fn faces_of(&self, e: usize) -> BTreeSet<u32> {
        self.rec.edges.get(e).map(|x| x.uses.iter().map(|u| u.face).collect()).unwrap_or_default()
    }

    /// The unit direction into planar face `f` across the straight edge of `u`, square to the
    /// edge: the face lies to the left of its loop seen from its surface's natural normal.
    pub(crate) fn direction_into_face(&self, f: usize, u: &OrientedEdge) -> Option<Vec3> {
        let (a, b) = self.ends(u.edge as usize)?;
        let dir = if u.forward { b - a } else { a - b };
        self.natural_plane_normal(f)?.cross(dir.normalized()?).normalized()
    }

    /// The edge's unit tangent along its interval (start to end vertex) at its start or end.
    pub(crate) fn tangent(&self, e: usize, at_end: bool) -> Option<Vec3> {
        let edge = self.rec.edges.get(e)?;
        curve_tangent(&edge.curve, if at_end { edge.interval[1] } else { edge.interval[0] })
    }
}

/// The unit tangent of a record curve at `t`, along increasing `t`.
pub(crate) fn curve_tangent(c: &Curve, t: f64) -> Option<Vec3> {
    curve_derivative(c, t)?.normalized().filter(|d| d.is_finite())
}

/// The first derivative of a record curve at `t` (of an intersection curve: its unit tangent).
pub(crate) fn curve_derivative(c: &Curve, t: f64) -> Option<Vec3> {
    if !t.is_finite() {
        return None;
    }
    let d = match c {
        Curve::Line { start, end } => v3(*end) - v3(*start),
        Curve::Circle { frame, radius } => {
            let (s, co) = t.sin_cos();
            (v3(frame.y_axis) * co - v3(frame.x_axis) * s) * *radius
        }
        Curve::Ellipse { frame, radii } => {
            let (s, co) = t.sin_cos();
            v3(frame.y_axis) * (radii[1] * co) - v3(frame.x_axis) * (radii[0] * s)
        }
        Curve::Polyline { points } => {
            let last = points.len().checked_sub(2)?;
            let i = (t.max(0.0).floor() as usize).min(last);
            v3(*points.get(i + 1)?) - v3(*points.get(i)?)
        }
        Curve::Spline { .. } | Curve::RationalSpline { .. } => match CurveEval::new(c).ok()? {
            CurveEval::Spline(s) => s.eval(t)?.1,
            _ => return None,
        },
        Curve::SurfaceIntersection { surfaces, leader, .. } => {
            // Along both surfaces: square to both normals, on the leader's side.
            let p = CurveEval::new(c).ok()?.point(t)?;
            let normal = |s: &Surface| -> Option<Vec3> {
                let e = SurfaceEval::new(s).ok()?;
                let uv = e.project(p, None, f64::INFINITY)?;
                let (_, su, sv) = e.derivs(uv[0], uv[1])?;
                Some(su.cross(sv))
            };
            let along = normal(&surfaces[0])?.cross(normal(&surfaces[1])?);
            let along = along.normalized()?;
            if along.dot(curve_tangent(leader, t)?) < 0.0 { along * -1.0 } else { along }
        }
    };
    d.is_finite().then_some(d)
}

/// Curvature-continuous (G2) fillets: each section is SMLib's G2 blend curve between the contact
/// lines the round of `radius` has (zero curvature at the contacts on planar faces), not a
/// circle. A configuration SMLib cannot blend so is refused, never given a circular round.
pub fn fillet_g2(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    body.require_brep("G2 fillet")?;
    if !(radius.is_finite() && radius > 1e-6 && radius < 1e6) {
        return Err(KernelError::Invalid("G2 fillet size must be positive and finite".into()));
    }
    if edges.is_empty() || edges.len() > 1000 || edges.iter().any(|p| ![p.x, p.y, p.z].into_iter().all(f64::is_finite)) {
        return Err(KernelError::Invalid("G2 fillet needs 1 to 1000 finite edge points".into()));
    }
    let out = crate::blend_smlib::fillet_g2(body, edges, radius)?;
    closed(body, out)
}

/// A fillet's result, refused when a face's boundary crosses itself (#871): rounds that
/// overlap where a wall is thinner than both leave such faces, which the topology checks
/// don't see but which show as holes in the model. A fillet either closes the solid or is
/// refused. Faces of `input` that crossed already and that the fillet carried over unchanged
/// (a sliver an earlier boolean left far from the rounds, #1488) are not the fillet's doing.
fn closed(input: &Body, b: Body) -> Result<Body> {
    let after = b.crossed_faces()?;
    if after.is_empty() {
        return Ok(b);
    }
    let before = input.crossed_faces()?;
    let tol = (input.size().max(b.size()) * 1e-6).max(1e-9);
    match after.iter().filter(|p| !before.iter().any(|q| q.dist(**p) <= tol)).count() {
        0 => Ok(b),
        _ => Err(KernelError::Failed(
            "fillet: the rounds would leave openings in the surface (they overlap where a wall or a face is narrower than both)".into(),
        )),
    }
}

/// Constant-radius fillet of the edges nearest to the given points. A radius too large for the
/// faces around the edges is refused with [`KernelError::TooLarge`] ([`fillet_max`] finds the
/// largest that works).
pub fn fillet(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    crate::blend_smlib::fillet(body, edges, radius).and_then(|b| closed(body, b))
}

/// [`fillet`] whose corners where three blended edges meet are G2 patches. SMLib's fillet
/// corners are a sphere or a Coons patch that meets the rounds tangentially, so this is refused
/// rather than quietly given those corners; [`fillet`] (the default) is unchanged.
pub fn fillet_g2_corners(body: &Body, edges: &[Vec3], radius: f64) -> Result<Body> {
    body.require_brep("fillet")?;
    if !(radius.is_finite() && radius > 0.0) {
        return Err(KernelError::Invalid("the blend radius must be finite and positive".into()));
    }
    let _ = edges;
    Err(crate::blend_smlib::unsupported("a fillet with G2 corner patches"))
}

/// The step of the radius search: three significant digits of the requested radius.
fn radius_step(radius: f64) -> f64 {
    10f64.powf(radius.log10().floor() - 3.0)
}

/// The largest radius below `radius` (to three significant digits) at which the fillet of
/// these edges works, with its result; `None` when none does. Each candidate is a real fillet
/// of the edges, so the radius returned is one that was built; the search runs up to 16 of
/// them, so it is for an explicit request, not every refusal.
pub fn fillet_max(body: &Body, edges: &[Vec3], radius: f64) -> Option<(f64, Body)> {
    if !(radius.is_finite() && radius > 1e-6) {
        return None;
    }
    let q = radius_step(radius);
    // Radii n·q for n in [0, hi): hi fails (it is the request, or above it), lo works.
    let mut hi = (radius / q).ceil() as u64;
    let mut lo = 0u64;
    let mut best: Option<Body> = None;
    // log2(10⁴) steps reach the step size; the cap keeps a slow body from stalling.
    for _ in 0..16 {
        if hi - lo <= 1 {
            break;
        }
        let mid = lo + (hi - lo) / 2;
        match fillet(body, edges, mid as f64 * q) {
            Ok(b) => {
                lo = mid;
                best = Some(b);
            }
            Err(_) => hi = mid,
        }
    }
    best.map(|b| (lo as f64 * q, b))
}

/// [`fillet_max`] for each group of the edges that may limit each other's radius (sharing a
/// face or a vertex, directly or through other edges in the group), so a round that fits isn't
/// reduced for a tight edge elsewhere. The radius per edge (in `edges`' order) and the result;
/// one radius for all (as [`fillet_max`]) when a group is refused for another reason than its
/// size, the groups don't build together, or there are too many to search (each search is up
/// to 16 fillets, and this runs in the live preview).
pub fn fillet_max_each(body: &Body, edges: &[Vec3], radius: f64) -> Option<(Vec<f64>, Body)> {
    const MAX_GROUPS: usize = 8;
    const MAX_SEARCHES: usize = 2;
    let one = || fillet_max(body, edges, radius).map(|(r, b)| (vec![r; edges.len()], b));
    let Some(groups) = interacting_groups(body, edges).filter(|g| g.len() > 1 && g.len() <= MAX_GROUPS) else { return one() };
    let mut radii = vec![radius; edges.len()];
    let mut searches = 0;
    for g in &groups {
        let pts: Vec<Vec3> = g.iter().filter_map(|i| edges.get(*i).copied()).collect();
        let r = match fillet(body, &pts, radius) {
            Ok(_) => continue,
            Err(KernelError::TooLarge(_)) if searches < MAX_SEARCHES => {
                searches += 1;
                match fillet_max(body, &pts, radius) {
                    Some((r, _)) => r,
                    None => return one(),
                }
            }
            Err(_) => return one(),
        };
        for i in g {
            if let Some(x) = radii.get_mut(*i) {
                *x = r;
            }
        }
    }
    match fillet_each(body, edges, &radii) {
        Ok(b) => Some((radii, b)),
        Err(_) => one(),
    }
}

/// The fillet of each edge at its own radius (`radii` in `edges`' order), all in one SMLib
/// blend.
pub fn fillet_each(body: &Body, edges: &[Vec3], radii: &[f64]) -> Result<Body> {
    if radii.len() != edges.len() {
        return Err(KernelError::Invalid("one radius per edge".into()));
    }
    if let Some(r) = radii.first().filter(|r| radii.iter().all(|x| x.to_bits() == r.to_bits())) {
        return fillet(body, edges, *r);
    }
    body.require_brep("fillet")?;
    crate::blend_smlib::fillet_each(body, edges, radii).and_then(|b| closed(body, b))
}

/// The points' edges (the nearest edge of the body to each) grouped where they share a face or
/// a vertex, directly or through other points' edges: indices into `pts`. `None` when a point
/// has no edge near it.
fn interacting_groups(body: &Body, pts: &[Vec3]) -> Option<Vec<Vec<usize>>> {
    let rec = Faces::of(body).ok()?;
    let size = body.size();
    let tol = (size * 2e-3).max(1e-3);
    // What each point's edge touches: its faces and its ends.
    let mut groups: Vec<(BTreeSet<u32>, BTreeSet<u32>, Vec<usize>)> = Vec::new();
    for (i, p) in pts.iter().enumerate() {
        let (idx, dist) = body.nearest_edge(*p, tol).ok()??;
        if dist > size * 0.05 + 1e-3 {
            return None;
        }
        let mut faces = rec.faces_of(idx);
        let mut ends: BTreeSet<u32> = rec.rec.edges.get(idx)?.vertices.into_iter().collect();
        let mut members = vec![i];
        // Join every group touching one of these faces or ends.
        let (meet, rest): (Vec<_>, Vec<_>) = groups.into_iter().partition(|(fs, vs, _)| !fs.is_disjoint(&faces) || !vs.is_disjoint(&ends));
        for (fs, vs, ms) in meet {
            faces.extend(fs);
            ends.extend(vs);
            members.extend(ms);
        }
        groups = rest;
        groups.push((faces, ends, members));
    }
    Some(
        groups
            .into_iter()
            .map(|(_, _, mut ms)| {
                ms.sort_unstable();
                ms
            })
            .collect(),
    )
}

/// Equal-distance chamfer of the edges nearest to the given points, `distance` back along each
/// face, neighbouring chamfers meeting in a mitre.
pub fn chamfer(body: &Body, edges: &[Vec3], distance: f64) -> Result<Body> {
    body.require_brep("chamfer")?;
    if !(distance.is_finite() && distance > 1e-6 && distance < 1e6) {
        return Err(KernelError::Invalid("chamfer size must be positive".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    crate::blend_smlib::chamfer(body, edges, distance)
}

/// The second side of an unequal chamfer: a distance along the other face, or the angle the
/// chamfer makes with the first face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ChamferSide {
    Distance(f64),
    Angle(f64),
}

/// A chamfer with different setbacks on the two faces of each edge: `d1` along the first face
/// (the one facing up most, the other with `flip`), the second side by distance or angle.
pub fn chamfer_sides(body: &Body, edges: &[Vec3], d1: f64, side: ChamferSide, flip: bool) -> Result<Body> {
    body.require_brep("chamfer")?;
    let ok = |x: f64| x.is_finite() && x > 1e-6 && x < 1e6;
    let valid = ok(d1)
        && match side {
            ChamferSide::Distance(d) => ok(d),
            ChamferSide::Angle(a) => a.is_finite() && a > 1e-3 && a < std::f64::consts::PI - 1e-3,
        };
    if !valid {
        return Err(KernelError::Invalid("chamfer sizes must be positive (the angle within 0°…180°)".into()));
    }
    if edges.is_empty() || edges.len() > 1000 {
        return Err(KernelError::Invalid("select 1…1000 edges".into()));
    }
    crate::blend_smlib::chamfer_sides(body, edges, d1, side, flip)
}

/// The picked edges' tangent chains, for a blend that is to run along them: `picks` (each the
/// nearest edge of `body` to it), then a point on every other edge that continues one of them
/// tangentially (within 0.5° at a shared vertex), until no more do. An edge only one face uses
/// ends a chain. Picks that find no edge are refused.
pub fn tangent_chain(body: &Body, picks: &[Vec3]) -> Result<Vec<Vec3>> {
    /// Edges meeting at an angle below this continue one another.
    const TANGENT_DEGREES: f64 = 0.5;
    if picks.is_empty() || picks.len() > 1000 || picks.iter().any(|p| !p.is_finite()) {
        return Err(KernelError::Invalid("select 1…1000 edges by finite points".into()));
    }
    let rec = Faces::of(body)?;
    let size = body.size();
    let mut chain: Vec<usize> = Vec::new();
    for p in picks {
        let (e, dist) = body.nearest_edge(*p, (size * 2e-3).max(1e-3))?.ok_or_else(|| KernelError::Invalid("the body has no edges".into()))?;
        if dist > size * 0.05 + 1e-3 {
            return Err(KernelError::Invalid(format!("no edge near the pick at ({:.3}, {:.3}, {:.3})", p.x, p.y, p.z)));
        }
        if !chain.contains(&e) {
            chain.push(e);
        }
    }
    let picked = chain.len();
    // The unit direction edge `e` leaves its end `v` in.
    let leaving = |e: usize, v: u32| -> Option<Vec3> {
        let [a, b] = rec.rec.edges.get(e)?.vertices;
        if a == b {
            return None;
        }
        if a == v { rec.tangent(e, false) } else { rec.tangent(e, true).map(|t| t * -1.0) }
    };
    let cos = TANGENT_DEGREES.to_radians().cos();
    let mut k = 0;
    while k < chain.len() {
        let Some(&e) = chain.get(k) else { break };
        k += 1;
        let Some(edge) = rec.rec.edges.get(e) else { continue };
        for v in edge.vertices {
            let Some(d) = leaving(e, v) else { continue };
            for (f, other) in rec.rec.edges.iter().enumerate() {
                if chain.contains(&f) || other.uses.len() < 2 || !other.vertices.contains(&v) {
                    continue;
                }
                if leaving(f, v).is_some_and(|g| d.dot(g) < -cos) {
                    chain.push(f);
                }
            }
        }
    }
    let mut out = picks.to_vec();
    for e in chain.into_iter().skip(picked) {
        let edge = rec.rec.edges.get(e).ok_or_else(|| KernelError::Failed("a chain edge".into()))?;
        let mid = CurveEval::new(&edge.curve)
            .ok()
            .and_then(|c| c.point(0.5 * (edge.interval[0] + edge.interval[1])))
            .ok_or_else(|| KernelError::Failed("a point on a chain edge".into()))?;
        out.push(mid);
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};

    /// A round (`round`) or an equal-distance chamfer of the edges nearest `pts`.
    fn blend(body: &Body, pts: &[Vec3], size: f64, round: bool) -> Result<Body> {
        if round { fillet(body, pts, size) } else { chamfer(body, pts, size) }
    }

    fn vol(b: &Body) -> f64 {
        b.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume
    }

    /// Whether a vertex of `b` lies within `tol` of `p`.
    fn has_vertex(b: &Body, p: Vec3, tol: f64) -> bool {
        crate::native::brep_record(b).unwrap().vertices.iter().any(|v| v3(v.point).dist(p) < tol)
    }

    #[test]
    fn g2_fillet_has_the_quintics_area() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(20.0, 0.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.face_count(), body.face_count() + 1);
        // Independent analytic area: integrating x(t) y'(t) for the quintic
        // gives 113/756 times setback² for a right-angle corner.
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 8000.0 - (113.0 / 756.0) * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
    }

    #[test]
    fn g2_fillet_handles_concave_edges_and_separate_edges() {
        let lp = Loop2::polygon(&[
            Vec2::new(0.0, 0.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(20.0, 10.0),
            Vec2::new(10.0, 10.0),
            Vec2::new(10.0, 20.0),
            Vec2::new(0.0, 20.0),
        ]);
        let body = crate::build::extrude(&Plane::XY, &[Region2 { outer: lp, holes: vec![] }], 0.0, 20.0).unwrap().remove(0);
        let out = fillet_g2(&body, &[Vec3::new(10.0, 10.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 6000.0 + (113.0 / 756.0) * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(20.0, 0.0, 10.0), Vec3::new(0.0, 20.0, 10.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.face_count(), body.face_count() + 2);
    }

    #[test]
    fn g2_fillet_refuses_sections_crossing_a_cap_hole() {
        // The hole lies inside the setback without touching either side face.
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(20.0, 0.0), Vec2::new(20.0, 20.0), Vec2::new(0.0, 20.0)]),
            holes: vec![Loop2::circle(Vec2::new(0.5, 0.5), 0.15)],
        };
        for plane in [Plane::XY, Plane::XZ] {
            let body = crate::extrude(&plane, std::slice::from_ref(&region), 0.0, 20.0).unwrap().remove(0);
            let pick = plane.to_world(Vec2::ZERO) + plane.normal() * 10.0;
            assert_eq!(body.crossed_face_loops().unwrap(), 0);
            let out = fillet_g2(&body, &[pick], 0.5).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            assert_eq!(out.crossed_face_loops().unwrap(), 0);
            assert_eq!(out.face_count(), body.face_count() + 1);
            assert!(fillet_g2(&body, &[pick], 2.0).is_err());
        }
    }

    #[test]
    fn g2_fillet_meets_sloped_planar_ends() {
        // Inward and outward sloping ends, and one square end, in two orientations (#2708).
        for plane in [Plane::XY, Plane::XZ] {
            for (left, right) in [(-0.5, -0.5), (0.5, 0.5), (0.0, 0.5), (-0.25, 0.5)] {
                let region = Region2 {
                    outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0 - right * 20.0, 20.0), Vec2::new(left * 20.0, 20.0)]),
                    holes: vec![],
                };
                let body = crate::extrude(&plane, &[region], 0.0, 20.0).unwrap().pop().unwrap();
                let pick = plane.to_world(Vec2::new(20.0, 0.0)) + plane.normal() * 20.0;
                let out = fillet_g2(&body, &[pick], 5.0).unwrap();
                assert!(out.validity().is_empty(), "{:?}", out.validity());
                assert_eq!(out.crossed_face_loops().unwrap(), 0);
                assert_eq!(out.face_count(), 7);
                // Analytic volume: the removed section has area 113 r² / 756
                // and first moment ∫y dA = 13711 r³ / 486486. Axial length is 40−(left+right)y.
                let removed = 40.0 * 113.0 / 756.0 * 25.0 - (left + right) * 13711.0 / 486486.0 * 125.0;
                let expected = 16000.0 - 4000.0 * (left + right) - removed;
                let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
                assert!((volume - expected).abs() < 0.05, "{volume} vs {expected}");
            }
        }
    }

    #[test]
    fn g2_fillet_on_concave_edge_meets_caps_sloped_in_both_directions() {
        let region = Region2 {
            outer: Loop2::polygon(&[
                Vec2::ZERO,
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 10.0),
                Vec2::new(10.0, 10.0),
                Vec2::new(10.0, 20.0),
                Vec2::new(0.0, 20.0),
            ]),
            holes: vec![],
        };
        let body = crate::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
        // z -> z + 0.3x + 0.4y tilts both caps while leaving the vertical edge
        // and its side planes unchanged. This shear preserves volume.
        let body = crate::transform_matrix(&body, [[1.0, 0.0, 0.3, 0.0], [0.0, 1.0, 0.4, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]).unwrap();
        let out = fillet_g2(&body, &[Vec3::new(10.0, 10.0, 17.0)], 2.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.crossed_face_loops().unwrap(), 0);
        let volume = out.mesh(&crate::MeshTolerance::chord(0.0001)).unwrap().measure().volume;
        let expected = 6000.0 + 113.0 / 756.0 * 4.0 * 20.0;
        assert!((volume - expected).abs() < 0.02, "{volume} vs {expected}");
    }

    #[test]
    fn g2_fillet_refuses_invalid_geometry() {
        let body = crate::build::box_solid(Vec3::ZERO, Vec3::new(20.0, 20.0, 20.0)).unwrap();
        let edge = [Vec3::new(20.0, 0.0, 10.0)];
        for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e7] {
            assert!(fillet_g2(&body, &edge, radius).is_err());
        }
        assert!(fillet_g2(&body, &[], 2.0).is_err());
        assert!(fillet_g2(&body, &[Vec3::new(f64::NAN, 0.0, 0.0)], 2.0).is_err());
        assert!(fillet_g2(&body, &[Vec3::new(1000.0, 0.0, 0.0)], 2.0).is_err());
        assert!(fillet_g2(&body, &edge, 25.0).is_err());
    }

    /// A box with a cylinder joined across one side (#556): the bottom edge of that side runs
    /// into the cylinder, which is curved where the edge ends.
    fn box_and_boss() -> Body {
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
        let c = crate::build::cylinder(Vec3::new(40.0, 20.0, 0.0), Vec3::Z, 12.0, 40.0).unwrap();
        crate::ops::boolean(&b, &c, crate::BoolOp::Union).unwrap().unwrap()
    }

    #[test]
    fn a_blend_runs_into_a_cylinder_at_its_end() {
        let body = box_and_boss();
        let v0 = vol(&body);
        // The edge (40, 0..8, 0) ends in the cylinder wall at y = 8.
        let len = 20.0 - (144.0f64 - 0.0).sqrt();
        for (round, size, area) in [(true, 0.5, (1.0 - std::f64::consts::PI / 4.0) * 0.25), (false, 0.5, 0.125)] {
            let out = blend(&body, &[Vec3::new(40.0, 4.0, 0.0)], size, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            let removed = v0 - vol(&out);
            // The section runs on a little past y = 8 under the cylinder wall.
            assert!(removed > area * len * 0.99 && removed < area * (len + 0.02), "{removed} vs {}", area * len);
            assert_eq!(out.face_count(), body.face_count() + 1);
        }
    }

    #[test]
    fn a_blend_ends_on_a_hole_wall() {
        // A plate with a hole cutting through the middle of its top front edge: each piece of
        // that edge ends on the hole's wall.
        let plate = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 10.0)).unwrap();
        let hole = crate::build::cylinder(Vec3::new(20.0, 0.0, -1.0), Vec3::Z, 5.0, 12.0).unwrap();
        let body = crate::ops::boolean(&plate, &hole, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        for (round, area) in [(true, (1.0 - std::f64::consts::PI / 4.0)), (false, 0.5)] {
            let out = blend(&body, &[Vec3::new(5.0, 0.0, 10.0)], 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            let removed = v0 - vol(&out);
            assert!(removed > area * 14.0 && removed < area * 15.2, "{removed}");
        }
    }

    #[test]
    fn a_chamfer_of_a_hole_corner_is_one_bevel() {
        let plate = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 10.0)).unwrap();
        let pocket = crate::build::box_solid(Vec3::new(10.0, 10.0, -1.0), Vec3::new(30.0, 30.0, 12.0)).unwrap();
        let body = crate::ops::boolean(&plate, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        let one = chamfer(&body, &[Vec3::new(10.0, 10.0, 5.0)], 2.0).unwrap();
        assert!(one.validity().is_empty(), "{:?}", one.validity());
        assert_eq!(one.unmeshed_faces(), 0);
        let added = vol(&one) - v0;
        // The bevel fills the hole corner: a right triangle of legs 2 mm, times the 10 mm edge.
        assert!(added > 19.0 && added < 21.0, "added {added}, want 20");
        assert_eq!(one.face_count(), body.face_count() + 1, "one bevel face");
        let corners = [Vec3::new(10.0, 10.0, 5.0), Vec3::new(30.0, 10.0, 5.0), Vec3::new(30.0, 30.0, 5.0), Vec3::new(10.0, 30.0, 5.0)];
        let all = chamfer(&body, &corners, 2.0).unwrap();
        assert!(all.validity().is_empty(), "{:?}", all.validity());
        let added = vol(&all) - v0;
        assert!(added > 76.0 && added < 84.0, "added {added}, want 80");
        assert_eq!(all.face_count(), body.face_count() + 4, "one bevel face per corner");
    }

    #[test]
    fn a_chamfer_ends_across_two_faces() {
        // A plate's outline chamfered, then the mitre at a corner chamfered: its low end meets
        // both side faces, and the chamfer plane cuts back the vertical edge between them.
        let k = 5.0;
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(100.0, 50.0, 20.0)).unwrap();
        let outline = [Vec3::new(50.0, 0.0, 20.0), Vec3::new(100.0, 25.0, 20.0), Vec3::new(50.0, 50.0, 20.0), Vec3::new(0.0, 25.0, 20.0)];
        let c = chamfer(&b, &outline, k).unwrap();
        let out = chamfer(&c, &[Vec3::new(k / 2.0, k / 2.0, 20.0 - k / 2.0)], 1.0).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.lumps().unwrap().len(), 1);
        assert_eq!(out.face_count(), c.face_count() + 1);
        // About the triangle (sides 1 and 1 at 120°) along the mitre; its ends slant.
        let removed = vol(&c) - vol(&out);
        let along = 0.5 * 120f64.to_radians().sin() * k * 3f64.sqrt();
        assert!(removed > along * 0.8 && removed < along * 1.2, "{removed} vs {along}");
    }

    #[test]
    fn a_curved_edge_blend_ends_in_a_face_at_an_angle() {
        // A groove of radius 8 along y with its axis 4 below the top: the arcs where it meets the
        // front and back faces end in the top face at 60° to it.
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 20.0, 20.0)).unwrap();
        let c = crate::build::cylinder(Vec3::new(20.0, -1.0, 16.0), Vec3::Y, 8.0, 22.0).unwrap();
        let body = crate::ops::boolean(&b, &c, crate::BoolOp::Cut).unwrap().unwrap();
        let v0 = vol(&body);
        for round in [true, false] {
            let out = blend(&body, &[Vec3::new(20.0, 0.0, 8.0)], 0.5, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // About the section along the arc (240°, at the section's distance from the axis).
            let area = if round { (1.0 - std::f64::consts::PI / 4.0) * 0.25 } else { 0.125 };
            let along = 8.2 * 240f64.to_radians();
            let removed = v0 - vol(&out);
            assert!(removed > area * along * 0.9 && removed < area * along * 1.1, "{removed} vs {}", area * along);
        }
    }

    #[test]
    fn a_blend_of_an_edge_running_into_a_rounded_corner_follows_it() {
        // A box with one vertical edge rounded: a top edge meeting that round smoothly is blended
        // with the round's top arc and the next top edge, as one smooth chain.
        let b = crate::build::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap();
        let rounded = fillet(&b, &[Vec3::new(40.0, 30.0, 10.0)], 5.0).unwrap();
        for round in [true, false] {
            let out = blend(&rounded, &[Vec3::new(20.0, 30.0, 20.0)], 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            assert_eq!(out.face_count(), rounded.face_count() + 3);
        }
    }

    #[test]
    fn a_blend_ends_where_a_joined_boss_turns_back_round_its_end() {
        // The box's top right edge runs into the cylinder at (40, 8, 20): the blend's end lies on
        // the cylinder where the box was, now laid bare.
        let body = box_and_boss();
        let v0 = vol(&body);
        for (round, area) in [(true, (1.0 - std::f64::consts::PI / 4.0) * 0.64), (false, 0.32)] {
            let out = blend(&body, &[Vec3::new(40.0, 4.0, 20.0)], 0.8, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            assert_eq!(out.lumps().unwrap().len(), 1);
            let removed = v0 - vol(&out);
            assert!((removed - area * 8.0).abs() < area * 8.0 * 1e-3, "{removed} vs {}", area * 8.0);
        }
    }

    #[test]
    fn a_blend_along_a_boss_ends_on_the_box_side_beside_it() {
        // The boss's bottom edge outside the box (a half circle) ends at the box's side, square to
        // it: the side face takes the end section.
        let body = box_and_boss();
        let v0 = vol(&body);
        for (round, area, c) in [
            (true, (1.0 - std::f64::consts::PI / 4.0) * 0.64, 0.8 * (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * (4.0 - std::f64::consts::PI))),
            (false, 0.32, 0.8 / 3.0),
        ] {
            let out = blend(&body, &[Vec3::new(52.0, 20.0, 0.0)], 0.8, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // Pappus: the section turned half round the axis at its centroid's radius.
            let want = area * std::f64::consts::PI * (12.0 - c);
            let removed = v0 - vol(&out);
            assert!((removed - want).abs() < want * 2e-3, "{removed} vs {want}");
            // The app's planned mesh must also close, with the same analytic removal.
            let planned = crate::issue_851_tests::planned_volume(&body) - crate::issue_851_tests::planned_volume(&out);
            assert!((planned - want).abs() < want * 2e-3, "display mesh: {planned} vs {want}");
        }
    }

    #[test]
    fn a_blend_ends_on_a_face_running_along_its_seam_twice() {
        // #691: a pocket's floor edge along its straight wall ends on a boss's cylinder, which a
        // STEP file gives as one face running along its seam edge twice.
        let b = crate::build::box_solid(Vec3::new(-55.0, -20.0, -30.0), Vec3::new(55.0, 50.0, 0.0)).unwrap();
        let boss = crate::build::cylinder(Vec3::new(55.0, 15.0, -50.0), Vec3::Z, 25.0, 50.0).unwrap();
        let b = crate::ops::boolean(&b, &boss, crate::BoolOp::Union).unwrap().unwrap();
        let pocket = crate::build::box_solid(Vec3::new(-45.0, -21.0, -15.0), Vec3::new(55.0, 10.0, 1.0)).unwrap();
        let wall = crate::build::cylinder(Vec3::new(55.0, 15.0, -20.0), Vec3::Z, 25.0, 30.0).unwrap();
        let pocket = crate::ops::boolean(&pocket, &wall, crate::BoolOp::Cut).unwrap().unwrap();
        let made = crate::ops::boolean(&b, &pocket, crate::BoolOp::Cut).unwrap().unwrap();
        let body = crate::step_import(&crate::step_export(&[&made], "test").unwrap()).unwrap().bodies.remove(0).body;
        let rec = crate::native::brep_record(&body).unwrap();
        let seam_twice = rec.faces.iter().flat_map(|f| f.loops.iter()).any(|l| match l {
            Loop::Edges { edges } => edges.iter().enumerate().any(|(k, x)| edges.iter().skip(k + 1).any(|y| y.edge == x.edge)),
            Loop::Vertex { .. } => false,
        });
        assert!(seam_twice);
        let v0 = vol(&body);
        // The edge runs from x = −45 to where the wall meets the cylinder, x = 55 − √(25² − 5²).
        let len = 45.0 + 55.0 - (600f64).sqrt();
        for (round, area) in [(true, 1.0 - std::f64::consts::PI / 4.0), (false, 0.5)] {
            let out = blend(&body, &[Vec3::new(0.0, 10.0, -15.0)], 1.0, round).unwrap();
            assert!(out.validity().is_empty(), "{:?}", out.validity());
            // A concave edge: the blend adds material.
            let added = vol(&out) - v0;
            assert!((added - area * len).abs() < area * len * 0.01, "{added} vs {}", area * len);
        }
    }

    #[test]
    fn a_chamfer_meets_sloped_end_planes() {
        for slope in [-0.5, 0.5] {
            let region = Region2 {
                outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(40.0, 0.0), Vec2::new(40.0 - slope * 20.0, 20.0), Vec2::new(slope * 20.0, 20.0)]),
                holes: vec![],
            };
            let body = crate::extrude(&Plane::XY, &[region], 0.0, 20.0).unwrap().pop().unwrap();
            let pick = [Vec3::new(20.0, 0.0, 20.0)];
            let result = chamfer(&body, &pick, 2.0).unwrap();
            let removed = 2.0 * (40.0 - 2.0 * slope * 2.0 / 3.0);
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&result).unwrap().volume;
            assert!((actual - removed).abs() < 1e-6, "{actual} vs {removed}");
            assert!(result.validity().is_empty());
            // The new corner vertices lie on both original end planes.
            for point in [Vec3::new(slope * 2.0, 2.0, 20.0), Vec3::new(40.0 - slope * 2.0, 2.0, 20.0)] {
                assert!(has_vertex(&result, point, 1e-8));
            }
            assert!(chamfer(&body, &pick, 25.0).is_err());
            // A fillet ends in each sloped end plane along an ellipse: the removed section's
            // length is taken at its centroid, c from the edge.
            let rounded = fillet(&body, &pick, 2.0).unwrap();
            let (area, c) =
                ((1.0 - std::f64::consts::PI / 4.0) * 4.0, 2.0 * (10.0 - 3.0 * std::f64::consts::PI) / (3.0 * (4.0 - std::f64::consts::PI)));
            let removed = area * (40.0 - 2.0 * slope * c);
            let actual = crate::measure(&body).unwrap().volume - crate::measure(&rounded).unwrap().volume;
            assert!((actual - removed).abs() < 1e-3 * removed, "{actual} vs {removed}");
            assert!(rounded.validity().is_empty());
        }
    }

    /// A pin standing in a socket, 3.5 mm from two of its walls, in a large block. The cutters
    /// for its tip reach several times the chamfer's size past each edge (more on a larger part),
    /// into the walls, and their union took bites out of them: 7.2 mm³ instead of the tip's.
    /// A cutter whose far corners are in the material is not used; the tip alone is chamfered.
    #[test]
    fn chamfering_a_pin_in_a_socket_leaves_the_socket_walls() {
        let block = crate::build::box_solid(Vec3::new(0.0, 0.0, -135.0), Vec3::new(82.0, 41.0, 19.0)).unwrap();
        let socket = crate::build::box_solid(Vec3::new(6.0, 11.5, 0.0), Vec3::new(21.0, 35.5, 25.0)).unwrap();
        let pin = crate::build::box_solid(Vec3::new(9.5, 30.5, -1.0), Vec3::new(12.5, 32.0, 10.0)).unwrap();
        let cut = crate::boolean(&block, &socket, crate::BoolOp::Cut).unwrap().unwrap();
        let body = crate::boolean(&cut, &pin, crate::BoolOp::Union).unwrap().unwrap();
        let tip = [Vec3::new(11.0, 30.5, 10.0), Vec3::new(9.5, 31.25, 10.0), Vec3::new(11.0, 32.0, 10.0), Vec3::new(12.5, 31.25, 10.0)];
        let d = 0.5;
        let out = chamfer(&body, &tip, d).unwrap();
        // Four wedges along the tip's edges, less the four corner pyramids each pair shares.
        let removed = 0.5 * d * d * 2.0 * (3.0 + 1.5) - 4.0 * d * d * d / 3.0;
        let actual = crate::measure(&body).unwrap().volume - crate::measure(&out).unwrap().volume;
        assert!((actual - removed).abs() < 1e-3, "{actual} vs {removed}");
        assert_eq!(out.face_count(), body.face_count() + 4);
        assert!(out.validity().is_empty(), "{:?}", out.validity());
    }
}

#[cfg(test)]
#[path = "curvecorner_tests.rs"]
mod corner_tests;
