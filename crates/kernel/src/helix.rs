//! Helical sweeps (coils, springs) and lofts to a point (pyramids, cones).

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

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
/// groove fading in or out over a runout: SMLib's radial screw sweep. `continuous_flanks`
/// joins the spline walls across cylinder seams; false retains the historical faces.
pub fn sweep_helix_radial(
    plane: &Plane,
    region: &Region2,
    axis_origin: Vec3,
    axis: Vec3,
    pitch: f64,
    turns: f64,
    law: &[(f64, f64)],
    continuous_flanks: bool,
) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep_helix_radial");
    let (k, profile) = helix_profile(plane, region, axis_origin, axis, pitch, turns)?;
    if !law.windows(2).all(|w| matches!(w, [a, b] if b.0 > a.0))
        || law.iter().any(|(t, o)| !(t.is_finite() && o.is_finite() && *t >= 0.0 && *t <= turns))
    {
        return Err(KernelError::Invalid("helix: the radial law's turns must ascend within the sweep's turns".into()));
    }
    crate::smlib_sweeps::screw_radial(plane, &crate::smlib_sweeps::pieces([&profile]), axis_origin, k, pitch, turns, law, continuous_flanks)
}

/// The widest cone angle (from the axis) a coil may take: its tangent stays bounded.
const MAX_COIL_TAPER: f64 = 80.0 * std::f64::consts::PI / 180.0;

/// [`sweep_helix`] on a cone (#4735): the profile moves `|pitch| · tan(taper)` further from the
/// axis each turn, `taper` in radians from the axis (negative narrows the coil) within ±80°. A
/// zero taper is the plain sweep. The profile must stay clear of the axis to the last turn: a
/// section that the narrowing carries through it is refused, whatever profile it is.
pub fn sweep_helix_tapered(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64, turns: f64, taper: f64) -> Result<Body> {
    let _span = solidcraft_profile::span("kernel", "sweep_helix_tapered");
    if !(taper.is_finite() && taper.abs() <= MAX_COIL_TAPER) {
        return Err(KernelError::Invalid("a coil's taper must be between -80 and 80 degrees".into()));
    }
    if taper == 0.0 {
        return sweep_helix(plane, region, axis_origin, axis, pitch, turns);
    }
    let (k, profile) = helix_profile(plane, region, axis_origin, axis, pitch, turns)?;
    let growth = pitch.abs() * taper.tan();
    // How near the profile comes to the axis, sampled along its segments.
    let nearest = profile
        .lp
        .segs
        .iter()
        .flat_map(|s| (0..=8).map(move |i| s.point_at(i as f64 / 8.0)))
        .map(|q| {
            let v = plane.to_world(q) - axis_origin;
            (v - k * k.dot(v)).len()
        })
        .fold(f64::INFINITY, f64::min);
    if nearest + growth.min(0.0) * turns <= 0.0 {
        return Err(KernelError::Invalid("the taper takes the coil through its axis before its last turn".into()));
    }
    sweep_helix_radial(plane, region, axis_origin, axis, pitch, turns, &[(0.0, 0.0), (turns, growth * turns)], true)
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
    let radial = |q: Vec2| {
        let w = plane.to_world(q) - axis_origin;
        (w - k * w.dot(k), w.dot(k))
    };
    let e = lp.segs.first().and_then(|s| radial(s.point_at(0.0)).0.normalized()).unwrap_or_else(|| k.any_perp());
    // Each point's height where the screw motion carries it into the half-plane through the first:
    // a profile across the helix (not in a plane of the axis) reaches further along it there.
    let along: Vec<f64> = lp
        .segs
        .iter()
        .flat_map(|s| (0..8).map(move |i| s.point_at(i as f64 / 8.0)))
        .map(|q| {
            let (v, h) = radial(q);
            h - pitch * v.dot(k.cross(e)).atan2(v.dot(e)) / std::f64::consts::TAU
        })
        .collect();
    let extent = along.iter().cloned().fold(f64::NEG_INFINITY, f64::max) - along.iter().cloned().fold(f64::INFINITY, f64::min);
    if turns > 1.0 && pitch.abs() <= extent {
        // A section exactly one pitch wide makes neighbouring turns share a face, which a single solid cannot hold yet.
        if pitch.abs() > 0.0 && (pitch.abs() - extent).abs() <= 1e-6 * extent {
            // Enough decimals that the example stays below the pitch when rounded (0.04 mm gives 0.03960, not 0.040).
            let places = (3.0 - extent.log10().floor()).clamp(3.0, 12.0) as usize;
            return Err(KernelError::Invalid(format!(
                "the coil runs into itself: neighbouring turns touch because the pitch equals the section ({extent:.places$} mm); make the section slightly smaller than the pitch, for example {:.places$} mm",
                extent * 0.99
            )));
        }
        return Err(KernelError::Invalid(format!(
            "the coil runs into itself: the pitch must exceed the section's extent along the axis ({extent:.3} mm)"
        )));
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
    // Wind the base so its normal points away from the apex.
    let above = (apex - plane.origin).dot(plane.normal()) > 0.0;
    let base = crate::numbered::number_loops([lp]).pop().ok_or_else(|| KernelError::Invalid("loft to a point: bad section".into()))?.oriented(above);
    crate::smlib_sweeps::loft_to_point(plane, &crate::smlib_sweeps::pieces([&base]), apex)
}

/// [`loft_to_point`] following one or two guide `rails` (polylines from the loop to the apex),
/// `fit` saying how they reshape the section on the way (#20985). Without rails, the plain loft to
/// a point.
pub fn loft_to_point_with_rails(plane: &Plane, lp: &Loop2, apex: Vec3, rails: &[Vec<Vec3>], fit: crate::RailFit) -> Result<Body> {
    if rails.is_empty() {
        return loft_to_point(plane, lp, apex);
    }
    let _span = solidcraft_profile::span("kernel", "loft_to_point_with_rails");
    if !apex.is_finite() || lp.segs.is_empty() || lp.segs.len() > 10_000 {
        return Err(KernelError::Invalid("loft to a point: bad section or apex".into()));
    }
    if ((apex - plane.origin).dot(plane.normal())).abs() < 1e-9 {
        return Err(KernelError::Invalid("the apex lies in the section's plane".into()));
    }
    let above = (apex - plane.origin).dot(plane.normal()) > 0.0;
    let base = crate::numbered::number_loops([lp]).pop().ok_or_else(|| KernelError::Invalid("loft to a point: bad section".into()))?.oriented(above);
    crate::smlib_sweeps::loft_to_point_with_rails(plane, &crate::smlib_sweeps::pieces([&base]), apex, rails, fit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Seg2;

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

    /// solidcraft#4735: a coil whose radius grows at a constant rate per turn. Pappus with the
    /// section's area 4π and its centre at 20 + 0.7·t mm, t = 0…3 turns (8 mm pitch, 5 degrees):
    /// the volumes are fixed numbers worked out by hand, 4986.09 and 4488.73 mm³; a zero taper is
    /// the plain coil's 4738.1.
    #[test]
    fn a_tapered_coil_follows_a_cone() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let region = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 2.0), holes: vec![] };
        for (deg, want) in [(5.0_f64, 4986.09), (-5.0, 4488.73), (0.0, 4738.1)] {
            let b = sweep_helix_tapered(&pl, &region, Vec3::ZERO, Vec3::Z, 8.0, 3.0, deg.to_radians()).unwrap();
            let v = crate::measure(&b).unwrap().volume;
            assert!((v - want).abs() / want < 1e-3, "{deg} deg: {v} vs {want}");
        }
    }

    /// A taper outside ±80 degrees, a non-finite one, and a narrowing that carries the section
    /// through the axis are refused, with a profile of any kind.
    #[test]
    fn a_taper_out_of_range_or_through_the_axis_is_refused() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        // A circle of radius 1 at 20 mm: 45 degrees closes in by 8 mm a turn, 24 mm over 3 turns.
        let circle = Region2 { outer: Loop2::circle(Vec2::new(20.0, 0.0), 1.0), holes: vec![] };
        let square = Region2 {
            outer: Loop2::polygon(&[Vec2::new(19.0, -1.0), Vec2::new(21.0, -1.0), Vec2::new(21.0, 1.0), Vec2::new(19.0, 1.0)]),
            holes: vec![],
        };
        for region in [&circle, &square] {
            let narrowing = (-45.0_f64).to_radians();
            let e = sweep_helix_tapered(&pl, region, Vec3::ZERO, Vec3::Z, 8.0, 3.0, narrowing).unwrap_err().to_string();
            assert!(e.contains("axis"), "{e}");
            for bad in [85.0_f64.to_radians(), -85.0_f64.to_radians(), f64::NAN, f64::INFINITY, 1e308] {
                let e = sweep_helix_tapered(&pl, region, Vec3::ZERO, Vec3::Z, 8.0, 3.0, bad).unwrap_err().to_string();
                assert!(e.contains("80"), "{bad}: {e}");
            }
            // Widening by the same angle is fine, and so is a narrowing that stays clear.
            assert!(sweep_helix_tapered(&pl, region, Vec3::ZERO, Vec3::Z, 8.0, 3.0, 45.0_f64.to_radians()).is_ok());
            assert!(sweep_helix_tapered(&pl, region, Vec3::ZERO, Vec3::Z, 8.0, 3.0, (-5.0_f64).to_radians()).is_ok());
        }
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

    /// solidcraft#11216: a curved section lofts to a point. The circle gives the cone,
    /// pi r^2 h / 3, as one wall (a seam) over the disk.
    #[test]
    fn cone_from_a_circle_to_a_point() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        let lp = Loop2::circle(Vec2::ZERO, 20.0);
        let b = loft_to_point(&pl, &lp, Vec3::new(0.0, 0.0, 30.0)).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        let want = std::f64::consts::PI * 400.0 * 10.0;
        assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
        assert!(b.face_count() <= 3, "faces {}", b.face_count());
    }

    /// A rounded square (half side 10, corners r 5) lofts to a point: four triangles and
    /// four cone pieces over the base, the pyramid's volume area x h / 3.
    #[test]
    fn a_rounded_square_lofts_to_a_point() {
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Y };
        let (a, r) = (10.0, 5.0);
        use std::f64::consts::FRAC_PI_2;
        let c = a - r;
        let lp = Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(-c, -a), b: Vec2::new(c, -a) },
                Seg2::Arc { center: Vec2::new(c, -c), radius: r, start: -FRAC_PI_2, sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(a, -c), b: Vec2::new(a, c) },
                Seg2::Arc { center: Vec2::new(c, c), radius: r, start: 0.0, sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(c, a), b: Vec2::new(-c, a) },
                Seg2::Arc { center: Vec2::new(-c, c), radius: r, start: FRAC_PI_2, sweep: FRAC_PI_2 },
                Seg2::Line { a: Vec2::new(-a, c), b: Vec2::new(-a, -c) },
                Seg2::Arc { center: Vec2::new(-c, -c), radius: r, start: 2.0 * FRAC_PI_2, sweep: FRAC_PI_2 },
            ],
        };
        let b = loft_to_point(&pl, &lp, Vec3::new(0.0, 0.0, 30.0)).unwrap();
        let v = crate::measure(&b).unwrap().volume;
        let area = 400.0 - (4.0 - std::f64::consts::PI) * r * r;
        let want = area * 30.0 / 3.0;
        assert!((v - want).abs() < 1e-6 * want, "{v} vs {want}");
        assert_eq!(b.face_count(), 9, "base, four triangles, four cone pieces");
    }
}
