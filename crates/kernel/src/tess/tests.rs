use std::collections::HashMap;

use solidcraft_geom::{Mesh, Vec3};

use super::*;
use crate::{Body, BoolOp};

/// Every triangle edge, with vertices merged by exact position, is shared by exactly two
/// triangles: the mesh is closed and its faces meet exactly (no cracks, no T-junctions).
fn assert_watertight(m: &Mesh, name: &str) {
    // By value: −0.0 and 0.0 are one position (adding 0.0 makes −0.0 into 0.0).
    let key = |i: u32| m.positions.get(i as usize).map(|p| [(p.x + 0.0).to_bits(), (p.y + 0.0).to_bits(), (p.z + 0.0).to_bits()]);
    let mut uses: HashMap<([u64; 3], [u64; 3]), i32> = HashMap::new();
    for t in &m.triangles {
        for e in 0..3 {
            let (Some(a), Some(b)) = (key(t[e]), key(t[(e + 1) % 3])) else { panic!("{name}: bad index") };
            if a == b {
                continue;
            }
            // Directed: a closed, consistently oriented mesh uses each edge once each way.
            *uses.entry((a, b)).or_default() += 1;
            *uses.entry((b, a)).or_default() -= 1;
        }
    }
    let open: Vec<_> = uses.iter().filter(|(_, n)| **n != 0).collect();
    assert!(open.is_empty(), "{name}: {} unmatched directed edges, e.g. {:?}", open.len(), open.first());
}

fn plan_body(b: &Body, tol: f64) -> (Mesh, PlanStats) {
    match plan(&b.solid, tol, Some(crate::tess::size::Turn { angle: crate::DISPLAY_ANGLE, floor: tol * 1e-3 }), b.boundary_tol) {
        Ok((p, stats)) => (p.evaluate(), stats),
        Err(e) => panic!("plan: {e}"),
    }
}

fn check(name: &str, b: &Body, tol: f64, volume: f64) -> PlanStats {
    let (m, stats) = plan_body(b, tol);
    assert_watertight(&m, name);
    let got = m.measure().volume;
    // Chord error tol over the surface: the volume is short by at most about area · tol.
    let area = m.measure().area;
    assert!((got - volume).abs() <= area * tol * 1.5 + 1e-9, "{name}: volume {got} vs {volume} (area {area}, tol {tol})");
    // Outward normals: the divergence theorem gives a positive volume only if they are.
    assert!(got > 0.0, "{name}: inside out");
    let faces = b.solid.face_iter().count();
    assert_eq!(stats.planar + stats.structured + stats.delaunay, faces, "{name}: every face planned");
    stats
}

fn ok<T>(r: crate::Result<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
    }
}

fn cut(a: &Body, b: &Body) -> Body {
    match crate::boolean(a, b, BoolOp::Cut) {
        Ok(Some(r)) => r,
        other => panic!("cut: {other:?}"),
    }
}

const PI: f64 = std::f64::consts::PI;

/// Smallest corner angle (degrees) over the mesh.
fn min_angle(m: &Mesh) -> f64 {
    let mut least = 180.0f64;
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        for (p, q, r) in [(a, b, c), (b, c, a), (c, a, b)] {
            let (u, v) = (q - p, r - p);
            if let (Some(u), Some(v)) = (u.normalized(), v.normalized()) {
                least = least.min(u.dot(v).clamp(-1.0, 1.0).acos().to_degrees());
            }
        }
    }
    least
}

#[test]
fn a_box_is_a_regular_grid_on_every_face() {
    // 40 × 30 × 20: sides at most 1/5 of the diagonal (10.77 mm), so 4 × 3 × 2 cells.
    let b = ok(crate::box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)));
    let (m, s) = plan_body(&b, 0.05);
    assert_eq!(s.planar, 6);
    assert_watertight(&m, "box");
    let cells = 2 * (4 * 3 + 4 * 2 + 3 * 2);
    assert_eq!(m.triangles.len(), 2 * cells, "two triangles per grid cell");
    assert!(min_angle(&m) > 40.0, "{}", min_angle(&m));
}

#[test]
fn primitives_are_watertight_with_the_right_volume() {
    for tol in [0.05, 0.005] {
        let s = check("box", &ok(crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 6.0, 4.0))), tol, 240.0);
        assert_eq!(s, PlanStats { planar: 6, structured: 0, delaunay: 0, filled: 0 });
        let s = check("cylinder", &ok(crate::cylinder(Vec3::new(1.0, 2.0, 3.0), Vec3::new(0.0, 1.0, 1.0), 2.5, 7.0)), tol, PI * 2.5 * 2.5 * 7.0);
        assert_eq!(s.delaunay, 0, "cylinder walls are structured");
        let s = check("torus", &ok(crate::torus(Vec3::ZERO, 6.0, 1.5)), tol, 2.0 * PI * PI * 6.0 * 1.5 * 1.5);
        assert_eq!(s.delaunay, 0, "torus patches are structured");
    }
}

#[test]
fn holes_and_trimmed_faces_are_watertight() {
    let tol = 0.01;
    let bx = ok(crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)));
    let pin = ok(crate::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0));
    check("box with a hole", &cut(&bx, &pin), tol, 400.0 - PI * 4.0 * 4.0);
    // A cylinder with a cross hole: trimmed cylinder halves (an iso-line grid cut by the hole)
    // and intersection-curve walls; no face needs Delaunay.
    let cyl = ok(crate::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0));
    let cross = ok(crate::cylinder(Vec3::new(5.0, -1.0, 2.0), Vec3::Y, 1.0, 12.0));
    let body = cut(&cyl, &cross);
    let (m, stats) = plan_body(&body, tol);
    assert_watertight(&m, "cross hole");
    assert_eq!(stats.delaunay, 0, "{stats:?}");
    // Against the reference mesher's volume (no closed form for the two-cylinder intersection).
    let Ok(reference) = body.tessellate(tol * 0.2) else { panic!("reference mesh") };
    let (a, b) = (m.measure().volume, reference.measure().volume);
    assert!((a - b).abs() < 0.01 * b, "cross hole volume {a} vs {b}");
}

#[test]
fn faces_and_edges_keep_the_body_order() {
    let body = ok(crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 5.0));
    let (m, _) = plan_body(&body, 0.01);
    let Ok(reference) = body.tessellate(0.01) else { panic!("reference mesh") };
    assert_eq!(m.edges.len(), reference.edges.len());
    assert_eq!(m.edge_faces, reference.edge_faces);
    // Same faces in the same order: each face's area agrees with the reference mesher's.
    let face_area = |m: &Mesh, f: u32| -> f64 {
        m.triangles
            .iter()
            .zip(&m.tri_face)
            .filter(|(_, tf)| **tf == f)
            .map(|(t, _)| {
                let p = |i: u32| m.positions.get(i as usize).copied().unwrap_or_default();
                (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0])).len() * 0.5
            })
            .sum()
    };
    for f in 0..body.solid.face_iter().count() as u32 {
        let (a, b) = (face_area(&m, f), face_area(&reference, f));
        assert!((a - b).abs() < 0.01 * b.max(1e-9), "face {f}: {a} vs {b}");
    }
    // Each edge polyline ends on the same points as the reference's.
    for (a, b) in m.edges.iter().zip(&reference.edges) {
        assert!(a.first().zip(b.first()).is_some_and(|(p, q)| p.dist(*q) < 1e-9));
        assert!(a.last().zip(b.last()).is_some_and(|(p, q)| p.dist(*q) < 1e-9));
    }
}

/// Side-by-side wireframes, reference mesher (left) and plan (right):
/// `SOLIDCRAFT_TESS_SVG=<dir> cargo test -p solidcraft-kernel render_wireframes -- --ignored`.
#[test]
#[ignore]
fn render_wireframes() {
    let Ok(dir) = std::env::var("SOLIDCRAFT_TESS_SVG") else { return };
    let tol = 0.02;
    let bx = ok(crate::box_solid(Vec3::ZERO, Vec3::new(10.0, 10.0, 4.0)));
    let pin = ok(crate::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0));
    let cross = ok(crate::cylinder(Vec3::new(5.0, -1.0, 2.0), Vec3::Y, 1.0, 12.0));
    let filleted =
        ok(crate::fillet(&bx, &[Vec3::new(5.0, 0.0, 4.0), Vec3::new(10.0, 5.0, 4.0), Vec3::new(5.0, 10.0, 4.0), Vec3::new(0.0, 5.0, 4.0)], 1.5));
    let shapes = [
        ("box_hole", cut(&bx, &pin)),
        ("cylinder", pin.clone()),
        ("cross_hole", cut(&pin, &cross)),
        ("torus", ok(crate::torus(Vec3::ZERO, 6.0, 1.5))),
        ("fillet", filleted),
    ];
    for (name, body) in shapes {
        let t0 = std::time::Instant::now();
        let Ok(old) = body.tessellate(tol) else { panic!("reference") };
        let t_old = t0.elapsed().as_secs_f64() * 1e3;
        let t0 = std::time::Instant::now();
        let new = match plan(&body.solid, tol, Some(crate::tess::size::Turn { angle: crate::DISPLAY_ANGLE, floor: tol * 1e-3 }), body.boundary_tol) {
            Ok((p, st)) => Some((p.evaluate(), st)),
            Err(e) => {
                println!("{name}: no plan ({e})");
                None
            }
        };
        let t_new = t0.elapsed().as_secs_f64() * 1e3;
        let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1600' height='860' style='background:#fff'>");
        let label = |x: f64, text: String| format!("<text x='{x}' y='40' font-family='sans-serif' font-size='26'>{text}</text>");
        svg += &label(20.0, format!("meshalgo (Delaunay): {} tris, {t_old:.1} ms", old.triangles.len()));
        svg += &draw(&old, 0.0);
        if let Some((m, st)) = &new {
            svg += &label(
                820.0,
                format!("plan: {} tris, {t_new:.1} ms  [{} planar, {} grid, {} cdt]", m.triangles.len(), st.planar, st.structured, st.delaunay),
            );
            svg += &draw(m, 800.0);
        }
        svg += "</svg>";
        let _ = std::fs::write(format!("{dir}/{name}.svg"), svg);
    }
}

/// Isometric, painter-sorted, flat-shaded triangles with their edges, in an 800×800 panel.
fn draw(m: &Mesh, x0: f64) -> String {
    let (c30, s30) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let proj = |p: Vec3| -> (f64, f64, f64) {
        // Turn 45° about Z, tilt 30° about X; depth along the view.
        let (x, y) = ((p.x - p.y) * std::f64::consts::FRAC_1_SQRT_2, (p.x + p.y) * std::f64::consts::FRAC_1_SQRT_2);
        (x, p.z * c30 - y * s30, y * c30 + p.z * s30)
    };
    let pts: Vec<(f64, f64, f64)> = m.positions.iter().map(|p| proj(*p)).collect();
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for p in &pts {
        lo = (lo.0.min(p.0), lo.1.min(p.1));
        hi = (hi.0.max(p.0), hi.1.max(p.1));
    }
    let k = 700.0 / (hi.0 - lo.0).max(hi.1 - lo.1).max(1e-9);
    let sx = |p: (f64, f64, f64)| (x0 + 50.0 + (p.0 - lo.0) * k, 800.0 - (p.1 - lo.1) * k);
    let mut tris: Vec<(f64, [u32; 3], u32)> =
        m.triangles.iter().zip(&m.tri_face).map(|(t, f)| (t.iter().map(|&i| pts[i as usize].2).sum::<f64>(), *t, *f)).collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    let light = Vec3::new(0.3, -0.5, 0.8).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let mut out = String::new();
    let tint = [(1.0, 1.0, 1.15), (1.15, 1.0, 1.0), (1.0, 1.15, 1.0), (1.1, 1.1, 0.9), (0.9, 1.1, 1.1), (1.1, 0.9, 1.1)];
    for (_, t, f) in tris {
        let (tr, tg, tb) = tint[f as usize % tint.len()];
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let n = (b - a).cross(c - a).normalized().unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let shade = (90.0 + 150.0 * n.dot(light).max(0.0)) as u8;
        let q: Vec<(f64, f64)> = t.iter().map(|&i| sx(pts[i as usize])).collect();
        // Back faces away (screen y points down, so outward triangles wind clockwise here).
        if (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[2].0 - q[0].0) * (q[1].1 - q[0].1) > 0.0 {
            continue;
        }
        let pp: Vec<String> = q.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        out += &format!(
            "<polygon points='{}' fill='rgb({},{},{})' stroke='#1d3557' stroke-width='0.5'/>",
            pp.join(" "),
            (f64::from(shade) * tr).min(255.0) as u8,
            (f64::from(shade) * tg).min(255.0) as u8,
            (f64::from(shade) * tb).min(255.0) as u8,
        );
    }
    out
}

/// Blind drilled holes (a box cut by a revolved drill with a 118° point, as the hole feature
/// builds it): the point's apex is a pole of the cone faces, two copies of one boundary node.
/// The plan is closed and has no triangle on both copies (no zero-area triangles), also where
/// such a triangle carried a boundary side of the cone.
#[test]
fn a_drill_point_apex_leaves_no_zero_area_triangles() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    // (box size, hole position, radius, depth)
    let cases =
        [(Vec3::new(40.0, 30.0, 15.0), Vec3::new(20.0, 15.0, 15.0), 3.4, 8.0), (Vec3::new(60.0, 40.0, 15.0), Vec3::new(50.0, 20.0, 15.0), 3.0, 10.0)];
    for (size, at, r, depth) in cases {
        let bx = ok(crate::box_solid(Vec3::ZERO, size));
        let tip = r / (118f64 / 2.0).to_radians().tan();
        let Some(rp) = Plane::new(at, Vec3::X, Vec3::Z) else { panic!("plane") };
        let pts = [Vec2::new(0.0, 0.6), Vec2::new(r, 0.6), Vec2::new(r, -depth), Vec2::new(0.0, -depth - tip)];
        let region = Region2 { outer: Loop2::polygon(&pts), holes: vec![] };
        let Some(drill) = ok(crate::revolve(&rp, &[region], Vec2::ZERO, Vec2::Y, std::f64::consts::TAU)).pop() else { panic!("drill") };
        let body = cut(&bx, &drill);
        let name = format!("hole r {r} depth {depth}");
        let (m, _) = plan_body(&body, 0.02);
        assert_watertight(&m, &name);
        for t in &m.triangles {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            assert!((b - a).cross(c - a).len() > 1e-12, "{name}: zero-area triangle {a:?} {b:?} {c:?}");
        }
    }
}

#[test]
fn a_placed_body_meshes_like_the_original() {
    let b = ok(crate::box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)));
    let placed = ok(crate::transform(&b, Vec3::new(15.0, -2.0, 8.0), Vec3::ZERO, Vec3::X, 0.7));
    let (m, _) = plan_body(&b, 0.005);
    let (p, _) = plan_body(&placed, 0.005);
    assert_eq!(m.tri_face, p.tri_face);
}

/// Seeds on a motorcycle trace start at the spacing of the boundary it leaves and grow
/// geometrically to the spacing at its other end (here 0.1 to 1 over 5 mm).
#[test]
fn trace_seeds_grade_from_one_end_to_the_other() {
    let t = graded([0.0, 0.0], [5.0, 0.0], 0.1, 1.0, 10.0, [1.0, 1.0], &|_, _| true);
    let all: Vec<f64> = std::iter::once(0.0).chain(t.iter().copied()).chain(std::iter::once(1.0)).map(|f| f * 5.0).collect();
    let sizes: Vec<f64> = all.windows(2).map(|w| w[1] - w[0]).collect();
    assert!((sizes[0] - 0.1).abs() < 0.03, "first {}", sizes[0]);
    assert!((sizes[sizes.len() - 1] - 1.0).abs() < 0.3, "last {}", sizes[sizes.len() - 1]);
    assert!(sizes.windows(2).all(|w| w[1] >= w[0] - 1e-12), "growing: {sizes:?}");
    // Equal ends: even; a segment no longer than its spacing: no seeds.
    let even = graded([0.0, 0.0], [4.0, 0.0], 1.0, 1.0, 10.0, [1.0, 1.0], &|_, _| true);
    assert_eq!(even.len(), 3);
    assert!(graded([0.0, 0.0], [0.5, 0.0], 1.0, 1.0, 10.0, [1.0, 1.0], &|_, _| true).is_empty());
}

/// A surface that joins itself along a direction is periodic there: its parameters repeat, so
/// the evaluator takes a parameter outside the range back into it. A boundary that runs across
/// the join is unwrapped past the range's end and must still land on the surface.
#[test]
fn a_closed_spline_surface_is_periodic_and_evaluates_outside_its_range() {
    use solidcraft_geom::tess::EvalSurface;
    // A five-sided tube: the control net's last column repeats its first.
    let corner = |k: usize| {
        let a = std::f64::consts::TAU * (k % 5) as f64 / 5.0;
        (10.0 * a.cos(), 10.0 * a.sin())
    };
    let mut ctrl = Vec::new();
    for j in 0..2 {
        for k in 0..=5 {
            let (x, y) = corner(k);
            ctrl.push([x, y, f64::from(j) * 8.0, 1.0]);
        }
    }
    let s = EvalSurface::Nurbs {
        degree: [1, 1],
        knots: [vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 5.0], vec![0.0, 0.0, 1.0, 1.0]],
        count: [6, 2],
        ctrl,
        closed: [true, false],
    };
    let dom = uv::domain(&s);
    assert_eq!(dom.period, [Some(5.0), None]);
    assert_eq!(dom.range, [Some((0.0, 5.0)), Some((0.0, 1.0))]);
    // A parameter a whole range below the start is the same point as one inside it.
    for (a, b) in [(-0.5, 4.5), (-2.0, 3.0), (5.5, 0.5), (7.0, 2.0)] {
        let (Some(out), Some(inside)) = (s.derivs(a, 0.25), s.derivs(b, 0.25)) else { panic!("evaluates") };
        assert!(out.0.dist(inside.0) < 1e-12, "u = {a} and u = {b}: {:?} vs {:?}", out.0, inside.0);
    }
    // The same net left open is not periodic, and keeps truck's end-span extension outside it.
    let EvalSurface::Nurbs { degree, knots, count, ctrl, .. } = s else { panic!("nurbs") };
    let open = EvalSurface::Nurbs { degree, knots, count, ctrl, closed: [false, false] };
    assert_eq!(uv::domain(&open).period, [None, None]);
    let (Some(out), Some(inside)) = (open.derivs(-0.5, 0.25), open.derivs(4.5, 0.25)) else { panic!("evaluates") };
    assert!(out.0.dist(inside.0) > 1.0, "an open surface does not wrap");
}

/// Each loop of a face is projected on its own, so on a periodic surface a hole can come out a
/// whole turn away from the boundary that holds it. Every loop is brought into the outer
/// boundary's period, where the hole cuts the face again.
#[test]
fn holes_are_brought_into_the_outer_boundarys_period() {
    let tau = std::f64::consts::TAU;
    let at = |u: f64, v: f64| seam::Loop { uv: vec![[u, v], [u + 0.2, v], [u + 0.2, v + 0.2], [u, v + 0.2]], pts: vec![Vec3::ZERO; 4] };
    let dom = uv::Domain { range: [Some((0.0, 1.0)), Some((0.0, tau))], period: [None, Some(tau)] };
    let mut loops = vec![at(0.0, -tau), at(0.4, 1.5), at(0.6, 1.5 - tau)];
    align_periods(&mut loops, &dom);
    // The outer loop stays where it is; both holes land beside it, a whole turn moved or not.
    assert_eq!(loops[0].uv[0], [0.0, -tau]);
    assert!((loops[1].uv[0][1] - (1.5 - tau)).abs() < 1e-12, "{:?}", loops[1].uv[0]);
    assert!((loops[2].uv[0][1] - (1.5 - tau)).abs() < 1e-12, "{:?}", loops[2].uv[0]);
    // Nothing moves along a direction that does not repeat.
    assert_eq!(loops[1].uv[0][0], 0.4);
}

/// A boundary that steps out to a node and straight back encloses no area: the pair is dropped,
/// so what is left no longer runs along itself. The two sides need only agree within the given
/// distance — a STEP writer may draw them as two curves a hair apart.
#[test]
fn zero_width_slits_are_dropped_from_a_boundary() {
    let p = |x: f64| Vec3::new(x, 0.0, 0.0);
    // A square with a slit poked into it from the corner (2, 1): out to (2, 2) and back.
    let slit = vec![
        p(0.0),
        p(1.0),
        Vec3::new(2.0, 1.0, 0.0),
        Vec3::new(2.0, 2.0, 0.0),
        Vec3::new(2.0 + 1e-9, 1.0, 0.0),
        p(3.0),
        Vec3::new(3.0, 3.0, 0.0),
        Vec3::new(0.0, 3.0, 0.0),
    ];
    let (out, known) = drop_slits(&[slit], &[Vec::new()], 1e-6);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0], vec![p(0.0), p(1.0), Vec3::new(2.0, 1.0, 0.0), p(3.0), Vec3::new(3.0, 3.0, 0.0), Vec3::new(0.0, 3.0, 0.0)]);
    assert_eq!(known[0].len(), out[0].len());
    // Too far apart to be one slit: the boundary is left as it is.
    let wide = vec![p(0.0), p(1.0), Vec3::new(2.0, 1.0, 0.0), Vec3::new(2.0, 2.0, 0.0), Vec3::new(2.5, 1.0, 0.0), p(3.0)];
    let kept = drop_slits(std::slice::from_ref(&wide), &[Vec::new()], 1e-6).0;
    assert_eq!(kept, vec![wide]);
}

/// A pointed end read back from STEP (a screw's tip, a cone): its cone face is written as the
/// rim, the seam up to the apex and the seam back, and a sector of one as its rim and two
/// sides meeting at the apex. Unwrapped to the nearest copy at every step, the jump at the
/// apex made such a loop seem to wind around the cone; the face could not be cut along a seam
/// it did not have, and the whole body lost its display plan.
#[test]
fn a_pointed_end_read_back_from_step_is_planned() {
    use solidcraft_geom::{Loop2, Plane, Region2, Vec2};
    let (r, tip, len) = (3.0, 4.0, 20.0);
    let Some(axis) = Plane::new(Vec3::ZERO, Vec3::X, Vec3::Z) else { panic!("plane") };
    let pts = [Vec2::new(0.0, 0.0), Vec2::new(r, tip), Vec2::new(r, len), Vec2::new(0.0, len)];
    for turn in [360.0f64, 300.0, 270.0, 200.0] {
        let region = Region2 { outer: Loop2::polygon(&pts), holes: vec![] };
        let Some(body) = ok(crate::revolve(&axis, &[region], Vec2::ZERO, Vec2::Y, turn.to_radians())).pop() else { panic!("revolve") };
        let imported = ok(crate::step_import(&ok(crate::step_export(&[&body], "pointed end"))));
        let Some(copy) = imported.bodies.first() else { panic!("no body") };
        let volume = std::f64::consts::PI * r * r * (len - tip + tip / 3.0) * turn / 360.0;
        check(&format!("pointed end turned {turn} deg"), &copy.body, 0.02, volume);
    }
}

/// A point on a spline that turns many times (a thread's helical flank, as other CAD systems
/// write it to STEP, has hundreds of spans along the helix) is found from a hint a few turns
/// away: between 16 starts across the whole spline Newton settled on another turn, the point
/// was taken as off the surface and the body lost its display plan.
#[test]
fn a_point_on_a_long_helical_spline_projects() {
    let (turns, per_turn, pitch) = (40usize, 24usize, 0.5);
    let n = turns * per_turn + 1;
    let at = |i: usize, r: f64| {
        let t = std::f64::consts::TAU * i as f64 / per_turn as f64;
        [r * t.cos(), r * t.sin(), pitch * t / std::f64::consts::TAU, 1.0]
    };
    let mut ctrl: Vec<[f64; 4]> = Vec::with_capacity(2 * n);
    for i in 0..n {
        ctrl.push(at(i, 0.7));
        ctrl.push(at(i, 0.55));
    }
    let mut ku = vec![0.0; 3];
    ku.extend((1..n - 2).map(|i| i as f64 / (n - 2) as f64));
    ku.extend([1.0; 3]);
    let s = EvalSurface::Nurbs { degree: [2, 1], knots: [ku, vec![0.0, 0.0, 1.0, 1.0]], count: [n, 2], ctrl, closed: [false, false] };
    let dom = uv::domain(&s);
    for (target, hint) in [([0.5, 1.0], [0.488, 1.0]), ([0.2, 0.0], [0.23, 1.0]), ([0.9, 0.5], [0.1, 0.5])] {
        let Some((p, _, _)) = s.derivs(target[0], target[1]) else { panic!("evaluate") };
        let Some(q) = uv::project(&s, &dom, p, Some(hint), 1e-4) else { panic!("{target:?} from {hint:?}: not found") };
        let Some((back, _, _)) = s.derivs(q[0], q[1]) else { panic!("evaluate") };
        assert!(back.dist(p) <= 1e-4, "{target:?} from {hint:?}: {q:?} is {} away", back.dist(p));
    }
}

/// A sliver face as STEP files write one: two edges between the same two vertices, a rounding
/// apart, so its boundary runs out and back along itself. Its slits dropped, nothing is left to
/// bound a face (it is planned empty); a boundary enclosing anything keeps its loop.
#[test]
fn a_boundary_running_back_along_itself_is_all_slit() {
    let out: Vec<Vec3> = (0..5).map(|i| Vec3::new(f64::from(i), 0.5 * f64::from(i), 0.0)).collect();
    let mut sliver = out.clone();
    sliver.extend(out.iter().rev().skip(1).take(3).map(|p| *p + Vec3::new(0.0, 1e-14, 0.0)));
    let (left, _) = drop_slits(&[sliver], &[Vec::new()], 1e-9);
    assert!(left.iter().all(|l| l.len() < 3), "{left:?}");
    let mut lens = out.clone();
    lens.extend(out.iter().rev().skip(1).take(3).map(|p| *p + Vec3::new(0.0, 1e-3, 0.0)));
    let (kept, _) = drop_slits(std::slice::from_ref(&lens), &[Vec::new()], 1e-9);
    assert_eq!(kept, vec![lens]);
}

/// The display plan of `b` at `tol` (and the display angle), evaluated: closed, with no
/// zero-area or needle triangle.
#[cfg(feature = "smlib")]
fn closed_display_mesh(name: &str, b: &Body, tol: f64) -> Mesh {
    let plan = ok(b.display_plan_angular(tol, crate::DISPLAY_ANGLE));
    let m = ok(b.display_mesh_of(&plan));
    assert_watertight(&m, &format!("{name} at {tol}"));
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let longest = (b - a).len().max((c - b).len()).max((a - c).len());
        assert!((b - a).cross(c - a).len() > 1e-6 * longest * longest, "{name} at {tol}: degenerate triangle {a:?} {b:?} {c:?}");
    }
    m
}

/// A sphere from SMLib cut through its centre: the half sphere is bounded by its rim and its
/// seam from the pole up to the rim, used both ways. The seam's two uses share their nodes, so
/// the mesh has no crack along it and nothing degenerate at the pole.
#[cfg(feature = "smlib")]
#[test]
fn a_half_sphere_from_smlib_is_closed_along_its_seam() {
    use solidcraft_smlib::{BooleanOp, Brep as S};
    let Ok(ball) = S::make_sphere([0.0; 3], 12.0) else { panic!("sphere") };
    let Ok(lid) = S::make_box([-20.0, -20.0, 0.0], [40.0, 40.0, 20.0]) else { panic!("box") };
    let Ok(Some(half)) = ball.boolean(&lid, BooleanOp::Subtract) else { panic!("cut") };
    let body = ok(crate::native::body_from_smlib(&half));
    for tol in [0.1, 0.01] {
        closed_display_mesh("half sphere", &body, tol);
    }
}
