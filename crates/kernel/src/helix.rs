//! Helical sweeps (coils, springs) and lofts to a point (pyramids, cones).

use mt::builder;
use solidcraft_geom::{Grid, Loop2, Mesh, Plane, Region2, Seg2, Vec2, Vec3};
use truck_modeling as mt;

use crate::body::{Body, Solid, p3};
use crate::build::{NetFit, cap_and_close, interpolate_net, uniform_wire};
use crate::numbered::NumberedLoop;
use crate::record::{self, Side};
use crate::{KernelError, Result, guard};

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rot(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Sweep a planar region's outer loop along a helix about `axis` through `axis_origin`:
/// `turns` revolutions (right-handed about `axis`), rising `pitch` per turn. The profile keeps
/// its attitude relative to the axis, as a screw motion does. The walls are B-spline surfaces
/// (one per profile segment); if those cannot be built, ruled walls between stations 3° apart
/// stand in.
pub fn sweep_helix(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("helix axis".into()))?;
    if !(pitch.is_finite() && turns.is_finite() && turns > 0.0 && turns <= 200.0 && pitch.abs() < 1e7 && axis_origin.is_finite()) {
        return Err(KernelError::Invalid("helix: turns must be in (0, 200] and the pitch finite".into()));
    }
    if region.area().abs() < 1e-9 {
        return Err(KernelError::Invalid("profile has no area".into()));
    }
    // The profile must clear itself after one turn. (Only the outer loop is swept.)
    let numbered = crate::numbered::number_regions(std::slice::from_ref(region)).pop().ok_or_else(|| KernelError::Invalid("profile".into()))?;
    let profile = numbered.outer.oriented(false);
    let lp = &profile.lp;
    let along: Vec<f64> =
        lp.segs.iter().flat_map(|s| (0..8).map(move |i| s.point_at(i as f64 / 8.0))).map(|q| (plane.to_world(q) - axis_origin).dot(k)).collect();
    let extent = along.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - along.iter().cloned().fold(f64::INFINITY, f64::min);
    if turns > 1.0 && pitch.abs() <= extent {
        return Err(KernelError::Invalid(format!("the coil runs into itself: the pitch must exceed the section ({extent:.3} mm)")));
    }
    if crate::backend()? == crate::Backend::Smlib {
        return crate::smlib_sweeps::screw(plane, &crate::smlib_sweeps::pieces([&profile]), axis_origin, k, pitch, turns);
    }
    // One smooth B-spline wall per profile segment; the ruled walls below are the fallback.
    if let Ok(b) = smooth_helix(plane, &profile, axis_origin, k, pitch, turns) {
        return Ok(b);
    }
    let n = ((turns * 120.0).ceil() as usize).clamp(4, 24_000);
    let total = std::f64::consts::TAU * turns;
    let mut wires = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let a = total * i as f64 / n as f64;
        let lift = k * (pitch * a / std::f64::consts::TAU);
        let o = axis_origin + rot(plane.origin - axis_origin, k, a) + lift;
        let pl = Plane { origin: o, x: rot(plane.x, k, a), y: rot(plane.y, k, a) };
        wires.push(uniform_wire(&pl, lp)?);
    }
    guard("helix sweep", || {
        // Every station's wire has the profile's segments, edge for edge.
        let mut faces: Vec<Side> = Vec::new();
        for w in wires.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let sides = builder::try_wire_homotopy(a, b).map_err(|e| KernelError::Failed(format!("helix: {e}")))?;
            faces.extend(record::ruled(&sides, a, b, &profile.from, &profile.from));
        }
        let (Some(first), Some(last)) = (wires.first(), wires.last()) else { return Err(KernelError::Failed("helix".into())) };
        cap_and_close(faces, std::slice::from_ref(first), std::slice::from_ref(last), false)
    })
}

/// A screw sweep of a convex polygon (in `plane`'s coordinates) as a closed triangle mesh,
/// wound outward: `stations` sections per turn, flat end caps. Watertight by construction, for
/// faceted booleans (modelled threads).
pub fn sweep_helix_mesh(plane: &Plane, polygon: &[Vec2], axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64, stations: usize) -> Result<Mesh> {
    let k = axis.normalized().ok_or_else(|| KernelError::Invalid("helix axis".into()))?;
    let m = polygon.len();
    if !(pitch.is_finite() && turns.is_finite() && turns > 0.0 && turns <= 200.0 && (3..=64).contains(&m) && (8..=720).contains(&stations)) {
        return Err(KernelError::Invalid("helix mesh: turns in (0, 200], 3…64 corners, 8…720 stations".into()));
    }
    let n = (turns * stations as f64).ceil() as usize;
    let total = std::f64::consts::TAU * turns;
    let mut mesh = Mesh::default();
    for i in 0..=n {
        let a = total * i as f64 / n as f64;
        for q in polygon {
            let w = plane.to_world(*q);
            mesh.positions.push(axis_origin + rot(w - axis_origin, k, a) + k * (pitch * a / std::f64::consts::TAU));
        }
    }
    let id = |i: usize, j: usize| u32::try_from(i * m + j % m).unwrap_or(0);
    for i in 0..n {
        for j in 0..m {
            mesh.triangles.push([id(i, j), id(i + 1, j), id(i + 1, j + 1)]);
            mesh.triangles.push([id(i, j), id(i + 1, j + 1), id(i, j + 1)]);
        }
    }
    for j in 1..m - 1 {
        mesh.triangles.push([id(0, 0), id(0, j), id(0, j + 1)]);
        mesh.triangles.push([id(n, 0), id(n, j + 1), id(n, j)]);
    }
    mesh.tri_face = vec![0; mesh.triangles.len()];
    if mesh.measure().volume < 0.0 {
        for t in &mut mesh.triangles {
            t.swap(1, 2);
        }
    }
    Ok(mesh)
}

/// Samples per profile segment and stations per turn of the smooth helical walls.
const SEG_SAMPLES: usize = 12;
const STATIONS_PER_TURN: f64 = 12.0;

/// Helical walls as B-spline surfaces: each profile segment, sampled at every station of the
/// screw motion, interpolated by cubics across the profile and then along the helix.
fn smooth_helix(plane: &Plane, profile: &NumberedLoop, o: Vec3, k: Vec3, pitch: f64, turns: f64) -> Result<Body> {
    let lp = &profile.lp;
    let nseg = lp.segs.len();
    if nseg == 0 || nseg > 64 {
        return Err(KernelError::Invalid("helix profile".into()));
    }
    let n = ((turns * STATIONS_PER_TURN).ceil() as usize).clamp(4, 4800);
    let total = std::f64::consts::TAU * turns;
    let at = |i: usize, q: solidcraft_geom::Vec2| {
        let a = total * i as f64 / n as f64;
        let w = plane.to_world(q);
        o + rot(w - o, k, a) + k * (pitch * a / std::f64::consts::TAU)
    };
    let fail = |w: &str| KernelError::Failed(format!("smooth helix: {w}"));
    // Per segment: the wall surface through its samples at every station.
    let mut faces_data = Vec::with_capacity(nseg);
    for s in &lp.segs {
        // u along the helix (the stations), v along the profile segment.
        let samples =
            Grid::from_fn([n + 1, SEG_SAMPLES], |i, j| at(i, s.point_at(j as f64 / (SEG_SAMPLES - 1) as f64))).ok_or_else(|| fail("samples"))?;
        faces_data.push(interpolate_net(&samples).ok_or_else(|| fail("wall surface"))?);
    }
    guard("smooth helix", || {
        let start: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(at(0, s.start())))).collect();
        let end: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(at(n, s.start())))).collect();
        let mut e0 = Vec::with_capacity(nseg);
        let mut e1 = Vec::with_capacity(nseg);
        let mut seams = Vec::with_capacity(nseg);
        for (j, NetFit { u_ends: [c0, c1], v_ends: [seam, _], .. }) in faces_data.iter().enumerate() {
            let (Some(a0), Some(b0), Some(a1), Some(b1)) = (start.get(j), start.get((j + 1) % nseg), end.get(j), end.get((j + 1) % nseg)) else {
                return Err(fail("vertices"));
            };
            e0.push(mt::Edge::try_new(a0, b0, mt::Curve::BSplineCurve(c0.clone())).map_err(|e| fail(&e.to_string()))?);
            e1.push(mt::Edge::try_new(a1, b1, mt::Curve::BSplineCurve(c1.clone())).map_err(|e| fail(&e.to_string()))?);
            seams.push(mt::Edge::try_new(a0, a1, mt::Curve::BSplineCurve(seam.clone())).map_err(|e| fail(&e.to_string()))?);
        }
        let mut walls = Vec::with_capacity(nseg);
        for (j, NetFit { surface: surf, .. }) in faces_data.iter().enumerate() {
            let (Some(a), Some(b), Some(sa), Some(sb)) = (e0.get(j), e1.get(j), seams.get(j), seams.get((j + 1) % nseg)) else {
                return Err(fail("edges"));
            };
            // Counter-clockwise in the surface's (u along the helix, v along the profile) space.
            let w: mt::Wire = vec![sa.clone(), b.clone(), sb.inverse(), a.inverse()].into();
            let wall = mt::Face::try_new(vec![w], mt::Surface::BSplineSurface(surf.clone())).map_err(|e| fail(&e.to_string()))?;
            walls.push((wall, Some(record::profile(profile.numbers(j)))));
        }
        let w0: mt::Wire = e0.iter().cloned().collect();
        let w1: mt::Wire = e1.iter().cloned().collect();
        // Either orientation of the walls; keep the one that measures as a proper solid.
        let expected = lp_area_path(plane, lp, o, k, turns);
        // A coarse mesh (within ~10%) is enough to tell a proper solid from an empty or inside-out one.
        let section_tol = (lp.signed_area().abs().sqrt() * 0.05).max(1e-3);
        for ws in [walls.clone(), walls.iter().map(|(f, prov)| (f.inverse(), prov.clone())).collect()] {
            if let Ok(b) = cap_and_close(ws, std::slice::from_ref(&w0), std::slice::from_ref(&w1), false)
                && b.tessellate(section_tol).is_ok_and(|m| (m.measure().volume - expected).abs() <= 0.15 * expected.abs())
            {
                return Ok(b);
            }
        }
        Err(fail("the walls do not close into the expected solid"))
    })
}

/// Pappus estimate of a screw sweep's volume: section area × the centroid's path around the axis.
fn lp_area_path(plane: &Plane, lp: &Loop2, o: Vec3, k: Vec3, turns: f64) -> f64 {
    let c = plane.to_world(lp.centroid());
    let d = c - o;
    let r = (d - k * d.dot(k)).len();
    lp.signed_area().abs() * std::f64::consts::TAU * r * turns
}

/// A solid from a planar loop of straight segments to an apex point off its plane (a pyramid).
pub fn loft_to_point(plane: &Plane, lp: &Loop2, apex: Vec3) -> Result<Body> {
    if !apex.is_finite() || lp.segs.is_empty() || lp.segs.len() > 10_000 {
        return Err(KernelError::Invalid("loft to a point: bad section or apex".into()));
    }
    if ((apex - plane.origin).dot(plane.normal())).abs() < 1e-9 {
        return Err(KernelError::Invalid("the apex lies in the section's plane".into()));
    }
    if lp.segs.iter().any(|s| !matches!(s, Seg2::Line { .. })) {
        return Err(KernelError::Invalid("not supported yet: lofting curved sections to a point".into()));
    }
    // Wind the base so its normal points away from the apex.
    let above = (apex - plane.origin).dot(plane.normal()) > 0.0;
    let base = crate::numbered::number_loops([lp]).pop().ok_or_else(|| KernelError::Invalid("loft to a point: bad section".into()))?.oriented(above);
    if crate::backend()? == crate::Backend::Smlib {
        return crate::smlib_sweeps::loft_to_point(plane, &crate::smlib_sweeps::pieces([&base]), apex);
    }
    let lp = &base.lp;
    guard("loft to point", || {
        let verts: Vec<mt::Vertex> = lp.segs.iter().map(|s| builder::vertex(p3(plane.to_world(s.start())))).collect();
        let top = builder::vertex(p3(apex));
        let m = verts.len();
        let edges: Vec<mt::Edge> = (0..m).filter_map(|i| Some(builder::line(verts.get(i)?, verts.get((i + 1) % m)?))).collect();
        let sides: Vec<mt::Edge> = verts.iter().map(|v| builder::line(v, &top)).collect();
        let mut faces = Vec::with_capacity(m + 1);
        // History as the faces are made: the base is the section (the start cap), each
        // triangle comes from its base segment.
        let mut history = Vec::with_capacity(m + 1);
        let bw: mt::Wire = edges.iter().cloned().collect();
        faces.push(builder::try_attach_plane(&[bw]).map_err(|e| KernelError::Failed(format!("loft base: {e}")))?);
        history.push(record::cap(false));
        for i in 0..m {
            let (Some(e), Some(up), Some(down)) = (edges.get(i), sides.get((i + 1) % m), sides.get(i)) else { continue };
            // Triangle i: along the base edge, up to the apex, back down — opposite to the base's winding.
            let w: mt::Wire = vec![e.inverse(), down.clone(), up.inverse()].into();
            faces.push(builder::try_attach_plane(&[w]).map_err(|e| KernelError::Failed(format!("loft side: {e}")))?);
            history.push(record::profile(base.numbers(i)));
        }
        let shell: mt::Shell = faces.into();
        let body = match Solid::try_new(vec![shell.clone()]) {
            Ok(s) => Body::new(s)?,
            Err(_) => {
                let inv: mt::Shell = shell.face_iter().map(|f| f.inverse()).collect();
                Body::new(Solid::try_new(vec![inv]).map_err(|e| KernelError::Failed(format!("loft to point: {e}")))?)?
            }
        };
        record::attach(body, &[], Some(history), Default::default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_geom::Vec2;

    #[test]
    fn coil_volume_is_section_times_helix_length() {
        // A 2 mm circle in a plane through the Z axis, 20 mm out, 3 turns, pitch 8: a screw
        // motion of an axial section sweeps π r² · 2πR · turns (spline helix: 4738.1).
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let region = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 2.0), holes: vec![] };
        let b = sweep_helix(&pl, &region, Vec3::ZERO, Vec3::Z, 8.0, 3.0).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        assert_eq!(b.face_count(), 4, "two smooth walls and two caps");
        let want = std::f64::consts::PI * 4.0 * std::f64::consts::TAU * 20.0 * 3.0;
        assert!((v - want).abs() / want < 1e-3, "{v} vs {want}");
        assert!(sweep_helix(&pl, &region, Vec3::ZERO, Vec3::Z, 3.0, 3.0).is_err(), "pitch below the section");
        assert!(sweep_helix(&pl, &region, Vec3::ZERO, Vec3::ZERO, 8.0, 3.0).is_err());
    }

    #[test]
    fn pyramid() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        let lp = Loop2::polygon(&[Vec2::new(-20.0, -20.0), Vec2::new(20.0, -20.0), Vec2::new(20.0, 20.0), Vec2::new(-20.0, 20.0)]);
        let b = loft_to_point(&pl, &lp, Vec3::new(0.0, 0.0, 30.0)).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        assert!((v - 1600.0 * 30.0 / 3.0).abs() < 1e-6, "{v}");
        assert_eq!(b.face_count(), 5);
        assert!(loft_to_point(&pl, &lp, Vec3::new(1.0, 1.0, 0.0)).is_err());
    }
}
