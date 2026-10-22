//! Chord-width and radius-law blends along curved edges: the width across the blend at every
//! station, and the radius along the edge, measured on the body the kernel built against values
//! worked out from the input geometry alone (the two cylinders' axes, the arc's axis).

use mt::{BoundedCurve, ParametricCurve, SearchNearestParameter};
use solidcraft_geom::Vec3;

use super::*;
use crate::blend::vtx;
use crate::blend_variable::{RadiusSpline, fillet_chord, fillet_variable, fillet_variable_points, fillet_variable_spline};
use crate::{Body, BoolOp, boolean, box_solid, cylinder, fillet, measure, step_export};

/// An oblique boss: a rod of radius 8 whose axis leans 26.57° from the plate's normal, standing on
/// a 5 mm plate. The edge where its side meets the plate's top plane is an ellipse whose two faces
/// run from 63° to 117° apart around it.
fn oblique_boss() -> (Body, Vec3, Vec3) {
    let plate = box_solid(Vec3::new(-40.0, -40.0, 0.0), Vec3::new(40.0, 40.0, 5.0)).unwrap();
    let base = Vec3::new(0.0, 0.0, -10.0);
    let axis = Vec3::new(0.5, 0.0, 1.0).normalized().unwrap();
    let rod = cylinder(base, axis, 8.0, 30.0).unwrap();
    (boolean(&plate, &rod, BoolOp::Union).unwrap().unwrap(), base, axis)
}

/// A 40 × 40 × 20 box whose front wall is an arc of radius 40 about a Z axis: `concave` cuts a
/// cylinder away from the front (the wall curves into the part), otherwise the part is the box
/// within the cylinder (the wall curves out). With the bottom arc's midpoint, the axis's base
/// point and the side of the wall the material is on (+1 away from the axis, −1 toward it).
fn arc_walled_box(concave: bool) -> (Body, Vec3, Vec3, f64) {
    let block = box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
    let (axis, op, side) =
        if concave { (Vec3::new(20.0, -30.0, 0.0), BoolOp::Cut, 1.0) } else { (Vec3::new(20.0, 70.0, 0.0), BoolOp::Intersect, -1.0) };
    let tool = cylinder(axis, Vec3::Z, 40.0, 20.0).unwrap();
    let body = boolean(&block, &tool, op).unwrap().unwrap();
    (body, axis + Vec3::new(0.0, side * 40.0, 0.0), axis, side)
}

/// The blend face a blend built over `input`: the fitted surface that is on neither of the faces
/// it runs between (a boolean leaves the curved faces it kept as fitted surfaces themselves).
fn blend_face(input: &Body, body: &Body) -> Option<mt::Face> {
    use mt::ParametricSurface;
    let on_input = |p: Vec3| input.solid.face_iter().any(|f| project(f, p, None).is_some_and(|(q, _, _)| q.dist(p) <= 1e-6));
    body.solid
        .face_iter()
        .filter(|f| matches!(f.oriented_surface(), mt::Surface::NurbsSurface(_)))
        .find(|f| !on_input(from_p3(f.surface().subs(0.5, 0.5))))
        .cloned()
}

/// The edges of the blend face's boundary, each once, with the point at the middle of it (the
/// blend is bounded by its two contacts and, at each end, an arc or the curve of an end face).
fn blend_boundary(input: &Body, body: &Body) -> Vec<(mt::Edge, Vec3)> {
    let Some(f) = blend_face(input, body) else { return Vec::new() };
    let mut out: Vec<(mt::Edge, Vec3)> = Vec::new();
    for w in f.absolute_boundaries() {
        for e in w.edge_iter() {
            if out.iter().any(|(x, _)| x.id() == e.id()) {
                continue;
            }
            let c = e.oriented_curve();
            let (lo, hi) = c.range_tuple();
            out.push((e.absolute_clone(), from_p3(c.subs((lo + hi) * 0.5))));
        }
    }
    out
}

/// The edges of a contact: the boundary of the blend face whose middle point `side` picks out.
fn contact(input: &Body, body: &Body, side: &dyn Fn(Vec3) -> bool) -> Vec<mt::Edge> {
    blend_boundary(input, body).into_iter().filter(|(_, m)| side(*m)).map(|(e, _)| e).collect()
}

/// The fraction of the way along `edge` (its absolute front to its back) of the point of it
/// nearest `q`.
fn along_fraction(edge: &mt::Edge, q: Vec3) -> Option<f64> {
    let e = edge.absolute_clone();
    let c = e.oriented_curve();
    let (lo, hi) = c.range_tuple();
    let steps = 512;
    let mut at = vec![from_p3(c.subs(lo))];
    let mut run = vec![0.0];
    for i in 1..=steps {
        let p = from_p3(c.subs(lo + (hi - lo) * i as f64 / steps as f64));
        run.push(run.last().copied()? + at.last().copied()?.dist(p));
        at.push(p);
    }
    let total = *run.last()?;
    let d = |i: usize| at.get(i).map(|p| p.dist(q)).unwrap_or(f64::INFINITY);
    let hint = (0..=steps).min_by(|x, y| d(*x).total_cmp(&d(*y))).unwrap_or(0);
    let t = c.search_nearest_parameter(p3(q), lo + (hi - lo) * hint as f64 / steps as f64, 100)?;
    let i = (((t - lo) / (hi - lo)) * steps as f64).clamp(0.0, steps as f64);
    let (k, f) = (i.floor() as usize, i.fract());
    let (Some(a), Some(b)) = (run.get(k), run.get((k + 1).min(steps))) else { return None };
    Some((a + (b - a) * f) / total)
}

/// The azimuth of `p` about the Z axis through `axis`.
fn azimuth(axis: Vec3, p: Vec3) -> f64 {
    (p.y - axis.y).atan2(p.x - axis.x)
}

/// The point of the curves running round the Z axis through `axis` at the azimuth `phi`.
fn at_azimuth(edges: &[mt::Edge], axis: Vec3, phi: f64) -> Option<Vec3> {
    let off = |p: Vec3| {
        let mut d = azimuth(axis, p) - phi;
        while d > std::f64::consts::PI {
            d -= std::f64::consts::TAU;
        }
        while d < -std::f64::consts::PI {
            d += std::f64::consts::TAU;
        }
        d
    };
    let (mut best, mut best_d) = (Vec3::ZERO, f64::INFINITY);
    for e in edges {
        let c = e.oriented_curve();
        let (lo, hi) = c.range_tuple();
        let steps = 512;
        let mut at = lo;
        let mut dist = f64::INFINITY;
        for i in 0..=steps {
            let t = lo + (hi - lo) * i as f64 / steps as f64;
            let d = off(from_p3(c.subs(t))).abs();
            if d < dist {
                (at, dist) = (t, d);
            }
        }
        let step = (hi - lo) / steps as f64;
        let (mut x0, mut x1) = ((at - step).max(lo), (at + step).min(hi));
        let g = 0.618_033_988_749_894_9;
        for _ in 0..80 {
            let (a, b) = (x1 - g * (x1 - x0), x0 + g * (x1 - x0));
            if off(from_p3(c.subs(a))).abs() < off(from_p3(c.subs(b))).abs() {
                x1 = b;
            } else {
                x0 = a;
            }
        }
        let t = 0.5 * (x0 + x1);
        let d = off(from_p3(c.subs(t))).abs();
        if d < best_d {
            (best, best_d) = (from_p3(c.subs(t)), d);
        }
    }
    (best_d < 1e-6).then_some(best)
}

/// A chord fillet along the cylinder-into-plane edge of an oblique boss — the two faces run from
/// 63° to 117° apart around it. The blend's chain stops at the seams between the patches a boolean
/// left of the rod's surface, where the end face is tangent to the rod; the cap that cuts the strip
/// back there is only placed for one radius, so a width that varies along the edge is refused
/// rather than approximated (the fixed radius of `fillet` along the same edge rolls).
#[test]
fn a_chord_fillet_along_an_oblique_boss_edge_is_refused() {
    let (body, _, _) = oblique_boss();
    let pick = Vec3::new(7.5 + 8.944_271_909_999_16, 0.0, 5.0);
    // A fixed radius rolls along the same chain: the edge itself is supported.
    assert!(fillet(&body, &[pick], 2.5).is_ok());
    let e = fillet_chord(&body, &[pick], 4.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
}

/// A chord fillet along an arc whose two faces meet at a right angle: the width across the blend
/// is the chord, so the round is the one of radius `chord / √2` (checked against the constant
/// fillet of that radius) and its two contacts are that far apart at every station.
#[test]
fn chord_fillet_along_an_arc_edge() {
    let (chord, r) = (3.0, 3.0 / 2f64.sqrt());
    let (body, mid, axis, _) = arc_walled_box(false);
    let blended = fillet_chord(&body, &[mid], chord).unwrap();
    assert!(blended.validity().is_empty(), "{:?}", blended.validity());
    assert_eq!(blended.unmeshed_faces(), 0);
    let plain = fillet(&body, &[mid], r).unwrap();
    let (made, exact) = (measure(&blended).unwrap().volume, measure(&plain).unwrap().volume);
    assert!((made - exact).abs() < exact * 1e-6, "volume {made} vs {exact}");
    // The contact on the plane and the one on the wall at the same azimuth: the width across.
    let plane = contact(&body, &blended, &|m| m.z.abs() < 1e-6);
    let wall = contact(&body, &blended, &|m| m.z.abs() >= 1e-6);
    assert!(!plane.is_empty() && !wall.is_empty(), "the contacts are not the plane's and the wall's");
    let mid_azimuth = azimuth(axis, mid);
    let mut worst: f64 = 0.0f64;
    let mut stations = 0;
    for i in 1..12 {
        let phi = mid_azimuth + (i as f64 / 12.0 - 0.5) * 0.9;
        let (Some(p), Some(w)) = (at_azimuth(&plane, axis, phi), at_azimuth(&wall, axis, phi)) else { continue };
        stations += 1;
        worst = worst.max((p.dist(w) - chord).abs());
    }
    eprintln!("arc chord {chord}: worst width error {worst:.6} mm at {stations} stations");
    assert!(stations >= 9, "only {stations} stations measured");
    assert!(worst < 0.01, "the width across the blend is {worst} mm off the chord {chord}");
}

/// A chord fillet on a straight edge between planar faces is still the exact local round: the
/// same body, bit for bit, as the fillet of the radius the chord gives (a right angle: chord c is
/// r = c / √2).
#[test]
fn chord_fillet_on_a_straight_edge_is_unchanged() {
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 20.0, 30.0)).unwrap();
    let (at, chord) = (Vec3::new(10.0, 10.0, 30.0), 4.0);
    let f = fillet_chord(&b, &[at], chord).unwrap();
    let k = fillet(&b, &[at], chord / 2f64.sqrt()).unwrap();
    assert_eq!(step_export(&[&f], "part").unwrap(), step_export(&[&k], "part").unwrap());
    assert_eq!(f.unmeshed_faces(), 0);
}

/// A variable radius along an arc edge: a ball whose radius is the law's at each station's fraction
/// of the edge's length rolls along it, and the setback on the planar face — the plane and the
/// wall meet at a right angle, so it is the section's radius — follows the law from end to end.
#[test]
fn variable_radius_along_an_arc_edge_follows_the_law() {
    let (body, mid, axis, _) = arc_walled_box(false);
    let edge = crate::surfaces::edge_near(&body, mid).unwrap();
    // Started from the edge's back end: the law runs the other way, as the positions are from the
    // end nearest `start`.
    let start = vtx(edge.absolute_back());
    // A wider law (2 → 4, the radius doubling over the arc) is out of reach of the fitted strip:
    // the sections' own last spans cannot be held to the fit's accuracy there. That is refused,
    // not approximated.
    for wide in [4.0, 5.0] {
        let e = fillet_variable(&body, mid, start, 2.0, wide).unwrap_err();
        assert!(e.to_string().contains("accuracy and section limits"), "2 -> {wide}: {e}");
    }
    let (r0, r1) = (2.0, 3.4);
    let blended = fillet_variable(&body, mid, start, r0, r1).unwrap();
    assert!(blended.validity().is_empty(), "{:?}", blended.validity());
    assert_eq!(blended.unmeshed_faces(), 0);
    let law = RadiusSpline::through(&[(0.0, r0), (1.0, r1)], false);
    // The law runs from the end nearest `start`: the chain's front here is the far end.
    let want = if vtx(edge.absolute_back()).dist(start) < vtx(edge.absolute_front()).dist(start) { law.reversed() } else { law.clone() };
    // `start` is the chain's back end, so the law is read back to front there: r0 at the back, r1
    // at the front.
    assert!((want.eval(1.0) - r0).abs() < 1e-12 && (want.eval(0.0) - r1).abs() < 1e-12);
    // The blend's contact on the bottom plane is at 40 − r from the axis.
    let plane = contact(&body, &blended, &|m| m.z.abs() < 1e-6);
    assert!(!plane.is_empty(), "no contact on the bottom plane");
    let mut worst: f64 = 0.0;
    let mut stations = 0;
    let mut shown = Vec::new();
    for e in &plane {
        let c = e.oriented_curve();
        let (lo, hi) = c.range_tuple();
        for i in 0..=32 {
            let p = from_p3(c.subs(lo + (hi - lo) * i as f64 / 32.0));
            let measured = 40.0 - (p - axis).len();
            let along = along_fraction(&edge, p).unwrap();
            // Leave the two ends, where the caps cut the contact: the blend closes on the end
            // faces there.
            if !(0.04..=0.96).contains(&along) {
                continue;
            }
            stations += 1;
            if shown.len() < 3 || (0.45..0.55).contains(&along) {
                shown.push((along, measured, want.eval(along)));
            }
            worst = worst.max((measured - want.eval(along)).abs());
        }
    }
    eprintln!("arc variable {r0} -> {r1}: worst radius error {worst:.6} mm at {stations} stations; law vs measured {shown:?}");
    assert!(stations >= 24, "only {stations} stations measured");
    assert!(worst < 0.02, "the section radius is {worst} mm off the law");
    // Wider, still inside what the strip holds: the same body as the exact straight-edge case of
    // that law is not built here, but the radius is the law's at every station measured.
}

/// A radius law that runs to zero at an end of a curved edge: refused by name. The blend would
/// close to the edge's end point, but the fitted strip pinches to a point there and the last span
/// of it cannot be held to the fit's accuracy: the straight-edge fillet, which closes exactly, is
/// the way (see `variable_radius_down_to_zero_at_one_end`).
#[test]
fn a_law_running_to_zero_along_a_curved_edge_is_refused() {
    let (body, mid, _, _) = arc_walled_box(false);
    let edge = crate::surfaces::edge_near(&body, mid).unwrap();
    for start in [vtx(edge.absolute_front()), vtx(edge.absolute_back())] {
        let e = fillet_variable(&body, mid, start, 3.0, 0.0).unwrap_err();
        assert!(e.to_string().contains("runs to zero along a curved edge"), "{e}");
        let law = RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], radii: vec![0.0, 3.0] };
        assert!(fillet_variable_spline(&body, mid, mid, &law).is_err());
        // A law that stays clear of zero along it rolls: the blend is closed, a three-sided face
        // where nothing closes it.
        let made = fillet_variable(&body, mid, start, 3.0, 2.4).unwrap();
        assert!(made.validity().is_empty(), "{:?}", made.validity());
        let mesh = made.mesh(&crate::MeshTolerance::chord(5e-4)).unwrap();
        let about = |o: Vec3| {
            mesh.triangles
                .iter()
                .map(|t| (mesh.positions[t[0] as usize] - o).dot((mesh.positions[t[1] as usize] - o).cross(mesh.positions[t[2] as usize] - o)) / 6.0)
                .sum::<f64>()
        };
        assert!((about(Vec3::ZERO) - about(Vec3::new(-100.0, 50.0, 30.0))).abs() < 1e-2, "the mesh is not closed");
    }
}

/// What a chord or a law the local geometry cannot hold is refused with: an error naming the size,
/// never a silent approximation and never a panic.
#[test]
fn a_chord_or_radius_the_faces_cannot_hold_is_refused() {
    let (body, mid, _, _) = arc_walled_box(false);
    // A chord of 30 across a 40 mm wide wall (r ≈ 21, past the side walls and the block).
    let e = fillet_chord(&body, &[mid], 30.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    assert!(fillet_chord(&body, &[mid], 3.0 / 2f64.sqrt()).is_ok());
    // A radius law too large for the wall.
    assert!(fillet_variable(&body, mid, Vec3::new(0.0, 30.0, 0.0), 2.0, 30.0).is_err());
    assert!(fillet_variable(&body, mid, Vec3::new(0.0, 30.0, 0.0), 20.0, 20.0).is_err());
    // Non-finite, negative and zero-at-both-ends laws: refused before the geometry is touched.
    let bad = |radii: Vec<f64>| RadiusSpline { degree: 1, knots: vec![0.0, 0.0, 1.0, 1.0], radii };
    assert!(fillet_variable_spline(&body, mid, mid, &bad(vec![2.0, f64::NAN])).is_err());
    assert!(fillet_variable_spline(&body, mid, mid, &bad(vec![2.0, f64::INFINITY])).is_err());
    assert!(fillet_variable_spline(&body, mid, mid, &bad(vec![2.0, -1.0])).is_err());
    assert!(fillet_variable_spline(&body, mid, mid, &bad(vec![0.0, 0.0])).is_err());
    assert!(fillet_variable_spline(&body, mid, mid, &RadiusSpline { degree: 1, knots: vec![0.0, 0.5, 1.0], radii: vec![2.0, 3.0] }).is_err());
    assert!(fillet_variable_points(&body, mid, mid, &[(0.0, 2.0), (1.5, 3.0)], false).is_err());
    // An edge the body does not have is refused by name, not approximated.
    assert!(fillet_chord(&body, &[Vec3::new(500.0, 500.0, 500.0)], 3.0).is_err());
}
