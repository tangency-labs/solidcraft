//! Independent analytic reference: the centre of a ball tangent to two perpendicular
//! cylinders lies on the intersection of their radius-offset cylinders.

use mt::{BoundedCurve, ParametricCurve, ParametricSurface, ParametricSurface3D, SearchNearestParameter};

use super::*;
use crate::blend::plane_normal;
use crate::issue_851_tests::planned_volume;
use crate::{Body, BoolOp, boolean, box_solid, cylinder, fillet};

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

/// The area and the first moments (about the corner's two faces) of a round's cross-section on a
/// square corner, the unit square less the quarter disc of radius 1 about (1, 1), sampled on a
/// fine grid: `f(s, h)` integrated over it, `s` the distance from one face and `h` from the other.
fn round_section_integral(f: impl Fn(f64, f64) -> f64) -> f64 {
    let n = 2000;
    let step = 1.0 / n as f64;
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n {
            let (s, h) = ((i as f64 + 0.5) * step, (j as f64 + 0.5) * step);
            if (1.0 - s).powi(2) + (1.0 - h).powi(2) >= 1.0 {
                sum += f(s, h);
            }
        }
    }
    sum * step * step
}

#[test]
fn round_an_open_rim_after_end_chamfers() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    // The rim of #1303, its inner top edge rounded where the chamfer was: each end of the chain
    // meets two bevels at a four-face vertex, and the round crosses the seam between them.
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
    let plain = crate::extrude(&Plane::XY, &[rim], 0.0, 13.0).unwrap().pop().unwrap();
    let bevelled = |size: f64| {
        let ends = [Vec3::new(31.0, 13.375, 13.0), Vec3::new(31.0, -13.375, 13.0), Vec3::new(31.0, 11.75, 8.0), Vec3::new(31.0, -11.75, 8.0)];
        crate::blend_corner::chamfer(&plain, &ends, size).unwrap()
    };
    let body = bevelled(1.0);
    let picks = [Vec3::new(-11.75, 0.0, 13.0), Vec3::new(15.0, 11.75, 13.0), Vec3::new(15.0, -11.75, 13.0)];
    let result = fillet(&body, &picks, 1.0).unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.crossed_face_loops().unwrap(), 0);
    // A point `s` from the inner wall and `h` below the top is taken away along the two lines
    // up to the nearer bevel (x + z = 43 above, x - y = 18.25 beside: x = 30 + min(s, h)) and
    // round the half ring at radius 11.75 + s.
    // The mesh inside the round takes slightly more than the round does: measured on the same rim
    // rounded without bevels, its square ends exact, and taken off.
    let along = round_section_integral(|s, h| 2.0 * (30.0 + s.min(h)) + std::f64::consts::PI * (11.75 + s));
    let square = round_section_integral(|s, _| 2.0 * 31.0 + std::f64::consts::PI * (11.75 + s));
    let bias = planned_volume(&plain) - planned_volume(&fillet(&plain, &picks, 1.0).unwrap()) - square;
    let removed = planned_volume(&body) - planned_volume(&result) - bias;
    assert!((removed - along).abs() < 0.003, "removed {removed}, expected {along} (mesh bias {bias})");
    // Each end meets the seam between the bevels where the round crosses it.
    for side in [1.0, -1.0] {
        let crossing = result.solid.vertex_iter().map(|v| from_p3(v.point())).find(|p| {
            let (s, h) = (p.y * side - 11.75, 13.0 - p.z);
            (p.x - (30.0 + s)).abs() < 1e-6 && (h - s).abs() < 1e-6 && s > 0.0 && s < 1.0
        });
        let crossing = crossing.unwrap_or_else(|| panic!("no vertex where the round crosses the bevels' seam on the {side} side"));
        // On the round: 1 from the axis through (y 12.75, z 12).
        let (s, h) = (crossing.y * side - 11.75, 13.0 - crossing.z);
        assert!(((1.0 - s).hypot(1.0 - h) - 1.0).abs() < 1e-5, "{crossing:?}");
    }
    assert!(fillet(&body, &picks, 4.0).is_err());
    // Bevels narrower than where the round meets their seam (0.3 against 1 - 1/sqrt 2 of 1.5):
    // the round would cross into the end face beyond them. Refused, not built across it.
    let refused = fillet(&bevelled(0.3), &picks, 1.5).unwrap_err().to_string();
    assert!(refused.contains("split round end"), "{refused}");
}

/// A plate (z 0 to 2) with an L-shaped wall standing on it (z 2 to 6), the wall's outer corner
/// rounded to radius 1: the wall's ends are flush with the plate's sides beneath them, and the
/// plate's sides run on slanted from there.
fn wall_on_a_plate() -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let polygon = |pts: &[(f64, f64)]| Region2 {
        outer: Loop2 {
            segs: pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect(),
        },
        holes: vec![],
    };
    let plate = polygon(&[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (28.0, -4.0), (28.0, 10.0), (12.0, 10.0), (0.0, 2.0)]);
    let wall = polygon(&[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (20.0, 2.0), (0.0, 2.0)]);
    let plate = crate::extrude(&Plane::XY, &[plate], 0.0, 2.0).unwrap().pop().unwrap();
    let wall = crate::extrude(&Plane::XY, &[wall], 2.0, 6.0).unwrap().pop().unwrap();
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    fillet(&body, &[Vec3::new(20.0, 2.0, 4.0)], 1.0).unwrap()
}

#[test]
fn round_the_foot_of_a_wall_flush_with_the_plate_sides() {
    // The foot of the wall (a line, the round at its corner and a line) rounded where it meets
    // the plate: each end of the chain is a vertex of four faces, the wall's end above the
    // plate's side flush with it and the plate's slanted side beyond, and the round ends in the
    // slanted side (the wall's end is where the round touches the wall).
    let body = wall_on_a_plate();
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let picks = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)];
    let r = 0.5;
    let result = fillet(&body, &picks, r).unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.crossed_face_loops().unwrap(), 0);
    let added = foot_round_added(r);
    let grown = planned_volume(&result) - planned_volume(&body);
    assert!((grown - added).abs() < 0.002, "added {grown}, expected {added}");
    // The round touches the wall's end at its edge with the plate's side beneath: one vertex
    // there, at the contact (y 2, z 2 + r at x 0; x 20, z 2 + r at y -20).
    for p in [Vec3::new(0.0, 2.0, 2.0 + r), Vec3::new(20.0, -20.0, 2.0 + r)] {
        let ids: std::collections::HashSet<_> = result.solid.vertex_iter().filter(|v| from_p3(v.point()).dist(p) < 1e-6).map(|v| v.id()).collect();
        assert_eq!(ids.len(), 1, "{p:?}");
    }
    // The old corner vertices are gone.
    for p in [Vec3::new(0.0, 2.0, 2.0), Vec3::new(20.0, -20.0, 2.0)] {
        assert!(!result.solid.vertex_iter().any(|v| from_p3(v.point()).dist(p) < 1e-6), "{p:?}");
    }
    // As wide as the wall is high less one: still a valid body. Higher than the wall: refused.
    let wide = fillet(&body, &picks, 3.0).unwrap();
    assert!(wide.validity().is_empty(), "{:?}", wide.validity());
    assert_eq!(wide.crossed_face_loops().unwrap(), 0);
    assert!(fillet(&body, &picks, 4.5).is_err());
}

#[test]
fn round_the_plate_top_edge_up_to_the_wall_ends() {
    // The plate's outer top edge, from one end of the wall to the other, rounded once its
    // corners are round: a convex chain whose ends meet the wall standing on the plate. The
    // seam between the wall's face and its end runs up from the chain's end, in the plane of the
    // plate's side: the round meets it at the side's contact, and the wall's face runs on down to
    // the round (the plate beneath the wall is kept).
    let rounded = 3.0;
    let mut body = wall_on_a_plate();
    let outline =
        [Vec3::new(0.0, 2.0, 0.0), Vec3::new(12.0, 10.0, 0.0), Vec3::new(28.0, 10.0, 0.0), Vec3::new(28.0, -4.0, 0.0), Vec3::new(20.0, -20.0, 0.0)];
    body = fillet(&body, &outline[1..4].iter().map(|p| *p + Vec3::new(0.0, 0.0, 1.0)).collect::<Vec<_>>(), rounded).unwrap();
    // Each corner: its turn, the setback of its round along both lines and the arc's middle.
    let dir = |a: Vec3, b: Vec3| (b - a).normalized().unwrap();
    let corners: Vec<(f64, f64, Vec3)> = (1..4)
        .map(|i| {
            let (a, b) = (dir(outline[i - 1], outline[i]), dir(outline[i], outline[i + 1]));
            let turn = a.dot(b).clamp(-1.0, 1.0).acos();
            let inward = (b - a).normalized().unwrap();
            let centre = outline[i] + inward * (rounded / (std::f64::consts::FRAC_PI_2 - turn / 2.0).sin());
            (turn, rounded * (turn / 2.0).tan(), centre - inward * rounded)
        })
        .collect();
    let top = Vec3::new(0.0, 0.0, 2.0);
    let mut picks: Vec<Vec3> = outline.windows(2).map(|w| (w[0] + w[1]) * 0.5 + top).collect();
    picks.extend(corners.iter().map(|c| c.2 + top));
    let r = 0.5;
    let result = fillet(&body, &picks, r).unwrap();
    assert!(result.validity().is_empty(), "{:?}", result.validity());
    assert_eq!(result.crossed_face_loops().unwrap(), 0);
    let removed = edge_round_removed(r);
    // The mesh inside the round's curve takes a few thousandths more; ends not cut back by the
    // wall at all would be 0.02 off.
    let lost = planned_volume(&body) - planned_volume(&result);
    assert!((lost - removed).abs() < 0.006, "removed {lost}, expected {removed}");
    // The round meets the seam up the wall's corner at the plate side's contact, 0.5 below the
    // plate's top: one vertex there at each end, and the old corners are gone.
    for p in [Vec3::new(0.0, 2.0, 2.0 - r), Vec3::new(20.0, -20.0, 2.0 - r)] {
        let ids: std::collections::HashSet<_> = result.solid.vertex_iter().filter(|v| from_p3(v.point()).dist(p) < 1e-6).map(|v| v.id()).collect();
        assert_eq!(ids.len(), 1, "{p:?}");
    }
    for p in [Vec3::new(0.0, 2.0, 2.0), Vec3::new(20.0, -20.0, 2.0)] {
        assert!(!result.solid.vertex_iter().any(|v| from_p3(v.point()).dist(p) < 1e-6), "{p:?}");
    }
    // Deeper than the plate is thick (its corners' rounds would take 3): refused at the ends.
    let refused = fillet(&body, &picks, 2.1).unwrap_err().to_string();
    assert!(refused.contains("larger than the neighbouring faces"), "{refused}");
}

/// The foot of the wall in [`wall_on_a_plate`] rounded `r` (line, the corner's round, line).
fn round_the_foot(body: &Body, r: f64) -> crate::Result<Body> {
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    fillet(body, &[Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)], r)
}

/// What the round of radius `r` at the foot of the wall in [`wall_on_a_plate`] adds. A point `s`
/// from the wall and `h` above the plate (in radii) is added along the lines from where the
/// slanted side passes it (x = 1.5 s r at the first end, y = -20 + 2 s r at the other) to the
/// corner's round, and round its quarter turn at radius 1 + s r.
fn foot_round_added(r: f64) -> f64 {
    round_section_integral(|s, _| {
        let (x0, y0) = (1.5 * s * r, -20.0 + 2.0 * s * r);
        (19.0 - x0) + (1.0 - y0) + std::f64::consts::FRAC_PI_2 * (1.0 + s * r)
    }) * r
        * r
}

/// What the round of radius `r` along the plate's top edge of [`plate_with_round_corners`] takes
/// away. A point `s` in from the plate's side (and `h` below its top) is taken away along the
/// lines between the corners' rounds and round each corner's turn at the radius `3 - s`, less
/// where the wall stands: the plane of its face cuts the run back by 1.5 s at the first end (the
/// line along (12, 8) meeting y = 2) and 2 s at the other (along (8, 16) meeting x = 20).
fn edge_round_removed(r: f64) -> f64 {
    let rounded = 3.0;
    let outline =
        [Vec3::new(0.0, 2.0, 0.0), Vec3::new(12.0, 10.0, 0.0), Vec3::new(28.0, 10.0, 0.0), Vec3::new(28.0, -4.0, 0.0), Vec3::new(20.0, -20.0, 0.0)];
    let dir = |a: Vec3, b: Vec3| (b - a).normalized().unwrap();
    // Each corner's turn and the setback of its round along both lines.
    let corners: Vec<(f64, f64)> = (1..4)
        .map(|i| {
            let turn = dir(outline[i - 1], outline[i]).dot(dir(outline[i], outline[i + 1])).clamp(-1.0, 1.0).acos();
            (turn, rounded * (turn / 2.0).tan())
        })
        .collect();
    let lines: f64 = outline
        .windows(2)
        .enumerate()
        .map(|(i, w)| {
            let back = |k: usize| if (1..4).contains(&k) { corners[k - 1].1 } else { 0.0 };
            w[0].dist(w[1]) - back(i) - back(i + 1)
        })
        .sum();
    let turns: f64 = corners.iter().map(|c| c.0).sum();
    round_section_integral(|s, _| {
        let s = s * r;
        lines + turns * (rounded - s) - 3.5 * s
    }) * r
        * r
}

#[test]
fn round_the_foot_of_a_wall_as_high_as_its_end_edge() {
    // The wall's end chamfered at the top: the edge between its face and its end runs from the
    // plate up to 5. A round of 3 reaches its far end exactly there (the wall's end flush with the
    // plate's side: the round meets that edge at its contact), using the edge up.
    let body = crate::blend_corner::chamfer(&wall_on_a_plate(), &[Vec3::new(0.0, 1.0, 6.0)], 1.0).unwrap();
    for r in [2.999, 3.0] {
        let result = round_the_foot(&body, r).unwrap_or_else(|e| panic!("r {r}: {e}"));
        assert!(result.validity().is_empty(), "r {r}: {:?}", result.validity());
        assert_eq!(result.crossed_face_loops().unwrap(), 0, "r {r}");
    }
    assert!(round_the_foot(&body, 3.001).is_err());
}

/// [`wall_on_a_plate`] with the plate's sides drafted by `angle` (about the plate's top, leaning
/// outward for a positive angle), the wall's corner rounded 1.
fn wall_on_a_drafted_plate(angle: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let polygon = |pts: &[(f64, f64)]| Region2 {
        outer: Loop2 {
            segs: pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect(),
        },
        holes: vec![],
    };
    let plate = polygon(&[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (28.0, -4.0), (28.0, 10.0), (12.0, 10.0), (0.0, 2.0)]);
    let wall = polygon(&[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (20.0, 2.0), (0.0, 2.0)]);
    let mut plate = crate::extrude(&Plane::XY, &[plate], 0.0, 2.0).unwrap().pop().unwrap();
    let neutral = Plane { origin: Vec3::new(0.0, 0.0, 2.0), x: Vec3::X, y: Vec3::Y };
    plate = crate::draft(&plate, &[Vec3::new(6.0, 6.0, 1.0), Vec3::new(24.0, -12.0, 1.0)], &neutral, -Vec3::Z, angle).unwrap();
    let wall = crate::extrude(&Plane::XY, &[wall], 2.0, 6.0).unwrap().pop().unwrap();
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    fillet(&body, &[Vec3::new(20.0, 2.0, 4.0)], 1.0).unwrap()
}

#[test]
fn round_the_foot_of_a_wall_over_drafted_plate_sides() {
    // Drafted 10 degrees, the plate's sides no longer hold the line of the seam up the wall's end:
    // the round crosses it inside, past the vertex, and the seam runs on to it. Next to nothing
    // (4e-5 degrees, a modelling error) and the seam is all but in the side's plane: still a
    // round, at small radii and large.
    for (degrees, radii) in [(10.0, [0.5, 2.0, 3.5]), (4e-5, [0.05, 0.5, 2.0])] {
        let body = wall_on_a_drafted_plate(f64::to_radians(degrees));
        for r in radii {
            let result = round_the_foot(&body, r).unwrap_or_else(|e| panic!("draft {degrees} r {r}: {e}"));
            assert!(result.validity().is_empty(), "draft {degrees} r {r}: {:?}", result.validity());
            assert_eq!(result.crossed_face_loops().unwrap(), 0, "draft {degrees} r {r}");
            assert!(planned_volume(&result) > planned_volume(&body), "draft {degrees} r {r}");
        }
    }
}

/// [`wall_on_a_plate`] with the plate's three outer corners rounded 3, and points on the lines of
/// the plate's outer top edge from one end of the wall to the other.
fn plate_with_round_corners() -> (Body, Vec<Vec3>) {
    let outline =
        [Vec3::new(0.0, 2.0, 0.0), Vec3::new(12.0, 10.0, 0.0), Vec3::new(28.0, 10.0, 0.0), Vec3::new(28.0, -4.0, 0.0), Vec3::new(20.0, -20.0, 0.0)];
    let body = fillet(&wall_on_a_plate(), &outline[1..4].iter().map(|p| *p + Vec3::new(0.0, 0.0, 1.0)).collect::<Vec<_>>(), 3.0).unwrap();
    (body, outline.windows(2).map(|w| (w[0] + w[1]) * 0.5 + Vec3::new(0.0, 0.0, 2.0)).collect())
}

/// The number of edges in the smooth chain through the edge of `body` nearest `p`.
fn chain_at(body: &Body, p: Vec3) -> usize {
    let all = Body::unique_edges(&body.solid);
    let (i, _) = body.nearest_edge(p, 1e-3).unwrap().unwrap();
    crate::blend::smooth_chain(&body.solid, &all[i]).unwrap().len()
}

#[test]
fn a_chain_ends_at_the_end_of_an_earlier_round_of_the_other_kind() {
    // Each round's end runs on smoothly from the other round's edge: the plate's top edge into the
    // end of the round at the wall's foot (up the plate's side and on up the wall's end), the foot
    // into the end of the round along the plate's edge (down the wall's face and on down the
    // plate's side). The rolling ball does not run on there: the chains end where they meet the
    // earlier round, as they did before it.
    let (body, outline) = plate_with_round_corners();
    let foot = Vec3::new(10.0, 2.0, 2.0);
    // The foot: its two lines and the wall corner's round. The plate's edge: four lines and the
    // three corners' rounds.
    assert_eq!(chain_at(&body, foot), 3);
    assert_eq!(chain_at(&body, outline[0]), 7);
    for r in [0.5, 1.0] {
        let footed = round_the_foot(&body, r).unwrap();
        assert_eq!(chain_at(&footed, outline[0]), 7, "r {r}");
        let edged = fillet(&body, &outline, r).unwrap();
        assert_eq!(chain_at(&edged, foot), 3, "r {r}");
    }
}

#[test]
fn a_chain_runs_on_into_an_earlier_round_of_its_own_kind() {
    // Untouched bodies keep their chains: the wall's foot, and a box's top loop round its rounded
    // corners. A top edge of a box runs on over the end of an earlier round along the next top edge
    // (both take material away) and down the box's corner.
    let foot = Vec3::new(10.0, 2.0, 2.0);
    assert_eq!(chain_at(&wall_on_a_plate(), foot), 3);
    let block = box_solid(Vec3::ZERO, Vec3::new(10.0, 8.0, 6.0)).unwrap();
    let corners = [Vec3::new(0.0, 0.0, 3.0), Vec3::new(10.0, 0.0, 3.0), Vec3::new(10.0, 8.0, 3.0), Vec3::new(0.0, 8.0, 3.0)];
    assert_eq!(chain_at(&fillet(&block, &corners, 1.0).unwrap(), Vec3::new(5.0, 0.0, 6.0)), 8);
    let edged = fillet(&block, &[Vec3::new(5.0, 0.0, 6.0)], 1.0).unwrap();
    assert_eq!(chain_at(&edged, Vec3::new(0.0, 4.0, 6.0)), 3);
}

/// A wall end of [`wall_on_a_plate`] (and of the bracket at the user's size): `V`, the wall's trace
/// on the plate leaving it, square to that into the plate's top, and tan of the top's angle there
/// (between the wall's trace and the plate's side: negative past 90°, infinite at it).
type WallEnd = (Vec3, Vec3, Vec3, f64);

fn wall_ends() -> [WallEnd; 2] {
    [(Vec3::new(0.0, 2.0, 2.0), Vec3::X, Vec3::Y, 2.0 / 3.0), (Vec3::new(20.0, -20.0, 2.0), Vec3::Y, Vec3::X, 0.5)]
}

/// The vertices of the merge corner of the foot's round `ra` and the plate edge's round `rb` at a
/// wall end (`P*`, where the two contacts on the plate's top cross; `q_W` and `q_S` on the line of
/// the wall's end, `ra` above and `rb` below the top), and the points where each round alone ended
/// and the merge has no vertex (`V`; `P'`, where the foot's contact meets the plate's side;
/// `P''`, where the edge's contact meets the wall's trace).
fn merge_points((v, w, m, tan): WallEnd, ra: f64, rb: f64) -> ([Vec3; 3], [Vec3; 3]) {
    let theta = if tan < 0.0 { std::f64::consts::PI + tan.atan() } else { tan.atan() };
    let (sin, cot) = (theta.sin(), theta.cos() / theta.sin());
    let p_star = v + w * (rb / sin + ra * cot) + m * ra;
    ([p_star, v + Vec3::Z * ra, v - Vec3::Z * rb], [v, v + w * (ra * cot) + m * ra, v + w * (rb / sin)])
}

/// The distinct vertices of `body` within `tol` of `p`.
fn vertices_at(body: &Body, p: Vec3, tol: f64) -> usize {
    body.solid.vertex_iter().filter(|v| from_p3(v.point()).dist(p) < tol).map(|v| v.id()).collect::<std::collections::HashSet<_>>().len()
}

/// A merged body: sound, with one vertex at each of `P*`, `q_W` and `q_S` at both ends, and none
/// where either round alone ended.
fn assert_merged(body: &Body, ends: &[WallEnd], (ra, rb): (f64, f64), tol: f64, how: &str) {
    assert!(body.validity().is_empty(), "{how}: {:?}", body.validity());
    assert_eq!(body.crossed_face_loops().unwrap(), 0, "{how}");
    for end in ends {
        let (corners, gone) = merge_points(*end, ra, rb);
        for p in corners {
            assert_eq!(vertices_at(body, p, tol), 1, "{how}: {p:?}");
        }
        for p in gone {
            assert_eq!(vertices_at(body, p, tol), 0, "{how}: {p:?}");
        }
    }
}

/// The edge of `body` from `a` to `b` (either way), and the two faces it joins.
fn edge_between(body: &Body, a: Vec3, b: Vec3, tol: f64) -> (mt::Edge, Vec<&mt::Face>) {
    let at = |v: &mt::Vertex, p: Vec3| from_p3(v.point()).dist(p) < tol;
    let edges: Vec<mt::Edge> = Body::unique_edges(&body.solid)
        .into_iter()
        .filter(|e| (at(e.front(), a) && at(e.back(), b)) || (at(e.front(), b) && at(e.back(), a)))
        .collect();
    assert_eq!(edges.len(), 1, "edges from {a:?} to {b:?}");
    let faces: Vec<&mt::Face> = body.solid.face_iter().filter(|f| f.edge_iter().any(|e| e.id() == edges[0].id())).collect();
    assert_eq!(faces.len(), 2);
    (edges[0].clone(), faces)
}

/// Points along the edge, its ends left out.
fn along_edge(e: &mt::Edge) -> Vec<Vec3> {
    let c = e.curve();
    let (lo, hi) = c.range_tuple();
    (1..16).map(|i| from_p3(c.subs(lo + (hi - lo) * f64::from(i) / 16.0))).collect()
}

/// The largest angle between the outward normals of the two faces along the edge from `a` to `b`.
fn crease(body: &Body, a: Vec3, b: Vec3, tol: f64) -> f64 {
    let (edge, faces) = edge_between(body, a, b, tol);
    along_edge(&edge)
        .into_iter()
        .map(|p| {
            let (n1, n2) = (project(faces[0], p, None).unwrap().1, project(faces[1], p, None).unwrap().1);
            n1.dot(n2).clamp(-1.0, 1.0).acos()
        })
        .fold(0.0, f64::max)
}

/// The area of the planar face of `body` with outward normal `n` through `at`, from its boundary
/// (finely sampled: its curved edges are straight or nearly so).
fn plane_face_area(body: &Body, n: Vec3, at: Vec3) -> f64 {
    let found: Vec<&mt::Face> = body
        .solid
        .face_iter()
        .filter(|f| plane_normal(f).is_some_and(|m| m.dot(n) > 1.0 - 1e-9) && project(f, at, None).is_some_and(|(q, _, _)| q.dist(at) < 1e-9))
        .collect();
    assert_eq!(found.len(), 1, "faces along {n:?} through {at:?}");
    let mut twice = 0.0;
    for w in found[0].boundaries() {
        let pts: Vec<Vec3> = w
            .edge_iter()
            .flat_map(|e| {
                let c = e.oriented_curve();
                let (lo, hi) = c.range_tuple();
                (0..256).map(move |i| from_p3(c.subs(lo + (hi - lo) * f64::from(i) / 256.0)))
            })
            .collect();
        twice += pts.iter().zip(pts.iter().cycle().skip(1)).map(|(p, q)| (*p - at).cross(*q - at).dot(n)).sum::<f64>();
    }
    0.5 * twice.abs()
}

/// `∫ f` over `[a, b]` by Simpson's rule on `n` (even) spans.
fn simpson(a: f64, b: f64, n: usize, f: impl Fn(f64) -> f64) -> f64 {
    let h = (b - a) / n as f64;
    (0..=n)
        .map(|i| {
            f(a + h * i as f64)
                * if i == 0 || i == n {
                    1.0
                } else if i % 2 == 1 {
                    4.0
                } else {
                    2.0
                }
        })
        .sum::<f64>()
        * h
        / 3.0
}

/// The volume the merge corner of the rounds `ra` (adding material, on the wall) and `rb` (taking
/// it away, along the plate's side) adds beyond the two rounds alone at a wall end where the
/// plate's top has the angle `theta` (negative: it takes away). Each section square to the
/// bisector of the top's corner, scaled across by cos(θ/2), holds the two rounds' sections as
/// circles: `A` about (ra, ra) on the wall's trace x = 0, `B` about (w − rb, −rb) on the side's
/// trace x = w. The merged profile runs along `A`, then along the two circles' other internal
/// tangent (the top is one), then along `B`; each round alone ends at the other's plane. Their
/// difference across the section, integrated over `w` from 0 to R = ra + rb, is the correction
/// with `dρ = dw/(2 sin(θ/2))` along the bisector and the areas unscaled by 1/cos(θ/2).
fn merge_correction(theta: f64, ra: f64, rb: f64) -> f64 {
    let r = ra + rb;
    // ∫ √(ρ² − x²) dx.
    let disc = |x: f64, rho: f64| {
        let x = x.clamp(-rho, rho);
        0.5 * (x * (rho * rho - x * x).sqrt() + rho * rho * (x / rho).asin())
    };
    // Under `A`'s lower arc and `B`'s upper arc, from x0 to x1 (B's for the section `w` wide).
    let under_a = |x0: f64, x1: f64| ra * (x1 - x0) - (disc(x1 - ra, ra) - disc(x0 - ra, ra));
    let under_b = |w: f64, x0: f64, x1: f64| -rb * (x1 - x0) + (disc(x1 - (w - rb), rb) - disc(x0 - (w - rb), rb));
    let added = |w: f64| {
        // The tangent's unit normal `n`, with n·(cA − cB) = R: the top's (0, 1), or the other.
        let (dx, dy) = (r - w, r);
        let (len, phi) = (dx.hypot(dy), dy.atan2(dx));
        let alpha = (r / len).clamp(-1.0, 1.0).acos();
        let n = [phi + alpha, phi - alpha]
            .map(|a| (a.cos(), a.sin()))
            .into_iter()
            .max_by(|x, y| x.0.hypot(x.1 - 1.0).total_cmp(&y.0.hypot(y.1 - 1.0)))
            .unwrap();
        let (a, b) = ((ra - ra * n.0, ra - ra * n.1), (w - rb + rb * n.0, -rb + rb * n.1));
        let merged = under_a(0.0, a.0) + 0.5 * (a.1 + b.1) * (b.0 - a.0) + under_b(w, b.0, w);
        let alone = under_a(0.0, ra.min(w)) + under_b(w, (w - rb).max(0.0), w);
        merged - alone
    };
    let mut cuts = [0.0, ra, rb, r];
    cuts.sort_by(f64::total_cmp);
    cuts.windows(2).map(|s| simpson(s[0], s[1], 2000, added)).sum::<f64>() / theta.sin()
}

#[test]
fn the_merge_correction_is_what_the_design_tabulates() {
    // Its own check of the reference: antisymmetric in the radii, nothing for equal ones, and the
    // design's table (computed by other means) to its last digit.
    let (first, second) = ((2.0f64 / 3.0).atan(), 0.5f64.atan());
    for (ra, rb, c1, c2) in [(0.5, 0.25, -0.00606, -0.00751), (1.0, 0.5, -0.04846, -0.06010), (2.0, 0.5, -0.3534, -0.4383)] {
        let digit = |x: f64| if x.abs() > 0.1 { 1e-4 } else { 1e-5 };
        assert!((merge_correction(first, ra, rb) - c1).abs() <= digit(c1), "{ra} {rb}: {}", merge_correction(first, ra, rb));
        assert!((merge_correction(second, ra, rb) - c2).abs() <= digit(c2), "{ra} {rb}: {}", merge_correction(second, ra, rb));
        assert!((merge_correction(first, ra, rb) + merge_correction(first, rb, ra)).abs() < 1e-9);
    }
    assert!(merge_correction(first, 0.5, 0.5).abs() < 1e-12);
}

#[test]
fn rounds_meeting_at_the_wall_ends_merge_alike_in_any_order() {
    // The round at the wall's foot (adding material) and the round along the plate's top edge
    // (taking it away) meet at both ends of the wall. One after the other either way round, in one
    // fillet or each at its own radius, they merge into the same band at each end: the foot's
    // round flows into the edge's through it, the wall's end keeps its face, and each round is as
    // it is alone outside the band.
    let (body, outline) = plate_with_round_corners();
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let foot = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)];
    let both: Vec<Vec3> = foot.iter().chain(&outline).copied().collect();
    let (first, second) = ((2.0f64 / 3.0).atan(), 0.5f64.atan());
    // The faces at each end: the wall's end, its face and the plate's side (outward normal and a
    // point inside each).
    let ends = [
        [
            (-Vec3::X, Vec3::new(0.0, 1.0, 1.0)),
            (Vec3::Y, Vec3::new(10.0, 2.0, 5.0)),
            (Vec3::new(-2.0, 3.0, 0.0) / 13f64.sqrt(), Vec3::new(6.0, 6.0, 1.0)),
        ],
        [
            (-Vec3::Y, Vec3::new(19.0, -20.0, 1.0)),
            (Vec3::X, Vec3::new(20.0, -10.0, 5.0)),
            (Vec3::new(2.0, -1.0, 0.0) / 5f64.sqrt(), Vec3::new(24.0, -12.0, 1.0)),
        ],
    ];
    let area = |b: &Body, (n, at): (Vec3, Vec3)| plane_face_area(b, n, at);
    for (ra, rb) in [(1.0, 0.5), (0.5, 1.0), (0.5, 0.5), (1.0, 1.0)] {
        let footed = round_the_foot(&body, ra).unwrap();
        let edged = fillet(&body, &outline, rb).unwrap();
        let radii: Vec<f64> = foot.iter().map(|_| ra).chain(outline.iter().map(|_| rb)).collect();
        let mut routes = vec![
            ("foot then edge", fillet(&footed, &outline, rb).unwrap()),
            ("edge then foot", round_the_foot(&edged, ra).unwrap()),
            ("each at its radius", crate::fillet_each(&body, &both, &radii).unwrap()),
        ];
        if ra == rb {
            routes.push(("together", fillet(&body, &both, ra).unwrap()));
        }
        // The earlier round's ends picked too (each from where its contact with the top meets the
        // side to the line of the wall's end), before the chain or after it: the merge takes them
        // up, and the corner is the same.
        let ends_of = |b: &Body, which: usize| -> Vec<Vec3> {
            wall_ends()
                .into_iter()
                .map(|end| {
                    let ([_, q_w, q_s], gone) = merge_points(end, ra, rb);
                    let q = if which == 1 { q_w } else { q_s };
                    crate::blend::edge_mid(&edge_between(b, gone[which], q, 1e-6).0).unwrap()
                })
                .collect()
        };
        let (caps_a, caps_b) = (ends_of(&footed, 1), ends_of(&edged, 2));
        let picks: Vec<Vec3> = caps_a.iter().chain(&outline).copied().collect();
        routes.push(("foot then edge, the foot's ends picked first", fillet(&footed, &picks, rb).unwrap()));
        let picks: Vec<Vec3> = foot.iter().chain(&caps_b).copied().collect();
        routes.push(("edge then foot, the edge's ends picked last", fillet(&edged, &picks, ra).unwrap()));
        // Tapering where the round doesn't fit has nothing to taper.
        let (tapered, tapers) = crate::fillet_tapered(&footed, &outline, rb).unwrap();
        assert!(tapers.iter().all(Option::is_none), "{ra} {rb}");
        routes.push(("tapered", tapered));
        // What the merge adds: each round alone, and the correction at each end.
        let correction = merge_correction(first, ra, rb) + merge_correction(second, ra, rb);
        let analytic = foot_round_added(ra) - edge_round_removed(rb) + correction;
        let (v_body, v_footed, v_edged) = (planned_volume(&body), planned_volume(&footed), planned_volume(&edged));
        let mut volumes = Vec::new();
        for (how, result) in &routes {
            let how = format!("{how}, foot {ra} edge {rb}");
            assert_merged(result, &wall_ends(), (ra, rb), 1e-6, &how);
            for (end, [e, w, s]) in wall_ends().into_iter().zip(ends) {
                // The wall's end keeps its area; its face is as the foot's round leaves it, the
                // plate's side as the edge's round leaves it.
                assert!((area(result, e) - area(&body, e)).abs() < 1e-9, "{how}: the wall's end");
                assert!((area(result, w) - area(&footed, w)).abs() < 1e-6, "{how}: the wall's face {} {}", area(result, w), area(&footed, w));
                assert!((area(result, s) - area(&edged, s)).abs() < 1e-6, "{how}: the plate's side {} {}", area(result, s), area(&edged, s));
                // The band joins both rounds smoothly, and stands upright along the wall's end.
                let ([p_star, q_w, q_s], _) = merge_points(end, ra, rb);
                for q in [q_w, q_s] {
                    let bend = crease(result, p_star, q, 1e-6);
                    assert!(bend < 1e-3, "{how}: {bend} rad between the band and the round to {q:?}");
                }
                let (on_l, faces) = edge_between(result, q_w, q_s, 1e-6);
                let band = faces.iter().find(|f| plane_normal(f).is_none()).unwrap();
                for p in along_edge(&on_l) {
                    let n = project(band, p, None).unwrap().1;
                    assert!(n.z.abs() < 1e-3, "{how}: the band's normal {n:?} at {p:?}");
                }
            }
            let v = planned_volume(result);
            // The mesh inside the rounds takes a few thousandths more (as for each alone).
            assert!((v - v_body - analytic).abs() < 0.008, "{how}: grown {}, expected {analytic}", v - v_body);
            // Against each round alone as meshed, the merge is the correction.
            let merged = v - (v_footed + v_edged - v_body);
            assert!((merged - correction).abs() < 0.002, "{how}: merged {merged}, expected {correction}");
            volumes.push(v);
            // Every route has its vertices where the first has them.
            assert!(same_vertices(result, &routes[0].1, 1e-6), "{how}: the vertices differ from {}'s", routes[0].0);
        }
        let v0 = volumes[0];
        assert!(volumes.iter().all(|v| (v - v0).abs() < 1e-6 * v0), "foot {ra} edge {rb}: {volumes:?}");
    }
}

/// The points of the distinct vertices of `body`.
fn vertex_points(body: &Body) -> Vec<Vec3> {
    let mut seen = std::collections::HashSet::new();
    body.solid.vertex_iter().filter(|v| seen.insert(v.id())).map(|v| from_p3(v.point())).collect()
}

/// Whether `a` and `b` have as many vertices, each of either within `tol` of one of the other's.
fn same_vertices(a: &Body, b: &Body, tol: f64) -> bool {
    let (pa, pb) = (vertex_points(a), vertex_points(b));
    let near = |p: &Vec3, ps: &[Vec3]| ps.iter().any(|q| q.dist(*p) < tol);
    pa.len() == pb.len() && pa.iter().all(|p| near(p, &pb)) && pb.iter().all(|p| near(p, &pa))
}

/// A prism over the polygon `pts` (in the plane z = 0 through `origin`), from `z0` to `z1` above.
fn prism_at(origin: Vec3, pts: &[(f64, f64)], z0: f64, z1: f64) -> Body {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let region = Region2 {
        outer: Loop2 {
            segs: pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect(),
        },
        holes: vec![],
    };
    crate::extrude(&Plane { origin, x: Vec3::X, y: Vec3::Y }, &[region], z0, z1).unwrap().pop().unwrap()
}

/// The volume through the display planner at chord tolerance `tol`, its mesh closed.
fn volume_at(b: &Body, tol: f64) -> f64 {
    let mesh = b.display_plan(tol).unwrap().evaluate();
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut uses: std::collections::HashMap<([u64; 3], [u64; 3]), i32> = std::collections::HashMap::new();
    for t in &mesh.triangles {
        let [a, b, c] = mesh.tri(t).unwrap();
        for (p, q) in [(a, b), (b, c), (c, a)] {
            *uses.entry((key(p), key(q))).or_default() += 1;
            *uses.entry((key(q), key(p))).or_default() -= 1;
        }
    }
    assert!(uses.values().all(|n| *n == 0), "the mesh has cracks");
    mesh.measure().volume
}

#[test]
fn the_merge_corner_at_a_hundredth_and_a_hundred_times_off_the_origin() {
    // [`plate_with_round_corners`] scaled and moved far from the origin: the same corner by every
    // route (either order, each at its radius, the earlier round's ends picked too) at every
    // scale, and with the plate's corners left sharp.
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let (first, second) = ((2.0f64 / 3.0).atan(), 0.5f64.atan());
    let (ra, rb) = (1.0, 0.5);
    let analytic = foot_round_added(ra) - edge_round_removed(rb) + merge_correction(first, ra, rb) + merge_correction(second, ra, rb);
    for k in [0.01, 100.0] {
        let origin = Vec3::new(37.0, -52.0, 13.0) * k;
        let at = |p: Vec3| origin + p * k;
        let plate = prism_at(
            origin,
            &[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (28.0, -4.0), (28.0, 10.0), (12.0, 10.0), (0.0, 2.0)]
                .map(|(x, y)| (x * k, y * k)),
            0.0,
            2.0 * k,
        );
        let wall = prism_at(
            origin,
            &[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (20.0, 2.0), (0.0, 2.0)].map(|(x, y)| (x * k, y * k)),
            2.0 * k,
            6.0 * k,
        );
        let mut body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
        body = fillet(&body, &[at(Vec3::new(20.0, 2.0, 4.0))], k).unwrap();
        let sharp = body.clone();
        body = fillet(&body, &[Vec3::new(12.0, 10.0, 1.0), Vec3::new(28.0, 10.0, 1.0), Vec3::new(28.0, -4.0, 1.0)].map(at), 3.0 * k).unwrap();
        let outline = [
            Vec3::new(0.0, 2.0, 2.0),
            Vec3::new(12.0, 10.0, 2.0),
            Vec3::new(28.0, 10.0, 2.0),
            Vec3::new(28.0, -4.0, 2.0),
            Vec3::new(20.0, -20.0, 2.0),
        ];
        let picks: Vec<Vec3> = outline.windows(2).map(|w| at((w[0] + w[1]) * 0.5)).collect();
        let foot = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)].map(at);
        let footed = fillet(&body, &foot, ra * k).unwrap();
        let edged = fillet(&body, &picks, rb * k).unwrap();
        let ends = wall_ends().map(|(v, w, m, tan)| (at(v), w, m, tan));
        let tol = 1e-6 * k;
        // The foot's ends, picked with the plate's edge.
        let caps = ends.map(|end| {
            let ([_, q_w, _], gone) = merge_points(end, ra * k, rb * k);
            crate::blend::edge_mid(&edge_between(&footed, gone[1], q_w, tol).0).unwrap()
        });
        let both: Vec<Vec3> = foot.iter().chain(&picks).copied().collect();
        let radii: Vec<f64> = foot.iter().map(|_| ra * k).chain(picks.iter().map(|_| rb * k)).collect();
        let routes = [
            ("foot then edge", fillet(&footed, &picks, rb * k).unwrap()),
            ("edge then foot", fillet(&edged, &foot, ra * k).unwrap()),
            ("each at its radius", crate::fillet_each(&body, &both, &radii).unwrap()),
            ("foot then edge, the foot's ends picked", fillet(&footed, &caps.iter().chain(&picks).copied().collect::<Vec<_>>(), rb * k).unwrap()),
        ];
        let mut volumes = Vec::new();
        for (how, result) in &routes {
            let how = format!("{how} at {k}x");
            assert_merged(result, &ends, (ra * k, rb * k), tol, &how);
            for end in ends {
                let ([p_star, q_w, q_s], _) = merge_points(end, ra * k, rb * k);
                for q in [q_w, q_s] {
                    let bend = crease(result, p_star, q, tol);
                    assert!(bend < 1e-3, "{how}: {bend} rad between the band and the round to {q:?}");
                }
            }
            let grown = (volume_at(result, 2e-4 * k) - volume_at(&body, 2e-4 * k)) / k.powi(3);
            assert!((grown - analytic).abs() < 0.008, "{how}: grown {grown}, expected {analytic}");
            volumes.push(grown);
            assert!(same_vertices(result, &routes[0].1, tol), "{how}: the vertices differ from {}'s", routes[0].0);
        }
        assert!(volumes.iter().all(|v| (v - volumes[0]).abs() < 1e-4), "{k}x: {volumes:?}");
        // The plate's corners left sharp: its straight top edges from the wall's ends round on
        // their own, and merge with the foot's round either way round.
        let tops = [Vec3::new(6.0, 6.0, 2.0), Vec3::new(24.0, -12.0, 2.0)].map(at);
        let edged = fillet(&sharp, &tops, rb * k).unwrap();
        assert!(edged.validity().is_empty(), "sharp at {k}x: {:?}", edged.validity());
        let a = fillet(&fillet(&sharp, &foot, ra * k).unwrap(), &tops, rb * k).unwrap();
        let b = fillet(&edged, &foot, ra * k).unwrap();
        for (how, result) in [("sharp, foot then edge", &a), ("sharp, edge then foot", &b)] {
            assert_merged(result, &ends, (ra * k, rb * k), tol, &format!("{how} at {k}x"));
        }
        let (va, vb) = (volume_at(&a, 2e-4 * k), volume_at(&b, 2e-4 * k));
        assert!((va - vb).abs() < 1e-6 * va, "sharp at {k}x: {va} {vb}");
        assert!(same_vertices(&a, &b, tol), "sharp at {k}x: the orders' vertices differ");
    }
}

/// The bracket at the user's size: a plate 10 thick, a wall 20 high on it with its outer corner
/// rounded 5 and its ends flush with the plate's sides, the plate's three outer corners rounded 10;
/// with points on the wall's foot and on the plate's top edge from one end of the wall to the other.
fn bracket() -> (Body, [Vec3; 2], Vec<Vec3>) {
    let plate = prism_at(
        Vec3::ZERO,
        &[(0.0, 0.0), (90.0, 0.0), (90.0, -100.0), (100.0, -100.0), (140.0, -20.0), (140.0, 50.0), (60.0, 50.0), (0.0, 10.0)],
        0.0,
        10.0,
    );
    let wall = prism_at(Vec3::ZERO, &[(0.0, 0.0), (90.0, 0.0), (90.0, -100.0), (100.0, -100.0), (100.0, 10.0), (0.0, 10.0)], 10.0, 30.0);
    let mut body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    body = fillet(&body, &[Vec3::new(100.0, 10.0, 20.0)], 5.0).unwrap();
    body = fillet(&body, &[Vec3::new(60.0, 50.0, 5.0), Vec3::new(140.0, 50.0, 5.0), Vec3::new(140.0, -20.0, 5.0)], 10.0).unwrap();
    let outline = [
        Vec3::new(0.0, 10.0, 10.0),
        Vec3::new(60.0, 50.0, 10.0),
        Vec3::new(140.0, 50.0, 10.0),
        Vec3::new(140.0, -20.0, 10.0),
        Vec3::new(100.0, -100.0, 10.0),
    ];
    (body, [Vec3::new(50.0, 10.0, 10.0), Vec3::new(100.0, -50.0, 10.0)], outline.windows(2).map(|w| (w[0] + w[1]) * 0.5).collect())
}

#[test]
fn the_bracket_at_the_users_size_merges_its_rounds_at_the_wall_ends() {
    // The foot rounded 2, the plate's edge 2 or 1.5, either first: the band needs about 5.7 to 7.5
    // of straight edge back from each wall end along both rounds, and has it.
    let (body, foot, outline) = bracket();
    let ends: [WallEnd; 2] = [(Vec3::new(0.0, 10.0, 10.0), Vec3::X, Vec3::Y, 2.0 / 3.0), (Vec3::new(100.0, -100.0, 10.0), Vec3::Y, Vec3::X, 0.5)];
    let footed = fillet(&body, &foot, 2.0).unwrap();
    for rb in [2.0, 1.5] {
        let edged = fillet(&body, &outline, rb).unwrap();
        let a = fillet(&footed, &outline, rb).unwrap();
        let b = fillet(&edged, &foot, 2.0).unwrap();
        for (how, result) in [("foot then edge", &a), ("edge then foot", &b)] {
            let how = format!("{how} {rb}");
            assert_merged(result, &ends, (2.0, rb), 1e-6, &how);
            for end in ends {
                let ([p_star, q_w, q_s], _) = merge_points(end, 2.0, rb);
                for q in [q_w, q_s] {
                    assert!(crease(result, p_star, q, 1e-6) < 1e-3, "{how}");
                }
            }
        }
        let (va, vb) = (planned_volume(&a), planned_volume(&b));
        assert!((va - vb).abs() < 1e-6 * va, "{rb}: {va} {vb}");
    }
}

#[test]
fn rounds_meeting_at_a_wall_end_are_refused_where_the_band_does_not_fit() {
    // The plate's side runs straight from the first wall end only to (3, 4), a corner rounded 1.
    // The band of a foot round 1 and an edge round 0.5 needs 2.55 of it (and 2.40 along the
    // wall): built. With a foot round of 1.5 it needs 3.46, and the side has 3.34 before its
    // corner's round: refused, saying so, either way round.
    let plate = prism_at(
        Vec3::ZERO,
        &[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (28.0, -4.0), (28.0, 10.0), (6.0, 10.0), (3.0, 4.0), (0.0, 2.0)],
        0.0,
        2.0,
    );
    let wall = prism_at(Vec3::ZERO, &[(0.0, 0.0), (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (20.0, 2.0), (0.0, 2.0)], 2.0, 6.0);
    let mut body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    body = fillet(&body, &[Vec3::new(20.0, 2.0, 4.0)], 1.0).unwrap();
    body = fillet(&body, &[Vec3::new(3.0, 4.0, 1.0)], 1.0).unwrap();
    body = fillet(&body, &[Vec3::new(6.0, 10.0, 1.0), Vec3::new(28.0, 10.0, 1.0), Vec3::new(28.0, -4.0, 1.0)], 3.0).unwrap();
    let outline =
        [Vec3::new(2.55, 3.7, 2.0), Vec3::new(4.5, 7.0, 2.0), Vec3::new(17.0, 10.0, 2.0), Vec3::new(28.0, 3.0, 2.0), Vec3::new(24.0, -12.0, 2.0)];
    let fits = fillet(&round_the_foot(&body, 1.0).unwrap(), &outline, 0.5).unwrap();
    assert_merged(&fits, &wall_ends(), (1.0, 0.5), 1e-6, "foot 1 then edge 0.5");
    let footed = round_the_foot(&body, 1.5).unwrap();
    let edged = fillet(&body, &outline, 0.5).unwrap();
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let both: Vec<Vec3> = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)].into_iter().chain(outline).collect();
    let radii: Vec<f64> = (0..both.len()).map(|i| if i < 3 { 1.5 } else { 0.5 }).collect();
    let each = crate::fillet_each(&body, &both, &radii).map(|_| ()).unwrap_err();
    for refused in [fillet(&footed, &outline, 0.5).unwrap_err(), round_the_foot(&edged, 1.5).unwrap_err(), each] {
        // Too large: smaller rounds may merge, so it is no decisive refusal.
        assert!(!crate::blend::decisive(&refused), "{refused}");
        assert!(matches!(&refused, KernelError::TooLarge(m) if m.starts_with(crate::blend::MIXED_END_SIZE)), "{refused}");
        let text = refused.to_string();
        assert!(text.contains("need 3.454 mm of straight edge before it, 3.34"), "{text}");
    }
    // The largest edge round that merges with the foot's 1.5 there: about 0.43 (0.4 builds, 0.45
    // needs 3.379 of the 3.340), found by the search as by hand.
    for r in [0.4, 0.35] {
        assert_merged(&fillet(&footed, &outline, r).unwrap(), &wall_ends()[..1], (1.5, r), 1e-6, &format!("edge {r}"));
    }
    let (largest, built) = crate::fillet_max(&footed, &outline, 0.5).unwrap();
    assert!((0.4..0.45).contains(&largest), "{largest}");
    assert_merged(&built, &wall_ends()[..1], (1.5, largest), 1e-6, "the largest");
    // A ball rolled along the plate's edge at a chord's width has no one radius for the band:
    // refused as such.
    let (rounded, outline) = plate_with_round_corners();
    let footed = round_the_foot(&rounded, 1.0).unwrap();
    let all = Body::unique_edges(&footed.solid);
    let (i, _) = footed.nearest_edge(outline[0], 1e-3).unwrap().unwrap();
    let ids = crate::blend::smooth_chain(&footed.solid, &all[i]).unwrap();
    let chord = crate::guard("test", || curve_blend_chord(&footed.solid, &ids, footed.size(), 0.5, true));
    assert!(chord.as_ref().is_err_and(crate::blend::decisive), "{:?}", chord.map(|_| ()));
}

/// [`wall_on_a_plate`] with the plate's corners left sharp and its side at the first wall end
/// turned to `deg` from the wall's trace. The wall's end there runs from (0, 2) to `foot`, above
/// the plate's edge flush with it, so the line of the wall's end stays where the wall's face meets
/// the plate's side. With a point on that side's top edge.
fn wall_on_a_turned_plate(deg: f64, foot: (f64, f64)) -> (Body, Vec3) {
    let theta = deg.to_radians();
    let x = 8.0 * theta.cos() / theta.sin();
    let plate = prism_at(Vec3::ZERO, &[foot, (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (28.0, -4.0), (28.0, 10.0), (x, 10.0), (0.0, 2.0)], 0.0, 2.0);
    let wall = prism_at(Vec3::ZERO, &[foot, (18.0, 0.0), (18.0, -20.0), (20.0, -20.0), (20.0, 2.0), (0.0, 2.0)], 2.0, 6.0);
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    (fillet(&body, &[Vec3::new(20.0, 2.0, 4.0)], 1.0).unwrap(), Vec3::new(x / 2.0, 6.0, 2.0))
}

#[test]
fn rounds_meeting_where_the_plate_side_is_square_to_the_wall_merge_in_any_order() {
    // At the first wall end the plate's side leaves the wall's trace square to it (and 2° either
    // side), the wall's end oblique above the plate's edge beneath it: the earlier round's contact
    // with the top runs square to the later chain there, or nearly. Each chain still ends at the
    // earlier round's end (the foot's three edges, the side's one), and the two rounds merge into
    // the same band in either order and each at its radius.
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let foot = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)];
    for deg in [90.0, 88.0, 92.0] {
        let (body, top) = wall_on_a_turned_plate(deg, (2.0, 0.0));
        let theta = f64::to_radians(deg);
        let end: WallEnd = (Vec3::new(0.0, 2.0, 2.0), Vec3::X, Vec3::Y, if deg == 90.0 { f64::INFINITY } else { theta.tan() });
        let pairs: &[(f64, f64)] = if deg == 90.0 { &[(1.0, 0.5), (0.5, 1.0), (0.5, 0.5)] } else { &[(1.0, 0.5)] };
        for &(ra, rb) in pairs {
            let footed = round_the_foot(&body, ra).unwrap();
            let edged = fillet(&body, &[top], rb).unwrap();
            assert_eq!(chain_at(&footed, top), 1, "{deg}° foot {ra}");
            assert_eq!(chain_at(&edged, foot[0]), 3, "{deg}° edge {rb}");
            let picks: Vec<Vec3> = foot.iter().copied().chain([top]).collect();
            let routes = [
                ("foot then edge", fillet(&footed, &[top], rb)),
                ("edge then foot", round_the_foot(&edged, ra)),
                ("each at its radius", crate::fillet_each(&body, &picks, &[ra, ra, ra, rb])),
            ];
            let correction = merge_correction(theta, ra, rb);
            let (v_body, v_footed, v_edged) = (planned_volume(&body), planned_volume(&footed), planned_volume(&edged));
            let first = routes[0].1.as_ref().unwrap();
            for (how, result) in &routes {
                let how = format!("{how} at {deg}°, foot {ra} edge {rb}");
                let result = result.as_ref().unwrap_or_else(|e| panic!("{how}: {e}"));
                assert_merged(result, &[end], (ra, rb), 1e-6, &how);
                let ([p_star, q_w, q_s], _) = merge_points(end, ra, rb);
                for q in [q_w, q_s] {
                    assert!(crease(result, p_star, q, 1e-6) < 1e-3, "{how}");
                }
                let merged = planned_volume(result) - (v_footed + v_edged - v_body);
                assert!((merged - correction).abs() < 0.002, "{how}: merged {merged}, expected {correction}");
                assert!(same_vertices(result, first, 1e-6), "{how}: the vertices differ from the first route's");
                assert!((planned_volume(result) - planned_volume(first)).abs() < 1e-6 * v_body, "{how}");
            }
        }
    }
    // The wall's end leaning out over the plate's edge (to (-1, 0)): a band reaching past it is
    // refused as such, not as some other size.
    let (body, top) = wall_on_a_turned_plate(90.0, (-1.0, 0.0));
    let refused = fillet(&round_the_foot(&body, 1.0).unwrap(), &[top], 0.25).unwrap_err();
    assert!(matches!(&refused, KernelError::TooLarge(m) if m.starts_with(crate::blend::MIXED_END_SIZE)), "{refused}");
    assert!(refused.to_string().contains("reach past the wall's end"), "{refused}");
}

/// A straight wall (x 0 to 40, y 0 to 2, z 3 to 8) on a plate 3 thick, its ends flush with the
/// plate's sides beneath them. The plate's sides leave the wall's ends at `deg` to its trace, run
/// straight for `side` to a sharp corner and on parallel to the wall. With a point on the wall's
/// foot and on each slanted side's top edge (0.9 of the way along), and the wall's ends.
fn wall_on_a_trapezoid(deg: f64, side: f64) -> (Body, Vec3, [Vec3; 2], [WallEnd; 2]) {
    wall_on_a_trapezoid_at(deg, side, Vec3::ZERO, 1.0)
}

/// [`wall_on_a_trapezoid`] scaled by `k` about the origin and moved by `origin`.
fn wall_on_a_trapezoid_at(deg: f64, side: f64, origin: Vec3, k: f64) -> (Body, Vec3, [Vec3; 2], [WallEnd; 2]) {
    let (c, s) = (deg.to_radians().cos(), deg.to_radians().sin());
    let plate = [(0.0, 0.0), (40.0, 0.0), (40.0, 2.0), (40.0 - side * c, 2.0 + side * s), (side * c, 2.0 + side * s), (0.0, 2.0)];
    let plate = prism_at(origin, &plate.map(|(x, y)| (x * k, y * k)), 0.0, 3.0 * k);
    let wall = prism_at(origin, &[(0.0, 0.0), (40.0, 0.0), (40.0, 2.0), (0.0, 2.0)].map(|(x, y)| (x * k, y * k)), 3.0 * k, 8.0 * k);
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    let tan = deg.to_radians().tan();
    let p = |x: f64, y: f64| origin + Vec3::new(x, y, 3.0) * k;
    let ends = [(p(0.0, 2.0), Vec3::X, Vec3::Y, tan), (p(40.0, 2.0), -Vec3::X, Vec3::Y, tan)];
    let at = 0.9 * side;
    (body, p(20.0, 2.0), [p(at * c, 2.0 + at * s), p(40.0 - at * c, 2.0 + at * s)], ends)
}

#[test]
fn rounds_meeting_at_a_wall_end_fit_alike_in_either_order_before_a_sharp_corner() {
    // The plate's side runs only 4 from each wall end to a sharp corner of 150°. The edge's round
    // runs on along the top past that corner (its contact with the top meets the next side
    // rb·cot 30° further on), so the band fits even where its corner on the top lies past the
    // side's 4. It fits by the same measure whichever round came first: every route builds it,
    // the larger round first or the smaller.
    let (body, foot, tops, ends) = wall_on_a_trapezoid(30.0, 4.0);
    let theta = 30f64.to_radians();
    for (ra, rb) in [(1.0, 1.3), (1.0, 2.0), (1.6, 0.5)] {
        let footed = fillet(&body, &[foot], ra).unwrap();
        let edged = fillet(&body, &tops, rb).unwrap();
        let picks = [foot, tops[0], tops[1]];
        let routes = [
            ("foot then edge", fillet(&footed, &tops, rb)),
            ("edge then foot", fillet(&edged, &[foot], ra)),
            ("each at its radius", crate::fillet_each(&body, &picks, &[ra, rb, rb])),
        ];
        let correction = 2.0 * merge_correction(theta, ra, rb);
        let mass = |b: &Body| crate::measure_mass(b).unwrap().volume;
        let (v_body, v_footed, v_edged) = (mass(&body), mass(&footed), mass(&edged));
        let first = routes[0].1.as_ref().unwrap_or_else(|e| panic!("foot {ra} edge {rb}: {e}"));
        for (how, result) in &routes {
            let how = format!("{how}, foot {ra} edge {rb}");
            let result = result.as_ref().unwrap_or_else(|e| panic!("{how}: {e}"));
            assert_merged(result, &ends, (ra, rb), 1e-6, &how);
            for end in ends {
                let ([p_star, q_w, q_s], _) = merge_points(end, ra, rb);
                for q in [q_w, q_s] {
                    assert!(crease(result, p_star, q, 1e-6) < 1e-3, "{how}");
                }
            }
            // The merge is the correction at both ends, to the measure's own noise on a body of
            // 860 mm³ (a hundredth of the correction where the rounds are as large as the top is
            // wide), and the same by every route.
            let merged = mass(result) - (v_footed + v_edged - v_body);
            assert!((merged - correction).abs() < 0.001 + 0.012 * correction.abs(), "{how}: merged {merged}, expected {correction}");
            assert!((mass(result) - mass(first)).abs() < 1e-6 * v_body, "{how}");
            assert!(same_vertices(result, first, 1e-6), "{how}: the vertices differ from the first route's");
        }
    }
}

#[test]
fn a_plate_edge_nearly_square_to_the_wall_merges_each_at_its_radius_either_size_first() {
    // The plate's sides leave the wall's ends at 85°, so the wall's end runs 5° off the plate's
    // side beneath it. The edge's round alone is refused there (it would end on a face almost
    // along its edge), but after the foot's round it ends on that round's end and merges with it.
    // Each at its radius builds that merge whichever radius is the larger.
    let (body, foot, tops, ends) = wall_on_a_trapezoid(85.0, 15.0);
    for (ra, rb) in [(1.0, 0.5), (0.5, 1.0)] {
        let alone = fillet(&body, &tops, rb).map(|_| ()).unwrap_err().to_string();
        assert!(alone.contains("an end face almost along the edge"), "{alone}");
        let merged = fillet(&fillet(&body, &[foot], ra).unwrap(), &tops, rb).unwrap();
        let how = format!("foot {ra} edge {rb}");
        assert_merged(&merged, &ends, (ra, rb), 1e-6, &how);
        let each = crate::fillet_each(&body, &[foot, tops[0], tops[1]], &[ra, rb, rb]).unwrap_or_else(|e| panic!("{how}: {e}"));
        assert_merged(&each, &ends, (ra, rb), 1e-6, &format!("each at its radius, {how}"));
        assert!(same_vertices(&each, &merged, 1e-6), "{how}");
    }
}

#[test]
fn the_rounds_trimmed_by_the_band_mesh_on_their_cylinders_at_narrow_corners() {
    // At a plate's corner of 10° or 15° each round's edge with the band leaves its contact with
    // the top at a narrow angle: the display mesh of both rounds stays on their cylinders there,
    // to the chord tolerance, as each round alone does (a grid fanned across that corner left them
    // by a hundred times as much).
    let tol = 5e-5;
    for (deg, ra, rb) in [(10.0f64, 0.5, 0.5), (15.0, 0.5, 1.0)] {
        let (body, foot, tops, ends) = wall_on_a_trapezoid(deg, 15.0);
        let merged = fillet(&fillet(&body, &[foot], ra).unwrap(), &tops, rb).unwrap();
        assert_merged(&merged, &ends, (ra, rb), 1e-6, &format!("{deg}°"));
        // Each round's cylinder at each end (a point on its axis, the axis, the radius) and `P*`.
        let theta = deg.to_radians();
        let rounds: Vec<(Vec3, Vec3, Vec3, f64, Vec3)> = ends
            .iter()
            .flat_map(|end| {
                let (v, w, _, _) = *end;
                let s = Vec3::new(w.x * theta.cos(), theta.sin(), 0.0);
                let (m_w, m_s) = ((s - w * w.dot(s)).normalized().unwrap(), (w - s * w.dot(s)).normalized().unwrap());
                let ([p_star, _, _], _) = merge_points(*end, ra, rb);
                [(v + m_w * ra + Vec3::Z * ra, w, Vec3::Z, ra, p_star), (v + m_s * rb - Vec3::Z * rb, s, Vec3::Z, rb, p_star)]
            })
            .collect();
        let off = |(a, d, _, r, _): &(Vec3, Vec3, Vec3, f64, Vec3), p: Vec3| {
            let q = p - *a;
            ((q - *d * q.dot(*d)).len() - r).abs()
        };
        let mesh = merged.display_plan(tol).unwrap().evaluate();
        let mut checked = 0;
        for (t, f) in mesh.triangles.iter().zip(&mesh.tri_face) {
            let corners = mesh.tri(t).unwrap();
            let centre = (corners[0] + corners[1] + corners[2]) * (1.0 / 3.0);
            // A triangle of a round near `P*`: its corners on that round's cylinder.
            let Some(round) = rounds.iter().find(|x| x.4.dist(centre) < 1.5 && corners.iter().all(|c| off(x, *c) < 1e-7)) else { continue };
            let gap = off(round, centre);
            assert!(gap < 4.0 * tol, "{deg}° face {f}: {gap} off its round at {centre:?}");
            checked += 1;
        }
        assert!(checked > 100, "{deg}°: {checked}");
    }
}

/// The chain along `pick` on `body` is `len` edges, and a round of it at each radius is built
/// sound.
fn chain_and_round_kept(body: &Body, pick: Vec3, len: usize, radii: &[f64]) {
    assert_eq!(chain_at(body, pick), len);
    for r in radii {
        let result = fillet(body, &[pick], *r).unwrap_or_else(|e| panic!("r {r}: {e}"));
        assert!(result.validity().is_empty(), "r {r}: {:?}", result.validity());
        assert_eq!(result.crossed_face_loops().unwrap(), 0, "r {r}");
        assert!(result.solid.face_iter().count() > body.solid.face_iter().count(), "r {r}");
    }
}

#[test]
fn a_chain_runs_on_over_a_designed_cove_on_an_oblique_side() {
    // A ramp's top runs down a cove (a cylinder bending up, no earlier round) into a step, cut by a
    // slanted side: the top edge of the side runs on smoothly over the cove's oblique seam with the
    // top and on up the step. No earlier round of the other kind ends there: the chain runs on.
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let line = |a: (f64, f64), b: (f64, f64)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) };
    let profile = Region2 {
        outer: Loop2 {
            segs: vec![
                line((0.0, 0.0), (20.0, 0.0)),
                line((20.0, 0.0), (20.0, 2.0)),
                line((20.0, 2.0), (10.0, 2.0)),
                Seg2::Arc { center: Vec2::new(10.0, 5.0), radius: 3.0, start: -std::f64::consts::FRAC_PI_2, sweep: -std::f64::consts::FRAC_PI_2 },
                line((7.0, 5.0), (7.0, 8.0)),
                line((7.0, 8.0), (0.0, 8.0)),
                line((0.0, 8.0), (0.0, 0.0)),
            ],
        },
        holes: vec![],
    };
    let ramp = crate::extrude(&Plane::YZ, &[profile], 0.0, 10.0).unwrap().pop().unwrap();
    // Away the side of x = 2 + 0.3 y.
    let cut = Region2 {
        outer: Loop2 {
            segs: vec![
                line((-5.0, -5.0), (0.5, -5.0)),
                line((0.5, -5.0), (9.5, 25.0)),
                line((9.5, 25.0), (-5.0, 25.0)),
                line((-5.0, 25.0), (-5.0, -5.0)),
            ],
        },
        holes: vec![],
    };
    let cut = crate::extrude(&Plane::XY, &[cut], -1.0, 10.0).unwrap().pop().unwrap();
    let body = boolean(&ramp, &cut, BoolOp::Cut).unwrap().unwrap();
    chain_and_round_kept(&body, Vec3::new(6.5, 15.0, 2.0), 3, &[0.5, 1.0]);
}

#[test]
fn a_chain_runs_on_over_a_wall_foot_round_whose_end_is_flush_with_the_side() {
    // A wall on a plate, its end in one plane with the plate's slanted side, its foot rounded: the
    // round's end runs across that one face from the plate's top edge up the wall's end. That is
    // no end against a separate end face ([`crate::merge_corner`]): the plate's top edge runs on
    // over it and up the wall's end.
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let polygon = |pts: &[(f64, f64)]| Region2 {
        outer: Loop2 {
            segs: pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect(),
        },
        holes: vec![],
    };
    let plate = crate::extrude(&Plane::XY, &[polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (5.0, 10.0)])], 0.0, 2.0).unwrap().pop().unwrap();
    let wall = crate::extrude(&Plane::XY, &[polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 2.0), (1.0, 2.0)])], 2.0, 6.0).unwrap().pop().unwrap();
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    for r in [0.5, 1.0] {
        let footed = fillet(&body, &[Vec3::new(10.0, 2.0, 2.0)], r).unwrap();
        chain_and_round_kept(&footed, Vec3::new(3.0, 6.0, 2.0), 3, &[0.5, 1.0]);
    }
}

/// Both rounds on `body`, the foot's picks `foot` at `ra` and the plate edge's `tops` at `rb`: one
/// after the other either way round and each at its radius, every route builds the same merge at
/// each of `ends`, the merge adding `correction` beyond the two rounds alone.
fn merges_alike(body: &Body, foot: &[Vec3], tops: &[Vec3], ends: &[WallEnd], (ra, rb): (f64, f64), correction: f64, how: &str) {
    let footed = fillet(body, foot, ra).unwrap_or_else(|e| panic!("{how}: the foot alone: {e}"));
    let edged = fillet(body, tops, rb).unwrap_or_else(|e| panic!("{how}: the edge alone: {e}"));
    let picks: Vec<Vec3> = foot.iter().chain(tops).copied().collect();
    let radii: Vec<f64> = foot.iter().map(|_| ra).chain(tops.iter().map(|_| rb)).collect();
    let routes = [
        ("foot then edge", fillet(&footed, tops, rb)),
        ("edge then foot", fillet(&edged, foot, ra)),
        ("each at its radius", crate::fillet_each(body, &picks, &radii)),
    ];
    merged_alike(body, (&footed, &edged), &routes, ends, (ra, rb), correction, how);
}

/// Every one of `routes` (from `body`, whose rounds alone are `footed` and `edged`) builds the same
/// merge at each of `ends`, the merge adding `correction` beyond the two rounds alone.
fn merged_alike(
    body: &Body,
    (footed, edged): (&Body, &Body),
    routes: &[(&str, crate::Result<Body>)],
    ends: &[WallEnd],
    (ra, rb): (f64, f64),
    correction: f64,
    how: &str,
) {
    let mass = planned_volume;
    let (v_body, v_footed, v_edged) = (mass(body), mass(footed), mass(edged));
    let first = routes[0].1.as_ref().unwrap_or_else(|e| panic!("{how}, {}, foot {ra} edge {rb}: {e}", routes[0].0));
    for (route, result) in routes {
        let how = format!("{how}, {route}, foot {ra} edge {rb}");
        let result = result.as_ref().unwrap_or_else(|e| panic!("{how}: {e}"));
        assert_merged(result, ends, (ra, rb), 1e-6, &how);
        for end in ends {
            let ([p_star, q_w, q_s], _) = merge_points(*end, ra, rb);
            for q in [q_w, q_s] {
                assert!(crease(result, p_star, q, 1e-6) < 1e-3, "{how}");
            }
        }
        // Against each round alone as meshed, the merge is the correction (to the mesh's two
        // thousandths at each end, as where the plate's side is square to the wall).
        let merged = mass(result) - (v_footed + v_edged - v_body);
        assert!((merged - correction).abs() < 0.002 * ends.len() as f64, "{how}: merged {merged}, expected {correction}");
        // The same body by every route: the same vertices, and the same volume as measured (the
        // display's mesh differs by its chord where the faces' parameters do).
        assert!(same_vertices(result, first, 1e-6), "{how}: the vertices differ from the first route's");
        let measured = |b: &Body| crate::measure_mass(b).unwrap().volume;
        assert!((measured(result) - measured(first)).abs() < 1e-6 * v_body, "{how}");
    }
}

/// [`a_chain_runs_on_over_a_wall_foot_round_whose_end_is_flush_with_the_side`]'s wall on a plate,
/// its end in one plane with the plate's slanted side; with a point on the wall's foot and on the
/// side's top edge, and the wall's end there.
fn wall_flush_with_a_slanted_side() -> (Body, Vec3, Vec3, WallEnd) {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let polygon = |pts: &[(f64, f64)]| Region2 {
        outer: Loop2 {
            segs: pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| Seg2::Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1) }).collect(),
        },
        holes: vec![],
    };
    let plate = crate::extrude(&Plane::XY, &[polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (5.0, 10.0)])], 0.0, 2.0).unwrap().pop().unwrap();
    let wall = crate::extrude(&Plane::XY, &[polygon(&[(0.0, 0.0), (20.0, 0.0), (20.0, 2.0), (1.0, 2.0)])], 2.0, 6.0).unwrap().pop().unwrap();
    let body = boolean(&plate, &wall, BoolOp::Union).unwrap().unwrap();
    (body, Vec3::new(10.0, 2.0, 2.0), Vec3::new(3.0, 6.0, 2.0), (Vec3::new(1.0, 2.0, 2.0), Vec3::X, Vec3::Y, 2.0))
}

#[test]
fn rounds_meeting_where_the_wall_end_is_flush_with_the_plate_side_merge_after_the_edge() {
    // A wall on a plate, its end in one plane with the plate's side beneath it: the wall's end and
    // the plate's side are one face. The foot's round after the plate edge's ends at that round's
    // end (which runs down the wall's face, square below the top) and merges with it, the band's
    // edge on the line of the wall's end; so do both in one fillet, and each at its radius where
    // the edge's round, the larger, comes first. The other way round the faces at the foot's end
    // are those of a pocket's rim meeting a rounded inner corner (the plate's top the pocket's
    // wall, the side its top): the edge's chain runs on over the foot's end and up the wall's end,
    // as round a pocket ([`a_chain_runs_on_over_a_wall_foot_round_whose_end_is_flush_with_the_side`]).
    // Square ends (a rectangular plate, its side 4 or 15 long to the next corner) and a side
    // slanted at tan⁻¹ 2.
    let square = |side: f64| {
        let (body, foot, tops, ends) = wall_on_a_trapezoid(90.0, side);
        (body, vec![foot], tops.to_vec(), ends.to_vec(), std::f64::consts::FRAC_PI_2)
    };
    let (slanted, foot, top, end) = wall_flush_with_a_slanted_side();
    let cases = [square(4.0), square(15.0), (slanted, vec![foot], vec![top], vec![end], 2f64.atan())];
    for (body, foot, tops, ends, theta) in &cases {
        let how = format!("{:.0}°, {} ends", theta.to_degrees(), ends.len());
        for (ra, rb) in [(0.5, 1.0), (0.5, 0.5), (1.0, 0.5)] {
            let footed = fillet(body, foot, ra).unwrap();
            let edged = fillet(body, tops, rb).unwrap();
            assert_eq!(chain_at(&edged, foot[0]), 1, "{how}, edge {rb}");
            assert_eq!(chain_at(&footed, tops[0]), 3, "{how}, foot {ra}");
            let picks: Vec<Vec3> = foot.iter().chain(tops).copied().collect();
            let radii: Vec<f64> = foot.iter().map(|_| ra).chain(tops.iter().map(|_| rb)).collect();
            let mut routes = vec![("edge then foot", fillet(&edged, foot, ra))];
            if ra < rb {
                routes.push(("each at its radius", crate::fillet_each(body, &picks, &radii)));
            }
            // In one fillet the foot's round may come first, the edge's then as after it: the
            // merge, or refused as too large (never the chain run on, which the fillet's chains
            // as picked keep it from).
            if ra == rb {
                match fillet(body, &picks, ra) {
                    Ok(b) => routes.push(("together", Ok(b))),
                    Err(e) => assert!(matches!(e, KernelError::TooLarge(_)), "{how}: {e}"),
                }
            }
            let correction = ends.len() as f64 * merge_correction(*theta, ra, rb);
            merged_alike(body, (&footed, &edged), &routes, ends, (ra, rb), correction, &how);
            // Foot then edge: refused, or the chain run on, sound.
            if let Ok(b) = fillet(&footed, tops, rb) {
                assert!(b.validity().is_empty(), "{how}: {:?}", b.validity());
                assert_eq!(b.crossed_face_loops().unwrap(), 0, "{how}");
            }
        }
    }
}

#[test]
fn a_round_merging_with_one_earlier_round_at_both_its_ends_does_so_in_either_order() {
    // The plate's corners rounded 2, its top edge is one chain of five edges from one wall end to
    // the other, and the foot's round one face along the whole wall: the edge's round merges with
    // that one round at both its ends, shortening its contact with the top at both. Every route
    // builds the same two bands.
    let (body, foot, tops, ends) = wall_on_a_trapezoid(30.0, 8.0);
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let corners = [Vec3::new(8.0 * c, 2.0 + 8.0 * s, 1.5), Vec3::new(40.0 - 8.0 * c, 2.0 + 8.0 * s, 1.5)];
    let body = fillet(&body, &corners, 2.0).unwrap();
    assert_eq!(chain_at(&body, tops[0]), 5);
    let correction = 2.0 * merge_correction(30f64.to_radians(), 1.0, 0.5);
    merges_alike(&body, &[foot], &[tops[0]], &ends, (1.0, 0.5), correction, "rounded corners");
    merges_alike(&body, &[foot], &[tops[0]], &ends, (0.5, 1.0), -correction, "rounded corners");
}

#[test]
fn rounds_meeting_where_the_wall_end_is_flush_with_a_short_or_thin_plate_side_merge_or_are_refused() {
    // The square plate's side only a little longer than the band needs, or the plate's edge rounded
    // as deep as the plate is thick and deeper: the foot's round after the edge's builds the merge
    // sound, or is refused.
    for (side, ra, rb) in [(1.03, 1.0, 1.0), (1.03, 0.5, 1.0), (15.0, 1.0, 2.999), (15.0, 1.0, 3.0), (15.0, 1.0, 3.5)] {
        let how = format!("side {side}, foot {ra} edge {rb}");
        let (body, foot, tops, ends) = wall_on_a_trapezoid(90.0, side);
        if let Ok(b) = fillet(&body, &tops, rb).and_then(|edged| fillet(&edged, &[foot], ra)) {
            assert_merged(&b, &ends, (ra, rb), 1e-6, &how);
        }
    }
}

#[test]
fn rounds_meeting_at_wall_ends_off_square_by_a_modelling_error_merge_in_either_order() {
    // The plate's sides drafted by next to nothing (a modelling error, or a file's rounding): the
    // line of the wall's end bends by that much at the top. The band is built as on square faces,
    // within the fit's tolerance of them, by every route; and the largest round that fits is found.
    let foot = [
        Vec3::new(10.0, 2.0, 2.0),
        Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0),
        Vec3::new(20.0, -10.0, 2.0),
    ];
    let tops = [Vec3::new(6.0, 6.0, 2.0), Vec3::new(24.0, -12.0, 2.0)];
    let picks: Vec<Vec3> = foot.iter().chain(&tops).copied().collect();
    let (ra, rb) = (1.0, 0.5);
    for degrees in [1e-6, 4e-5, 1e-4] {
        let body = wall_on_a_drafted_plate(f64::to_radians(degrees));
        let footed = round_the_foot(&body, ra).unwrap();
        let edged = fillet(&body, &tops, rb).unwrap();
        let routes = [
            ("foot then edge", fillet(&footed, &tops, rb)),
            ("edge then foot", round_the_foot(&edged, ra)),
            ("each at its radius", crate::fillet_each(&body, &picks, &[ra, ra, ra, rb, rb])),
        ];
        let first = routes[0].1.as_ref().unwrap_or_else(|e| panic!("draft {degrees}: {e}"));
        for (how, result) in &routes {
            let how = format!("draft {degrees}, {how}");
            let result = result.as_ref().unwrap_or_else(|e| panic!("{how}: {e}"));
            assert_merged(result, &wall_ends(), (ra, rb), 1e-5, &how);
            assert!(same_vertices(result, first, 1e-5), "{how}");
            assert!((planned_volume(result) - planned_volume(first)).abs() < 1e-6 * planned_volume(&body), "{how}");
        }
        // (It searches below the radius asked for, taken to be refused.)
        let (r, _) = crate::fillet_max(&footed, &tops, rb).unwrap_or_else(|| panic!("draft {degrees}: none fits"));
        assert!(r > 0.99 * rb, "draft {degrees}: {r}");
    }
    // Off by a twentieth of a degree the band would leave the faces: refused as too large for
    // them (smaller rounds may fit), never decisively, or built sound.
    let body = wall_on_a_drafted_plate(f64::to_radians(0.05));
    let footed = round_the_foot(&body, ra).unwrap();
    match fillet(&footed, &tops, rb) {
        Ok(b) => {
            assert!(b.validity().is_empty(), "{:?}", b.validity());
            assert_eq!(b.crossed_face_loops().unwrap(), 0);
        }
        Err(e) => assert!(!e.to_string().starts_with(crate::blend::MIXED_END), "{e}"),
    }
}

#[test]
fn a_small_part_far_from_the_origin_rounds_and_merges_as_at_it() {
    // [`wall_on_a_trapezoid`] at a hundredth, at the origin and 100 and 1000 mm from it: the foot's
    // round alone builds wherever the part is (the check of what it adds measures about the part,
    // not the origin), and so does the merge, either order alike.
    let (ra, rb) = (0.005, 0.01);
    let mut grown = Vec::new();
    for far in [0.0, 100.0, 1000.0] {
        let origin = Vec3::new(far, -0.6 * far, 0.3 * far);
        let (body, foot, tops, ends) = wall_on_a_trapezoid_at(110.0, 15.0, origin, 0.01);
        let footed = fillet(&body, &[foot], ra).unwrap_or_else(|e| panic!("{far}: the foot alone: {e}"));
        assert!(footed.validity().is_empty(), "{far}: {:?}", footed.validity());
        let a = fillet(&footed, &tops, rb).unwrap_or_else(|e| panic!("{far}: foot then edge: {e}"));
        let b = fillet(&fillet(&body, &tops, rb).unwrap(), &[foot], ra).unwrap_or_else(|e| panic!("{far}: edge then foot: {e}"));
        for (how, result) in [("foot then edge", &a), ("edge then foot", &b)] {
            assert_merged(result, &ends, (ra, rb), 1e-8, &format!("{far}: {how}"));
        }
        assert!(same_vertices(&a, &b, 1e-8), "{far}");
        let v = |x: &Body| crate::measure_mass(x).unwrap().volume;
        grown.push((v(&footed) - v(&body), v(&a) - v(&body), v(&b) - v(&body)));
    }
    // What each adds, the same either order and wherever the part is (measured about the part, to
    // a part in 10⁴ of the foot's round; about the origin, 1000 mm out, it was off by 300 times
    // that round).
    let (g0, a0, b0) = grown[0];
    assert!(g0 > 0.0 && (a0 - b0).abs() < 1e-3 * g0, "{grown:?}");
    for (g, a, b) in &grown {
        assert!((g - g0).abs() < 1e-4 * g0 && (a - a0).abs() < 1e-4 * g0 && (b - b0).abs() < 1e-4 * g0, "{grown:?}");
    }
}

/// A 12 by 4 bar standing on the origin, 10 high, its four upright edges rounded 1.5.
fn rounded_bar() -> Body {
    let bar = box_solid(Vec3::new(-6.0, -2.0, -5.0), Vec3::new(6.0, 2.0, 5.0)).unwrap();
    fillet(&bar, &[Vec3::new(-6.0, -2.0, 0.0), Vec3::new(6.0, -2.0, 0.0), Vec3::new(6.0, 2.0, 0.0), Vec3::new(-6.0, 2.0, 0.0)], 1.5).unwrap()
}

#[test]
fn a_chain_runs_on_round_a_tilted_slot_and_a_boss_on_a_tilted_plate() {
    // A rounded slot sunk at a slant into a block's top, and a rounded bar standing on a tilted
    // plate: the rim and the foot run round the slot's and the bar's rounded corners, whose seams
    // cross them obliquely and which bend away from the ball. They are the design's corners, not
    // an earlier round's ends: the chains go all the way round and a round of one edge takes them.
    let tilt = 20f64.to_radians();
    let slot = crate::transform(&rounded_bar(), Vec3::new(0.0, 0.0, 10.0), Vec3::ZERO, Vec3::Y, tilt).unwrap();
    let block = box_solid(Vec3::new(-15.0, -10.0, 0.0), Vec3::new(15.0, 10.0, 10.0)).unwrap();
    let slotted = boolean(&block, &slot, BoolOp::Cut).unwrap().unwrap();
    chain_and_round_kept(&slotted, Vec3::new(0.0, 2.0, 10.0), 8, &[0.5]);
    let plate = box_solid(Vec3::new(-15.0, -10.0, -2.0), Vec3::new(15.0, 10.0, 0.0)).unwrap();
    let plate = crate::transform(&plate, Vec3::ZERO, Vec3::ZERO, Vec3::Y, tilt).unwrap();
    let boss = boolean(&plate, &rounded_bar(), BoolOp::Union).unwrap().unwrap();
    chain_and_round_kept(&boss, Vec3::new(0.0, 2.0, 0.0), 8, &[0.5]);
}

#[test]
fn a_sharp_plate_edge_rounds_into_the_wall_end_and_merges_with_the_foot() {
    // [`wall_on_a_plate`] with the plate's corners left sharp: the top edge from each wall end to
    // the plate's next corner is a chain of one straight edge. At the wall's end its end faces are
    // the wall's face and its end (flush with the plate's side beneath): the round runs on past
    // the vertex to the line between them, as it does along the rounded outline. With the foot
    // rounded too, in either order or each at its radius, the two merge at both wall ends.
    let body = wall_on_a_plate();
    let corner = Vec3::new(19.0 + std::f64::consts::FRAC_1_SQRT_2, 1.0 + std::f64::consts::FRAC_1_SQRT_2, 2.0);
    let foot = [Vec3::new(10.0, 2.0, 2.0), corner, Vec3::new(20.0, -10.0, 2.0)];
    let tops = [Vec3::new(6.0, 6.0, 2.0), Vec3::new(24.0, -12.0, 2.0)];
    let (first, second) = ((2.0f64 / 3.0).atan(), 0.5f64.atan());
    // Each edge's round is cut off at both ends by planes square to the top along parallel
    // traces (y = 2 and y = 10 along the first edge, x = 20 and x = 28 along the second): every
    // line along it is as long as the edge.
    let lines = 208f64.sqrt() + 320f64.sqrt();
    let v_body = planned_volume(&body);
    for (ra, rb) in [(1.0, 0.5), (0.5, 1.0)] {
        let edged = fillet(&body, &tops, rb).unwrap();
        assert!(edged.validity().is_empty(), "{rb}: {:?}", edged.validity());
        assert_eq!(edged.crossed_face_loops().unwrap(), 0);
        let removed = (1.0 - std::f64::consts::FRAC_PI_4) * rb * rb * lines;
        let lost = v_body - planned_volume(&edged);
        assert!((lost - removed).abs() < 0.006, "edge {rb}: removed {lost}, expected {removed}");
        // Alone, the round meets the line of the wall's end at its side contact and the wall's
        // trace at its top contact; the old corner is gone.
        for end in wall_ends() {
            let ([_, _, q_s], [v, _, p2]) = merge_points(end, ra, rb);
            assert_eq!((vertices_at(&edged, q_s, 1e-6), vertices_at(&edged, v, 1e-6), vertices_at(&edged, p2, 1e-6)), (1, 0, 1), "edge {rb}");
        }
        let footed = round_the_foot(&body, ra).unwrap();
        let radii: Vec<f64> = foot.iter().map(|_| ra).chain(tops.iter().map(|_| rb)).collect();
        let both: Vec<Vec3> = foot.iter().chain(&tops).copied().collect();
        let routes = [
            ("foot then edge", fillet(&footed, &tops, rb).unwrap()),
            ("edge then foot", round_the_foot(&edged, ra).unwrap()),
            ("each at its radius", crate::fillet_each(&body, &both, &radii).unwrap()),
        ];
        let analytic = foot_round_added(ra) - removed + merge_correction(first, ra, rb) + merge_correction(second, ra, rb);
        for (how, result) in &routes {
            let how = format!("{how}, foot {ra} edge {rb}");
            assert_merged(result, &wall_ends(), (ra, rb), 1e-6, &how);
            for end in wall_ends() {
                let ([p_star, q_w, q_s], _) = merge_points(end, ra, rb);
                for q in [q_w, q_s] {
                    assert!(crease(result, p_star, q, 1e-6) < 1e-3, "{how}");
                }
            }
            let grown = planned_volume(result) - v_body;
            assert!((grown - analytic).abs() < 0.008, "{how}: grown {grown}, expected {analytic}");
            let v0 = planned_volume(&routes[0].1);
            assert!((planned_volume(result) - v0).abs() < 1e-6 * v0, "{how}");
            assert!(same_vertices(result, &routes[0].1, 1e-6), "{how}: the vertices differ from {}'s", routes[0].0);
        }
    }
}

#[test]
fn hostile_sizes_and_picks_at_the_merge_corner_are_refused_or_built_sound() {
    // The plate's edge rounded where it meets the foot's round at the wall's ends (and the foot's
    // ends picked with it), at sizes and picks nobody means: refused, or a sound body.
    let (body, outline) = plate_with_round_corners();
    let footed = round_the_foot(&body, 1.0).unwrap();
    let sound = |how: String, r: crate::Result<Body>| {
        if let Ok(b) = r {
            assert!(b.validity().is_empty(), "{how}: {:?}", b.validity());
            assert_eq!(b.crossed_face_loops().unwrap(), 0, "{how}");
        }
    };
    let caps: Vec<Vec3> = wall_ends()
        .into_iter()
        .map(|end| {
            let ([_, q_w, _], gone) = merge_points(end, 1.0, 0.5);
            crate::blend::edge_mid(&edge_between(&footed, gone[1], q_w, 1e-6).0).unwrap()
        })
        .collect();
    let with_caps: Vec<Vec3> = outline.iter().chain(&caps).copied().collect();
    for r in [f64::NAN, f64::INFINITY, -1.0, 0.0, 1e-300, 1e-7, 1.999, 2.0, 2.001, 1e6, 1e300] {
        sound(format!("edge {r}"), fillet(&footed, &outline, r));
        sound(format!("edge and caps {r}"), fillet(&footed, &with_caps, r));
        let radii: Vec<f64> = outline.iter().enumerate().map(|(i, _)| if i == 0 { r } else { 0.5 }).collect();
        sound(format!("each {r}"), crate::fillet_each(&footed, &outline, &radii));
    }
    // A radius the band has no room for: the largest that builds, or none.
    if let Some((r, b)) = crate::fillet_max(&footed, &outline, 2.5) {
        sound(format!("largest {r}"), Ok(b));
    }
    // Picks off every edge, or not numbers at all; radii that don't match the picks.
    for p in [Vec3::new(f64::NAN, 0.0, 2.0), Vec3::new(1e300, 0.0, 0.0), Vec3::new(0.0, 2.0, 1e-300)] {
        let picks: Vec<Vec3> = outline.iter().copied().chain([p]).collect();
        sound(format!("pick {p:?}"), fillet(&footed, &picks, 0.5));
    }
    assert!(crate::fillet_each(&footed, &outline, &[0.5]).is_err());
    sound("no picks".into(), fillet(&footed, &[], 0.5));
}
