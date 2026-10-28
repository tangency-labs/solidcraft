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

/// The body's schema 4 record, which the planner takes.
fn record(b: &Body) -> solidcraft_geom::brep::Brep {
    ok(crate::brep_record(b))
}

fn plan_body(b: &Body, tol: f64) -> (Mesh, PlanStats) {
    match plan(&record(b), tol, Some(crate::tess::size::Turn { angle: crate::DISPLAY_ANGLE, floor: tol * 1e-3 })) {
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
    let faces = b.face_count();
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
        assert_eq!(s, PlanStats { planar: 6, ..PlanStats::default() });
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
    // Against the body's mesh at a fifth of the tolerance (no closed form for the two-cylinder
    // intersection).
    let Ok(reference) = body.mesh(&crate::MeshTolerance::chord(tol * 0.2)) else { panic!("finer mesh") };
    let (a, b) = (m.measure().volume, reference.measure().volume);
    assert!((a - b).abs() < 0.01 * b, "cross hole volume {a} vs {b}");
}

/// Through holes in pairs whose circles touch at one point (rig booleans/307): the plate's faces
/// have one loop round each pair, through their common vertex twice. Both passes are one point of
/// the face's (u, v), not two nodes the pcurves put 1e-14 apart, whose constraints crossed
/// ("boundary crosses itself").
#[test]
fn holes_touching_at_a_point_are_watertight() {
    let mut body = ok(crate::box_solid(Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 5.0)));
    for (x, y) in [(25.0, 0.0), (-25.0, 0.0), (0.0, 25.0), (0.0, -25.0), (15.0, 0.0), (-15.0, 0.0), (0.0, 15.0), (0.0, -15.0)] {
        body = cut(&body, &ok(crate::cylinder(Vec3::new(x, y, -1.0), Vec3::Z, 5.0, 7.0)));
    }
    check("holes touching at a point", &body, 0.01, 10000.0 * 5.0 - 8.0 * PI * 25.0 * 5.0);
}

#[test]
fn faces_and_edges_keep_the_body_order() {
    let body = ok(crate::cylinder(Vec3::ZERO, Vec3::Z, 2.0, 5.0));
    let (m, _) = plan_body(&body, 0.01);
    // Body::mesh, which plans the body's own record: the same faces and edges, in its order.
    let Ok(reference) = body.mesh(&crate::MeshTolerance::chord(0.01)) else { panic!("body mesh") };
    assert_eq!(m.edges.len(), reference.edges.len());
    assert_eq!(m.edge_faces, reference.edge_faces);
    // Same faces in the same order: each face's area agrees with the body mesh's.
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
    for f in 0..body.face_count() as u32 {
        let (a, b) = (face_area(&m, f), face_area(&reference, f));
        assert!((a - b).abs() < 0.01 * b.max(1e-9), "face {f}: {a} vs {b}");
    }
    // Each edge polyline ends on the same points as the body mesh's.
    for (a, b) in m.edges.iter().zip(&reference.edges) {
        assert!(a.first().zip(b.first()).is_some_and(|(p, q)| p.dist(*q) < 1e-9));
        assert!(a.last().zip(b.last()).is_some_and(|(p, q)| p.dist(*q) < 1e-9));
    }
}

/// Side-by-side wireframes, the body's mesh at the chord alone (left) and the plan at the
/// display angle (right):
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
        let Ok(old) = body.mesh(&crate::MeshTolerance::chord(tol)) else { panic!("body mesh") };
        let t_old = t0.elapsed().as_secs_f64() * 1e3;
        let t0 = std::time::Instant::now();
        let new = match plan(&record(&body), tol, Some(crate::tess::size::Turn { angle: crate::DISPLAY_ANGLE, floor: tol * 1e-3 })) {
            Ok((p, st)) => Some((p.evaluate(), st)),
            Err(e) => {
                println!("{name}: no plan ({e})");
                None
            }
        };
        let t_new = t0.elapsed().as_secs_f64() * 1e3;
        let mut svg = String::from("<svg xmlns='http://www.w3.org/2000/svg' width='1600' height='860' style='background:#fff'>");
        let label = |x: f64, text: String| format!("<text x='{x}' y='40' font-family='sans-serif' font-size='26'>{text}</text>");
        svg += &label(20.0, format!("chord alone: {} tris, {t_old:.1} ms", old.triangles.len()));
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

/// Distance of `p` from the analytic surface `fs`.
fn off_surface(fs: &crate::FaceSurface, p: Vec3) -> f64 {
    use crate::FaceSurface as F;
    match *fs {
        F::Plane { normal, point } => (p - point).dot(normal.vec()).abs(),
        F::Cylinder { origin, axis, radius } => {
            let v = p - origin;
            ((v - axis.vec() * v.dot(axis.vec())).len() - radius).abs()
        }
        F::Cone { apex, axis, half_angle } => {
            let v = p - apex;
            let h = v.dot(axis.vec());
            ((v - axis.vec() * h).len() * half_angle.cos() - h * half_angle.sin()).abs()
        }
        F::Sphere { center, radius } => ((p - center).len() - radius).abs(),
        F::Torus { center, axis, major, minor } => {
            let v = p - center;
            let h = v.dot(axis.vec());
            (((v - axis.vec() * h).len() - major.abs()).hypot(h) - minor).abs()
        }
    }
}

/// The farthest a triangle's centroid lies from its face's analytic surface, over the faces
/// that have one.
fn max_chord(b: &Body, m: &Mesh) -> f64 {
    let surfs: Vec<Option<crate::FaceSurface>> = (0..b.face_count()).map(|f| crate::face_surface(b, f)).collect();
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .filter_map(|(t, f)| {
            let fs = surfs.get(*f as usize).copied().flatten()?;
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            Some(off_surface(&fs, (a + b + c) * (1.0 / 3.0)))
        })
        .fold(0.0, f64::max)
}

/// The display plan of `b` at `tol` (and the display angle), evaluated: closed, with no
/// zero-area or needle triangle.
fn closed_display_mesh(name: &str, b: &Body, tol: f64) -> Mesh {
    let plan = ok(b.mesh_plan(&crate::MeshTolerance { chord: tol, angle: Some(crate::DISPLAY_ANGLE) }));
    let m = ok(b.mesh_of(&plan));
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
        let m = closed_display_mesh("half sphere", &body, tol);
        let chord = max_chord(&body, &m);
        assert!(chord <= tol * 1.05, "half sphere at {tol}: chord {chord}");
    }
}

/// Every triangle of a display plan lies within the chord tolerance of its face, whatever
/// layout the face got: on a doubly curved grid the sags along both directions add up at a
/// cell's middle, and a trimmed sphere face round a pocket is not a grid at all.
#[test]
fn display_plans_stay_within_the_chord_tolerance() {
    let ball = ok(crate::sphere(Vec3::ZERO, 12.0));
    let pocket = cut(&ok(crate::sphere(Vec3::ZERO, 10.0)), &ok(crate::box_solid(Vec3::new(-2.0, -2.0, 5.0), Vec3::new(2.0, 2.0, 11.0))));
    let ring = ok(crate::torus(Vec3::ZERO, 22.0, 4.0));
    for (name, body) in [("sphere", &ball), ("pocket in a sphere", &pocket), ("torus", &ring)] {
        for tol in [0.1, 0.01] {
            let m = closed_display_mesh(name, body, tol);
            let chord = max_chord(body, &m);
            assert!(chord <= tol * 1.05, "{name} at {tol}: chord {chord}");
        }
    }
}

/// SMLib's torus is one face bounded by its two seams: a grid over the whole parameter box.
#[cfg(feature = "smlib")]
#[test]
fn an_smlib_torus_stays_within_the_chord_tolerance() {
    let Ok(ring) = solidcraft_smlib::Brep::make_torus([0.0; 3], 22.0, 4.0) else { panic!("torus") };
    let body = ok(crate::native::body_from_smlib(&ring));
    for tol in [0.1, 0.01] {
        let m = closed_display_mesh("SMLib torus", &body, tol);
        let chord = max_chord(&body, &m);
        assert!(chord <= tol * 1.05, "SMLib torus at {tol}: chord {chord}");
    }
}

// Modelled threads from SMLib: a screw-swept groove cut from a bore or a shaft. The groove's
// walls are splines winding once per pitch for the thread's whole length, and they split the
// bore or shaft into one face per turn.

/// A groove of trapezoid `section` (radius, axial offset) in the plane through the z axis along
/// y, carried `turns` turns of `pitch` about z from `base`, as an SMLib solid.
#[cfg(feature = "smlib")]
fn smlib_groove(section: [(f64, f64); 4], base: f64, pitch: f64, turns: f64) -> solidcraft_smlib::Brep {
    use solidcraft_geom::{Loop2, Vec2};
    smlib_screw(Loop2::polygon(&section.map(|(r, z)| Vec2::new(r, z))), base, pitch, turns)
}

/// The `section` (radius, axial offset) in the plane through the z axis along y, carried
/// `turns` turns of `pitch` about z from `base`, as an SMLib solid.
#[cfg(feature = "smlib")]
fn smlib_screw(section: solidcraft_geom::Loop2, base: f64, pitch: f64, turns: f64) -> solidcraft_smlib::Brep {
    use solidcraft_geom::{Plane, Region2};
    let origin = Vec3::new(0.0, 0.0, base);
    let plane = Plane { origin, x: Vec3::Y, y: Vec3::Z };
    let region = Region2 { outer: section, holes: Vec::new() };
    let Some(numbered) = crate::numbered::number_regions(std::slice::from_ref(&region)).pop() else { panic!("profile") };
    let profile = numbered.outer.oriented(false);
    let groove = ok(crate::smlib_sweeps::screw(&plane, &crate::smlib_sweeps::pieces([&profile]), origin, Vec3::Z, pitch, turns));
    ok(crate::native::smlib_body(&groove))
}

#[cfg(feature = "smlib")]
fn smlib_cut(a: &solidcraft_smlib::Brep, b: &solidcraft_smlib::Brep) -> solidcraft_smlib::Brep {
    match a.boolean(b, solidcraft_smlib::BooleanOp::Subtract) {
        Ok(Some(r)) => r,
        other => panic!("cut: {other:?}"),
    }
}

/// The ISO basic profile's groove for pitch `p` and major diameter `d`: internal, from 0.1 p
/// inside the hole wall of radius `wall` out to the major radius; external, from the root out
/// past the shaft's surface (radius `wall`).
#[cfg(feature = "smlib")]
fn iso_groove(p: f64, d: f64, wall: f64, internal: bool) -> [(f64, f64); 4] {
    let h = p * 3f64.sqrt() / 2.0;
    let slope = (PI / 6.0).tan();
    let (r0, w0, r1, w1) = if internal {
        let r_in = wall.min(d / 2.0 - 5.0 * h / 8.0) - 0.1 * p;
        (r_in, (p / 8.0 + 2.0 * (d / 2.0 - r_in) * slope) / 2.0, d / 2.0, p / 16.0)
    } else {
        let (r_root, r_out) = (d / 2.0 - 5.0 * h / 8.0, wall.max(d / 2.0) + 0.05 * p);
        (r_root, p / 8.0, r_out, (p / 4.0 + 2.0 * (r_out - r_root) * slope) / 2.0)
    };
    [(r0, -w0), (r1, -w1), (r1, w1), (r0, w0)]
}

/// Each face's triangles, in 3D, cover as much area as its surface over the triangles' own
/// parameters, within 5% (a coarse tolerance on a small curved face, a few percent): nothing
/// zigzags across the face, which multiplies its area. The surface's area by the midpoint rule
/// on sixteen pieces of each parameter triangle.
#[cfg(feature = "smlib")]
fn assert_faces_lie_flat(name: &str, plan: &solidcraft_geom::tess::TessPlan) {
    for fp in &plan.faces {
        let Some(s) = fp.surface.and_then(|i| plan.surfaces.get(i as usize)) else { continue };
        let at = |i: u32| {
            fp.fixed.get(i as usize).copied().unwrap_or_else(|| {
                let q = fp.uv[i as usize];
                s.eval(q[0], q[1]).0
            })
        };
        let (mut flat, mut surface) = (0.0, 0.0);
        for t in &fp.triangles {
            let [a, b, c] = t.map(|i| fp.uv[i as usize]);
            let area = 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
            flat += 0.5 * (at(t[1]) - at(t[0])).cross(at(t[2]) - at(t[0])).len();
            const N: usize = 4;
            for i in 0..N {
                for j in 0..N - i {
                    // The upright piece, and the inverted one beside it where there is one.
                    let pieces: &[(f64, f64)] =
                        if i + j + 1 < N { &[(1.0 / 3.0, 1.0 / 3.0), (2.0 / 3.0, 2.0 / 3.0)] } else { &[(1.0 / 3.0, 1.0 / 3.0)] };
                    for &(di, dj) in pieces {
                        let (x, y) = ((i as f64 + di) / N as f64, (j as f64 + dj) / N as f64);
                        let q = [a[0] + (b[0] - a[0]) * x + (c[0] - a[0]) * y, a[1] + (b[1] - a[1]) * x + (c[1] - a[1]) * y];
                        if let Some((_, su, sv)) = s.derivs(q[0], q[1]) {
                            surface += su.cross(sv).len() * area / (N * N) as f64;
                        }
                    }
                }
            }
        }
        assert!((flat - surface).abs() <= 0.05 * surface, "{name}: face {} has {flat} mm² of triangles over {surface} mm² of surface", fp.face);
    }
}

/// #1995: an M8x1 thread through a 7 mm hole in a 20 mm block, both ends open. Truck meshed the
/// groove's winding walls with triangles reaching past both ends of the block; the body's mesh
/// now comes from the display plan, inside the block and with the analytic volume.
#[cfg(feature = "smlib")]
#[test]
fn a_modelled_thread_in_a_bore_meshes_inside_its_block() {
    use solidcraft_smlib::Brep as S;
    let (Ok(block), Ok(bore)) = (S::make_box([-10.0, -10.0, 0.0], [20.0, 20.0, 10.0]), S::make_cylinder([0.0, 0.0, -1.0], 3.5, 12.0)) else {
        panic!("stock")
    };
    let threaded = smlib_cut(&smlib_cut(&block, &bore), &smlib_groove(iso_groove(1.0, 8.0, 3.5, true), -1.0, 1.0, 12.0));
    let body = ok(crate::native::body_from_smlib(&threaded));
    let m = ok(body.mesh(&crate::MeshTolerance::chord(0.01)));
    let (lo, hi) = m
        .positions
        .iter()
        .fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    assert!(lo.z > -1e-6 && hi.z < 10.0 + 1e-6 && lo.x > -10.0 - 1e-6 && hi.x < 10.0 + 1e-6, "mesh outside the block: {lo:?} {hi:?}");
    // Block less the bore less ten turns of the groove where it is in the material (as the
    // kernel case works it out from the ISO profile).
    let volume = m.measure().volume;
    assert!((volume - 3567.1757).abs() < 0.002 * 3567.1757, "volume {volume}");
    for tol in [0.1, 0.01] {
        let m = closed_display_mesh("thread in a bore", &body, tol);
        let chord = max_chord(&body, &m);
        assert!(chord <= tol * 1.05, "thread in a bore at {tol}: chord {chord}");
        assert_faces_lie_flat("thread in a bore", &ok(body.mesh_plan(&crate::MeshTolerance { chord: tol, angle: Some(crate::DISPLAY_ANGLE) })));
    }
}

/// An M10 thread over the lower half of a 30 mm shaft. The groove's root band leaves the shaft's
/// end face obliquely: a strip a quarter pitch wide winding ten turns, narrower everywhere than
/// a triangle's side, with no four corners to blend. The butterfly of a round face made it a
/// grid of ~200 000 points whose triangles zigzagged along the strip (six times the band's area,
/// and off the surface by more than the tolerance); it is its boundary nodes alone now.
#[cfg(feature = "smlib")]
#[test]
fn a_thread_root_band_running_out_through_an_end_face_lies_on_its_surface() {
    let Ok(shaft) = solidcraft_smlib::Brep::make_cylinder([0.0; 3], 5.0, 30.0) else { panic!("shaft") };
    let threaded = smlib_cut(&shaft, &smlib_groove(iso_groove(1.5, 10.0, 5.0, false), -1.5, 1.5, 11.0));
    let body = ok(crate::native::body_from_smlib(&threaded));
    let m = ok(body.mesh(&crate::MeshTolerance::chord(0.01)));
    let (lo, hi) = m
        .positions
        .iter()
        .fold((Vec3::new(f64::MAX, f64::MAX, f64::MAX), Vec3::new(f64::MIN, f64::MIN, f64::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    assert!(lo.z > -1e-6 && hi.z < 30.0 + 1e-6 && hi.x < 5.0 + 1e-6, "mesh outside the shaft: {lo:?} {hi:?}");
    for tol in [0.1, 0.01] {
        let m = closed_display_mesh("thread run out", &body, tol);
        let chord = max_chord(&body, &m);
        assert!(chord <= tol * 1.05, "thread run out at {tol}: chord {chord}");
        assert!(m.triangles.len() < 60_000, "thread run out at {tol}: {} triangles", m.triangles.len());
        assert_faces_lie_flat("thread run out", &ok(body.mesh_plan(&crate::MeshTolerance { chord: tol, angle: Some(crate::DISPLAY_ANGLE) })));
    }
}

/// #793: a coil (a 1 mm wire at 20 mm, six turns of pitch 4) cut in two across its axis. Its
/// halves go to the display plan for their mesh; at the measuring tolerance a half's wall wants
/// more cells than a grid may have, and its cut grid gave up for an under-refined Delaunay (the
/// halves measured 1.5% short of the whole). The cut grid now thins its lines to fit.
#[cfg(feature = "smlib")]
#[test]
fn the_halves_of_a_split_smlib_coil_measure_as_the_whole() {
    use solidcraft_geom::{Loop2, Vec2};
    use solidcraft_smlib::{BooleanOp, Brep as S};
    let coil = smlib_screw(Loop2::circle(Vec2::new(20.0, 0.0), 1.0), 0.0, 4.0, 6.0);
    let Ok(top) = S::make_box([-30.0, -30.0, 12.5], [60.0, 60.0, 30.0]) else { panic!("box") };
    let (Ok(Some(upper)), Ok(Some(lower))) = (coil.boolean(&top, BooleanOp::Intersect), coil.boolean(&top, BooleanOp::Subtract)) else {
        panic!("split")
    };
    let volume = |b: &S| ok(crate::measure(&ok(crate::native::body_from_smlib(b)))).volume;
    // π r² · 2πR · turns; SMLib's wall is a spline within a ten-thousandth of the pitch.
    let (whole, analytic) = (volume(&coil), PI * 2.0 * PI * 20.0 * 6.0);
    assert!((whole - analytic).abs() < 3e-3 * analytic, "coil {whole} vs {analytic}");
    // The halves are held to the closed form itself: the whole coil's one wall is meshed
    // within 3e-3 of it, which made the whole a looser yardstick than the truth.
    let halves = volume(&upper) + volume(&lower);
    assert!((halves - analytic).abs() < 1e-3 * analytic, "halves {halves} vs {analytic} (the whole measures {whole})");
}

/// Thinning keeps a grid within its cap whatever the rounding, both ends of each direction,
/// and never fewer than two lines a direction (a cap below four points takes four).
#[test]
fn thinned_grid_lines_stay_within_the_cap() {
    let lines = |n: usize| (0..n).map(|i| i as f64 / (n - 1) as f64).collect::<Vec<f64>>();
    for (nu, nv, cap) in [(1237, 122, 150_000), (8192, 3, 1000), (3, 8192, 10), (2, 2, 1), (5000, 5000, 4)] {
        let (mut us, mut vs) = (lines(nu), lines(nv));
        grid::thin(&mut us, &mut vs, cap);
        assert!(us.len() * vs.len() <= cap.max(4), "{nu}x{nv} to {cap}: {}x{}", us.len(), vs.len());
        assert!(us.len() >= 2 && vs.len() >= 2);
        assert!(us.first() == Some(&0.0) && us.last() == Some(&1.0) && vs.first() == Some(&0.0) && vs.last() == Some(&1.0));
    }
}

/// Whatever the chord tolerance, no edge segment spans more than a quarter turn of its curve:
/// a 1 mm hole through a 990 mm plate, at a 2 mm chord and no facet angle, keeps a polygon with
/// area. Each half arc of the hole was one chord, two chords on one line: a hole of no area.
#[test]
fn a_hole_far_below_the_tolerance_keeps_its_area() {
    let plate = ok(crate::box_solid(Vec3::ZERO, Vec3::new(990.0, 990.0, 10.0)));
    let (axis, r) = (Vec3::new(495.0, 495.0, 0.0), 0.5);
    let body = cut(&plate, &ok(crate::cylinder(axis - Vec3::Z, Vec3::Z, r, 12.0)));
    let rec = record(&body);
    let (p, _) = ok(plan(&rec, 2.0, None));
    let mut rims = 0;
    for (ei, e) in rec.edges.iter().enumerate() {
        if matches!(e.curve, Curve::Line { .. }) {
            continue;
        }
        let pts = &p.edges[ei];
        // On the hole's rim: every chord at most a quarter turn's, r·√2.
        let off = |q: &Vec3| (Vec3::new(q.x, q.y, 0.0).dist(axis) - r).abs();
        assert!(pts.iter().all(|q| off(q) < 1e-6), "edge {ei} is not on the hole");
        for w in pts.windows(2) {
            assert!(w[0].dist(w[1]) <= r * 2f64.sqrt() * (1.0 + 1e-6), "edge {ei}: a chord of {} spans more than a quarter turn", w[0].dist(w[1]));
        }
        rims += 1;
    }
    assert!(rims >= 2, "{rims} rim edges");
    let m = p.evaluate();
    assert_watertight(&m, "plate with a small hole");
    // The hole's wall: π · 1 · 10 less its chords' shortfall (a square: 2√2 · 0.5 · 10 at least).
    let wall = m.measure().area - 2.0 * (990.0 * 990.0 + 2.0 * 990.0 * 10.0) + 2.0 * PI * r * r;
    assert!(wall > 4.0 * 2f64.sqrt() * r * 10.0 * 0.99, "the hole's wall measures {wall}");
}

/// Boundary nodes at the very same (u, v) are one vertex to the triangulation: two holes that
/// touch at a vertex they share (a ring standing on a perforated plate, tangent to the holes
/// beside it) bound a face the triangles follow, though each hole has its own copy of the node.
#[test]
fn loops_touching_at_a_shared_vertex_bound_the_face() {
    let outer = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
    let left = [[1.0, 1.0], [1.0, 3.0], [2.0, 2.0]];
    let right = [[2.0, 2.0], [3.0, 3.0], [3.0, 1.0]];
    let uvs: Vec<[f64; 2]> = outer.iter().chain(&left).chain(&right).copied().collect();
    let fixed: Vec<Vec3> = uvs.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    let ranges = vec![0..4, 4..7, 7..10];
    let fill = cdt::Fill { seeds: &[], refine: None, shape: false, lenient: false };
    let Some((_, triangles)) = cdt::triangulate(&uvs, &ranges, fill) else { panic!("triangulates") };
    assert!(closed_boundary(&ranges, &fixed, &uvs, &triangles), "the triangles follow both holes through their shared vertex");
}

/// A face whose loop is a thin lune meshes: a half disc of radius 2.5 less a circle through the
/// same ends 0.03 mm inside it at the middle, its inner side split at 11.25°. Each edge is
/// sampled to the 0.1 mm chord on its own: the outer arc's chords (every 22.5°) sag 0.048 mm,
/// more than the curves are apart, so the one from 0° to 22.5° passes inside the inner side's
/// node at 11.25° and crosses its chords; no triangulation follows a boundary that crosses
/// itself (the sliver caps of a joined ring of cylinder segments). The crossing chords are
/// halved on their edges until none cross.
#[test]
fn a_thin_lune_whose_chords_cross_meshes() {
    use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2};
    let (r, gap, h, tol) = (2.5, 0.03, 5.0, 0.1);
    // The inner circle: centre (−d, 0) and radius r + d·…, through (0, ±r), its middle at r − gap.
    let d = (r * r - (r - gap) * (r - gap)) / (2.0 * (r - gap));
    let (centre, r2) = (Vec2::new(-d, 0.0), r - gap + d);
    let top = r.atan2(d);
    let split = 11.25f64.to_radians();
    let segs = vec![
        Seg2::Arc { center: Vec2::ZERO, radius: r, start: -0.5 * PI, sweep: PI },
        Seg2::Arc { center: centre, radius: r2, start: top, sweep: split - top },
        Seg2::Arc { center: centre, radius: r2, start: split, sweep: -top - split },
    ];
    let lune = Region2 { outer: Loop2 { segs }, holes: vec![] };
    let Some(body) = ok(crate::extrude(&Plane::XY, &[lune], 0.0, h)).pop() else { panic!("one body") };
    let rec = record(&body);
    let (p, stats) = match plan(&rec, tol, None) {
        Ok(r) => r,
        Err(e) => panic!("lune: {e}"),
    };
    assert_eq!(stats.planar + stats.structured + stats.delaunay, body.face_count(), "every face planned");
    let m = p.evaluate();
    assert_watertight(&m, "lune");
    assert!(m.measure().volume > 0.0, "inside out");
}

/// A doubly curved patch 40 mm across, and a loop on it in (u, v) whose right side folds back
/// past itself near the top: the last chord of the fold crosses the side, as a cusp or a sliver
/// whose sides sag across each other leaves a face's boundary.
fn a_face_crossing_itself() -> (EvalSurface, seam::Loop, size::Rules) {
    let mut ctrl = Vec::new();
    for j in 0..3 {
        for i in 0..3 {
            let bump = if i == 1 && j == 1 { 12.0 } else { 0.0 };
            ctrl.push([20.0 * f64::from(i), 20.0 * f64::from(j), bump, 1.0]);
        }
    }
    let s = EvalSurface::Nurbs {
        degree: [2, 2],
        knots: [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]],
        count: [3, 3],
        ctrl,
        closed: [false, false],
    };
    let corners = [[0.1, 0.1], [0.9, 0.1], [0.9, 0.7], [0.7, 0.95], [0.86, 0.8], [0.1, 0.9]];
    let mut uv: Vec<[f64; 2]> = Vec::new();
    for k in 0..corners.len() {
        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
        for t in 0..8 {
            let f = f64::from(t) / 8.0;
            uv.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
        }
    }
    let pts: Vec<Vec3> = uv.iter().filter_map(|q| s.derivs(q[0], q[1]).map(|(p, _, _)| p)).collect();
    assert_eq!(pts.len(), uv.len());
    (s, seam::Loop { uv, pts }, size::Rules::new(0.05, 40.0, None))
}

/// A curved face crossing itself in (u, v) is refused by every method; the lenient fill leaves
/// the crossing side out and plans the rest in (u, v), with points inside the face on its surface
/// (the GPU evaluates them there), not flat between boundary nodes.
#[test]
fn a_curved_face_crossing_itself_is_filled_on_its_surface() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let loops = std::slice::from_ref(&l);
    let mut stats = PlanStats::default();
    assert!(plan_face(0, "spline", &s, loops, &rules, size, 0.0, &mut stats, Lenient::No).is_err());
    let Ok(fp) = plan_face(0, "spline", &s, loops, &rules, size, 0.0, &mut stats, Lenient::Yes) else { panic!("a lenient fill") };
    assert!(!fp.triangles.is_empty());
    assert!(fp.uv.len() > fp.fixed.len(), "{} points, {} on the boundary", fp.uv.len(), fp.fixed.len());
    // Each point inside the face on the surface where its parameters put it.
    let mesh = TessPlan { surfaces: vec![s.clone()], faces: vec![FacePlan { surface: Some(0), ..fp.clone() }], ..TessPlan::default() }.evaluate();
    for (q, p) in fp.uv.iter().zip(&mesh.positions).skip(fp.fixed.len()) {
        let Some((on, _, _)) = s.derivs(q[0], q[1]) else { panic!("evaluates at {q:?}") };
        assert!(on.dist(*p) < 1e-9, "{q:?}");
    }
}

/// The face filled leniently is named in the plan's stats with why the strict plan refused it, and
/// counted under no method: the display has no hole, and the defect is still reported.
#[test]
fn a_face_filled_leniently_is_named_in_the_stats() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let Ok((fp, stats)) = plan_or_fill(7, "spline", &s, std::slice::from_ref(&l), &rules, size, 0.0, None) else { panic!("filled leniently") };
    assert!(!fp.triangles.is_empty());
    assert_eq!((stats.planar, stats.structured, stats.delaunay), (0, 0, 0));
    let [lenient] = stats.lenient.as_slice() else { panic!("one lenient face: {:?}", stats.lenient) };
    assert_eq!(lenient.face, 7);
    assert!(lenient.reason.contains("face 7 (spline)"), "{}", lenient.reason);
}

/// A face whose boundary still crosses itself after untangling (two edges leaving a vertex
/// tangentially: their chords cross however often they are halved, as on one of Poppy
/// Humanoid's faces) is filled leniently straight away with that reason, and named: the body's
/// other faces are planned as usual instead of the whole body failing with it.
#[test]
fn a_face_untangling_cannot_resolve_is_filled_leniently_and_named() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &uv::domain(&s), &rules);
    let why = "face 7 (spline): boundary crosses itself: edges [1, 2] run closer together than their chords can follow";
    let Ok((fp, stats)) = plan_or_fill(7, "spline", &s, std::slice::from_ref(&l), &rules, size, 0.0, Some(why)) else { panic!("filled leniently") };
    assert!(!fp.triangles.is_empty());
    let [lenient] = stats.lenient.as_slice() else { panic!("one lenient face: {:?}", stats.lenient) };
    assert_eq!(lenient.face, 7);
    assert!(lenient.reason.contains(why), "{}", lenient.reason);
}

#[test]
fn a_face_bounded_by_two_short_arcs_keeps_an_area_to_mesh() {
    // A lens on z = 0 between two radius-1000 circles crossing at (±10, 0), in a record whose
    // other face, a 2 m square, sets the segment length: each arc sags 0.05 mm off its chord,
    // inside a 0.5 mm tolerance, so each samples as one segment and the two chords lie on one
    // line, a boundary of no area. Its loop's segments are split until it encloses one.
    use crate::sew::{PartEdge, PartFace, PartUse, Parts, assemble};
    use solidcraft_geom::brep::{Curve, Frame, Surface};
    use std::f64::consts::{PI, TAU};
    let (r, half) = (1000.0f64, 10.0f64);
    let c = (r * r - half * half).sqrt();
    let circle = |cy: f64| Curve::Circle { frame: Frame { origin: [0.0, cy, 0.0], x_axis: [1.0, 0.0, 0.0], y_axis: [0.0, 1.0, 0.0] }, radius: r };
    let top = c.atan2(half);
    let line = |a: [f64; 3], z: [f64; 3]| Curve::Line { start: a, end: z };
    let sq = [[3000.0, 0.0, 0.0], [5000.0, 0.0, 0.0], [5000.0, 2000.0, 0.0], [3000.0, 2000.0, 0.0]];
    let plane = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] };
    let mut edges = vec![
        PartEdge { curve: circle(-c), interval: [top, PI - top], vertices: [0, 1] },
        PartEdge { curve: circle(c), interval: [PI + top, TAU - top], vertices: [1, 0] },
    ];
    for k in 0..4 {
        edges.push(PartEdge { curve: line(sq[k], sq[(k + 1) % 4]), interval: [0.0, 1.0], vertices: [2 + k as u32, 2 + ((k + 1) % 4) as u32] });
    }
    let uses = |es: &[u32]| es.iter().map(|&edge| PartUse { edge, forward: true, given: None }).collect::<Vec<_>>();
    let mut vertices = vec![[half, 0.0, 0.0], [-half, 0.0, 0.0]];
    vertices.extend(sq);
    let parts = Parts {
        vertices,
        edges,
        faces: vec![
            PartFace { surface: plane.clone(), forward: true, loops: vec![uses(&[0, 1])] },
            PartFace { surface: plane, forward: true, loops: vec![uses(&[2, 3, 4, 5])] },
        ],
        tolerance: 1e-6,
    };
    let made = ok(assemble(parts, false));
    let (plan, _) = ok(plan(&made.brep, 0.5, None));
    let lens = made.faces.first().and_then(|&f| plan.faces.get(f)).expect("the lens's plan");
    assert!(!lens.triangles.is_empty(), "the lens has triangles");
}

/// Half of a square pyramid's sides as one spline face: degree 1, closed round the square
/// (u in [0, 4], a corner at each whole u) and from the base (v = 0) up to a top side collapsed
/// to a square 1e-7 mm across, not to a point, as the patch's side in Poppy Humanoid's connector
/// a quarter of a sphere cap's corner sits on. Its loop runs down the side at u = 0, along the
/// base to u = 2 and up again, then across the top back to the corner it started at, half a
/// period along. Taken for no pole, that step is taken as the nearest copy, and the loop winds
/// once round the surface with no partner to cut it along.
fn pyramid_past_a_near_pole(pole_tolerance: f64) -> solidcraft_geom::brep::Brep {
    use solidcraft_geom::brep::{self as b, Curve, Curve2, Surface};
    let top = 1e-7;
    let corner = |u: usize, r: f64, z: f64| {
        let [x, y] = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]][u % 4];
        [r * x, r * y, z]
    };
    let points: Vec<Vec<[f64; 3]>> = (0..5).map(|u| vec![corner(u, 1.0, 0.0), corner(u, top, 1.0)]).collect();
    let surface = Surface::Spline { u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0], v_knots: vec![0.0, 0.0, 1.0, 1.0], points };
    let edge =
        |curve: Curve, interval: [f64; 2], vertices: [u32; 2]| b::Edge { nurbs: None, curve, interval, vertices, tolerance: 1e-6, uses: Vec::new() };
    let edges = vec![
        edge(Curve::Line { start: corner(0, 1.0, 0.0), end: corner(0, top, 1.0) }, [0.0, 1.0], [1, 0]),
        edge(Curve::Polyline { points: vec![corner(0, 1.0, 0.0), corner(1, 1.0, 0.0), corner(2, 1.0, 0.0)] }, [0.0, 2.0], [1, 2]),
        edge(Curve::Line { start: corner(2, 1.0, 0.0), end: corner(2, top, 1.0) }, [0.0, 1.0], [2, 0]),
    ];
    let line = |t: [f64; 2], a: [f64; 2], z: [f64; 2]| Curve2::Spline { knots: vec![t[0], t[0], t[1], t[1]], points: vec![a, z] };
    let uses = vec![
        b::OrientedEdge { edge: 0, forward: false, pcurve: line([0.0, 1.0], [0.0, 0.0], [0.0, 1.0]) },
        b::OrientedEdge { edge: 1, forward: true, pcurve: line([0.0, 2.0], [0.0, 0.0], [2.0, 0.0]) },
        b::OrientedEdge { edge: 2, forward: true, pcurve: line([0.0, 1.0], [2.0, 0.0], [2.0, 1.0]) },
    ];
    let face = b::Face { nurbs: None, surface, domain: [[0.0, 2.0], [0.0, 1.0]], loops: vec![b::Loop::Edges { edges: uses }] };
    let vertex = |point: [f64; 3], tolerance: f64| b::Vertex { point, tolerance };
    let loose = b::Brep {
        version: b::VERSION,
        tolerance: 1e-7,
        regions: vec![b::Region { shells: vec![0], void: false }],
        shells: vec![b::Shell::Faces { faces: vec![b::OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
        faces: vec![face],
        edges,
        vertices: vec![vertex([0.0, 0.0, 1.0], pole_tolerance), vertex(corner(0, 1.0, 0.0), 1e-7), vertex(corner(2, 1.0, 0.0), 1e-7)],
    };
    ok(loose.canonical().map_err(|e| crate::KernelError::Invalid(format!("{e:?}")))).brep
}

/// The corner of [`pyramid_past_a_near_pole`] within its vertex's tolerance of the collapsed top
/// side is a pole, and the face plans strictly over both its sides; with a tolerance short of
/// the top's size it is no pole, and the loop winds.
#[test]
fn a_loop_corner_within_its_tolerance_of_a_pole_is_a_pole() {
    let (made, stats) = ok(plan(&pyramid_past_a_near_pole(1e-6), 0.01, None));
    assert!(stats.lenient.is_empty() && stats.unmeshed.is_empty(), "{stats:?}");
    // Two of the pyramid's sides, each a triangle of area √3 / 2.
    let area = made.evaluate().measure().area;
    assert!((area - 3f64.sqrt()).abs() < 0.01 * 3f64.sqrt(), "area {area}");
    let Err(e) = plan(&pyramid_past_a_near_pole(1e-8), 0.01, None) else { panic!("no pole without the tolerance") };
    assert!(e.to_string().contains("winds around the surface"), "{e}");
}
