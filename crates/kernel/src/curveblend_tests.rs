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
    // Two picks of the same refused chain used to repeat the rolling-ball build,
    // even though no successful blend had changed its geometry.
    crate::blend::ROLLS.with(|n| n.set(0));
    let e = fillet(&body, &[seam - Vec3::Z * 2.0, seam + Vec3::Z * 2.0], 3.0).unwrap_err();
    assert!(matches!(e, KernelError::TooLarge(_)), "{e}");
    assert_eq!(crate::blend::ROLLS.with(|n| n.get()), 1);
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

/// #3549: every rolling-ball station used to evaluate the same 2,601 surface points
/// again. The prepared search retains the cold inversion's answer on both sides of
/// a rational cylindrical patch, including near its ends.
#[test]
fn rolling_sections_reuse_the_surface_search_grid() {
    let body = crate::box_solid(Vec3::ZERO, Vec3::new(20.0, 30.0, 10.0)).unwrap();
    let rounded = fillet(&body, &[Vec3::new(0.0, 0.0, 5.0)], 2.0).unwrap();
    let face = rounded.solid.face_iter().find(|f| matches!(f.surface(), mt::Surface::NurbsSurface(_))).unwrap();
    for face in [face.clone(), face.inverse()] {
        let projection = SectionProjection::default();
        let surface = face.oriented_surface();
        let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() else { panic!("bounded round") };
        for i in 0..20 {
            let u = u0 + (u1 - u0) * (f64::from(i) + 0.25) / 20.0;
            let v = v0 + (v1 - v0) * (f64::from(19 - i) + 0.75) / 20.0;
            let at = from_p3(surface.subs(u, v));
            let normal = surface.normal(u, v);
            let normal = Vec3::new(normal.x, normal.y, normal.z).normalized().unwrap();
            for offset in [-0.5, 0.5] {
                let p = at + normal * offset;
                let cold = project(&face, p, None).unwrap();
                let cached = projection.project(&face, p, None).unwrap();
                assert!(cold.0.dist(cached.0) < 1e-10, "{cold:?} vs {cached:?}");
                assert!(cold.1.dot(cached.1) > 1.0 - 1e-10);
                assert!(cached.0.dist(at) < 1e-8);
            }
        }
        let prepared = projection.faces.borrow();
        assert_eq!(prepared.len(), 1);
        assert!(prepared.contains_key(&(face.id(), face.orientation())));
    }
}

/// The banded factorization gives the same controls as the old pivoted dense solve,
/// including the maximum refinement used by a rolling-ball strip. Independent
/// polynomial values between stations also check that the curve still interpolates.
#[test]
fn uniform_cubic_band_solve_matches_dense_interpolation() {
    for count in [4, 5, 8, 24, 47, 93, 185, 369] {
        let u: Vec<f64> = (0..count).map(|i| i as f64 / (count - 1) as f64).collect();
        let value = |t: f64| Vec3::new(20.0 * t, 100.0 * t * t, 50.0 * t * t * t);
        let points: Vec<Vec3> = u.iter().copied().map(value).collect();
        let system = crate::build::UniformCubic::new(count).unwrap();
        let values: Vec<_> = points.iter().map(|p| [p.x, p.y, p.z]).collect();
        let controls = system.solve(&values).unwrap().into_iter().map(|p| mt::Point3::new(p[0], p[1], p[2])).collect();
        let band = mt::BSplineCurve::try_new(system.knots, controls).unwrap();
        let dense = crate::build::interpolate_cubic_at(&points, &u).unwrap();
        for (a, b) in band.control_points().iter().zip(dense.control_points()) {
            assert!(from_p3(*a).dist(from_p3(*b)) < 1e-9, "count {count}: {a:?} vs {b:?}");
        }
        for i in 0..=50 {
            let t = f64::from(i) / 50.0;
            assert!(from_p3(band.subs(t)).dist(value(t)) < 1e-9, "count {count}, t {t}");
        }
    }
}

/// Face selection across pieces of one cylinder must see one rim, including a
/// chord-sized Boolean fragment; it must not try to fillet artificial seams.
#[test]
fn cylindrical_blend_supports_coalesce_with_their_rims() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let angles = [-1.0_f64, -0.2, -0.2 + 3.4e-6, 0.2, 1.0];
    let at = |r: f64, a: f64| Vec2::new(r * a.cos(), r * a.sin());
    let mut segments = Vec::new();
    for pair in angles.windows(2) {
        segments.push(Seg2::Arc { center: Vec2::ZERO, radius: 30.0, start: pair[0], sweep: pair[1] - pair[0] });
    }
    segments.push(Seg2::Line { a: at(30.0, 1.0), b: at(25.0, 1.0) });
    for pair in angles.windows(2).rev() {
        segments.push(Seg2::Arc { center: Vec2::ZERO, radius: 25.0, start: pair[1], sweep: pair[0] - pair[1] });
    }
    segments.push(Seg2::Line { a: at(25.0, -1.0), b: at(30.0, -1.0) });
    let region = Region2 { outer: Loop2 { segs: segments }, holes: vec![] };
    let body = crate::extrude(&Plane::XY, &[region], 0.0, 10.0).unwrap().remove(0);
    assert_eq!(body.face_count(), 12);
    // A Boolean may represent this 0.1-micrometre rim arc by its chord.
    let solid = body.deep_copy();
    let tiny = Body::unique_edges(&solid)
        .into_iter()
        .find(|e| {
            let (a, b) = (from_p3(e.front().point()), from_p3(e.back().point()));
            a.z.abs() < 1e-9 && b.z.abs() < 1e-9 && a.dist(b) < 0.001 && a.x.hypot(a.y) > 29.
        })
        .unwrap();
    let chord = mt::builder::line(tiny.absolute_front(), tiny.absolute_back());
    let subst = std::collections::HashMap::from([(tiny.id(), chord)]);
    // Boolean trim pieces retain the very same underlying support. Give the
    // separately authored profile arcs that common support for this fixture.
    let support = |radius: f64, convex: bool| {
        let (a, b) = if convex { (10., 0.) } else { (0., 10.) };
        let line = mt::Curve::Line(mt::Line(p3(Vec3::new(radius, 0., a)), p3(Vec3::new(radius, 0., b))));
        mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(line, p3(Vec3::ZERO), mt::Vector3::unit_z())))
    };
    let faces: Vec<_> = solid
        .face_iter()
        .map(|f| {
            let f = crate::blend::rebuild_face(f, &subst, &[]).unwrap();
            if matches!(f.surface(), mt::Surface::Plane(_)) {
                return f;
            }
            let points: Vec<_> = f.vertex_iter().map(|v| from_p3(v.point())).collect();
            let radius = if points.iter().all(|p| p.x.hypot(p.y) > 29.) { 30. } else { 25. };
            mt::Face::try_new(f.boundaries(), support(radius, radius == 30.)).unwrap()
        })
        .collect();
    let body = Body::new_placed(crate::body::Solid::try_new(vec![faces.into()]).unwrap(), false, None, true).unwrap();
    let body = crate::boolean_record::operand(&body, 0, &[(body.face_count(), body.edge_count())]);
    let healed = crate::blend_heal::coalesce(&body, &[Vec3::new(30., 0., 10.), Vec3::new(25., 0., 10.)]).unwrap();
    assert_eq!(healed.face_count(), 6);
    assert_eq!(healed.edge_count(), 12);
    assert!(healed.validity().is_empty(), "{:?}", healed.validity());
    let volume = crate::measure(&healed).unwrap().volume;
    assert!((volume - 2750.0).abs() < 0.3, "{volume}");
    assert!(healed.history().is_some());
    let split_at = Vec3::new(30. * (-0.2_f64).cos(), 30. * (-0.2_f64).sin(), 0.);
    let split = body.clone().with_split_lines(Some(std::sync::Arc::new(vec![vec![split_at, split_at + Vec3::Z * 10.]])));
    let kept = crate::blend_heal::coalesce(&split, &[Vec3::new(30., 0., 10.), Vec3::new(25., 0., 10.)]).unwrap();
    assert_eq!(kept.face_count(), 7, "an authored cylinder seam must survive");
    assert!(kept.validity().is_empty());
    let round = fillet(&body, &[Vec3::new(30.0, 0.0, 10.0)], 1.0).unwrap();
    assert!(round.validity().is_empty(), "{:?}", round.validity());
    assert!(crate::measure(&round).unwrap().volume < volume);
}

#[test]
fn flat_and_nearly_flat_fillet_seams_have_a_distinct_error() {
    use crate::extrude;
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    for rise in [0., 0.001] {
        let points = [Vec2::new(0., 0.), Vec2::new(20., 0.), Vec2::new(20., 10.), Vec2::new(10., 10. + rise), Vec2::new(0., 10.)];
        let outer = Loop2 { segs: (0..points.len()).map(|i| Seg2::Line { a: points[i], b: points[(i + 1) % points.len()] }).collect() };
        let body = extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0., 10.).unwrap().remove(0);
        crate::blend::ROLLS.with(|n| n.set(0));
        let error = fillet(&body, &[Vec3::new(10., 10. + rise, 5.)], 1.).unwrap_err();
        assert!(matches!(error, KernelError::Invalid(_)), "{error}");
        assert!(error.to_string().contains("approximately 180 degrees"), "{error}");
        assert!(!error.to_string().contains("mixes convex"));
        assert_eq!(crate::blend::ROLLS.with(|n| n.get()), 0);
    }
}

#[test]
fn fillet_selection_preflight_rejects_hostile_points() {
    let body = crate::box_solid(Vec3::ZERO, Vec3::new(20., 20., 10.)).unwrap();
    for points in
        [vec![], vec![Vec3::new(f64::NAN, 0., 0.)], vec![Vec3::new(f64::INFINITY, 0., 0.)], vec![Vec3::ZERO; 1001], vec![Vec3::new(1e10, 0., 0.)]]
    {
        assert!(crate::check_fillet_edges(&body, &points).is_err());
    }
}

/// The bottom rim is concave, the two wall corners and top rim convex. They
/// round together by following the new base fillet's end arcs, not by treating
/// mixed sense alone as an error.
#[test]
fn a_cylindrical_face_can_round_its_convex_and_concave_edges_together() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let at = |r: f64, a: f64| Vec2::new(r * a.cos(), r * a.sin());
    let outer = Loop2 {
        segs: vec![
            Seg2::Arc { center: Vec2::ZERO, radius: 30., start: -1., sweep: 2. },
            Seg2::Line { a: at(30., 1.), b: at(25., 1.) },
            Seg2::Arc { center: Vec2::ZERO, radius: 25., start: 1., sweep: -2. },
            Seg2::Line { a: at(25., -1.), b: at(30., -1.) },
        ],
    };
    let post = crate::extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 5., 25.).unwrap().remove(0);
    let base = crate::box_solid(Vec3::new(-40., -40., 0.), Vec3::new(40., 40., 10.)).unwrap();
    let body = boolean(&base, &post, BoolOp::Union).unwrap().unwrap();
    let points = [
        Vec3::new(30., 0., 10.),
        Vec3::new(30., 0., 30.),
        Vec3::new(30. * 1_f64.cos(), 30. * 1_f64.sin(), 20.),
        Vec3::new(30. * 1_f64.cos(), -30. * 1_f64.sin(), 20.),
    ];
    for picks in [points.to_vec(), points.into_iter().rev().collect()] {
        let out = fillet(&body, &picks, 2.).unwrap();
        assert!(out.validity().is_empty(), "{:?}", out.validity());
        assert_eq!(out.crossed_face_loops().unwrap(), 0);
        assert!(crate::tess::boundaries_on_surfaces(&out.solid, out.size() * 1e-4));
        assert!(out.face_count() > body.face_count());
    }
}

/// The index must select the *same sample*, including ties, as the old scan;
/// the sample is only a seed for the unchanged exact Newton projection.
#[test]
fn spatial_projection_seeds_match_cold_grids_and_preserve_surface_changes() {
    use mt::{InnerSpace, Invertible, Transformed};
    let control: Vec<Vec<_>> =
        (0..4).map(|i| (0..4).map(|j| mt::Point3::new(i as f64 * 2.0, j as f64 * 3.0, 0.13 * (i * j) as f64)).collect()).collect();
    let spline = mt::BSplineSurface::new((mt::KnotVec::bezier_knot(3), mt::KnotVec::bezier_knot(3)), control.clone());
    let rational = mt::NurbsSurface::new(mt::BSplineSurface::new(
        (mt::KnotVec::bezier_knot(3), mt::KnotVec::bezier_knot(3)),
        control
            .iter()
            .enumerate()
            .map(|(i, row)| {
                row.iter()
                    .enumerate()
                    .map(|(j, p)| {
                        let w = 1.0 + (i + j) as f64 * 0.17;
                        mt::Vector4::new(p.x * w, p.y * w, p.z * w, w)
                    })
                    .collect()
            })
            .collect(),
    ));
    let profile = mt::Curve::Line(mt::Line(mt::Point3::new(4.0, 0.0, -2.0), mt::Point3::new(4.0, 0.0, 9.0)));
    let mut revolved =
        mt::Processor::new(mt::RevolutedCurve::by_revolution(profile, mt::Point3::new(0.0, 0.0, 0.0), mt::Vector3::new(0.0, 0.0, 1.0)));
    revolved.transform_by(mt::Matrix4::new(1.2, 0.0, 0.0, 0.0, 0.3, 0.8, 0.0, 0.0, 0.0, 0.0, -1.1, 0.0, 2.0, -3.0, 4.0, 1.0));
    let mut changed = spline.clone();
    for p in changed.control_points_mut() {
        p.x += 2.125;
        p.z -= 0.75;
    }
    let mut surfaces = vec![
        mt::Surface::BSplineSurface(spline),
        mt::Surface::NurbsSurface(rational),
        mt::Surface::RevolutedCurve(revolved),
        mt::Surface::BSplineSurface(changed),
    ];
    // Reversed charts swap the revolved parameter axes; the cache cannot alias
    // these with the forward surface or with a modified geometry snapshot.
    surfaces.extend(surfaces.clone().into_iter().map(|mut s| {
        s.invert();
        s
    }));
    for surface in surfaces.iter().chain(surfaces.iter().rev()) {
        let (Some(ur), Some(vr)) = surface.try_range_tuple() else { panic!("bounded fixture") };
        let division = if matches!(surface, mt::Surface::RevolutedCurve(_)) { 100 } else { 50 };
        for k in 0..12 {
            let u = ur.0 + (ur.1 - ur.0) * f64::from(k) / 11.0;
            let v = vr.0 + (vr.1 - vr.0) * f64::from(11 - k) / 11.0;
            let on = surface.subs(u, v);
            let n = surface.normal(u, v);
            let p = on + n * 0.23;
            let reference = mt::algo::surface::presearch(surface, p, (ur, vr), division);
            assert_eq!(mt::projection_seed(surface, p), Some(reference));
            let expected = mt::algo::surface::search_nearest_parameter(surface, p, reference, 100);
            let actual = surface.search_nearest_parameter(p, None, 100);
            match (expected, actual) {
                (Some(a), Some(b)) => assert!((surface.subs(a.0, a.1) - surface.subs(b.0, b.1)).magnitude() < 1e-9),
                (None, None) => {}
                other => panic!("projection changed: {other:?}"),
            }
        }
    }
    // A rank-deficient patch has many identical grid points. Tie handling must
    // retain the first grid index instead of depending on tree traversal order.
    let tied = mt::Surface::BSplineSurface(mt::BSplineSurface::new(
        (mt::KnotVec::bezier_knot(1), mt::KnotVec::bezier_knot(1)),
        vec![vec![mt::Point3::new(0.0, 0.0, 0.0); 2]; 2],
    ));
    assert_eq!(mt::projection_seed(&tied, mt::Point3::new(1.0, 2.0, 3.0)), Some((0.0, 0.0)));
    assert!(mt::projection_seed(&tied, mt::Point3::new(f64::NAN, 0.0, 0.0)).is_none());
    let mut invalid = control;
    invalid[0][0].x = f64::NAN;
    let invalid = mt::Surface::BSplineSurface(mt::BSplineSurface::new((mt::KnotVec::bezier_knot(3), mt::KnotVec::bezier_knot(3)), invalid));
    assert!(mt::projection_seed(&invalid, mt::Point3::new(1.0, 2.0, 3.0)).is_none());
    let oversized = mt::Surface::BSplineSurface(mt::BSplineSurface::new(
        (mt::KnotVec::bezier_knot(1), mt::KnotVec::uniform_knot(1, 5999)),
        vec![vec![mt::Point3::new(0.0, 0.0, 0.0); 6000]; 2],
    ));
    assert!(mt::projection_seed(&oversized, mt::Point3::new(1.0, 2.0, 3.0)).is_none());
    std::thread::scope(|scope| {
        for surface in &surfaces {
            scope.spawn(move || {
                let (Some(ur), Some(vr)) = surface.try_range_tuple() else { panic!("bounded fixture") };
                for k in 0..4 {
                    let p = mt::Point3::new(f64::from(k), 1.23, -0.73);
                    let division = if matches!(surface, mt::Surface::RevolutedCurve(_)) { 100 } else { 50 };
                    assert_eq!(mt::projection_seed(surface, p), Some(mt::algo::surface::presearch(surface, p, (ur, vr), division)));
                }
            });
        }
    });
}
