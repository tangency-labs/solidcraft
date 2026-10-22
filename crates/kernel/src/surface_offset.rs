//! Offset surfaces (Surface › Offset): faces of a body copied out at a signed distance along
//! their normals, as a new surface body. Planes and circular cylinders move exactly (as Thicken
//! moves them); any other face gets a B-spline surface fitted through offset samples on a grid,
//! refined until it is within a tolerance of the true offset everywhere it is checked. An
//! offset as deep as a face's smallest concave radius would fold it over, and is refused,
//! naming that radius. Neighbouring faces moved to the same place along their shared edge
//! (meeting smoothly) keep sharing it; across a sharp edge they come apart. Faces that close up
//! make a solid.

use std::collections::HashMap;

use mt::{BoundedCurve, InnerSpace, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};
use solidcraft_geom::{Grid, Vec3};
use truck_modeling as mt;

use crate::body::{Body, from_p3, p3};
use crate::build::interpolate_net;
use crate::history::FaceOrigin;
use crate::surfaces::{Offset, map_curve, map_surface, surface_body};
use crate::{KernelError, Result, guard};

/// Grid samples a side of a fitted offset starts with, and the most it is refined to.
const START_GRID: usize = 12;
const MAX_GRID: usize = 96;
/// Samples along an edge of a fitted offset.
const EDGE_SAMPLES: usize = 33;

/// How one face moves along its normals: exactly (a plane, a circular cylinder) or onto
/// surfaces fitted for the distances it was made for. Shared by Offset Surface and Thicken.
pub(crate) struct FaceMover {
    face: mt::Face,
    how: How,
}

enum How {
    Exact(Offset),
    /// Each fitted distance (by its bits) and its surface (absolute parameters of the face's own
    /// surface); the face's outward sign (its orientation).
    Fitted {
        surfaces: Vec<(u64, mt::BSplineSurface<mt::Point3>)>,
        sign: f64,
    },
}

impl FaceMover {
    /// A mover for `face` at each of `distances` (along its outward normal), refused where a
    /// distance would fold it over; fitted faces within `tolerance`.
    pub(crate) fn new(face: &mt::Face, distances: &[f64], tolerance: f64) -> Result<FaceMover> {
        let how = match Offset::of(face) {
            Some(off) => {
                if let Offset::Cylinder { r, out, .. } = off
                    && let Some(d) = distances.iter().find(|d| r + out * **d <= tolerance)
                {
                    return Err(KernelError::Invalid(format!("{d} mm is as deep as the cylinder's radius ({r:.4} mm): it would fold the face over")));
                }
                How::Exact(off)
            }
            None => {
                let mut surfaces = Vec::new();
                for d in distances.iter().filter(|d| **d != 0.0) {
                    if !surfaces.iter().any(|(b, _)| *b == d.to_bits()) {
                        surfaces.push((d.to_bits(), fit(face, *d, tolerance)?));
                    }
                }
                How::Fitted { surfaces, sign: if face.orientation() { 1.0 } else { -1.0 } }
            }
        };
        Ok(FaceMover { face: face.clone(), how })
    }

    /// Where the point `p` of the face goes at distance `d`.
    pub(crate) fn point(&self, p: Vec3, d: f64) -> Result<Vec3> {
        match &self.how {
            How::Exact(off) => Ok(off.map(p, d)),
            How::Fitted { .. } if d == 0.0 => Ok(p),
            How::Fitted { sign, .. } => offset_point(&self.face.surface(), p, sign * d),
        }
    }

    /// An edge curve of the face moved by `d` (exact, or a cubic through moved samples).
    pub(crate) fn curve(&self, c: &mt::Curve, d: f64) -> Result<mt::Curve> {
        match &self.how {
            How::Exact(off) => map_curve(off, c, d),
            How::Fitted { .. } if d == 0.0 => Ok(c.clone()),
            How::Fitted { .. } => {
                let (t0, t1) = c.range_tuple();
                let pts: Vec<Vec3> = (0..EDGE_SAMPLES)
                    .map(|k| self.point(from_p3(c.subs(t0 + (t1 - t0) * k as f64 / (EDGE_SAMPLES - 1) as f64)), d))
                    .collect::<Result<_>>()?;
                Ok(mt::Curve::BSplineCurve(crate::build::interpolate_cubic(&pts).ok_or_else(|| KernelError::Failed("offset edge".into()))?))
            }
        }
    }

    /// The face's surface moved by `d` (absolute: the face's orientation is kept apart).
    pub(crate) fn surface(&self, d: f64) -> Result<mt::Surface> {
        match &self.how {
            How::Exact(off) => map_surface(off, &self.face.surface(), d),
            How::Fitted { .. } if d == 0.0 => Ok(self.face.surface()),
            How::Fitted { surfaces, .. } => surfaces
                .iter()
                .find(|(b, _)| *b == d.to_bits())
                .map(|(_, s)| mt::Surface::BSplineSurface(s.clone()))
                .ok_or_else(|| KernelError::Failed("offset: no surface fitted for that distance".into())),
        }
    }
}

/// The picked faces of `body` (indices in its face order) offset by `distance` along their
/// outward normals (negative: against them) into one surface body. `tolerance`: how far a
/// fitted face may stray from the true offset. A distance of zero copies the faces. The
/// history is over `[body]`: each face is the offset of the one it came from.
pub fn offset_surface(body: &Body, faces: &[usize], distance: f64, tolerance: f64) -> Result<Body> {
    body.require_brep("offset")?;
    if !(distance.is_finite() && distance.abs() < 1e6) {
        return Err(KernelError::Invalid("the offset distance must be finite".into()));
    }
    if !(tolerance.is_finite() && (1e-6..=10.0).contains(&tolerance)) {
        return Err(KernelError::Invalid("the offset tolerance must be 0.000001…10 mm".into()));
    }
    let all: Vec<mt::Face> = body.solid.face_iter().cloned().collect();
    let mut picked: Vec<usize> = Vec::new();
    for &i in faces {
        if i >= all.len() {
            return Err(KernelError::Invalid("no such face".into()));
        }
        if !picked.contains(&i) {
            picked.push(i);
        }
    }
    if picked.is_empty() {
        return Err(KernelError::Invalid("pick the faces to offset".into()));
    }
    let chosen: Vec<(usize, &mt::Face)> = picked.iter().filter_map(|i| all.get(*i).map(|f| (*i, f))).collect();
    if distance == 0.0 {
        let copy = surface_body(chosen.iter().map(|(_, f)| (*f).clone()).collect());
        return Ok(crate::record::rebuilt_body(copy, &[&body.solid], &crate::record::Rebuild::default()));
    }
    let size = body.size().max(1e-6);
    let moves: Vec<FaceMover> = chosen.iter().map(|(_, f)| FaceMover::new(f, &[distance], tolerance)).collect::<Result<_>>()?;
    guard("offset surface", || {
        let near = |a: Vec3, b: Vec3| a.dist(b) <= tolerance.max(size * 1e-7) * 4.0;
        // Moved vertices and edges, shared while every face moves them to the same place.
        let mut verts: HashMap<mt::VertexID, Vec<mt::Vertex>> = HashMap::new();
        let mut edges: HashMap<mt::EdgeID, Vec<mt::Edge>> = HashMap::new();
        let mut out = Vec::with_capacity(chosen.len());
        let mut history = Vec::with_capacity(chosen.len());
        for ((i, f), mv) in chosen.iter().zip(&moves) {
            let map = |p: Vec3| mv.point(p, distance);
            let mut vertex = |v: &mt::Vertex| -> Result<mt::Vertex> {
                let at = map(from_p3(v.point()))?;
                let list = verts.entry(v.id()).or_default();
                if let Some(found) = list.iter().find(|m| near(from_p3(m.point()), at)) {
                    return Ok(found.clone());
                }
                let nv = mt::Vertex::new(p3(at));
                list.push(nv.clone());
                Ok(nv)
            };
            let mut wires = Vec::new();
            for w in f.absolute_boundaries() {
                let mut wire = Vec::new();
                for e in w.edge_iter() {
                    let abs = if e.orientation() { e.clone() } else { e.inverse() };
                    let (a, b) = (vertex(abs.front())?, vertex(abs.back())?);
                    let c = abs.curve();
                    let (t0, t1) = c.range_tuple();
                    let mid = map(from_p3(c.subs(0.5 * (t0 + t1))))?;
                    let list = edges.entry(abs.id()).or_default();
                    let shared = list.iter().find(|m| {
                        m.front().id() == a.id() && m.back().id() == b.id() && {
                            let mc = m.curve();
                            let (s0, s1) = mc.range_tuple();
                            near(from_p3(mc.subs(0.5 * (s0 + s1))), mid)
                        }
                    });
                    let moved = match shared {
                        Some(m) => m.clone(),
                        None => {
                            let curve = mv.curve(&c, distance)?;
                            let m = mt::Edge::new_unchecked(&a, &b, curve);
                            list.push(m.clone());
                            m
                        }
                    };
                    wire.push(if e.orientation() { moved } else { moved.inverse() });
                }
                wires.push(mt::Wire::from(wire));
            }
            let surface = mv.surface(distance)?;
            let mut nf = mt::Face::new_unchecked(wires, surface);
            if !f.orientation() {
                nf.invert();
            }
            out.push(nf);
            history.push(crate::record::generated(vec![FaceOrigin::Offset { operand: 0, face: *i }]));
        }
        let operand = crate::record::Operand::new(body);
        // Faces that close up (every face of a smooth closed body) make a solid, as Stitch makes
        // one.
        let shell = mt::Shell::from(out);
        let result = match shell.shell_condition() {
            mt::ShellCondition::Closed => match crate::body::Solid::try_new(vec![shell.clone()]) {
                Ok(solid) => Body::new(solid)?,
                Err(_) => surface_body(shell.into_iter().collect()),
            },
            _ => surface_body(shell.into_iter().collect()),
        };
        crate::record::attach(result, &[operand], Some(history), HashMap::new())
    })
}

/// The point `p` on `s` (absolute) moved by `d` along the surface's own normal there.
fn offset_point(s: &mt::Surface, p: Vec3, d: f64) -> Result<Vec3> {
    let (u, v) = s.search_nearest_parameter(p3(p), None, 100).ok_or_else(|| KernelError::Failed("offset: a point off its face".into()))?;
    let n = from_p3(mt::Point3::new(0.0, 0.0, 0.0) + s.normal(u, v));
    let n =
        n.normalized().ok_or_else(|| KernelError::Invalid("the face has a point with no normal (a pole or a crease): not supported yet".into()))?;
    Ok(from_p3(s.subs(u, v)) + n * d)
}

/// The fitted offset of a face that is not a plane or a circular cylinder.
fn fit(f: &mt::Face, distance: f64, tolerance: f64) -> Result<mt::BSplineSurface<mt::Point3>> {
    let s = f.surface();
    let sign = if f.orientation() { 1.0 } else { -1.0 };
    let d = sign * distance;
    // The face's span in its surface's parameters: its boundary, found on the surface.
    let (mut u0, mut u1, mut v0, mut v1) = (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    for e in f.absolute_boundaries().iter().flat_map(|w| w.edge_iter()) {
        let c = e.curve();
        let (t0, t1) = c.range_tuple();
        for k in 0..=16 {
            let q = c.subs(t0 + (t1 - t0) * k as f64 / 16.0);
            if let Some((u, v)) = s.search_nearest_parameter(q, None, 100) {
                (u0, u1, v0, v1) = (u0.min(u), u1.max(u), v0.min(v), v1.max(v));
            }
        }
    }
    if !(u0.is_finite() && u1 > u0 && v0.is_finite() && v1 > v0) {
        return Err(KernelError::Failed("offset: the face's span on its surface".into()));
    }
    let at = |a: f64, b: f64| -> Result<Vec3> {
        let (u, v) = (u0 + (u1 - u0) * a, v0 + (v1 - v0) * b);
        let n = from_p3(mt::Point3::new(0.0, 0.0, 0.0) + s.normal(u, v));
        let n = n
            .normalized()
            .ok_or_else(|| KernelError::Invalid("the face has a point with no normal (a pole or a crease): not supported yet".into()))?;
        Ok(from_p3(s.subs(u, v)) + n * d)
    };
    // Folding over: moving a point by d along its normal shrinks lengths along a principal
    // direction by 1 - d·k (k > 0 where the surface bends toward its normal); at zero the
    // offset reaches the centre of curvature and folds. Refused with a margin, naming the
    // tightest radius on that side.
    let mut tightest: Option<f64> = None;
    for i in 0..=START_GRID {
        for j in 0..=START_GRID {
            let (u, v) = (u0 + (u1 - u0) * i as f64 / START_GRID as f64, v0 + (v1 - v0) * j as f64 / START_GRID as f64);
            let Some((k1, k2)) = principal_curvatures(&s, u, v) else { continue };
            for k in [k1, k2] {
                if d * k > 0.0 && 1.0 - d * k <= 0.05 {
                    let r = 1.0 / k.abs();
                    tightest = Some(tightest.map_or(r, |t: f64| t.min(r)));
                }
            }
        }
    }
    if let Some(r) = tightest {
        return Err(KernelError::Invalid(format!(
            "the offset is as deep as the face's smallest concave radius on that side ({r:.4} mm): it would fold the face over"
        )));
    }
    let mut n = START_GRID;
    loop {
        let grid = Grid::from_fn([n + 1, n + 1], |i, j| at(i as f64 / n as f64, j as f64 / n as f64))
            .ok_or_else(|| KernelError::TooLarge("offset: the sample grid".into()))?
            .try_map(|p| p)?;
        let surface = interpolate_net(&grid).map(|fit| fit.surface).ok_or_else(|| KernelError::Failed("offset: fitting the surface".into()))?;
        // Checked between the samples, where a fit strays most.
        let mut worst = 0.0f64;
        for i in 0..n {
            for j in 0..n {
                let (a, b) = ((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64);
                worst = worst.max(from_p3(surface.subs(a, b)).dist(at(a, b)?));
            }
        }
        if worst <= tolerance {
            // Re-parameterised to the face's own surface span, so boundary points found on the
            // old surface map onto it.
            return Ok(reparameterised(surface, (u0, u1), (v0, v1)));
        }
        if n >= MAX_GRID {
            return Err(KernelError::Invalid(format!(
                "the offset can't be fitted within {tolerance} mm (off by up to {worst:.4} mm): try a larger tolerance"
            )));
        }
        n = (n * 2).min(MAX_GRID);
    }
}

/// The principal curvatures of `s` at (u, v), signed: positive where the surface bends toward
/// its normal there. `None` where it has no normal.
fn principal_curvatures(s: &mt::Surface, u: f64, v: f64) -> Option<(f64, f64)> {
    let (su, sv) = (s.uder(u, v), s.vder(u, v));
    let n = s.normal(u, v);
    if !(n.magnitude2() > 0.5) {
        return None;
    }
    let (e, f, g) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let (l, m, nn) = (s.uuder(u, v).dot(n), s.uvder(u, v).dot(n), s.vvder(u, v).dot(n));
    let det = e * g - f * f;
    if det.abs() < 1e-18 {
        return None;
    }
    let k = (l * nn - m * m) / det;
    let h = (e * nn - 2.0 * f * m + g * l) / (2.0 * det);
    let disc = (h * h - k).max(0.0).sqrt();
    Some((h + disc, h - disc))
}

/// `s` (on [0, 1]²) with its knots stretched onto `u` × `v`.
fn reparameterised(s: mt::BSplineSurface<mt::Point3>, u: (f64, f64), v: (f64, f64)) -> mt::BSplineSurface<mt::Point3> {
    let stretch = |kv: &mt::KnotVec, (a, b): (f64, f64)| mt::KnotVec::from(kv.iter().map(|k| a + (b - a) * k).collect::<Vec<_>>());
    let (ku, kv) = (stretch(s.uknot_vec(), u), stretch(s.vknot_vec(), v));
    mt::BSplineSurface::new((ku, kv), s.control_points().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{box_solid, cylinder, measure, sphere};

    fn rel(a: f64, b: f64) -> f64 {
        (a - b).abs() / b.abs().max(1e-9)
    }
    fn face_at(b: &Body, p: Vec3) -> usize {
        crate::surfaces::nearest_face(b, p).unwrap()
    }
    fn edges(b: &Body) -> usize {
        b.solid.edge_iter().map(|e| e.id()).collect::<std::collections::HashSet<_>>().len()
    }

    /// Planes move exactly: a box's top 5 up is a 10 × 10 square at z = 15. Two faces across a
    /// sharp edge come apart (8 edges, none shared); a zero distance copies.
    #[test]
    fn planes_move_and_sharp_edges_come_apart() {
        let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
        let top = face_at(&b, Vec3::new(5.0, 5.0, 10.0));
        let up = offset_surface(&b, &[top], 5.0, 0.01).unwrap();
        let m = measure(&up).unwrap();
        assert!(up.is_surface() && rel(m.area, 100.0) < 1e-9 && (m.bbox.min.z - 15.0).abs() < 1e-9, "{m:?}");
        let side = face_at(&b, Vec3::new(5.0, 0.0, 5.0));
        let two = offset_surface(&b, &[top, side], 1.0, 0.01).unwrap();
        assert!(two.face_count() == 2 && edges(&two) == 8);
        let copy = offset_surface(&b, &[top, side], 0.0, 0.01).unwrap();
        assert!(copy.face_count() == 2 && rel(measure(&copy).unwrap().area, 200.0) < 1e-9);
    }

    /// A cylinder's side (two halves) moved out by 2 keeps its halves joined, with radius 12:
    /// area 2π·12·20. Moved in past its axis, refused.
    #[test]
    fn a_cylinder_side_changes_radius() {
        let c = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap();
        let d = 10.0 / 2f64.sqrt();
        let halves = [face_at(&c, Vec3::new(d, d, 10.0)), face_at(&c, Vec3::new(-d, -d, 10.0))];
        let out = offset_surface(&c, &halves, 2.0, 0.01).unwrap();
        assert!(out.face_count() == 2);
        let want = 2.0 * std::f64::consts::PI * 12.0 * 20.0;
        assert!(rel(measure(&out).unwrap().area, want) < 1e-3, "{} vs {want}", measure(&out).unwrap().area); // (measured on meshes)
        let joined = offset_surface(&c, &halves, 0.0, 0.01).unwrap();
        assert_eq!(edges(&out), edges(&joined), "the halves still share their seams");
        assert!(offset_surface(&c, &halves, -10.5, 0.01).is_err());
    }

    /// A sphere (fitted faces) moved out by 2: area 4π·12², within the tolerance; moved in to
    /// its centre, refused with its radius named.
    #[test]
    fn a_sphere_is_fitted_and_refused_past_its_centre() {
        let s = sphere(Vec3::ZERO, 10.0).unwrap();
        let all: Vec<usize> = (0..s.face_count()).collect();
        let out = offset_surface(&s, &all, 2.0, 0.01).unwrap();
        let want = 4.0 * std::f64::consts::PI * 144.0;
        assert!(rel(measure(&out).unwrap().area, want) < 2e-3, "{} vs {want}", measure(&out).unwrap().area); // (measured on meshes)
        // The patches meet smoothly: their offsets still share every edge, and close up.
        assert_eq!(edges(&out), edges(&s));
        assert!(!out.is_surface(), "the offset sphere closes into a solid");
        assert!(rel(measure(&out).unwrap().volume, 4.0 / 3.0 * std::f64::consts::PI * 1728.0) < 2e-3, "outward"); // (measured on meshes)
        let e = offset_surface(&s, &all, -10.0, 0.01).unwrap_err().to_string();
        assert!(e.contains("10.0000"), "{e}");
    }

    /// A doubly curved patch (a saddle, a B-spline height field) moved by 1.5: every point of the
    /// offset lies 1.5 from the patch, within the tolerance.
    #[test]
    fn a_bspline_patch_is_offset_within_the_tolerance() {
        let b = box_solid(Vec3::new(-14.0, -14.0, 0.0), Vec3::new(14.0, 14.0, 20.0)).unwrap();
        let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
        let cup = crate::boolean(&b, &tool, crate::BoolOp::Cut).unwrap().unwrap();
        let z = |x: f64| 50.0 - (1600.0 - x * x).sqrt();
        let rim = [Vec3::new(14.0, 0.0, z(14.0)), Vec3::new(0.0, 14.0, z(0.0)), Vec3::new(-14.0, 0.0, z(-14.0)), Vec3::new(0.0, -14.0, z(0.0))];
        let cover = crate::patch_edges(&cup, &rim).unwrap();
        let face = cover.solid.face_iter().next().unwrap().clone();
        let out = offset_surface(&cover, &[0], 1.5, 0.01).unwrap();
        let mesh = out.mesh(&crate::MeshTolerance::chord(0.05)).unwrap();
        let s = face.surface();
        for p in mesh.positions.iter().step_by(7) {
            let (u, v) = s.search_nearest_parameter(p3(*p), None, 100).unwrap();
            let dist = from_p3(s.subs(u, v)).dist(*p);
            assert!((dist - 1.5).abs() < 0.03, "{dist} at {p:?}");
        }
    }
}
