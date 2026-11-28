//! Helical sweeps (coils, springs) and lofts to a point (pyramids, cones).

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::Mesh;
use solidcraft_geom::prelude::*;

use crate::body::Body;
use crate::{KernelError, Result};

/// The most turns a helical sweep takes: the kernel's, as many as its spans hold.
pub const MAX_HELIX_TURNS: f64 = solidcore_ops::sweep::SCREW_MAX_TURNS;

/// Rotate `v` about the unit axis `k` by `ang` (Rodrigues).
fn rot(v: Vec3, k: Vec3, ang: f64) -> Vec3 {
    v * ang.cos() + k.cross(v) * ang.sin() + k * (k.dot(v) * (1.0 - ang.cos()))
}

/// Whether a screw motion sweeps `region` on `plane` without folding over itself: about `axis`
/// through `axis_origin`, rising `pitch` per turn. Per radian of turn a point `r` from the axis
/// origin moves `k × r + (pitch / 2π) k`, so its speed across the plane, along the plane's unit
/// normal `n`, is `n · (k × r) + (pitch / 2π) (n · k)`: affine over the plane. While it keeps one
/// sign over the whole profile, every point of the profile leaves the plane on the same side and
/// the sweep is a solid (a plane across the axis: a twisted extrusion, each slice square to the axis
/// the profile turned, volume area × height; a plane parallel to the axis off it: volume
/// 2π · turns · area · the centroid's distance from the line in the plane nearest the axis).
/// Where it changes sign or reaches zero inside the profile, the profile sweeps back over
/// itself. A plane through the axis (the incidence test a revolve axis has in its sketch plane,
/// measured by solidcore's `line_plane_deviation`) is not tested here: there the speed is the
/// distance from the axis, and the section's own checks keep it clear of the axis.
///
/// # Errors
/// [`KernelError::Invalid`] for a profile that would sweep over itself, or a plane or axis that
/// is not finite. The speeds are solidcore's `screw_speed_range`.
pub fn require_unfolded_screw_section(plane: &Plane, region: &Region2, axis_origin: Vec3, axis: Vec3, pitch: f64) -> Result<()> {
    let arr = |v: Vec3| [v.x, v.y, v.z];
    let (tilt, offset) =
        solidcore_ops::axial_section::line_plane_deviation(arr(plane.origin), arr(plane.x), arr(plane.y), arr(axis_origin), arr(axis))
            .ok_or_else(|| KernelError::Invalid("the coil's profile plane or axis".into()))?;
    if tilt <= 1e-9 && offset <= 1e-6 {
        return Ok(());
    }
    let points: Vec<[f64; 3]> =
        region.outer.segs.iter().flat_map(|s| (0..32).map(move |i| s.point_at(i as f64 / 32.0))).map(|q| arr(plane.to_world(q))).collect();
    let (lo, hi) = solidcore_ops::axial_section::screw_speed_range(arr(plane.x), arr(plane.y), arr(axis_origin), arr(axis), pitch, &points)
        .ok_or_else(|| KernelError::Invalid("the coil's profile plane or axis".into()))?;
    let scale = hi.abs().max(lo.abs()).max(f64::MIN_POSITIVE);
    if !(lo.is_finite() && hi.is_finite()) || !(lo > 1e-9 * scale || hi < -1e-9 * scale) {
        return Err(KernelError::Invalid(
            "the coil's profile would sweep over itself: as the coil turns, part of it moves off its plane on one side and part on the other (it reaches the line in its plane where the motion runs along the plane); keep the profile to one side of that line, or sketch it on a plane through the axis"
                .into(),
        ));
    }
    Ok(())
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
    if !(pitch.is_finite() && turns.is_finite() && turns > 0.0 && turns <= MAX_HELIX_TURNS && pitch.abs() < 1e7 && axis_origin.is_finite()) {
        return Err(KernelError::Invalid(format!("helix: turns must be in (0, {MAX_HELIX_TURNS}] and the pitch finite")));
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
    crate::smlib_sweeps::loft_to_point(plane, &loft_to_point_plan(plane, lp, apex)?, apex)
}

/// [`loft_to_point`]'s section, checked and wound so its normal points away from the apex, as
/// the pieces it lofts: what another kernel lofts, and what names its faces after.
pub(crate) fn loft_to_point_plan(plane: &Plane, lp: &Loop2, apex: Vec3) -> Result<Vec<crate::smlib_sweeps::Piece>> {
    if !apex.is_finite() || lp.segs.is_empty() || lp.segs.len() > 10_000 {
        return Err(KernelError::Invalid("loft to a point: bad section or apex".into()));
    }
    if ((apex - plane.origin).dot(plane.normal())).abs() < 1e-9 {
        return Err(KernelError::Invalid("the apex lies in the section's plane".into()));
    }
    // Wind the base so its normal points away from the apex.
    let above = (apex - plane.origin).dot(plane.normal()) > 0.0;
    let base = crate::numbered::number_loops([lp]).pop().ok_or_else(|| KernelError::Invalid("loft to a point: bad section".into()))?.oriented(above);
    Ok(crate::smlib_sweeps::pieces([&base]))
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

    /// A plane across the axis (a circle on XY, axis Z): the speed across the plane is the rise
    /// alone, pitch / 2π, one sign everywhere, so it sweeps (a twisted extrusion). The axial plane
    /// moved 5 mm off the axis, with the circle 20 mm along it: the speed is the distance along the
    /// plane from the line nearest the axis, 18..22, one sign. The same plane with the circle
    /// centred on that line spans −2..2 and folds; so does a plane tilted 45° whose speed
    /// passes zero inside the profile.
    #[test]
    fn a_screw_section_is_refused_only_where_it_folds() {
        let circle = |x: f64| Region2 { outer: Loop2::circle(Vec2::new(x, 0.0), 2.0), holes: vec![] };
        assert!(require_unfolded_screw_section(&Plane::XY, &circle(20.0), Vec3::ZERO, Vec3::Z, 8.0).is_ok());
        let off = Plane { origin: Vec3::new(0.0, 5.0, 0.0), x: Vec3::X, y: Vec3::Z };
        assert!(require_unfolded_screw_section(&off, &circle(20.0), Vec3::ZERO, Vec3::Z, 8.0).is_ok());
        assert!(require_unfolded_screw_section(&off, &circle(-20.0), Vec3::ZERO, Vec3::Z, 8.0).is_ok());
        let err = require_unfolded_screw_section(&off, &circle(0.0), Vec3::ZERO, Vec3::Z, 8.0).unwrap_err().to_string();
        assert!(err.contains("would sweep over itself"), "{err}");
        let axial = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        assert!(require_unfolded_screw_section(&axial, &circle(20.0), Vec3::ZERO, Vec3::Z, 8.0).is_ok());
        // Tilted 45° about X through the origin: n = (0, −s, s), s = 1/√2; a point (x, y') on the
        // plane is (x, s y', s y'); n·(k×r) = −s·x, the rise s·8/2π ≈ 0.90. Zero at x ≈ 1.27, inside
        // a circle of radius 2 at x = 0.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let tilted = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::new(0.0, s, s) };
        let err = require_unfolded_screw_section(&tilted, &circle(0.0), Vec3::ZERO, Vec3::Z, 8.0).unwrap_err().to_string();
        assert!(err.contains("would sweep over itself"), "{err}");
    }

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

    /// #22577: the issue's thread along a whole rod, a V 1 wide at radius 8.5 with its point at
    /// 7.2, on the XZ plane about the X axis from x = -1, 202 turns of pitch 1.25 (the bound was
    /// 200). Its top line runs along the axis, so its three walls come in 404 half turns, between
    /// two caps. Pappus gives the volume, off by at most the tolerance (a ten-thousandth of the
    /// pitch) times the walls' area, each wall at most its line's length times the helix at its
    /// outer end.
    #[test]
    fn a_coil_of_more_than_200_turns_builds() {
        use std::f64::consts::TAU;
        let pl = Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z };
        let v = Loop2::polygon(&[Vec2::new(-1.5, 8.5), Vec2::new(-0.5, 8.5), Vec2::new(-1.0, 7.2)]);
        let region = Region2 { outer: v, holes: vec![] };
        let (pitch, turns) = (1.25, 202.0);
        let b = sweep_helix(&pl, &region, Vec3::new(-1.0, 0.0, 0.0), Vec3::X, pitch, turns).unwrap();
        assert_eq!(b.face_count(), 3 * 404 + 2, "three walls in 404 half turns, two caps");
        let want = 0.5 * 1.3 * TAU * (7.2 + 8.5 + 8.5) / 3.0 * turns;
        let bound = pitch * 1e-4 * (1.0 + 2.0 * 0.5f64.hypot(1.3)) * (TAU * 8.5).hypot(pitch) * turns;
        let got = crate::measure(&b).unwrap().volume;
        assert!((got - want).abs() <= bound, "{got} vs {want} within {bound}");
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
