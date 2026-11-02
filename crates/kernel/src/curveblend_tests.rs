//! Independent analytic reference: the centre of a ball tangent to two perpendicular
//! cylinders lies on the intersection of their radius-offset cylinders.

use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};

use super::*;
use crate::blend::plane_normal;
use crate::issue_851_tests::planned_volume;
use crate::{Body, BoolOp, boolean, cylinder, fillet};

#[test]
fn issue_1303_chamfer_open_rim_after_end_chamfer() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let rim = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(31.0, -15.0), b: Vec2::new(31.0, -11.75) },
                Seg2::Line { a: Vec2::new(31.0, -11.75), b: Vec2::new(0.0, -11.75) },
                Seg2::Arc { center: Vec2::ZERO, radius: 11.75, start: -std::f64::consts::FRAC_PI_2, sweep: -std::f64::consts::PI },
                Seg2::Line { a: Vec2::new(0.0, 11.75), b: Vec2::new(31.0, 11.75) },
                Seg2::Line { a: Vec2::new(31.0, 11.75), b: Vec2::new(31.0, 15.0) },
                Seg2::Line { a: Vec2::new(31.0, 15.0), b: Vec2::new(0.0, 15.0) },
                Seg2::Arc { center: Vec2::ZERO, radius: 15.0, start: std::f64::consts::FRAC_PI_2, sweep: std::f64::consts::PI },
                Seg2::Line { a: Vec2::new(0.0, -15.0), b: Vec2::new(31.0, -15.0) },
            ],
        },
        holes: vec![],
    };
    let mut body = crate::extrude(&Plane::XY, &[rim], 0.0, 13.0).unwrap().pop().unwrap();
    // Exact local end bevels, meeting at the four-face vertices (30, +/-11.75, 13).
    body = crate::blend_corner::chamfer(
        &body,
        &[Vec3::new(31.0, 13.375, 13.0), Vec3::new(31.0, -13.375, 13.0), Vec3::new(31.0, 11.75, 8.0), Vec3::new(31.0, -11.75, 8.0)],
        1.0,
    )
    .unwrap();
    let picks = [Vec3::new(-11.75, 0.0, 13.0), Vec3::new(15.0, 11.75, 13.0), Vec3::new(15.0, -11.75, 13.0)];
    let result = crate::chamfer(&body, &picks, 1.0).unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.crossed_face_loops().unwrap(), 0);
    // At height 12 + u, the removed half-ring area is pi/2 * (2*r*u + u^2).
    // The straight strips end at min(31-u, 30+v), 0 <= v <= u. Their integral
    // over 0 <= u <= 1 is 30 + 1/6; this includes both split end sections.
    let expected = 30.0 + 1.0 / 6.0 + std::f64::consts::PI * (11.75 / 2.0 + 1.0 / 6.0);
    let volume = |b: &Body| b.tessellate_with(0.0001, true).unwrap().measure().volume;
    let removed = volume(&body) - volume(&result);
    assert!((removed - expected).abs() < 0.02, "removed {removed}, expected {expected}");
    for p in [Vec3::new(30.5, 12.25, 12.5), Vec3::new(30.5, -12.25, 12.5)] {
        assert!(result.solid.vertex_iter().any(|v| from_p3(v.point()).dist(p) < 1e-6));
    }
    for selection in [vec![picks[0]], picks.into_iter().rev().collect()] {
        let other = crate::chamfer(&body, &selection, 1.0).unwrap();
        assert!(other.validity().is_empty(), "{:?}", other.validity());
        assert!((volume(&body) - volume(&other) - expected).abs() < 0.02);
    }
    assert!(crate::chamfer(&body, &picks, 4.0).is_err());
}

fn tee(scale: f64, origin: Vec3) -> Body {
    let main = cylinder(origin + Vec3::new(-40.0, 0.0, 0.0) * scale, Vec3::X, 15.0 * scale, 80.0 * scale).unwrap();
    let branch = cylinder(origin, Vec3::Z, 10.0 * scale, 35.0 * scale).unwrap();
    boolean(&main, &branch, BoolOp::Union).unwrap().unwrap()
}

fn analytic_section(theta: f64, scale: f64, origin: Vec3) -> (Vec3, Vec3, Vec3) {
    let centre = Vec3::new(13.0 * theta.cos(), 13.0 * theta.sin(), (324.0 - 169.0 * theta.sin().powi(2)).sqrt());
    let main = Vec3::new(centre.x, centre.y * 15.0 / 18.0, centre.z * 15.0 / 18.0);
    let branch = Vec3::new(centre.x * 10.0 / 13.0, centre.y * 10.0 / 13.0, centre.z);
    (origin + centre * scale, origin + main * scale, origin + branch * scale)
}

fn nearest_blend(surfaces: &[mt::Surface], point: Vec3) -> (f64, Vec3) {
    surfaces
        .iter()
        .filter_map(|s| {
            let (u, v) = s.search_nearest_parameter(p3(point), None, 100)?;
            let actual = from_p3(s.subs(u, v));
            let n = s.normal(u, v);
            Some((actual.dist(point), Vec3::new(n.x, n.y, n.z).normalized()?))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap()
}

fn distance_to_analytic_arc(theta: f64, point: Vec3) -> f64 {
    let (centre, main, branch) = analytic_section(theta, 1.0, Vec3::ZERO);
    let a = (main - centre).normalized().unwrap();
    let b = (branch - centre).normalized().unwrap();
    let plane = a.cross(b).normalized().unwrap();
    let radial = point - centre;
    let projected = radial - plane * radial.dot(plane);
    let direction = projected.normalized().unwrap();
    let angle = a.dot(b).clamp(-1.0, 1.0).acos();
    let from_main = a.dot(direction).clamp(-1.0, 1.0).acos();
    let from_branch = b.dot(direction).clamp(-1.0, 1.0).acos();
    if (from_main + from_branch - angle).abs() < 1e-7 { point.dist(centre + direction * 3.0) } else { point.dist(main).min(point.dist(branch)) }
}

fn distance_to_analytic_band(point: Vec3) -> f64 {
    // The arc's XY azimuth stays between its two contacts' azimuths. Their difference
    // for this tee is less than 0.1 rad; bracket generously around the query azimuth.
    let azimuth = point.y.atan2(point.x);
    let (mut lo, mut hi) = (azimuth - 0.3, azimuth + 0.3);
    for _ in 0..55 {
        let a = lo + (hi - lo) * 0.38196601125;
        let b = lo + (hi - lo) * 0.61803398875;
        if distance_to_analytic_arc(a, point) < distance_to_analytic_arc(b, point) {
            hi = b;
        } else {
            lo = a;
        }
    }
    distance_to_analytic_arc((lo + hi) * 0.5, point)
}

#[test]
fn fitted_contact_edges_coincide_with_their_surface_boundaries() {
    let sections: Vec<Vec<Section>> = (0..4)
        .map(|k| {
            (0..SAMPLES)
                .map(|i| {
                    let theta = std::f64::consts::FRAC_PI_2 * (k as f64 + i as f64 / (SAMPLES - 1) as f64);
                    let (c, q1, q2) = analytic_section(theta, 1.0, Vec3::ZERO);
                    Section { c, q1, q2 }
                })
                .collect()
        })
        .collect();
    for k in 0..sections.len() {
        let fit = crate::guard("contact consistency", || fit_sections(&sections, k, true, true)).unwrap();
        for (side, curve) in fit.contacts.iter().enumerate() {
            let (lo, hi) = curve.range_tuple();
            for i in 0..=200 {
                let parameter = lo + (hi - lo) * i as f64 / 200.0;
                let edge = from_p3(curve.subs(parameter));
                let surface = from_p3(fit.surface.subs(side as f64, parameter));
                assert!(edge.dist(surface) < 1e-9, "contact-to-surface gap {}", edge.dist(surface));
            }
        }
    }
}

#[test]
fn rolling_ball_matches_analytic_tee_and_contacts() {
    for (scale, origin) in [(1.0, Vec3::ZERO), (0.25, Vec3::new(23.0, -17.0, 9.0))] {
        let body = tee(scale, origin);
        let point = origin + Vec3::new(0.0, 10.0, 125f64.sqrt()) * scale;
        let blend = fillet(&body, &[point], 3.0 * scale).unwrap();
        assert!(blend.validity().is_empty(), "{:?}", blend.validity());
        let surfaces: Vec<_> = blend.solid.face_iter().map(|f| f.oriented_surface()).filter(|s| matches!(s, mt::Surface::NurbsSurface(_))).collect();
        assert!(!surfaces.is_empty());
        let mut distance: f64 = 0.0;
        let mut tangent_angle: f64 = 0.0;
        for i in 0..96 {
            // Offset the angular samples from patch seams; contact endpoints are tested below.
            let theta = std::f64::consts::TAU * (i as f64 + 0.37) / 96.0;
            let (centre, main, branch) = analytic_section(theta, scale, origin);
            let a = (main - centre).normalized().unwrap();
            let b = (branch - centre).normalized().unwrap();
            let angle = a.dot(b).clamp(-1.0, 1.0).acos();
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                // Spherical linear interpolation of the radial unit vectors, independent of
                // the production rational control-point construction and fitting parameters.
                let radial = (a * ((1.0 - fraction) * angle).sin() + b * (fraction * angle).sin()) / angle.sin();
                let exact = centre + radial * (3.0 * scale);
                let (error, normal) = nearest_blend(&surfaces, exact);
                distance = distance.max(error);
                if fraction == 0.0 || fraction == 1.0 {
                    let want = (centre - exact).normalized().unwrap();
                    tangent_angle = tangent_angle.max(normal.dot(want).abs().min(1.0).acos().to_degrees());
                }
            }
        }
        assert!(distance <= 0.005, "scale {scale}: analytic surface deviation {distance} mm");
        assert!(tangent_angle <= 0.5, "scale {scale}: contact normals differ by {tangent_angle} degrees");

        // The reverse direction uses points on the actual trimmed face mesh, so an
        // untrimmed neighbouring surface cannot conceal an overshooting blend patch.
        let mesh = blend.tessellate_with(0.002, true).unwrap();
        let mut reverse_distance: f64 = 0.0;
        let mut probes = 0;
        for point in mesh.positions.iter().step_by(19) {
            let p = (*point - origin) / scale;
            if (p.x * p.x + p.y * p.y).sqrt() <= 10.000001
                || (p.y * p.y + p.z * p.z).sqrt() <= 15.000001
                || !(10.0..20.0).contains(&p.z)
                || p.x.abs() > 16.0
            {
                continue;
            }
            probes += 1;
            reverse_distance = reverse_distance.max(distance_to_analytic_band(p) * scale);
        }
        assert!(probes > 300, "only {probes} trimmed surface points checked");
        assert!(reverse_distance <= 0.005, "scale {scale}: reverse analytic deviation {reverse_distance} mm");
        eprintln!("scale {scale}: surface {distance:.9} mm, reverse {reverse_distance:.9} mm, contact normals {tangent_angle:.6} degrees");

        let mut contact_count = 0;
        for edge in Body::unique_edges(&blend.solid) {
            let curve = edge.oriented_curve();
            let (lo, hi) = curve.range_tuple();
            let endpoints = [from_p3(curve.subs(lo)), from_p3(curve.subs(hi))].map(|p| (p - origin) / scale);
            if endpoints.iter().any(|p| !(9.0..20.0).contains(&p.z) || p.x.abs() > 16.0) {
                continue;
            }
            let branch = endpoints.iter().all(|p| ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 1e-5);
            let main = endpoints.iter().all(|p| ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() < 1e-5);
            if !branch && !main {
                continue;
            }
            contact_count += 1;
            for i in 0..=200 {
                let p = (from_p3(curve.subs(lo + (hi - lo) * i as f64 / 200.0)) - origin) / scale;
                let radius_error = if branch { ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() } else { ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() };
                assert!(radius_error * scale <= 0.002, "supporting cylinder deviation {} mm", radius_error * scale);
            }
        }
        assert!(contact_count >= 4, "only {contact_count} contacts checked");
    }
}

#[test]
fn rolling_ball_refinement_cap_returns_an_error_and_preserves_input() {
    let body = tee(1.0, Vec3::ZERO);
    let solid = body.deep_copy();
    let ids: Vec<_> = Body::unique_edges(&solid)
        .iter()
        .filter(|edge| {
            let curve = edge.oriented_curve();
            let (lo, hi) = curve.range_tuple();
            let p = from_p3(curve.subs((lo + hi) * 0.5));
            p.z > 10.0 && ((p.x * p.x + p.y * p.y).sqrt() - 10.0).abs() < 0.01 && ((p.y * p.y + p.z * p.z).sqrt() - 15.0).abs() < 0.01
        })
        .map(mt::Edge::id)
        .collect();
    assert!(!ids.is_empty());
    let before = crate::step_export(&[&body], "unchanged").unwrap();
    let input_before = crate::step_export(&[&Body::new(solid.clone()).unwrap()], "unchanged").unwrap();
    // Four sections deliberately undersample the saddle; production starts at 24.
    let error = crate::guard("limited rolling ball", || curve_blend_with_limit(&solid, &ids, body.size(), 3.0, true, 4)).unwrap_err();
    assert!(error.to_string().contains("accuracy and section limits"), "{error}");
    assert_eq!(crate::step_export(&[&body], "unchanged").unwrap(), before);
    assert_eq!(crate::step_export(&[&Body::new(solid.clone()).unwrap()], "unchanged").unwrap(), input_before);
}

/// Plan-view construction of a round between two overlapping circles (a concave vertical edge
/// where two Z cylinders meet): the ball centre `f`, the circle and fillet arc angles, and the
/// area the round fills in, all independent of the kernel.
struct Gap {
    /// Points along the round's arc in plan, contacts included.
    arc: Vec<Vec3>,
    /// Arc angles on circle 1, circle 2 and the fillet between the corner and the contacts.
    angles: [f64; 3],
    area: f64,
}

fn gap(c1: Vec3, r1: f64, c2: Vec3, r2: f64, corner: Vec3, r: f64) -> Gap {
    // The ball centre is r from both circles, on the corner's side of the centre line.
    let (a, b) = (r1 + r, r2 + r);
    let d = c1.dist(c2);
    let u = (c2 - c1) * (1.0 / d);
    let along = (a * a - b * b + d * d) / (2.0 * d);
    let mut side = Vec3::new(-u.y, u.x, 0.0);
    if (corner - c1).dot(side) < 0.0 {
        side = side * -1.0;
    }
    let f = c1 + u * along + side * (a * a - along * along).sqrt();
    let (t1, t2) = (c1 + (f - c1) * (r1 / a), c2 + (f - c2) * (r2 / b));
    let angle = |c: Vec3, p: Vec3, q: Vec3| (p - c).normalized().unwrap().dot((q - c).normalized().unwrap()).clamp(-1.0, 1.0).acos();
    let angles = [angle(c1, t1, corner), angle(c2, corner, t2), angle(f, t2, t1)];
    // Shoelace over the three short arcs: circle 1 to the corner, circle 2 on, fillet back.
    let mut ring = Vec::new();
    for (c, p, q) in [(c1, t1, corner), (c2, corner, t2), (f, t2, t1)] {
        let (p0, q0) = (p - c, q - c);
        let (a0, a1) = (p0.y.atan2(p0.x), q0.y.atan2(q0.x));
        let mut sweep = a1 - a0;
        while sweep > std::f64::consts::PI {
            sweep -= std::f64::consts::TAU;
        }
        while sweep < -std::f64::consts::PI {
            sweep += std::f64::consts::TAU;
        }
        let rr = p0.len();
        for i in 0..20_000 {
            let t = a0 + sweep * i as f64 / 20_000.0;
            ring.push(c + Vec3::new(t.cos(), t.sin(), 0.0) * rr);
        }
    }
    let area = ring.iter().map(|p| solidcraft_geom::Vec2::new(p.x, p.y)).collect::<solidcraft_geom::Ring<_>>().signed_area().abs();
    let (p, q) = ((t1 - f).normalized().unwrap(), (t2 - f).normalized().unwrap());
    let arc = (0..=8).map(|i| angles[2] * i as f64 / 8.0).map(|t| f + (p * ((angles[2] - t).sin()) + q * t.sin()) * (r / angles[2].sin())).collect();
    Gap { arc, angles, area }
}

#[test]
fn rolling_ball_along_a_straight_edge_between_cylinders_trims_curved_end_seams() {
    // Two Z cylinders crossing square to each other: the vertical lines where they meet are
    // concave edges between two curved faces, and the blend ends on the planar caps, where the
    // seams are arcs.
    let (r1, r2, h, r) = (17.0, 9.5, 10.0, 6.0);
    let (c1, c2) = (Vec3::ZERO, Vec3::new(r1, r2, 0.0));
    let a = cylinder(c1, Vec3::Z, r1, h).unwrap();
    let b = cylinder(c2, Vec3::Z, r2, h).unwrap();
    let body = boolean(&a, &b, BoolOp::Union).unwrap().unwrap();
    // The corner: one of the two points where the circles cross.
    let d = c1.dist(c2);
    let u = (c2 - c1) * (1.0 / d);
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let corner = c1 + u * along + Vec3::new(-u.y, u.x, 0.0) * (r1 * r1 - along * along).sqrt();
    let g = gap(c1, r1, c2, r2, corner, r);
    let blend = fillet(&body, &[corner + Vec3::Z * (h / 2.0)], r).unwrap();
    assert!(blend.validity().is_empty(), "{:?}", blend.validity());
    // One blend face; the edge is replaced by two contact lines and two end arcs.
    let (before, after) = (crate::measure(&body).unwrap(), crate::measure(&blend).unwrap());
    assert_eq!((after.faces, after.edges, after.vertices), (before.faces + 1, before.edges + 3, before.vertices + 2), "{before:?} -> {after:?}");
    // The round fills the gap over the full height. The caps gain it twice; the walls lose
    // their arcs to the contacts and gain the round.
    let (m0, m1) = (body.tessellate_with(0.0005, true).unwrap().measure(), blend.tessellate_with(0.0005, true).unwrap().measure());
    let volume = h * g.area;
    assert!((m1.volume - m0.volume - volume).abs() < volume * 3e-3, "added volume {} vs {volume}", m1.volume - m0.volume);
    let [a1, a2, af] = g.angles;
    let area = 2.0 * g.area + h * (r * af - r1 * a1 - r2 * a2);
    assert!((m1.area - m0.area - area).abs() < area.abs() * 3e-3, "added area {} vs {area}", m1.area - m0.area);
    // The round is the analytic arc at every height, from contact to contact.
    let surfaces: Vec<_> = blend.solid.face_iter().map(|f| f.oriented_surface()).filter(|s| matches!(s, mt::Surface::NurbsSurface(_))).collect();
    let mut worst: f64 = 0.0;
    for z in [0.0, 0.25, 0.5, 0.75, 1.0] {
        for p in &g.arc {
            worst = worst.max(nearest_blend(&surfaces, *p + Vec3::Z * (h * z)).0);
        }
    }
    assert!(worst < 1e-3, "blend surface is {worst} mm off the analytic round");
}

#[test]
fn closed_curved_seam_is_not_supported() {
    // A closed seam (one edge starting and ending at the same vertex, as a full cap circle can
    // be in imported geometry) has no "between" two contacts, so trimming it is refused rather
    // than guessed.
    let body = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 5.0).unwrap();
    let arc = body.solid.edge_iter().find(|e| !matches!(e.curve(), mt::Curve::Line(_))).expect("a cap arc");
    let circle = mt::Edge::new_unchecked(arc.front(), arc.front(), arc.curve());
    assert_eq!(circle.front(), circle.back());
    // Contacts on the circle itself: a third and two thirds of the way round.
    let (lo, hi) = circle.curve().range_tuple();
    let on = |f: f64| mt::Vertex::new(circle.curve().subs(lo + (hi - lo) * f));
    for (a, b) in [(Some(on(1.0 / 3.0)), None), (None, Some(on(2.0 / 3.0))), (Some(on(1.0 / 3.0)), Some(on(2.0 / 3.0))), (None, None)] {
        let err = trim_curved_seam(&circle, a, b, 20.0).unwrap_err();
        assert!(err.to_string().contains("not supported yet: a closed curved edge"), "{err}");
    }
}

#[test]
fn rolling_ball_past_the_end_seam_is_too_large_and_reduces() {
    // The crossing cylinders of the test above with the small one's centre inside the large
    // one: a round of 3 would reach past the end of the cap arc it trims, so it is too large
    // (a smaller one fits), and the largest that fits is the analytic round over the full height
    // (checked on its surface: its added volume is too small beside the body's to measure).
    let (r1, r2, d, h) = (17.0, 9.5, 12.0, 10.0);
    let (c1, c2) = (Vec3::ZERO, Vec3::new(d, 0.0, 0.0));
    let body = boolean(&cylinder(c1, Vec3::Z, r1, h).unwrap(), &cylinder(c2, Vec3::Z, r2, h).unwrap(), BoolOp::Union).unwrap().unwrap();
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let corner = Vec3::new(along, (r1 * r1 - along * along).sqrt(), 0.0);
    let at = [corner + Vec3::Z * (h / 2.0)];
    let e = fillet(&body, &at, 3.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    let (r, blend) = crate::fillet_max(&body, &at, 3.0).unwrap();
    assert!(r > 1.5 && r < 3.0, "{r}");
    assert!(blend.validity().is_empty(), "{:?}", blend.validity());
    let g = gap(c1, r1, c2, r2, corner, r);
    let surfaces: Vec<_> = blend.solid.face_iter().map(|f| f.oriented_surface()).filter(|s| matches!(s, mt::Surface::NurbsSurface(_))).collect();
    let mut worst: f64 = 0.0;
    for z in [0.0, 0.25, 0.5, 0.75, 1.0] {
        for p in &g.arc {
            worst = worst.max(nearest_blend(&surfaces, *p + Vec3::Z * (h * z)).0);
        }
    }
    assert!(worst < 1e-3, "blend surface is {worst} mm off the analytic round at r {r}");
}

#[test]
fn a_refused_edge_is_not_rolled_again_after_blends_far_from_it() {
    // The crossing cylinders' seam refuses a round of 3 (too large). A bar joined to them has
    // two straight edges at its far end that round on their own. Each blend there leaves the
    // seam's surroundings as they were, so the seam is tried once, not once per blend, and the
    // refusal is the same.
    let (r1, r2, d, h) = (17.0, 9.5, 12.0, 10.0);
    let body = boolean(&cylinder(Vec3::ZERO, Vec3::Z, r1, h).unwrap(), &cylinder(Vec3::new(d, 0.0, 0.0), Vec3::Z, r2, h).unwrap(), BoolOp::Union)
        .unwrap()
        .unwrap();
    let bar = crate::box_solid(Vec3::new(-80.0, -5.0, 0.0), Vec3::new(-10.0, 5.0, h)).unwrap();
    let body = boolean(&body, &bar, BoolOp::Union).unwrap().unwrap();
    let along = (r1 * r1 - r2 * r2 + d * d) / (2.0 * d);
    let seam = Vec3::new(along, (r1 * r1 - along * along).sqrt(), h / 2.0);
    let ends = [Vec3::new(-80.0, -5.0, h / 2.0), Vec3::new(-80.0, 5.0, h / 2.0)];
    crate::blend::ROLLS.with(|n| n.set(0));
    let e = fillet(&body, &[seam, ends[0], ends[1]], 3.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    let rolls = crate::blend::ROLLS.with(|n| n.get());
    assert_eq!(rolls, 1, "the seam was rolled {rolls} times");
    // The far edges alone still round.
    assert!(fillet(&body, &ends, 3.0).is_ok());
}

/// At a knife edge (the plane and the wall's normals opposite) the ball's centre is the same
/// whichever face is side 1: `r` inside the plane and `r` outside the wall's cylinder.
#[test]
fn knife_edge_ball_centre_does_not_depend_on_the_side_order() {
    const R: f64 = 21.0;
    let tip = Vec3::new(62.0 + 1.0 / 6.0, 0.0, 5.0);
    let slab = crate::box_solid(Vec3::new(0.0, -R, 0.0), Vec3::new(tip.x, 0.0, 10.0)).unwrap();
    let disc = cylinder(Vec3::new(tip.x, -R, 0.0), Vec3::Z, R, 10.0).unwrap();
    let body = boolean(&slab, &disc, BoolOp::Cut).unwrap().unwrap();
    let faces: Vec<&mt::Face> = body.solid.face_iter().collect();
    let plane = faces.iter().copied().find(|f| plane_normal(f).is_some_and(|n| n.y > 0.99)).unwrap();
    let wall = faces.iter().copied().find(|f| plane_normal(f).is_none()).unwrap();
    for r in [0.5, 2.0, 5.0] {
        let centre = Vec3::new(tip.x - (4.0 * R * r).sqrt(), -r, tip.z);
        // `cusp` is +1 when n1 × t points into the cusp: the wall's normal is −y, the plane's +y.
        for (f1, f2, cusp) in [(wall, plane, 1.0), (plane, wall, -1.0)] {
            let s = solve(&[f1], &[f2], tip, Vec3::Z, r, -1.0, cusp, None, 1e-6).unwrap();
            assert!(s.c.dist(centre) < 1e-6, "r={r} cusp={cusp}: centre {:?}, expected {centre:?}", s.c);
        }
    }
}

/// #896: the seams where rounds run into the faces beside them are picked to be rounded again.
/// The faces meet tangentially there, so there is nothing to round: the refusal says so, rather
/// than that blending such an edge isn't supported yet.
#[test]
fn a_smooth_seam_is_refused_as_having_no_corner() {
    let b = crate::box_solid(Vec3::ZERO, Vec3::new(150.0, 200.0, 70.0)).unwrap();
    let mut edges = Vec::new();
    for (x, y) in [(0.0, 0.0), (150.0, 0.0), (0.0, 200.0), (150.0, 200.0)] {
        edges.push(Vec3::new(x, y, 35.0));
    }
    for (a, z) in [(0.0, 0.0), (150.0, 0.0), (0.0, 70.0), (150.0, 70.0)] {
        edges.push(Vec3::new(a, 100.0, z));
    }
    for (y, z) in [(0.0, 0.0), (200.0, 0.0), (0.0, 70.0), (200.0, 70.0)] {
        edges.push(Vec3::new(75.0, y, z));
    }
    let rounded = fillet(&b, &edges, 10.0).unwrap();
    // The seam on the top face beside the round of the edge along y at x = 0, and the seam
    // with the end face x = 0 beside the round of the top edge along y.
    for seam in [Vec3::new(10.0, 100.0, 70.0), Vec3::new(0.0, 100.0, 60.0)] {
        for chamfer in [false, true] {
            let r = if chamfer { crate::chamfer(&rounded, &[seam], 1.0) } else { fillet(&rounded, &[seam], 1.0) };
            match r {
                Err(KernelError::Invalid(m)) => assert!(m.contains("smooth") && m.contains("no corner"), "{m}"),
                other => panic!("{seam:?}: {:?}", other.map(|_| "a body")),
            }
        }
    }
    // A sharp edge of a box with one round still takes a fillet beside the round's seams.
    let one = fillet(&crate::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap(), &[Vec3::new(40.0, 30.0, 10.0)], 5.0).unwrap();
    assert!(fillet(&one, &[Vec3::new(20.0, 0.0, 20.0)], 1.0).is_ok());
    assert!(matches!(fillet(&one, &[Vec3::new(35.0, 30.0, 10.0)], 1.0), Err(KernelError::Invalid(_))));
}

/// A 40 × 40 × 20 box whose front wall is an arc of radius 40 about a Z axis: `concave` cuts a
/// cylinder away from the front (the wall curves into the part), otherwise the part is the box
/// within the cylinder (the wall curves out). The arc meets the planar side walls x = 0 and
/// x = 40 at an angle, not square. With the bottom arc's midpoint, the axis's base point and the
/// side of the wall the material is on (+1 away from the axis, −1 toward it).
fn arc_walled_box(concave: bool) -> (Body, Vec3, Vec3, f64) {
    let block = crate::box_solid(Vec3::ZERO, Vec3::new(40.0, 40.0, 20.0)).unwrap();
    let (axis, op, side) =
        if concave { (Vec3::new(20.0, -30.0, 0.0), BoolOp::Cut, 1.0) } else { (Vec3::new(20.0, 70.0, 0.0), BoolOp::Intersect, -1.0) };
    let tool = cylinder(axis, Vec3::Z, 40.0, 20.0).unwrap();
    let body = boolean(&block, &tool, op).unwrap().unwrap();
    (body, axis + Vec3::new(0.0, side * 40.0, 0.0), axis, side)
}

/// Volume the blend removes along the bottom arc: the corner's cross-section at each distance
/// ρ from the axis (`depth(x)`, x ∈ [0, size] from the wall into the material), times the arc
/// of radius ρ between the side walls.
fn removed_volume(side: f64, size: f64, depth: impl Fn(f64) -> f64) -> f64 {
    let steps = 4000;
    let h = size / steps as f64;
    (0..=steps)
        .map(|i| {
            let x = i as f64 * h;
            let rho = 40.0 + side * x;
            let w = if i == 0 || i == steps {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            w * depth(x) * 2.0 * rho * (20.0 / rho).asin()
        })
        .sum::<f64>()
        * h
        / 3.0
}

#[test]
fn rolling_ball_along_an_arc_ends_in_side_walls_at_an_angle() {
    // The bottom edge of an arc wall ends in the side walls at an angle (#556). The round is the
    // torus about the cylinder's axis (centre circle of radius 40 ± r at height r), trimmed by
    // the side walls; the chamfer is the cone between the two contacts.
    let r = 2.0;
    for concave in [true, false] {
        let (body, mid, axis, side) = arc_walled_box(concave);
        let v0 = planned_volume(&body);
        for round in [true, false] {
            let what = format!("{} {}", if concave { "concave" } else { "convex" }, if round { "fillet" } else { "chamfer" });
            let blend = if round { fillet(&body, &[mid], r) } else { crate::chamfer(&body, &[mid], r) }.unwrap_or_else(|e| panic!("{what}: {e}"));
            assert!(blend.validity().is_empty(), "{what}: {:?}", blend.validity());
            let volume = planned_volume(&blend);
            let removed =
                if round { removed_volume(side, r, |x| r - (r * r - (r - x) * (r - x)).max(0.0).sqrt()) } else { removed_volume(side, r, |x| r - x) };
            assert!((v0 - volume - removed).abs() < removed * 3e-3, "{what}: removed {} vs {removed}", v0 - volume);
            // The blend face stays between the side walls and, for the round, on the torus.
            let surfaces: Vec<_> =
                blend.solid.face_iter().map(|f| f.oriented_surface()).filter(|s| matches!(s, mt::Surface::NurbsSurface(_))).collect();
            assert!(!surfaces.is_empty(), "{what}: no blend face");
            let mesh = blend.tessellate_with(0.0005, true).unwrap();
            let (lo, hi) = mesh.positions.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
            assert!(lo > -1e-3 && hi < 40.0 + 1e-3, "{what}: the blend runs past the side walls: x {lo}..{hi}");
            if round {
                // Sections of the round across the arc: the quarter circle about the centre
                // ring (radius 40 ± r about the axis, at height r) from contact to contact.
                let ring = 40.0 + side * r;
                let mut worst: f64 = 0.0;
                for i in 0..=40 {
                    let x = 40.0 * i as f64 / 40.0;
                    for a in 1..8 {
                        let t = std::f64::consts::FRAC_PI_2 * a as f64 / 8.0;
                        let (rho, z) = (ring - side * r * t.cos(), r - r * t.sin());
                        let dx = x - axis.x;
                        let dy = (rho * rho - dx * dx).sqrt() * if concave { 1.0 } else { -1.0 };
                        let p = Vec3::new(x, axis.y + dy, z);
                        worst = worst.max(nearest_blend(&surfaces, p).0);
                    }
                }
                assert!(worst < 1e-3, "{what}: blend surface is {worst} mm off the torus");
            }
        }
    }
}

#[test]
fn rolling_ball_past_a_slanted_end_face_is_too_large_and_reduces() {
    // On the outward-curving wall a round of 4 would run its bottom contact into the back face
    // before it reaches the side wall (the contact circle of radius 36 meets x = 0 at y ≈ 40.07):
    // too large, and the largest that fits rounds the edge.
    let (body, mid, _, _) = arc_walled_box(false);
    let e = fillet(&body, &[mid], 4.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    let (r, blend) = crate::fillet_max(&body, &[mid], 4.0).unwrap();
    assert!(r > 3.0 && r < 4.0, "{r}");
    assert!(blend.validity().is_empty(), "{:?}", blend.validity());
}

#[test]
fn rolling_ball_runs_into_a_knife_edge_tip() {
    // The corner of a block outside a cylinder of radius 21 about (21, −21): its wall meets the
    // back face y = 0 tangentially at (21, 0) and the left face x = 0 at (0, −21), knife-edge
    // tips at both ends of the arc. The round of the bottom arc runs into both, cut off by the
    // end faces' planes, which touch the wall contact at the tips without crossing it: at
    // distance ρ from the axis it runs from x = 0 to y = 0, ρ (asin(21 / ρ) − acos(21 / ρ)).
    let block = crate::box_solid(Vec3::new(0.0, -21.0, 0.0), Vec3::new(21.0, 0.0, 10.0)).unwrap();
    let body = boolean(&block, &cylinder(Vec3::new(21.0, -21.0, -1.0), Vec3::Z, 21.0, 12.0).unwrap(), BoolOp::Cut).unwrap().unwrap();
    let mid = Vec3::new(21.0 - 21.0 * std::f64::consts::FRAC_1_SQRT_2, -21.0 + 21.0 * std::f64::consts::FRAC_1_SQRT_2, 0.0);
    let r = 2.0;
    let blend = fillet(&body, &[mid], r).unwrap();
    assert!(blend.validity().is_empty(), "{:?}", blend.validity());
    let (n, h) = (4000, r / 4000.0);
    let removed = (0..=n)
        .map(|i| {
            let x = i as f64 * h;
            let rho = 21.0 + x;
            let w = if i == 0 || i == n {
                1.0
            } else if i % 2 == 1 {
                4.0
            } else {
                2.0
            };
            w * (r - (r * r - (r - x) * (r - x)).max(0.0).sqrt()) * rho * ((21.0 / rho).asin() - (21.0 / rho).acos())
        })
        .sum::<f64>()
        * h
        / 3.0;
    let got = planned_volume(&body) - planned_volume(&blend);
    assert!((got - removed).abs() < removed * 5e-3, "removed {got} vs {removed}");
}

#[test]
fn rolling_ball_turns_about_a_rounded_corner_of_its_radius() {
    // The box with the inward-curving wall, its 60° corner at x = 0 rounded (r 2) first. The
    // bottom edges then run: the side wall's, the corner round's (an arc of radius 2) and the
    // arc wall's into the slanted side wall at x = 40. Rounded again with r 2 the ball can't
    // roll round the corner: it turns about one point there, and the blend is a piece of sphere.
    // It is what the rolling round tends to as its radius tends to the corner's, too large past
    // it, and its mesh is closed.
    let (body, _, _, _) = arc_walled_box(true);
    let body = fillet(&body, &[Vec3::new(0.0, 4.641016151377546, 10.0)], 2.0).unwrap();
    // The corner round's centre is 4 from the corner along its bisector, (2, 8.105): its arc's
    // middle is 2 back toward the corner.
    let ends = [Vec3::new(0.0, 25.0, 0.0), Vec3::new(1.0, 6.373, 0.0), Vec3::new(20.0, 10.0, 0.0)];
    let v0 = planned_volume(&body);
    let removed = |r: f64| {
        let b = fillet(&body, &ends, r).unwrap_or_else(|e| panic!("r {r}: {e}"));
        assert!(b.validity().is_empty(), "r {r}: {:?}", b.validity());
        v0 - planned_volume(&b)
    };
    let (a, b, at) = (removed(1.96), removed(1.98), removed(2.0));
    let expected = 2.0 * b - a;
    assert!((at - expected).abs() < expected * 2e-3, "removed {at} at r 2, {a} and {b} before it");
    let e = fillet(&body, &ends, 2.5).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    let (r, _) = crate::fillet_max(&body, &ends, 2.5).unwrap();
    assert!(r > 1.5 && r <= 2.0 + 1e-9, "{r}");
}

#[test]
fn rolling_ball_along_a_straight_edge_ends_in_a_curved_face() {
    // The box with the inward-curving wall: the bottom edge of the side wall x = 0 runs from the
    // back to the curved wall (#556). The round of radius 2 along it is cut off by that wall:
    // at d from the side wall it runs from y = −30 + √(40² − (d − 20)²) to the back at y = 40.
    let (body, _, _, _) = arc_walled_box(true);
    let r = 2.0;
    for round in [true, false] {
        let blend =
            if round { fillet(&body, &[Vec3::new(0.0, 25.0, 0.0)], r) } else { crate::chamfer(&body, &[Vec3::new(0.0, 25.0, 0.0)], r) }.unwrap();
        assert!(blend.validity().is_empty(), "{:?}", blend.validity());
        let (n, h) = (4000, r / 4000.0);
        let removed = (0..=n)
            .map(|i| {
                let d = i as f64 * h;
                let w = if i == 0 || i == n {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                };
                let depth = if round { r - (r * r - (r - d) * (r - d)).max(0.0).sqrt() } else { r - d };
                w * depth * (40.0 - (-30.0 + (1600.0 - (d - 20.0) * (d - 20.0)).sqrt()))
            })
            .sum::<f64>()
            * h
            / 3.0;
        let (m0, m1) = (body.tessellate_with(0.0005, true).unwrap(), blend.tessellate_with(0.0005, true).unwrap());
        let got = m0.measure().volume - m1.measure().volume;
        assert!((got - removed).abs() < removed * 3e-3, "round {round}: removed {got} vs {removed}");
        let volume_about = |o: Vec3| {
            let at = |i: u32| m1.positions[i as usize] - o;
            m1.triangles.iter().map(|t| at(t[0]).dot(at(t[1]).cross(at(t[2]))) / 6.0).sum::<f64>()
        };
        let shift = volume_about(Vec3::ZERO) - volume_about(Vec3::new(-100.0, 50.0, 30.0));
        assert!(shift.abs() < 1e-2, "round {round}: the mesh is not closed (volume moves {shift} with the origin)");
    }
}

#[test]
fn chamfer_a_chain_of_a_line_and_an_arc_into_a_slanted_end() {
    // A 40 × 40 block, 20 high, whose front runs straight for 20 and then along an arc of
    // radius 29 about (20, 29), tangent to it, up to (40, 8): the right face meets the arc at an
    // angle. The top front chain chamfered by 2 (the planes' and the cone's sections are 2 × 2
    // corners, 2 − w high at w in from the edge): 2 a millimetre along the line, and along the
    // arc out to the right face, at each distance ρ from its axis, the height times the arc of
    // radius ρ inside x ≤ 40.
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let start = -std::f64::consts::FRAC_PI_2;
    let end = (-21.0f64).atan2(20.0);
    let segs = vec![
        Seg2::Line { a: Vec2::new(0.0, 0.0), b: Vec2::new(20.0, 0.0) },
        Seg2::Arc { center: Vec2::new(20.0, 29.0), radius: 29.0, start, sweep: end - start },
        Seg2::Line { a: Vec2::new(40.0, 8.0), b: Vec2::new(40.0, 40.0) },
        Seg2::Line { a: Vec2::new(40.0, 40.0), b: Vec2::new(0.0, 40.0) },
        Seg2::Line { a: Vec2::new(0.0, 40.0), b: Vec2::new(0.0, 0.0) },
    ];
    let body = crate::extrude(&Plane::XY, &[Region2 { outer: Loop2 { segs }, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
    let volume = |b: &Body| b.tessellate_with(0.0005, true).unwrap().measure().volume;
    let made = crate::chamfer(&body, &[Vec3::new(10.0, 0.0, 20.0)], 2.0).unwrap();
    let n = 2000;
    let h = 2.0 / n as f64;
    let arc = (0..=n)
        .map(|i| {
            let rho = 27.0 + i as f64 * h;
            let w = if i == 0 || i == n { 1.0 } else { [2.0, 4.0][i % 2] };
            w * (rho - 27.0) * rho * (20.0 / rho).asin()
        })
        .sum::<f64>()
        * h
        / 3.0;
    let want = 40.0 + arc;
    let got = volume(&body) - volume(&made);
    assert!((got - want).abs() < want * 1e-3, "removed {got} vs {want}");
}

/// #1667: an R1 round around the end of a long bar whose four long edges are R5. The chain is
/// 290 mm lines and quarter turns of 7.85 mm, so the lines' sections are spaced far wider than
/// the turns'; borrowing the turns' sections for the lines' ends made their strips overshoot and
/// fold back past the joints, the strips' meshes folded with them, and the measured volume came
/// out thousands of mm³ off. The R1 pipe is exact: the corner's section, `1 - π/4`, along the
/// lines, and round each turn's axis at the section's centroid (Pappus).
#[test]
fn round_along_long_lines_and_small_turns_measures_its_volume() {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};
    let (w, d, h) = (300.0, 60.0, 40.0);
    let bar = crate::build::box_solid(Vec3::new(-w / 2.0, 0.0, 0.0), Vec3::new(w / 2.0, d, h)).unwrap();
    let long = [Vec3::new(-w / 2.0, d / 2.0, 0.0), Vec3::new(w / 2.0, d / 2.0, 0.0), Vec3::new(-w / 2.0, d / 2.0, h), Vec3::new(w / 2.0, d / 2.0, h)];
    let rounded = fillet(&bar, &long, 5.0).unwrap();
    // The rolling ball along the end face's loop (the exact blends would take this loop; a
    // loop next to free-form faces, as in the issue, comes here).
    let ids: Vec<mt::EdgeID> = crate::body::Body::unique_edges(&rounded.solid)
        .into_iter()
        .filter(|e| [e.front(), e.back()].iter().all(|v| crate::blend::vtx(v).y.abs() < 1e-9))
        .map(|e| e.id())
        .collect();
    assert_eq!(ids.len(), 8);
    let (solid, _) = crate::guard("test", || curve_blend(&rounded.solid, &ids, rounded.size(), 1.0, true)).unwrap();
    let body = Body::new(solid).unwrap();
    assert!(body.validity().is_empty(), "{:?}", body.validity());
    let section = 1.0 - FRAC_PI_4;
    // The section's centroid lies this far from its corner along each side.
    let inset = (5.0 / 6.0 - FRAC_PI_4) / section;
    let lines = 2.0 * (w - 10.0) + 2.0 * (h - 10.0);
    let turns = 4.0 * FRAC_PI_2 * (5.0 - inset);
    let want = w * d * h - 4.0 * 25.0 * section * d - section * (lines + turns);
    let m = crate::measure(&body).unwrap();
    assert!((m.volume - want).abs() < 1e-5 * want, "{} vs {want}", m.volume);
    // Centred across the bar, as the part is.
    assert!(m.centroid.x.abs() < 1e-3, "centre of mass x {}", m.centroid.x);
}

/// The closed profile of four natural fit splines, each through three points (its ends and a
/// bowed middle), as the sketch's fit-point spline makes them: two cubic spans a side, joined
/// smoothly within a side and at an angle at the four corners. Rotated a quarter turn about
/// (10, 10) side to side.
fn spline_sided_profile() -> solidcraft_geom::Region2 {
    use solidcraft_geom::{Loop2, Region2, Seg2, Vec2};
    let side = [Vec2::new(0.0, 0.0), Vec2::new(10.0, -3.0), Vec2::new(20.0, 0.0)];
    let turn = |p: Vec2, q: usize| (0..q).fold(p - Vec2::new(10.0, 10.0), |v, _| Vec2::new(-v.y, v.x)) + Vec2::new(10.0, 10.0);
    let mut segs = Vec::new();
    for q in 0..4 {
        let [p0, p1, p2] = side.map(|p| turn(p, q));
        // Natural ends: the middle point's second derivative, chord-length parameters.
        let (h0, h1) = (p0.dist(p1), p1.dist(p2));
        let m = ((p2 - p1) / h1 - (p1 - p0) / h0) * (6.0 / (2.0 * (h0 + h1)));
        let spans = [(p0, p1, h0, Vec2::ZERO, m), (p1, p2, h1, m, Vec2::ZERO)];
        for (a, b, h, ma, mb) in spans {
            let slope = (b - a) / h;
            let d0 = (slope - (ma * 2.0 + mb) * (h / 6.0)) * h;
            let d1 = (slope + (ma + mb * 2.0) * (h / 6.0)) * h;
            segs.push(Seg2::Cubic { p0: a, p1: a + d0 / 3.0, p2: b - d1 / 3.0, p3: b });
        }
    }
    Region2 { outer: Loop2 { segs }, holes: vec![] }
}

/// #3779: small equal chamfers and rounds round the top rim of an extruded profile of spline
/// sides. Each side's two spans roll as one chain; its sections resampled by distance along the
/// centres are solved at the spline's own point and tangent there (the chord between centres
/// tilted the section plane off the spline and the ball did not fit), and each chain ends in the
/// next side's wall at the corner. The rim is 4 × 20.9 mm long; the removed volume is the
/// section's area times the rim's length, within the corners' share.
#[test]
fn small_blends_round_a_spline_sided_rim() {
    use solidcraft_geom::Plane;
    let body = crate::extrude(&Plane::XY, &[spline_sided_profile()], 0.0, 2.0).unwrap().pop().unwrap();
    // A point in the middle of each span of the top rim.
    let picks: Vec<Vec3> = crate::body::Body::unique_edges(&body.solid)
        .into_iter()
        .filter(|e| [e.front(), e.back()].iter().all(|v| (crate::blend::vtx(v).z - 2.0).abs() < 1e-9))
        .map(|e| {
            let c = e.curve();
            let (a, b) = c.range_tuple();
            from_p3(c.subs((a + b) * 0.5))
        })
        .collect();
    assert_eq!(picks.len(), 8);
    let rim: f64 = crate::body::Body::unique_edges(&body.solid)
        .into_iter()
        .filter(|e| [e.front(), e.back()].iter().all(|v| (crate::blend::vtx(v).z - 2.0).abs() < 1e-9))
        .map(|e| {
            let c = e.curve();
            let (a, b) = c.range_tuple();
            (0..200).map(|i| from_p3(c.subs(a + (b - a) * i as f64 / 200.0)).dist(from_p3(c.subs(a + (b - a) * (i + 1) as f64 / 200.0)))).sum::<f64>()
        })
        .sum();
    let volume = |b: &Body| crate::measure(b).unwrap().volume;
    let v0 = volume(&body);
    let d: f64 = 0.1;
    for round in [false, true] {
        let what = if round { "fillet" } else { "chamfer" };
        let made = if round { fillet(&body, &picks, d) } else { crate::chamfer(&body, &picks, d) }.unwrap_or_else(|e| panic!("{what} {d}: {e}"));
        assert!(made.validity().is_empty(), "{what}: {:?}", made.validity());
        assert_eq!(made.crossed_face_loops().unwrap(), 0, "{what}");
        let area = if round { d * d * (1.0 - std::f64::consts::FRAC_PI_4) } else { d * d / 2.0 };
        let removed = v0 - volume(&made);
        let want = area * rim;
        assert!((removed - want).abs() < want * 0.05, "{what}: removed {removed} vs {want}");
    }
}
