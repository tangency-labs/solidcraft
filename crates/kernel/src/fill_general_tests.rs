//! The general fill on bodies (#759): G1 and G2 to the faces beside the loop, loops that are not
//! height fields, N-sided loops and points the patch passes through. Every number is checked
//! against the analytic surface the loop was cut from, or against a quadrature of the fitted
//! surface itself, never against the mesher alone.

use mt::{BoundedCurve, InnerSpace, ParametricCurve, ParametricSurface, ParametricSurface3D};
use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};
use truck_modeling as mt;

use super::{PATCH_ANGLE_TOL_DEG, PatchReport, PatchSpec, face_tolerance, patch_edges_report};
use crate::body::{Body, from_p3, p3};
use crate::bridge::Continuity;
use crate::{BoolOp, boolean, box_solid, copy_faces, cylinder, extrude, measure, patch_edges, stitch, torus};

/// The surface's parameters nearest `p`: the nearest of a grid of samples (or `hint`), then
/// Gauss–Newton within the domain.
fn nearest(s: &mt::Surface, p: Vec3, hint: Option<(f64, f64)>) -> (f64, f64) {
    let (Some((u0, u1)), Some((v0, v1))) = s.try_range_tuple() else { panic!("a bounded surface") };
    let p = p3(p);
    let (mut u, mut v) = match hint {
        Some(h) => h,
        None => {
            let n = 100;
            let mut best = (f64::MAX, u0, v0);
            for i in 0..=n {
                for j in 0..=n {
                    let (u, v) = (u0 + (u1 - u0) * i as f64 / n as f64, v0 + (v1 - v0) * j as f64 / n as f64);
                    let d = (s.subs(u, v) - p).magnitude();
                    if d < best.0 {
                        best = (d, u, v);
                    }
                }
            }
            (best.1, best.2)
        }
    };
    for _ in 0..60 {
        let (q, a, b) = (s.subs(u, v), s.uder(u, v), s.vder(u, v));
        let r = q - p;
        let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
        let det = aa * bb - ab * ab;
        if det.abs() < 1e-300 {
            break;
        }
        let (ga, gb) = (a.dot(r), b.dot(r));
        u = (u - (bb * ga - ab * gb) / det).clamp(u0, u1);
        v = (v - (aa * gb - ab * ga) / det).clamp(v0, v1);
    }
    (u, v)
}

fn v3(v: mt::Vector3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Points along every edge of the loop: `per` on each.
fn rim_points(face: &mt::Face, per: usize) -> Vec<Vec3> {
    let mut out = Vec::new();
    for e in face.boundaries().iter().flat_map(|w| w.edge_iter()) {
        let c = e.oriented_curve();
        let (t0, t1) = c.range_tuple();
        for k in 0..per {
            out.push(from_p3(c.subs(t0 + (t1 - t0) * (k as f64 + 0.5) / per as f64)));
        }
    }
    out
}

/// Along the loop, densely: the farthest a rim point is from the patch, the largest angle
/// (degrees) between the patch's normal and the neighbour's exact one (`normal(p)`, one sign for
/// the whole loop: a fold would show as 180°), and the largest difference between the patch's
/// normal curvature across the loop and `curvature(p)`, the neighbour's.
fn along_the_loop(fill: &Body, normal: &dyn Fn(Vec3) -> Vec3, curvature: &dyn Fn(Vec3) -> f64) -> (f64, f64, f64) {
    let face = fill.solid.face_iter().next().unwrap().clone();
    let s = face.surface();
    let pts = rim_points(&face, 200);
    let mut found = Vec::with_capacity(pts.len());
    let mut hint = None;
    for p in &pts {
        let uv = nearest(&s, *p, hint);
        hint = Some(uv);
        found.push(uv);
    }
    // One sign for the whole loop: the one most rim points agree with.
    let patch_normal = |(u, v): (f64, f64)| v3(s.normal(u, v));
    let agree: f64 = pts.iter().zip(&found).map(|(p, uv)| patch_normal(*uv).dot(normal(*p)).signum()).sum();
    let sign = agree.signum();
    let (mut gap, mut angle, mut kappa) = (0.0f64, 0.0f64, 0.0f64);
    for (k, (p, &(u, v))) in pts.iter().zip(&found).enumerate() {
        gap = gap.max(from_p3(s.subs(u, v)).dist(*p));
        let n = patch_normal((u, v));
        let want = normal(*p) * sign;
        angle = angle.max(n.dot(want).clamp(-1.0, 1.0).acos().to_degrees());
        // Across the loop: the tangent-plane direction square to the rim.
        let next = pts.get(k + 1).or(pts.get(k.wrapping_sub(1))).copied().unwrap();
        let t = (next - *p).normalized().unwrap();
        let Some(d) = n.cross(t).normalized() else { continue };
        let (su, sv) = (v3(s.uder(u, v)), v3(s.vder(u, v)));
        // d = a·su + b·sv.
        let (aa, ab, bb) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = aa * bb - ab * ab;
        let (a, b) = ((bb * d.dot(su) - ab * d.dot(sv)) / det, (aa * d.dot(sv) - ab * d.dot(su)) / det);
        // The second derivatives by central differences of the surface's own points.
        let h = 1e-3;
        let e = |du: f64, dv: f64| from_p3(s.subs(u + du * h, v + dv * h));
        let suu = (e(1.0, 0.0) - e(0.0, 0.0) * 2.0 + e(-1.0, 0.0)) * (1.0 / (h * h));
        let svv = (e(0.0, 1.0) - e(0.0, 0.0) * 2.0 + e(0.0, -1.0)) * (1.0 / (h * h));
        let suv = (e(1.0, 1.0) - e(1.0, -1.0) - e(-1.0, 1.0) + e(-1.0, -1.0)) * (0.25 / (h * h));
        let second = suu * (a * a) + suv * (2.0 * a * b) + svv * (b * b);
        kappa = kappa.max((second.dot(want) - curvature(*p)).abs());
    }
    (gap, angle, kappa)
}

/// `∫∫ f(S, S_u × S_v) du dv` over the face's own region of its surface (its loop taken into
/// parameter space by the nearest parameters of points along it), by the midpoint rule on a
/// `n × n` grid over the region's box: the quadrature the mesher's area is checked against.
fn quadrature(face: &mt::Face, n: usize, f: &dyn Fn(Vec3, Vec3) -> f64) -> f64 {
    let s = face.surface();
    let mut poly: Vec<(f64, f64)> = Vec::new();
    let mut hint = None;
    for p in rim_points(face, 400) {
        let uv = nearest(&s, p, hint);
        hint = Some(uv);
        poly.push(uv);
    }
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for &(u, v) in &poly {
        lo = (lo.0.min(u), lo.1.min(v));
        hi = (hi.0.max(u), hi.1.max(v));
    }
    let inside = |u: f64, v: f64| {
        let mut w = false;
        for k in 0..poly.len() {
            let (a, b) = (poly[k], poly[(k + 1) % poly.len()]);
            if (a.1 > v) != (b.1 > v) && u < a.0 + (v - a.1) / (b.1 - a.1) * (b.0 - a.0) {
                w = !w;
            }
        }
        w
    };
    let (du, dv) = ((hi.0 - lo.0) / n as f64, (hi.1 - lo.1) / n as f64);
    let mut sum = 0.0;
    for i in 0..n {
        for j in 0..n {
            let (u, v) = (lo.0 + (i as f64 + 0.5) * du, lo.1 + (j as f64 + 0.5) * dv);
            if inside(u, v) {
                sum += f(from_p3(s.subs(u, v)), v3(s.uder(u, v)).cross(v3(s.vder(u, v)))) * du * dv;
            }
        }
    }
    sum
}

/// The fill is one face on the open body's own edges, within the fill's tolerance of them; its
/// mesh has no folded triangle (each faces the way its corners' normals do); and the mesher's
/// area agrees with a quadrature of the surface over its own region. Returns the area.
fn check_face(name: &str, open: &Body, fill: &Body) -> f64 {
    assert!(fill.is_surface());
    assert_eq!(fill.face_count(), 1, "{name}: one face");
    let own: Vec<mt::EdgeID> = open.solid.edge_iter().map(|e| e.id()).collect();
    assert!(fill.solid.edge_iter().all(|e| own.contains(&e.id())), "{name}: an edge that is not the body's");
    let face = fill.solid.face_iter().next().unwrap().clone();
    let s = face.surface();
    let tol = face_tolerance(open);
    let mut off = 0.0f64;
    let mut hint = None;
    for p in rim_points(&face, 64) {
        let uv = nearest(&s, p, hint);
        hint = Some(uv);
        off = off.max(from_p3(s.subs(uv.0, uv.1)).dist(p));
    }
    assert!(off <= tol * 1.01, "{name}: an edge is {off} off the face (tolerance {tol})");
    let m = fill.mesh(&crate::MeshTolerance::chord(0.01)).unwrap();
    assert!(!m.triangles.is_empty(), "{name}: the face meshes");
    for t in &m.triangles {
        let [a, b, c] = m.tri(t).unwrap();
        let v = (b - a).cross(c - a);
        let n: Vec3 = t.iter().map(|&i| m.normals[i as usize]).fold(Vec3::ZERO, |s, x| s + x);
        if v.len() > 1e-10 {
            assert!(v.dot(n) > 0.0, "{name}: a folded triangle");
        }
    }
    let area = measure(fill).unwrap().area;
    let quad = quadrature(&face, 500, &|_, n| n.len());
    eprintln!("{name}: one face, edges within {off:e} (tolerance {tol:e}), mesh area {area:.4}, quadrature {quad:.4}");
    assert!((area - quad).abs() < 3e-3 * quad, "{name}: mesh area {area} against the surface's {quad}");
    area
}

fn spec(g: Continuity) -> PatchSpec {
    PatchSpec { continuity: g, ..PatchSpec::default() }
}

fn report(name: &str, r: &PatchReport) {
    eprintln!(
        "{name}: fill reports gap {:e}, normals within {:.4}° ({:.4}° near clashing corners), curvature mismatch {:e}, points within {:e}",
        r.gap, r.normal_deviation_deg, r.normal_deviation_all_deg, r.curvature_mismatch, r.point_gap
    );
}

/// A cylinder about z of radius 10 and 30 high with its top face taken off; points around the rim.
fn open_tube() -> (Body, Vec<Vec3>) {
    let solid = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 30.0).unwrap();
    let open = copy_faces(&solid, &[&around(10.0, 15.0)[..], &[Vec3::ZERO]].concat()).unwrap();
    assert!(open.is_surface());
    let rim = (0..8).map(|k| Vec2::from_angle(k as f64 * 0.785).extend(0.0) * 10.0 + Vec3::Z * 30.0).collect();
    (open, rim)
}

/// `body` without its planar faces but those at height `z0` or `z1` (the ends of a cylinder):
/// a cut solid's walls (and ends) as a surface, the faces the cut made taken away.
fn curved_and_ends(body: &Body, ends: &[f64]) -> Body {
    let drop: Vec<usize> = body
        .solid
        .face_iter()
        .enumerate()
        .filter(|(_, f)| match f.surface() {
            mt::Surface::Plane(p) => !ends.iter().any(|z| (p.origin().z - z).abs() < 1e-9 && p.normal().z.abs() > 1.0 - 1e-12),
            _ => false,
        })
        .map(|(i, _)| i)
        .collect();
    let open = crate::remove_faces(body, &drop).unwrap();
    assert!(open.is_surface());
    open
}

/// A point on each edge that bounds only one face of `body` (its free edges): the middle of
/// each, where the edge picker samples it.
fn free_edge_points(body: &Body) -> Vec<Vec3> {
    let mut uses: std::collections::HashMap<mt::EdgeID, (usize, mt::Edge)> = std::collections::HashMap::new();
    for f in body.solid.face_iter() {
        for e in f.boundaries().iter().flat_map(|w| w.edge_iter()) {
            uses.entry(e.id()).or_insert((0, e.clone())).0 += 1;
        }
    }
    let mut points = uses
        .values()
        .filter(|(n, _)| *n == 1)
        .map(|(_, e)| {
            let c = e.curve();
            let (t0, t1) = c.range_tuple();
            from_p3(c.subs(0.5 * (t0 + t1)))
        })
        .collect::<Vec<_>>();
    // In a fixed order, whatever the map's.
    points.sort_by(|a, b| (a.z, a.y, a.x).partial_cmp(&(b.z, b.y, b.x)).unwrap());
    points
}

/// Points round the circle of radius `r` about z at height `z`, every 30°.
fn around(r: f64, z: f64) -> Vec<Vec3> {
    (0..12).map(|k| Vec2::from_angle(0.1 + k as f64 * std::f64::consts::TAU / 12.0).extend(0.0) * r + Vec3::Z * z).collect()
}

fn radial(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.y, 0.0).normalized().unwrap()
}

#[test]
fn an_opened_tube_end_capped_g1_and_g2_meets_the_wall() {
    let (open, rim) = open_tube();
    let mut kappas = Vec::new();
    for g in [Continuity::G1, Continuity::G2] {
        let name = format!("tube end {}", g.as_str());
        let started = std::time::Instant::now();
        let (fill, r) = patch_edges_report(&open, &rim, &spec(g)).unwrap();
        eprintln!("{name}: filled in {:?}", started.elapsed());
        report(&name, &r);
        assert!(r.general);
        let area = check_face(&name, &open, &fill);
        // The wall's normal is radial and it does not bend along its axis.
        let (gap, angle, kappa) = along_the_loop(&fill, &radial, &|_| 0.0);
        eprintln!(
            "{name}: along the rim (1600 points): within {gap:e} of it, normal within {angle:.5}° of the wall's, curvature across {kappa:e} /mm (the wall's 0)"
        );
        assert!(angle < 0.1, "{name}: the normal turns {angle}° from the wall's");
        kappas.push(kappa);
        // A dome over the rim, not a flat lid: more than the disc.
        assert!(area > std::f64::consts::PI * 100.0 * 1.05, "{name}: {area}");
        // Sealed: the tube's volume plus the dome's, the dome's by the divergence theorem over the
        // patch (∫∫ (z − 30)·n_z dA), independent of the mesher.
        let closed = stitch(&[&open, &fill], 0.0).unwrap();
        assert!(!closed.is_surface(), "{name}: the cap closes the tube");
        let face = fill.solid.face_iter().next().unwrap().clone();
        let dome = quadrature(&face, 500, &|p, n| (p.z - 30.0) * n.z).abs();
        let (got, want) = (measure(&closed).unwrap().volume, std::f64::consts::PI * 100.0 * 30.0 + dome);
        eprintln!("{name}: sealed volume {got:.3}, tube + dome {want:.3}");
        assert!((got - want).abs() < 2e-4 * want, "{name}: {got} vs {want}");
    }
    eprintln!("tube end: curvature across the rim g1 {:e}, g2 {:e} /mm", kappas[0], kappas[1]);
    assert!(kappas[1] < kappas[0], "g2 holds the wall's curvature closer than g1");
}

/// A cylinder about z (radius 10, 40 high) with a window `half` either side of the x axis
/// through its wall, from z = 15 to 25; the wall and both ends without the faces the cut made (the
/// window's edges bound only the wall), and a point on each of the window's edges.
fn windowed(half: f64) -> (Body, Body, Vec<Vec3>) {
    let solid = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
    let x = 10.0 * half.cos();
    let tool = box_solid(Vec3::new(x, -11.0, 15.0), Vec3::new(11.0, 11.0, 25.0)).unwrap();
    let cut = boolean(&solid, &tool, BoolOp::Cut).unwrap().unwrap();
    let open = curved_and_ends(&cut, &[0.0, 40.0]);
    let rim = free_edge_points(&open);
    (solid, open, rim)
}

#[test]
fn a_window_cut_through_a_cylinder_wall_past_half_way_fills_g1_close_to_the_cylinder() {
    // 240° round: seen along any direction the window folds over itself, so it bounds no height
    // field; the old fill refused it.
    let half = 2.0 * std::f64::consts::PI / 3.0;
    let (solid, open, rim) = windowed(half);
    for g in [Continuity::G0, Continuity::G1] {
        let name = format!("window {}", g.as_str());
        let (fill, r) = patch_edges_report(&open, &rim, &spec(g)).unwrap();
        report(&name, &r);
        let area = check_face(&name, &open, &fill);
        let (gap, angle, _) = along_the_loop(&fill, &radial, &|_| 0.0);
        eprintln!("{name}: along the window (1600 points): within {gap:e}, normal within {angle:.5}° of the wall's");
        // How far the fill strays from the cylinder it replaces, inside.
        let face = fill.solid.face_iter().next().unwrap().clone();
        let s = face.surface();
        let (Some((u0, u1)), Some((v0, v1))) = s.try_range_tuple() else { panic!() };
        let mut stray = 0.0f64;
        for i in 0..=40 {
            for j in 0..=40 {
                let p = from_p3(s.subs(u0 + (u1 - u0) * i as f64 / 40.0, v0 + (v1 - v0) * j as f64 / 40.0));
                let ang = p.y.atan2(p.x);
                if ang.abs() < half - 0.05 && p.z > 15.5 && p.z < 24.5 {
                    stray = stray.max((Vec2::new(p.x, p.y).len() - 10.0).abs());
                }
            }
        }
        let exact = 10.0 * 2.0 * half * 10.0;
        let closed = stitch(&[&open, &fill], 0.0).unwrap();
        let (got, want) = (measure(&closed).unwrap().volume, measure(&solid).unwrap().volume);
        eprintln!(
            "{name}: area {area:.3} (the cylinder's window {exact:.3}), the fill within {stray:.4} of the cylinder inside, sealed volume {got:.2} against the whole cylinder's {want:.2}"
        );
        assert!(!closed.is_surface());
        if g == Continuity::G1 {
            assert!(angle < 0.1, "{name}: the normal turns {angle}° from the wall's");
            assert!((area - exact).abs() < 0.01 * exact && stray < 0.25, "{name}: {area} vs {exact}, stray {stray}");
            assert!((got - want).abs() < 0.005 * want, "{name}: {got} vs {want}");
        }
    }
}

#[test]
fn a_saddle_window_in_a_torus_fills_g0_and_g1() {
    // The inner wall of a torus (major 20, minor 8) is saddle-shaped; a window 240° round it
    // between z = ±3 bounds no height field.
    let solid = torus(Vec3::ZERO, 20.0, 8.0).unwrap();
    let tool = box_solid(Vec3::new(-6.0, -13.0, -3.0), Vec3::new(13.0, 13.0, 3.0)).unwrap();
    let cut = boolean(&solid, &tool, BoolOp::Cut).unwrap().unwrap();
    let open = curved_and_ends(&cut, &[]);
    let rim = free_edge_points(&open);
    // The torus's normal: from the tube's centre circle.
    let normal = |p: Vec3| {
        let c = Vec3::new(p.x, p.y, 0.0).normalized().unwrap() * 20.0;
        (p - c).normalized().unwrap()
    };
    for g in [Continuity::G0, Continuity::G1] {
        let name = format!("torus window {}", g.as_str());
        let (fill, r) = patch_edges_report(&open, &rim, &spec(g)).unwrap();
        report(&name, &r);
        check_face(&name, &open, &fill);
        let (gap, angle, _) = along_the_loop(&fill, &normal, &|_| 0.0);
        eprintln!("{name}: along the window (1600 points): within {gap:e}, normal within {angle:.5}° of the torus's");
        if g == Continuity::G1 {
            assert!(angle < 0.1, "{name}: {angle}°");
        }
    }
}

/// A cylinder about z (radius 10, 40 high) with a pentagonal pocket cut into its wall from the
/// +x side (the pentagon in the yz plane about z = 20, circumradius 6): the wall and both ends
/// copied out, and a point on each of the pocket rim's five edges.
fn pentagon_window() -> (Body, Vec<Vec3>) {
    let solid = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 40.0).unwrap();
    let corners: Vec<Vec2> = (0..5).map(|k| Vec2::from_angle(0.3 + k as f64 * std::f64::consts::TAU / 5.0) * 6.0 + Vec2::new(0.0, 20.0)).collect();
    let prism = extrude(&Plane::YZ, &[Region2 { outer: Loop2::polygon(&corners), holes: vec![] }], 0.0, 15.0).unwrap().pop().unwrap();
    let cut = boolean(&solid, &prism, BoolOp::Cut).unwrap().unwrap();
    let open = curved_and_ends(&cut, &[0.0, 40.0]);
    let rim = free_edge_points(&open);
    (open, rim)
}

#[test]
fn a_five_sided_window_fills_as_one_face_g1_and_through_a_point() {
    let (open, rim) = pentagon_window();
    let (fill, r) = patch_edges_report(&open, &rim, &spec(Continuity::G1)).unwrap();
    report("pentagon g1", &r);
    assert!(fill.edge_count() >= 5, "the rim's own edges (five, or more where a seam splits one)");
    check_face("pentagon g1", &open, &fill);
    let (gap, angle, _) = along_the_loop(&fill, &radial, &|_| 0.0);
    eprintln!("pentagon g1: along the rim: within {gap:e}, normal within {angle:.5}° of the wall's");
    assert!(angle < 0.1, "{angle}°");
    // Through a point half a millimetre out from the wall in the middle (G0 and G1).
    let point = Vec3::new(10.5, 0.0, 20.0);
    for g in [Continuity::G0, Continuity::G1] {
        let name = format!("pentagon {} through a point", g.as_str());
        let spec = PatchSpec { continuity: g, points: vec![point], ..PatchSpec::default() };
        let (fill, r) = patch_edges_report(&open, &rim, &spec).unwrap();
        report(&name, &r);
        check_face(&name, &open, &fill);
        let s = fill.solid.face_iter().next().unwrap().surface();
        let (u, v) = nearest(&s, point, None);
        let off = from_p3(s.subs(u, v)).dist(point);
        eprintln!("{name}: the point is {off:e} off the surface");
        assert!(off <= face_tolerance(&open), "{name}: {off}");
    }
}

#[test]
fn a_hexagonal_rim_fills_through_a_point_and_g1_to_walls_meeting_at_angles_holds_away_from_the_corners() {
    // A hexagonal prism's top cut by a sideways cylinder: six walls meeting at 120°.
    let corners: Vec<Vec2> = (0..6).map(|k| Vec2::from_angle(0.3 + k as f64 * std::f64::consts::TAU / 6.0) * 20.0).collect();
    let prism = extrude(&Plane::XY, &[Region2 { outer: Loop2::polygon(&corners), holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap();
    let tool = cylinder(Vec3::new(0.0, -60.0, 50.0), Vec3::Y, 40.0, 120.0).unwrap();
    let solid = boolean(&prism, &tool, BoolOp::Cut).unwrap().unwrap();
    let mut keep = vec![Vec3::ZERO];
    let mut rim = Vec::new();
    for k in 0..6 {
        let m = corners[k].lerp(corners[(k + 1) % 6], 0.5);
        keep.push(m.extend(5.0));
        rim.push(m.extend(50.0 - (1600.0 - m.x * m.x).sqrt()));
    }
    let open = copy_faces(&solid, &keep).unwrap();
    let plain = patch_edges(&open, &rim).unwrap();
    let sealed = |f: &Body| measure(&stitch(&[&open, f], 0.0).unwrap()).unwrap().volume;
    // Six sides, G0, through a point 3 mm above the middle of the plain fill: one face.
    let s = plain.solid.face_iter().next().unwrap().surface();
    let (u, v) = nearest(&s, Vec3::new(0.0, 0.0, 12.0), None);
    let point = from_p3(s.subs(u, v)) + Vec3::Z * 3.0;
    let (lifted, r) = patch_edges_report(&open, &rim, &PatchSpec { points: vec![point], ..PatchSpec::default() }).unwrap();
    report("hexagon through a point", &r);
    assert_eq!(lifted.edge_count(), 6, "the rim's own six edges");
    check_face("hexagon through a point", &open, &lifted);
    let s = lifted.solid.face_iter().next().unwrap().surface();
    let (u, v) = nearest(&s, point, None);
    let off = from_p3(s.subs(u, v)).dist(point);
    eprintln!("hexagon through a point: the point is {off:e} off the surface; sealed {:.2} against {:.2} plain", sealed(&lifted), sealed(&plain));
    assert!(off <= face_tolerance(&open) && sealed(&lifted) > sealed(&plain));
    // G1 to the walls: at a corner no surface is tangent to both walls (its tangent plane there
    // holds both rim edges), so G1 is asked only away from the corners. On walls standing square
    // to the patch the corner is pinched too sharply for the finest grid to hold the rim within
    // the fill's tolerance, and the fill says so.
    match patch_edges_report(&open, &rim, &spec(Continuity::G1)) {
        Ok((fill, r)) => {
            report("hexagon g1", &r);
            check_face("hexagon g1", &open, &fill);
            assert!(r.normal_deviation_deg <= PATCH_ANGLE_TOL_DEG);
            assert!(sealed(&fill) > sealed(&plain));
        }
        Err(e) => {
            eprintln!("hexagon g1: {e}");
            assert!(e.to_string().contains("not supported yet"), "{e}");
        }
    }
}

#[test]
fn per_edge_continuity_and_hostile_specs() {
    let (open, rim) = pentagon_window();
    // G1 on two edges only.
    let spec = PatchSpec { edges: vec![(rim[0], Continuity::G1), (rim[2], Continuity::G2)], ..PatchSpec::default() };
    let (fill, r) = patch_edges_report(&open, &rim, &spec).unwrap();
    report("pentagon, two edges g1/g2", &r);
    check_face("pentagon, two edges g1/g2", &open, &fill);
    let bad = |spec: PatchSpec| patch_edges_report(&open, &rim, &spec).is_err();
    let g1 = PatchSpec { continuity: Continuity::G1, ..PatchSpec::default() };
    assert!(bad(PatchSpec { continuity: Continuity::G3, ..g1.clone() }));
    assert!(bad(PatchSpec { edges: vec![(rim[0], Continuity::G3)], ..g1.clone() }));
    assert!(bad(PatchSpec { edges: vec![(Vec3::new(0.0, 0.0, 40.0), Continuity::G1)], ..g1.clone() }), "an edge off the loop");
    assert!(bad(PatchSpec { edges: vec![(Vec3::new(f64::NAN, 0.0, 0.0), Continuity::G1)], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(f64::INFINITY, 0.0, 0.0)], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(10.0, 0.0, 20.0); 65], ..g1.clone() }));
    assert!(bad(PatchSpec { points: vec![Vec3::new(-10.0, 0.0, 20.0)], ..g1.clone() }), "a point off the far side");
    for crown in [f64::NAN, 3.5, -1.0] {
        assert!(bad(PatchSpec { crown, ..g1.clone() }), "crown {crown}");
    }
    assert!(bad(PatchSpec { crown: 0.1, points: vec![Vec3::new(10.2, 0.0, 20.0)], ..PatchSpec::default() }), "a crown with no g1 edge");
    // A crown fills the middle out from the wall (the side the wall's normals face) and leaves
    // the loop, and G1 across it, as they were.
    let (plain, _) = patch_edges_report(&open, &rim, &g1).unwrap();
    let (full, r) = patch_edges_report(&open, &rim, &PatchSpec { crown: 0.1, ..g1.clone() }).unwrap();
    report("pentagon g1 crown 0.1", &r);
    check_face("pentagon g1 crown 0.1", &open, &full);
    let (_, angle, _) = along_the_loop(&full, &radial, &|_| 0.0);
    let middle = |b: &Body| {
        let s = b.solid.face_iter().next().unwrap().surface();
        let (u, v) = nearest(&s, Vec3::new(10.0, 0.0, 20.0), None);
        let p = from_p3(s.subs(u, v));
        Vec2::new(p.x, p.y).len()
    };
    let (a, b) = (middle(&plain), middle(&full));
    eprintln!("pentagon g1: the middle {a:.4} from the axis plain, {b:.4} with crown 0.1; normal within {angle:.5}° of the wall's");
    assert!(b > a + 0.5 && angle < 0.1, "{a} {b} {angle}");
    // The edges of a closed solid lie between two faces: g1 has no one face to meet.
    let b = box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 10.0)).unwrap();
    let top = [Vec3::new(5.0, 0.0, 10.0), Vec3::new(10.0, 5.0, 10.0), Vec3::new(5.0, 10.0, 10.0), Vec3::new(0.0, 5.0, 10.0)];
    assert!(patch_edges_report(&b, &top, &PatchSpec { continuity: Continuity::G1, ..PatchSpec::default() }).is_err());
}
