//! Helical sweeps (coils, springs) and lofts to a point (pyramids, cones).

use solidcraft_geom::{Loop2, Mesh, Plane, Region2, Seg2, Vec2, Vec3};

use crate::body::Body;
use crate::{KernelError, Result};

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rot(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Sweep a planar region's outer loop along a helix about `axis` through `axis_origin`:
/// `turns` revolutions (right-handed about `axis`), rising `pitch` per turn. The profile keeps
/// its attitude relative to the axis, as a screw motion does: SMLib's screw sweep, one wall per
/// profile segment.
pub fn sweep_helix(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep_helix");
    let (k, profile) = helix_profile(plane, region, axis_origin, axis, pitch, turns)?;
    crate::smlib_sweeps::screw(plane, &crate::smlib_sweeps::pieces([&profile]), axis_origin, k, pitch, turns)
}

/// [`sweep_helix`] with the profile moved off the axis as it turns: at turn t it first moves
/// `offset(t)` mm away from the axis (negative: towards it) in its half-plane, `offset` piecewise
/// linear through `law`'s (turn, offset) pairs, turns ascending within [0, `turns`]. A thread's
/// groove fading in or out over a runout: SMLib's radial screw sweep.
pub fn sweep_helix_radial(
    plane: &Plane,
    region: &Region2,
    axis_origin: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep_helix_radial");
    if law.is_empty() {
        return sweep_helix(plane, region, axis_origin, axis, pitch, turns);
    }
    let (k, profile) = helix_profile(plane, region, axis_origin, axis, pitch, turns)?;
    if !law.windows(2).all(|w| matches!(w, [a, b] if b.0 > a.0))
        || law.iter().any(|(t, o)| !(t.is_finite() && o.is_finite() && *t >= 0.0 && *t <= turns))
    {
        return Err(KernelError::Invalid("helix: the radial law's turns must ascend within the sweep's turns".into()));
    }
    crate::smlib_sweeps::screw_radial(plane, &crate::smlib_sweeps::pieces([&profile]), axis_origin, k, pitch, turns, law)
}

/// A helical sweep's axis and profile (the region's outer loop), checked: finite turns and
/// pitch, an area, and a profile that clears itself after one turn.
fn helix_profile(
    plane: &Plane,
    region: &Region2,
    axis_origin: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
) -> Result<(Vec3, crate::numbered::NumberedLoop)> {
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
    Ok((k, profile))
}

/// A screw sweep of a convex polygon (in `plane`'s coordinates) as a closed triangle mesh,
/// wound outward: `stations` sections per turn, flat end caps. Watertight by construction, for
/// faceted booleans (modelled threads).
pub fn sweep_helix_mesh(plane: &Plane, polygon: &[Vec2], axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64, stations: usize) -> Result<Mesh> {
    let _span = solidcraft_profile::span("kernel", "sweep_helix_mesh");
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

/// A solid from a planar loop of straight segments to an apex point off its plane (a pyramid):
/// SMLib's loft to a point.
pub fn loft_to_point(plane: &Plane, lp: &Loop2, apex: Vec3) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "loft_to_point");
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
    crate::smlib_sweeps::loft_to_point(plane, &crate::smlib_sweeps::pieces([&base]), apex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coil_volume_is_section_times_helix_length() {
        // A 2 mm circle in a plane through the Z axis, 20 mm out, 3 turns, pitch 8: a screw
        // motion of an axial section sweeps π r² · 2πR · turns (spline helix: 4738.1).
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let region = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 2.0), holes: vec![] };
        let b = sweep_helix(&pl, &region, Vec3::ZERO, Vec3::Z, 8.0, 3.0).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        // SMLib sweeps the circle (one segment) as one smooth wall.
        assert_eq!(b.face_count(), 3, "one smooth wall and two caps");
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
