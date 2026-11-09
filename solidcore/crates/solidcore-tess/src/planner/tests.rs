//! The planner on whole bodies, and on the faces and loops it is made of.
//!
//! The bodies are B-rep records SolidCraft modelled, frozen as gzipped JSON under `tests/data` (one file
//! per body, `sizes.json` with each body's size, the size its mesh tolerance rules take); see
//! `tests/data/generate_fixtures.rs.txt` for how they are made.

use std::collections::HashMap;

use super::*;
use crate::{Mesh, MeshTolerance};

/// The frozen record `name` (`tests/data/<name>.json.gz`).
pub(crate) fn fixture(name: &str) -> brep::Brep {
    use std::io::Read as _;
    let path = format!("{}/tests/data/{name}.json.gz", env!("CARGO_MANIFEST_DIR"));
    let file = std::fs::File::open(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut json = String::new();
    flate2::read::GzDecoder::new(file)
        .read_to_string(&mut json)
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The size of the body `name` was frozen from: its box diagonal as SolidCraft measured it when
/// it was made, which its mesh tolerance rules ([`MeshTolerance::rules`]) take.
fn size_of(name: &str) -> f64 {
    let path = format!("{}/tests/data/sizes.json", env!("CARGO_MANIFEST_DIR"));
    let json = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let sizes: HashMap<String, f64> =
        serde_json::from_str(&json).unwrap_or_else(|e| panic!("{path}: {e}"));
    *sizes
        .get(name)
        .unwrap_or_else(|| panic!("no size for {name}"))
}

/// Every triangle edge, with vertices merged by exact position, is shared by exactly two
/// triangles: the mesh is closed and its faces meet exactly (no cracks, no T-junctions).
fn assert_watertight(m: &Mesh, name: &str) {
    // By value: −0.0 and 0.0 are one position (adding 0.0 makes −0.0 into 0.0).
    let key = |i: u32| {
        m.positions.get(i as usize).map(|p| {
            [
                (p.x + 0.0).to_bits(),
                (p.y + 0.0).to_bits(),
                (p.z + 0.0).to_bits(),
            ]
        })
    };
    let mut uses: HashMap<([u64; 3], [u64; 3]), i32> = HashMap::new();
    for t in &m.triangles {
        for e in 0..3 {
            let (Some(a), Some(b)) = (key(t[e]), key(t[(e + 1) % 3])) else {
                panic!("{name}: bad index")
            };
            if a == b {
                continue;
            }
            // Directed: a closed, consistently oriented mesh uses each edge once each way.
            *uses.entry((a, b)).or_default() += 1;
            *uses.entry((b, a)).or_default() -= 1;
        }
    }
    let open: Vec<_> = uses.iter().filter(|(_, n)| **n != 0).collect();
    assert!(
        open.is_empty(),
        "{name}: {} unmatched directed edges, e.g. {:?}",
        open.len(),
        open.first()
    );
}

/// The mesh of `p`, every face of which evaluates (none refused).
fn evaluated(p: &TessPlan) -> Mesh {
    let (m, refused) = p.evaluate();
    assert!(
        refused.is_empty(),
        "faces refused at evaluation: {refused:?}"
    );
    m
}

/// The signed volume `m` encloses (the divergence theorem: positive only with outward normals).
fn volume(m: &Mesh) -> f64 {
    let mut v6 = 0.0;
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        v6 += a.dot(b.cross(c));
    }
    v6 / 6.0
}

/// The area of `m`'s triangles.
fn area(m: &Mesh) -> f64 {
    m.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            (b - a).cross(c - a).len() * 0.5
        })
        .sum()
}

/// The display plan of `b` at `tol` as the tests plan it: the chord `tol` and the display angle
/// with its floor a thousandth of the chord.
fn plan_body(b: &brep::Brep, tol: f64) -> (Mesh, PlanStats) {
    match plan_at(
        b,
        tol,
        Some(size::Turn {
            angle: crate::DISPLAY_ANGLE,
            floor: tol * 1e-3,
        }),
    ) {
        Ok((p, stats)) => (evaluated(&p), stats),
        Err(e) => panic!("plan: {e}"),
    }
}

/// The plan of the body `name` at the tolerance `t`, as its size sets the rules: what the body's
/// own mesh is made from.
fn body_plan(name: &str, t: &MeshTolerance) -> TessPlan {
    match plan(&fixture(name), t, size_of(name)) {
        Ok((p, _)) => p,
        Err(e) => panic!("{name}: plan: {e}"),
    }
}

fn check(name: &str, b: &brep::Brep, tol: f64, want: f64) -> PlanStats {
    let (m, stats) = plan_body(b, tol);
    assert_watertight(&m, name);
    let got = volume(&m);
    // Chord error tol over the surface: the volume is short by at most about area · tol.
    let area = area(&m);
    assert!(
        (got - want).abs() <= area * tol * 1.5 + 1e-9,
        "{name}: volume {got} vs {want} (area {area}, tol {tol})"
    );
    // Outward normals: the divergence theorem gives a positive volume only if they are.
    assert!(got > 0.0, "{name}: inside out");
    let faces = b.faces.len();
    assert_eq!(
        stats.planar + stats.structured + stats.delaunay,
        faces,
        "{name}: every face planned"
    );
    stats
}

fn ok<T>(r: Result<T>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{e}"),
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
    let b = fixture("box_40x30x20");
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
        let s = check("box", &fixture("box_10x6x4"), tol, 240.0);
        assert_eq!(
            s,
            PlanStats {
                planar: 6,
                ..PlanStats::default()
            }
        );
        let s = check(
            "cylinder",
            &fixture("cylinder_tilted"),
            tol,
            PI * 2.5 * 2.5 * 7.0,
        );
        assert_eq!(s.delaunay, 0, "cylinder walls are structured");
        let s = check(
            "torus",
            &fixture("torus_6_1_5"),
            tol,
            2.0 * PI * PI * 6.0 * 1.5 * 1.5,
        );
        assert_eq!(s.delaunay, 0, "torus patches are structured");
    }
}

#[test]
fn holes_and_trimmed_faces_are_watertight() {
    let tol = 0.01;
    check(
        "box with a hole",
        &fixture("box_with_hole"),
        tol,
        400.0 - PI * 4.0 * 4.0,
    );
    // A cylinder with a cross hole: trimmed cylinder halves (an iso-line grid cut by the hole)
    // and intersection-curve walls; no face needs Delaunay.
    let body = fixture("cylinder_with_cross_hole");
    let (m, stats) = plan_body(&body, tol);
    assert_watertight(&m, "cross hole");
    assert_eq!(stats.delaunay, 0, "{stats:?}");
    // Against the body's mesh at a fifth of the tolerance (no closed form for the two-cylinder
    // intersection).
    let reference = evaluated(&body_plan(
        "cylinder_with_cross_hole",
        &MeshTolerance::chord(tol * 0.2),
    ));
    let (a, b) = (volume(&m), volume(&reference));
    assert!((a - b).abs() < 0.01 * b, "cross hole volume {a} vs {b}");
}

/// Through holes in pairs whose circles touch at one point (eight holes of radius 5 through a
/// 100 mm plate, at 15 and 25 mm from its middle along both axes): the plate's faces
/// have one loop round each pair, through their common vertex twice. Both passes are one point of
/// the face's (u, v), not two nodes the pcurves put 1e-14 apart, whose constraints crossed
/// ("boundary crosses itself").
#[test]
fn holes_touching_at_a_point_are_watertight() {
    let body = fixture("plate_with_holes_touching_at_points");
    check(
        "holes touching at a point",
        &body,
        0.01,
        10000.0 * 5.0 - 8.0 * PI * 25.0 * 5.0,
    );
}

/// Side-by-side wireframes, the body's mesh at the chord alone (left) and the plan at the
/// display angle (right):
/// `SOLIDCORE_TESS_SVG=<dir> cargo test -p solidcore-tess render_wireframes -- --ignored`.
#[test]
#[ignore = "writes SVG files to the directory SOLIDCORE_TESS_SVG names"]
fn render_wireframes() {
    let Ok(dir) = std::env::var("SOLIDCORE_TESS_SVG") else {
        return;
    };
    let tol = 0.02;
    let shapes = [
        ("box_hole", "box_with_hole"),
        ("cylinder", "pin_cylinder"),
        ("cross_hole", "cylinder_with_cross_hole"),
        ("torus", "torus_6_1_5"),
        ("fillet", "box_with_filleted_top"),
    ];
    for (name, fixture_name) in shapes {
        let body = fixture(fixture_name);
        let t0 = std::time::Instant::now();
        let old = evaluated(&body_plan(fixture_name, &MeshTolerance::chord(tol)));
        let t_old = t0.elapsed().as_secs_f64() * 1e3;
        let t0 = std::time::Instant::now();
        let new = match plan_at(
            &body,
            tol,
            Some(size::Turn {
                angle: crate::DISPLAY_ANGLE,
                floor: tol * 1e-3,
            }),
        ) {
            Ok((p, st)) => Some((evaluated(&p), st)),
            Err(e) => {
                println!("{name}: no plan ({e})");
                None
            }
        };
        let t_new = t0.elapsed().as_secs_f64() * 1e3;
        let mut svg = String::from(
            "<svg xmlns='http://www.w3.org/2000/svg' width='1600' height='860' style='background:#fff'>",
        );
        let label = |x: f64, text: String| {
            format!("<text x='{x}' y='40' font-family='sans-serif' font-size='26'>{text}</text>")
        };
        svg += &label(
            20.0,
            format!("chord alone: {} tris, {t_old:.1} ms", old.triangles.len()),
        );
        svg += &draw(&old, 0.0);
        if let Some((m, st)) = &new {
            svg += &label(
                820.0,
                format!(
                    "plan: {} tris, {t_new:.1} ms  [{} planar, {} grid, {} cdt]",
                    m.triangles.len(),
                    st.planar,
                    st.structured,
                    st.delaunay
                ),
            );
            svg += &draw(m, 800.0);
        }
        svg += "</svg>";
        let _ = std::fs::write(format!("{dir}/{name}.svg"), svg);
    }
}

/// Isometric, painter-sorted, flat-shaded triangles with their edges, in an 800×800 panel.
fn draw(m: &Mesh, x0: f64) -> String {
    use std::fmt::Write as _;
    let (c30, s30) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let proj = |p: Vec3| -> (f64, f64, f64) {
        // Turn 45° about Z, tilt 30° about X; depth along the view.
        let (x, y) = (
            (p.x - p.y) * std::f64::consts::FRAC_1_SQRT_2,
            (p.x + p.y) * std::f64::consts::FRAC_1_SQRT_2,
        );
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
    let mut tris: Vec<(f64, [u32; 3], u32)> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .map(|(t, f)| (t.iter().map(|&i| pts[i as usize].2).sum::<f64>(), *t, *f))
        .collect();
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    let light = Vec3::new(0.3, -0.5, 0.8)
        .normalized()
        .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
    let mut out = String::new();
    let tint = [
        (1.0, 1.0, 1.15),
        (1.15, 1.0, 1.0),
        (1.0, 1.15, 1.0),
        (1.1, 1.1, 0.9),
        (0.9, 1.1, 1.1),
        (1.1, 0.9, 1.1),
    ];
    for (_, t, f) in tris {
        let (tr, tg, tb) = tint[f as usize % tint.len()];
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let n = (b - a)
            .cross(c - a)
            .normalized()
            .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
        let shade = (90.0 + 150.0 * n.dot(light).max(0.0)) as u8;
        let q: Vec<(f64, f64)> = t.iter().map(|&i| sx(pts[i as usize])).collect();
        // Back faces away (screen y points down, so outward triangles wind clockwise here).
        if (q[1].0 - q[0].0) * (q[2].1 - q[0].1) - (q[2].0 - q[0].0) * (q[1].1 - q[0].1) > 0.0 {
            continue;
        }
        let pp: Vec<String> = q.iter().map(|(x, y)| format!("{x:.1},{y:.1}")).collect();
        let _ = write!(
            out,
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
    // (radius, depth) of each hole.
    let cases = [
        ("box_with_drill_hole_r3_4_depth_8", 3.4, 8.0),
        ("box_with_drill_hole_r3_depth_10", 3.0, 10.0),
    ];
    for (fixture_name, r, depth) in cases {
        let body = fixture(fixture_name);
        let name = format!("hole r {r} depth {depth}");
        let (m, _) = plan_body(&body, 0.02);
        assert_watertight(&m, &name);
        for t in &m.triangles {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            assert!(
                (b - a).cross(c - a).len() > 1e-12,
                "{name}: zero-area triangle {a:?} {b:?} {c:?}"
            );
        }
    }
}

#[test]
fn a_placed_body_meshes_like_the_original() {
    let (m, _) = plan_body(&fixture("box_2x3x4"), 0.005);
    let (p, _) = plan_body(&fixture("box_2x3x4_placed"), 0.005);
    assert_eq!(m.tri_face, p.tri_face);
}

/// Seeds on a motorcycle trace start at the spacing of the boundary it leaves and grow
/// geometrically to the spacing at its other end (here 0.1 to 1 over 5 mm).
#[test]
fn trace_seeds_grade_from_one_end_to_the_other() {
    let t = graded(
        [0.0, 0.0],
        [5.0, 0.0],
        0.1,
        1.0,
        10.0,
        [1.0, 1.0],
        &|_, _| Ok(true),
    )
    .unwrap();
    let all: Vec<f64> = std::iter::once(0.0)
        .chain(t.iter().copied())
        .chain(std::iter::once(1.0))
        .map(|f| f * 5.0)
        .collect();
    let sizes: Vec<f64> = all.windows(2).map(|w| w[1] - w[0]).collect();
    assert!((sizes[0] - 0.1).abs() < 0.03, "first {}", sizes[0]);
    assert!(
        (sizes[sizes.len() - 1] - 1.0).abs() < 0.3,
        "last {}",
        sizes[sizes.len() - 1]
    );
    assert!(
        sizes.windows(2).all(|w| w[1] >= w[0] - 1e-12),
        "growing: {sizes:?}"
    );
    // Equal ends: even; a segment no longer than its spacing: no seeds.
    let even = graded(
        [0.0, 0.0],
        [4.0, 0.0],
        1.0,
        1.0,
        10.0,
        [1.0, 1.0],
        &|_, _| Ok(true),
    )
    .unwrap();
    assert_eq!(even.len(), 3);
    assert!(
        graded(
            [0.0, 0.0],
            [0.5, 0.0],
            1.0,
            1.0,
            10.0,
            [1.0, 1.0],
            &|_, _| Ok(true)
        )
        .unwrap()
        .is_empty()
    );
}

/// Whether face `face` of `b` is a plane, cylinder, cone, sphere or torus in its record (the faces
/// [`max_chord`] measures).
fn analytic(b: &brep::Brep, face: usize) -> bool {
    b.faces.get(face).is_some_and(|f| {
        !matches!(
            f.surface,
            brep::Surface::Spline { .. }
                | brep::Surface::RationalSpline { .. }
                | brep::Surface::PlacedRevolution { .. }
        )
    })
}

/// The farthest a triangle's centroid lies from its face's surface, over the faces whose record
/// surface is a plane, cylinder, cone, sphere or torus: the distance to the nearest point of the
/// record's own exact surface (its kind's closed form, [`SurfaceEval`]), found in the record's
/// (u, v) by the kernel's projection ([`SurfaceEval::project`]), not on the planner's form of it,
/// so a wrong conversion of the record's surface would show here.
fn max_chord(body: &brep::Brep, plan: &TessPlan) -> f64 {
    use solidcore::eval::SurfaceEval;
    let mut worst: f64 = 0.0;
    for f in &plan.faces {
        if !analytic(body, f.face as usize) {
            continue;
        }
        let record = &body.faces[f.face as usize];
        let Ok(exact) = SurfaceEval::new(&kernel::surface(&record.surface).unwrap()) else {
            panic!("face {}: the record's evaluator", f.face)
        };
        let s = &plan.surfaces[f.surface.unwrap() as usize];
        let at = |i: u32| -> Vec3 {
            let q = f.uv[i as usize];
            f.fixed
                .get(i as usize)
                .copied()
                .unwrap_or_else(|| kernel::record_vec3(s.derivs(q[0], q[1]).unwrap().0))
        };
        for t in &f.triangles {
            let [a, b, c] = t.map(at);
            let centroid = (a + b + c) * (1.0 / 3.0);
            let on = exact
                .project(kernel::vec3(centroid), None, f64::INFINITY)
                .unwrap_or_else(|| panic!("face {}: no projection of {centroid:?}", f.face));
            let Some((q, _, _)) = exact.derivs(on[0], on[1]).map(kernel::record_triple) else {
                panic!("face {}: the record's surface at {on:?}", f.face)
            };
            worst = worst.max(q.dist(centroid));
        }
    }
    worst
}

/// The display tolerance at chord `tol`: the display angle as well.
fn display(tol: f64) -> MeshTolerance {
    MeshTolerance {
        chord: tol,
        angle: Some(crate::DISPLAY_ANGLE),
    }
}

/// The display plan of the body `name` at `tol` (and the display angle), evaluated: closed,
/// with no zero-area or needle triangle.
fn closed_display_mesh(name: &str, fixture_name: &str, tol: f64) -> Mesh {
    let m = evaluated(&body_plan(fixture_name, &display(tol)));
    assert_watertight(&m, &format!("{name} at {tol}"));
    for t in &m.triangles {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let longest = (b - a).len().max((c - b).len()).max((a - c).len());
        assert!(
            (b - a).cross(c - a).len() > 1e-6 * longest * longest,
            "{name} at {tol}: degenerate triangle {a:?} {b:?} {c:?}"
        );
    }
    m
}

/// A sphere from SMLib cut through its centre: the half sphere is bounded by its rim and its
/// seam from the pole up to the rim, used both ways. The seam's two uses share their nodes, so
/// the mesh has no crack along it and nothing degenerate at the pole.
#[test]
fn a_half_sphere_from_smlib_is_closed_along_its_seam() {
    let body = fixture("smlib_half_sphere");
    for tol in [0.1, 0.01] {
        closed_display_mesh("half sphere", "smlib_half_sphere", tol);
        let chord = max_chord(&body, &body_plan("smlib_half_sphere", &display(tol)));
        assert!(chord <= tol * 1.05, "half sphere at {tol}: chord {chord}");
    }
}

/// Every triangle of a display plan lies within the chord tolerance of its face, whatever
/// layout the face got: on a doubly curved grid the sags along both directions add up at a
/// cell's middle, and a trimmed sphere face round a pocket is not a grid at all. A drilled hole
/// has a cylinder and a cone (the drill point), each measured on its record's exact surface.
#[test]
fn display_plans_stay_within_the_chord_tolerance() {
    for (name, fixture_name) in [
        ("sphere", "sphere_r12"),
        ("pocket in a sphere", "sphere_r10_with_pocket"),
        ("torus", "torus_22_4"),
        ("drilled hole", "box_with_drill_hole_r3_depth_10"),
    ] {
        let body = fixture(fixture_name);
        for tol in [0.1, 0.01] {
            closed_display_mesh(name, fixture_name, tol);
            let chord = max_chord(&body, &body_plan(fixture_name, &display(tol)));
            assert!(chord <= tol * 1.05, "{name} at {tol}: chord {chord}");
        }
    }
}

/// SMLib's torus is one face bounded by its two seams: a grid over the whole parameter box.
#[test]
fn an_smlib_torus_stays_within_the_chord_tolerance() {
    let body = fixture("smlib_torus_22_4");
    for tol in [0.1, 0.01] {
        closed_display_mesh("SMLib torus", "smlib_torus_22_4", tol);
        let chord = max_chord(&body, &body_plan("smlib_torus_22_4", &display(tol)));
        assert!(chord <= tol * 1.05, "SMLib torus at {tol}: chord {chord}");
    }
}

// Modelled threads from SMLib: a screw-swept groove cut from a bore or a shaft. The groove's
// walls are splines winding once per pitch for the thread's whole length, and they split the
// bore or shaft into one face per turn.

/// Each face's triangles, in 3D, cover as much area as its surface over the triangles' own
/// parameters, within 5% (a coarse tolerance on a small curved face, a few percent) and the
/// sails along its boundary: nothing zigzags across the face, which multiplies its area. The
/// surface's area by the midpoint rule on sixteen pieces of each parameter triangle. A face's
/// boundary sides are its edges' chords, each up to the chord tolerance `tol` off its edge, and
/// between a chord of length L and its edge lies a sail of surface of at most 2/3 L tol (a
/// parabolic segment) that flat triangles bridge rather than follow: a face no wider than its
/// chords sag (the thread's crest band where it runs out through the block's end, 0.06 mm sag
/// at 0.1) takes up to that much more or less area however it is triangulated inside.
fn assert_faces_lie_flat(name: &str, plan: &TessPlan, tol: f64) {
    const N: usize = 4;
    for fp in &plan.faces {
        let Some(s) = fp.surface.and_then(|i| plan.surfaces.get(i as usize)) else {
            continue;
        };
        let at = |i: u32| {
            fp.fixed.get(i as usize).copied().unwrap_or_else(|| {
                let q = fp.uv[i as usize];
                match s.eval(q[0], q[1]) {
                    Ok(pn) => kernel::record_pair(pn).0,
                    Err(e) => panic!("{name}: face {} does not evaluate at {q:?}: {e:?}", fp.face),
                }
            })
        };
        let (mut flat, mut surface) = (0.0, 0.0);
        for t in &fp.triangles {
            let [a, b, c] = t.map(|i| fp.uv[i as usize]);
            let area = 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
            flat += 0.5 * (at(t[1]) - at(t[0])).cross(at(t[2]) - at(t[0])).len();
            for i in 0..N {
                for j in 0..N - i {
                    // The upright piece, and the inverted one beside it where there is one.
                    let pieces: &[(f64, f64)] = if i + j + 1 < N {
                        &[(1.0 / 3.0, 1.0 / 3.0), (2.0 / 3.0, 2.0 / 3.0)]
                    } else {
                        &[(1.0 / 3.0, 1.0 / 3.0)]
                    };
                    for &(di, dj) in pieces {
                        let (x, y) = ((i as f64 + di) / N as f64, (j as f64 + dj) / N as f64);
                        let q = [
                            a[0] + (b[0] - a[0]) * x + (c[0] - a[0]) * y,
                            a[1] + (b[1] - a[1]) * x + (c[1] - a[1]) * y,
                        ];
                        if let Some((_, su, sv)) = s.derivs(q[0], q[1]).map(kernel::record_triple) {
                            surface += su.cross(sv).len() * area / (N * N) as f64;
                        }
                    }
                }
            }
        }
        // The boundary: the sides of exactly one triangle.
        let mut sides: HashMap<(u32, u32), u32> = HashMap::new();
        for t in &fp.triangles {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                *sides.entry((a.min(b), a.max(b))).or_default() += 1;
            }
        }
        let boundary: f64 = sides
            .iter()
            .filter(|(_, n)| **n == 1)
            .map(|((a, b), _)| at(*a).dist(at(*b)))
            .sum();
        let sails = 2.0 / 3.0 * boundary * tol;
        assert!(
            (flat - surface).abs() <= 0.05 * surface + sails,
            "{name}: face {} has {flat} mm² of triangles over {surface} mm² of surface (its boundary's sails {sails})",
            fp.face
        );
    }
}

/// The box of `m`'s points.
fn bounds(m: &Mesh) -> (Vec3, Vec3) {
    m.positions.iter().fold(
        (
            Vec3::new(f64::MAX, f64::MAX, f64::MAX),
            Vec3::new(f64::MIN, f64::MIN, f64::MIN),
        ),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    )
}

/// An M8x1 thread through a 7 mm hole in a 20 mm block, both ends open. An earlier mesher
/// meshed the groove's winding walls with triangles reaching past both ends of the block; the
/// body's mesh now comes from the display plan, inside the block and with the analytic volume.
#[test]
fn a_modelled_thread_in_a_bore_meshes_inside_its_block() {
    let name = "smlib_thread_in_bore";
    let body = fixture(name);
    let m = evaluated(&body_plan(name, &MeshTolerance::chord(0.01)));
    let (lo, hi) = bounds(&m);
    assert!(
        lo.z > -1e-6 && hi.z < 10.0 + 1e-6 && lo.x > -10.0 - 1e-6 && hi.x < 10.0 + 1e-6,
        "mesh outside the block: {lo:?} {hi:?}"
    );
    // Block less the bore less ten turns of the groove where it is in the material (as the
    // kernel case works it out from the ISO profile).
    let volume = volume(&m);
    assert!(
        (volume - 3567.1757).abs() < 0.002 * 3567.1757,
        "volume {volume}"
    );
    for tol in [0.1, 0.01] {
        closed_display_mesh("thread in a bore", name, tol);
        let chord = max_chord(&body, &body_plan(name, &display(tol)));
        assert!(
            chord <= tol * 1.05,
            "thread in a bore at {tol}: chord {chord}"
        );
        assert_faces_lie_flat("thread in a bore", &body_plan(name, &display(tol)), tol);
    }
}

/// An M10 thread over the lower half of a 30 mm shaft. The groove's root band leaves the shaft's
/// end face obliquely: a strip a quarter pitch wide winding ten turns, narrower everywhere than
/// a triangle's side, with no four corners to blend. The butterfly of a round face made it a
/// grid of ~200 000 points whose triangles zigzagged along the strip (six times the band's area,
/// and off the surface by more than the tolerance); it is its boundary nodes alone now.
#[test]
fn a_thread_root_band_running_out_through_an_end_face_lies_on_its_surface() {
    let name = "smlib_thread_run_out";
    let body = fixture(name);
    let m = evaluated(&body_plan(name, &MeshTolerance::chord(0.01)));
    let (lo, hi) = bounds(&m);
    assert!(
        lo.z > -1e-6 && hi.z < 30.0 + 1e-6 && hi.x < 5.0 + 1e-6,
        "mesh outside the shaft: {lo:?} {hi:?}"
    );
    for tol in [0.1, 0.01] {
        let m = closed_display_mesh("thread run out", name, tol);
        let chord = max_chord(&body, &body_plan(name, &display(tol)));
        assert!(
            chord <= tol * 1.05,
            "thread run out at {tol}: chord {chord}"
        );
        assert!(
            m.triangles.len() < 60_000,
            "thread run out at {tol}: {} triangles",
            m.triangles.len()
        );
        assert_faces_lie_flat("thread run out", &body_plan(name, &display(tol)), tol);
    }
}

/// Thinning keeps a grid within its cap whatever the rounding, both ends of each direction,
/// and never fewer than two lines a direction (a cap below four points takes four).
#[test]
fn thinned_grid_lines_stay_within_the_cap() {
    let lines = |n: usize| {
        (0..n)
            .map(|i| i as f64 / (n - 1) as f64)
            .collect::<Vec<f64>>()
    };
    for (nu, nv, cap) in [
        (1237, 122, 150_000),
        (8192, 3, 1000),
        (3, 8192, 10),
        (2, 2, 1),
        (5000, 5000, 4),
    ] {
        let (mut us, mut vs) = (lines(nu), lines(nv));
        grid::thin(&mut us, &mut vs, cap);
        assert!(
            us.len() * vs.len() <= cap.max(4),
            "{nu}x{nv} to {cap}: {}x{}",
            us.len(),
            vs.len()
        );
        assert!(us.len() >= 2 && vs.len() >= 2);
        assert!(
            us.first() == Some(&0.0)
                && us.last() == Some(&1.0)
                && vs.first() == Some(&0.0)
                && vs.last() == Some(&1.0)
        );
    }
}

/// Whatever the chord tolerance, no edge segment spans more than a quarter turn of its curve:
/// a 1 mm hole through a 990 mm plate, at a 2 mm chord and no facet angle, keeps a polygon with
/// area. Each half arc of the hole was one chord, two chords on one line: a hole of no area.
#[test]
fn a_hole_far_below_the_tolerance_keeps_its_area() {
    let (axis, r) = (Vec3::new(495.0, 495.0, 0.0), 0.5);
    let rec = fixture("plate_990_with_small_hole");
    let (p, _) = ok(plan_at(&rec, 2.0, None));
    let mut rims = 0;
    for (ei, e) in rec.edges.iter().enumerate() {
        if matches!(e.curve, Curve::Line { .. }) {
            continue;
        }
        let pts = &p.edges[ei];
        // On the hole's rim: every chord at most a quarter turn's, r·√2.
        let off = |q: &Vec3| (Vec3::new(q.x, q.y, 0.0).dist(axis) - r).abs();
        assert!(
            pts.iter().all(|q| off(q) < 1e-6),
            "edge {ei} is not on the hole"
        );
        for w in pts.windows(2) {
            assert!(
                w[0].dist(w[1]) <= r * 2f64.sqrt() * (1.0 + 1e-6),
                "edge {ei}: a chord of {} spans more than a quarter turn",
                w[0].dist(w[1])
            );
        }
        rims += 1;
    }
    assert!(rims >= 2, "{rims} rim edges");
    let m = evaluated(&p);
    assert_watertight(&m, "plate with a small hole");
    // The hole's wall: π · 1 · 10 less its chords' shortfall (a square: 2√2 · 0.5 · 10 at least).
    let wall = area(&m) - 2.0 * (990.0 * 990.0 + 2.0 * 990.0 * 10.0) + 2.0 * PI * r * r;
    assert!(
        wall > 4.0 * 2f64.sqrt() * r * 10.0 * 0.99,
        "the hole's wall measures {wall}"
    );
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
    let fill = cdt::Fill {
        seeds: &[],
        refine: None,
        shape: false,
        lenient: false,
    };
    let Ok(Some((_, triangles))) = cdt::triangulate(&uvs, &ranges, fill) else {
        panic!("triangulates")
    };
    assert!(
        closed_boundary(&ranges, &fixed, &uvs, &triangles),
        "the triangles follow both holes through their shared vertex"
    );
}

/// A face meeting itself along a line (a hole's wall tangent inside the wall round it, cut from
/// the boss side's top rim down to the hole's bottom): its loop runs up the slit and back down
/// it, with the face on both sides. The iso-line grid's band beside the slit is the face on
/// either side of it, and the triangles follow the slit both ways and cover the whole square.
#[test]
fn a_slit_in_a_trimmed_face_has_the_face_on_both_sides() {
    // The square [0, 8] × [0, 4], its loop starting at the slit's tip (3, 1) up to the top, round
    // the square and back down the slit.
    let mut uvs: Vec<[f64; 2]> = (1..=4).map(|k| [3.0, f64::from(k)]).collect();
    uvs.extend((0..3).rev().map(|k| [f64::from(k), 4.0]));
    uvs.extend((0..4).rev().map(|k| [0.0, f64::from(k)]));
    uvs.extend((1..=8).map(|k| [f64::from(k), 0.0]));
    uvs.extend((1..=4).map(|k| [8.0, f64::from(k)]));
    uvs.extend((3..8).rev().map(|k| [f64::from(k), 4.0]));
    uvs.extend((2..4).rev().map(|k| [3.0, f64::from(k)]));
    let fixed: Vec<Vec3> = uvs.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    let ranges = [std::ops::Range {
        start: 0,
        end: uvs.len(),
    }];
    let traces = motorcycle::traces(&uvs, &ranges, [1.0, 1.0], 0.0);
    let lines = |n: usize, top: f64| {
        (0..=n)
            .map(|k| top * k as f64 / n as f64)
            .collect::<Vec<f64>>()
    };
    let Some((extra, triangles)) = cutgrid::cut_grid(
        &uvs,
        &ranges,
        &lines(16, 8.0),
        &lines(8, 4.0),
        &traces,
        [1.0, 1.0],
    ) else {
        panic!("meshes")
    };
    assert!(
        closed_boundary(&ranges, &fixed, &uvs, &triangles),
        "the triangles follow the slit both ways"
    );
    let mut all = uvs.clone();
    all.extend(extra);
    let area: f64 = triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| all[i as usize]);
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]))
        })
        .sum();
    assert!((area - 32.0).abs() < 1e-12, "area {area}");
}

/// A face whose loop is a thin lune meshes: a half disc of radius 2.5 less a circle through the
/// same ends 0.03 mm inside it at the middle, its inner side split at 11.25°. Each edge is
/// sampled to the 0.1 mm chord on its own: the outer arc's chords (every 22.5°) sag 0.048 mm,
/// more than the curves are apart, so the one from 0° to 22.5° passes inside the inner side's
/// node at 11.25° and crosses its chords; no triangulation follows a boundary that crosses
/// itself (the sliver caps of a joined ring of cylinder segments). The crossing chords are
/// halved on their edges until none cross. (The prism of the lune, 5 mm high.)
#[test]
fn a_thin_lune_whose_chords_cross_meshes() {
    let tol = 0.1;
    let rec = fixture("thin_lune_prism");
    let (p, stats) = match plan_at(&rec, tol, None) {
        Ok(r) => r,
        Err(e) => panic!("lune: {e}"),
    };
    assert_eq!(
        stats.planar + stats.structured + stats.delaunay,
        rec.faces.len(),
        "every face planned"
    );
    let m = evaluated(&p);
    assert_watertight(&m, "lune");
    assert!(volume(&m) > 0.0, "inside out");
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
    let s = EvalSurface::nurbs(
        [2, 2],
        [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 0., 1., 1., 1.]],
        [3, 3],
        ctrl,
    );
    let corners = [
        [0.1, 0.1],
        [0.9, 0.1],
        [0.9, 0.7],
        [0.7, 0.95],
        [0.86, 0.8],
        [0.1, 0.9],
    ];
    let mut uv: Vec<[f64; 2]> = Vec::new();
    for k in 0..corners.len() {
        let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
        for t in 0..8 {
            let f = f64::from(t) / 8.0;
            uv.push([a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]);
        }
    }
    let pts: Vec<Vec3> = uv
        .iter()
        .filter_map(|q| {
            s.derivs(q[0], q[1])
                .map(kernel::record_triple)
                .map(|(p, _, _)| p)
        })
        .collect();
    assert_eq!(pts.len(), uv.len());
    (
        s,
        seam::Loop { uv, pts },
        size::Rules::new(0.05, 40.0, None),
    )
}

/// A curved face crossing itself in (u, v) is refused by every method; the lenient fill leaves
/// the crossing side out and plans the rest in (u, v), with points inside the face on its surface
/// (the GPU evaluates them there), not flat between boundary nodes.
#[test]
fn a_curved_face_crossing_itself_is_filled_on_its_surface() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &solidcore::eval::uv::domain(&s), &rules);
    let loops = std::slice::from_ref(&l);
    let mut stats = PlanStats::default();
    assert!(
        plan_face(
            0,
            "spline",
            &s,
            loops,
            &rules,
            &size,
            0.0,
            &mut stats,
            Lenient::No
        )
        .is_err()
    );
    let Ok(fp) = plan_face(
        0,
        "spline",
        &s,
        loops,
        &rules,
        &size,
        0.0,
        &mut stats,
        Lenient::Yes,
    ) else {
        panic!("a lenient fill")
    };
    assert!(!fp.triangles.is_empty());
    assert!(
        fp.uv.len() > fp.fixed.len(),
        "{} points, {} on the boundary",
        fp.uv.len(),
        fp.fixed.len()
    );
    // Each point inside the face on the surface where its parameters put it.
    let mesh = evaluated(&TessPlan {
        surfaces: vec![s.clone()],
        faces: vec![FacePlan {
            surface: Some(0),
            ..fp.clone()
        }],
        ..TessPlan::default()
    });
    for (q, p) in fp.uv.iter().zip(&mesh.positions).skip(fp.fixed.len()) {
        let Some((on, _, _)) = s.derivs(q[0], q[1]).map(kernel::record_triple) else {
            panic!("evaluates at {q:?}")
        };
        assert!(on.dist(*p) < 1e-9, "{q:?}");
    }
}

/// The face filled leniently is named in the plan's stats with why the strict plan refused it, and
/// counted under no method: the display has no hole, and the defect is still reported.
#[test]
fn a_face_filled_leniently_is_named_in_the_stats() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &solidcore::eval::uv::domain(&s), &rules);
    let Ok((fp, stats)) = plan_or_fill(
        7,
        "spline",
        &s,
        std::slice::from_ref(&l),
        &rules,
        &size,
        0.0,
        None,
    ) else {
        panic!("filled leniently")
    };
    assert!(!fp.triangles.is_empty());
    assert_eq!((stats.planar, stats.structured, stats.delaunay), (0, 0, 0));
    let [lenient] = stats.lenient.as_slice() else {
        panic!("one lenient face: {:?}", stats.lenient)
    };
    assert_eq!(lenient.face, 7);
    assert!(
        lenient.reason.contains("face 7 (spline)"),
        "{}",
        lenient.reason
    );
}

/// A face whose boundary still crosses itself after untangling (two edges leaving a vertex
/// tangentially: their chords cross however often they are halved, as on one of Poppy
/// Humanoid's faces) is filled leniently straight away with that reason, and named: the body's
/// other faces are planned as usual instead of the whole body failing with it.
#[test]
fn a_face_untangling_cannot_resolve_is_filled_leniently_and_named() {
    let (s, l, rules) = a_face_crossing_itself();
    let size = size::face_size(&s, &solidcore::eval::uv::domain(&s), &rules);
    let why = "face 7 (spline): boundary crosses itself: edges [1, 2] run closer together than their chords can follow";
    let Ok((fp, stats)) = plan_or_fill(
        7,
        "spline",
        &s,
        std::slice::from_ref(&l),
        &rules,
        &size,
        0.0,
        Some(why),
    ) else {
        panic!("filled leniently")
    };
    assert!(!fp.triangles.is_empty());
    let [lenient] = stats.lenient.as_slice() else {
        panic!("one lenient face: {:?}", stats.lenient)
    };
    assert_eq!(lenient.face, 7);
    assert!(lenient.reason.contains(why), "{}", lenient.reason);
}

#[test]
fn a_face_bounded_by_two_short_arcs_keeps_an_area_to_mesh() {
    // A lens on z = 0 between two radius-1000 circles crossing at (±10, 0), in a record whose
    // other face, a 2 m square, sets the segment length: each arc sags 0.05 mm off its chord,
    // inside a 0.5 mm tolerance, so each samples as one segment and the two chords lie on one
    // line, a boundary of no area. Its loop's segments are split until it encloses one.
    let (r, half) = (1000.0f64, 10.0f64);
    let c = (r * r - half * half).sqrt();
    let made = fixture("lens_and_square");
    // The lens: the one face its circles bound.
    let lenses: Vec<usize> = (0..made.faces.len())
        .filter(|&f| {
            made.edges.iter().any(|e| {
                matches!(e.curve, Curve::Circle { .. })
                    && e.uses.iter().any(|u| u.face as usize == f)
            })
        })
        .collect();
    let [lens_face] = lenses.as_slice() else {
        panic!("one lens: {lenses:?}")
    };
    let (plan, _) = ok(plan_at(&made, 0.5, None));
    let lens = plan.faces.get(*lens_face).expect("the lens's plan");
    // One round of halving puts a node at each arc's middle, (0, ±(r − c)) (the plane's (u, v)
    // is its (x, y)): the triangles fill the rhombus with diagonals 2·half and 2(r − c).
    let area: f64 = lens
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| lens.uv[i as usize]);
            0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
        })
        .sum();
    let rhombus = 2.0 * half * (r - c);
    assert!(
        (area - rhombus).abs() < 1e-9 * rhombus,
        "{area} vs {rhombus}"
    );
}

/// Half of a square pyramid's sides as one spline face: degree 1, closed round the square
/// (u in [0, 4], a corner at each whole u) and from the base (v = 0) up to a top side collapsed
/// to a square 1e-7 mm across, not to a point, as the patch's side in Poppy Humanoid's connector
/// a quarter of a sphere cap's corner sits on. Its loop runs down the side at u = 0, along the
/// base to u = 2 and up again, then across the top back to the corner it started at, half a
/// period along. Taken for no pole, that step is taken as the nearest copy, and the loop winds
/// once round the surface with no partner to cut it along.
fn pyramid_past_a_near_pole(pole_tolerance: f64) -> brep::Brep {
    use solidcore_record::brep::{self as b, Curve, Curve2, Surface};
    let top = 1e-7;
    let corner = |u: usize, r: f64, z: f64| {
        let [x, y] = [[1.0, 0.0], [0.0, 1.0], [-1.0, 0.0], [0.0, -1.0]][u % 4];
        [r * x, r * y, z]
    };
    let points: Vec<Vec<[f64; 3]>> = (0..5)
        .map(|u| vec![corner(u, 1.0, 0.0), corner(u, top, 1.0)])
        .collect();
    let surface = Surface::Spline {
        u_knots: vec![0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        points,
    };
    let edge = |curve: Curve, interval: [f64; 2], vertices: [u32; 2]| b::Edge {
        nurbs: None,
        curve,
        interval,
        vertices,
        tolerance: 1e-6,
        uses: Vec::new(),
    };
    let edges = vec![
        edge(
            Curve::Line {
                start: corner(0, 1.0, 0.0),
                end: corner(0, top, 1.0),
            },
            [0.0, 1.0],
            [1, 0],
        ),
        edge(
            Curve::Polyline {
                points: vec![
                    corner(0, 1.0, 0.0),
                    corner(1, 1.0, 0.0),
                    corner(2, 1.0, 0.0),
                ],
            },
            [0.0, 2.0],
            [1, 2],
        ),
        edge(
            Curve::Line {
                start: corner(2, 1.0, 0.0),
                end: corner(2, top, 1.0),
            },
            [0.0, 1.0],
            [2, 0],
        ),
    ];
    let line = |t: [f64; 2], a: [f64; 2], z: [f64; 2]| Curve2::Spline {
        knots: vec![t[0], t[0], t[1], t[1]],
        points: vec![a, z],
    };
    let uses = vec![
        b::OrientedEdge {
            edge: 0,
            forward: false,
            pcurve: line([0.0, 1.0], [0.0, 0.0], [0.0, 1.0]),
        },
        b::OrientedEdge {
            edge: 1,
            forward: true,
            pcurve: line([0.0, 2.0], [0.0, 0.0], [2.0, 0.0]),
        },
        b::OrientedEdge {
            edge: 2,
            forward: true,
            pcurve: line([0.0, 1.0], [2.0, 0.0], [2.0, 1.0]),
        },
    ];
    let face = b::Face {
        nurbs: None,
        surface,
        domain: [[0.0, 2.0], [0.0, 1.0]],
        loops: vec![Loop::Edges { edges: uses }],
    };
    let vertex = |point: [f64; 3], tolerance: f64| b::Vertex { point, tolerance };
    let loose = b::Brep {
        version: b::VERSION,
        tolerance: 1e-7,
        regions: vec![b::Region {
            shells: vec![0],
            void: false,
        }],
        shells: vec![Shell::Faces {
            faces: vec![b::OrientedFace {
                face: 0,
                forward: true,
            }],
            wires: Vec::new(),
        }],
        faces: vec![face],
        edges,
        vertices: vec![
            vertex([0.0, 0.0, 1.0], pole_tolerance),
            vertex(corner(0, 1.0, 0.0), 1e-7),
            vertex(corner(2, 1.0, 0.0), 1e-7),
        ],
    };
    ok(loose
        .canonical()
        .map_err(|e| TessError::Invalid(format!("{e:?}"))))
    .brep
}

/// The corner of [`pyramid_past_a_near_pole`] within its vertex's tolerance of the collapsed top
/// side is a pole, and the face plans strictly over both its sides; with a tolerance short of
/// the top's size it is no pole, and the loop winds.
#[test]
fn a_loop_corner_within_its_tolerance_of_a_pole_is_a_pole() {
    let (made, stats) = ok(plan_at(&pyramid_past_a_near_pole(1e-6), 0.01, None));
    assert!(
        stats.lenient.is_empty() && stats.unmeshed.is_empty(),
        "{stats:?}"
    );
    // Two of the pyramid's sides, each a triangle of area √3 / 2.
    let area = area(&evaluated(&made));
    assert!(
        (area - 3f64.sqrt()).abs() < 0.01 * 3f64.sqrt(),
        "area {area}"
    );
    let Err(e) = plan_at(&pyramid_past_a_near_pole(1e-8), 0.01, None) else {
        panic!("no pole without the tolerance")
    };
    assert!(e.to_string().contains("winds around the surface"), "{e}");
}

/// A whole sphere as one face (its seam the only edge) in a body many times its size: at the
/// body's chord the seam gets two segments, pole to equator to pole, and every node off the poles
/// follows a pole. The loop used to start at a pole and close across the other, which unwrapped
/// to a whole turn: "boundary winds around the surface", and the sphere was left out (a sealed
/// void r 2 in a 60 mm block). It meshes closed.
#[test]
fn a_small_sphere_void_in_a_large_block_meshes() {
    let name = "block_with_sphere_void";
    let hollow = fixture(name);
    let size = size_of(name);
    let tol = size * 0.01;
    let (m, stats) = match plan_at(&hollow, tol, None) {
        Ok((p, stats)) => (evaluated(&p), stats),
        Err(e) => panic!("plan: {e}"),
    };
    assert!(
        stats.unmeshed.is_empty(),
        "{:?}",
        stats.unmeshed.iter().map(|u| &u.reason).collect::<Vec<_>>()
    );
    assert_watertight(&m, "block with a sphere void");
    // No face without triangles in the body's coarse mesh (at a hundredth of its size, at least
    // 1 µm and at most 1 mm).
    let coarse = body_plan(name, &MeshTolerance::chord((size * 0.01).clamp(1e-3, 1.0)));
    assert_eq!(
        coarse
            .faces_with_triangles(hollow.faces.len())
            .iter()
            .filter(|h| !**h)
            .count(),
        0
    );
}

/// A sheet whose section is an S (a cubic 30 long, 1.5 off its chord each way, its middle on the
/// chord), straight across, meshed as fans with one triangle flat on the chord across its middle
/// (A, B at the sides, C a hair above the chord a third along). The chord's middle lies on the
/// surface while the triangle's short sides sag: splitting those left a child with the chord and
/// its vertex nearer to it, without end (a loft side of a bike frame refined to slivers until the
/// point budget ran out, its area counted three times). The flat triangle now splits the chord,
/// and refinement ends within its budget with no sliver.
#[test]
fn sag_refinement_across_an_inflection_ends_without_slivers() {
    let mut ctrl = Vec::new();
    for j in 0..2 {
        for (i, z) in [0.0, 4.0, -4.0, 0.0].into_iter().enumerate() {
            ctrl.push([10.0 * i as f64, 10.0 * f64::from(j), z, 1.0]);
        }
    }
    let s = EvalSurface::nurbs(
        [3, 1],
        [vec![0., 0., 0., 0., 1., 1., 1., 1.], vec![0., 0., 1., 1.]],
        [4, 2],
        ctrl,
    );
    // Boundary: bottom and top rows of n + 1 nodes (their sides within the tolerance, 0.5), A and
    // B halfway up the ends. Inside: C. Bottom fan from A, the triangle A B C, top fan from C.
    let n = 6u32;
    let mut uv: Vec<[f64; 2]> = Vec::new();
    for i in 0..=n {
        uv.push([f64::from(i) / f64::from(n), 0.0]);
    }
    for i in 0..=n {
        uv.push([f64::from(i) / f64::from(n), 1.0]);
    }
    let (bottom, top) = (0u32, n + 1);
    let (a, b, c) = (2 * n + 2, 2 * n + 3, 2 * n + 4);
    uv.extend([[0.0, 0.5], [1.0, 0.5], [1.0 / 3.0, 0.5 + 1e-3]]);
    let mut tris: Vec<[u32; 3]> = vec![[a, bottom, bottom + 1]];
    tris.extend((1..n).map(|i| [a, bottom + i, bottom + i + 1]));
    tris.push([a, bottom + n, b]);
    tris.push([a, b, c]);
    tris.push([c, b, top + n]);
    tris.extend((0..n).rev().map(|i| [c, top + i + 1, top + i]));
    tris.push([c, top, a]);
    // The sides along the ends are straight.
    let fixed: Vec<Vec3> = Vec::new();
    let tol = 0.5;
    let start = uv.len();
    sag::refine(
        &s,
        tol,
        &mut sag::Poles::new(&s, 1e-9),
        &fixed,
        &mut uv,
        &mut tris,
        20_000,
    )
    .unwrap();
    assert!(
        uv.len() - start < 1_000,
        "{} points added: refinement ran to its budget",
        uv.len() - start
    );
    for t in &tris {
        let [p, q, r] = t.map(|i| uv[i as usize]);
        let area = (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
        let longest = [(p, q), (q, r), (r, p)]
            .iter()
            .map(|(x, y)| (x[0] - y[0]).hypot(x[1] - y[1]))
            .fold(0.0, f64::max);
        assert!(
            area > 1e-6 * longest * longest,
            "a sliver {t:?}: {p:?} {q:?} {r:?}"
        );
    }
}

/// A sheet curved across (a parabola 20 wide, 3.2 high at a tenth in from either side) and two
/// triangles standing on a chord across the curve: their corners on the surface, their planes
/// across it, so they count far more than the area under them. The flip lays them along the
/// sheet, on the straight side along it.
#[test]
fn triangles_standing_across_a_curved_sheet_are_flipped_along_it() {
    let mut ctrl = Vec::new();
    for j in 0..2 {
        for (i, z) in [5.0, -5.0, 5.0].into_iter().enumerate() {
            ctrl.push([10.0 * i as f64 - 10.0, 10.0 * f64::from(j), z, 1.0]);
        }
    }
    let s = EvalSurface::nurbs(
        [2, 1],
        [vec![0., 0., 0., 1., 1., 1.], vec![0., 0., 1., 1.]],
        [3, 2],
        ctrl,
    );
    let uv = vec![[0.1, 0.5], [0.9, 0.5], [0.5, 0.52], [0.5, 0.48]];
    let mut tris = vec![[0, 1, 2], [1, 0, 3]];
    let area = |tris: &[[u32; 3]]| -> f64 {
        tris.iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| {
                    let q = uv[i as usize];
                    match s.eval(q[0], q[1]) {
                        Ok(pn) => kernel::record_pair(pn).0,
                        Err(e) => panic!("evaluates at {q:?}: {e:?}"),
                    }
                });
                (b - a).cross(c - a).len() / 2.0
            })
            .sum()
    };
    let before = area(&tris);
    sag::flip(&s, 0.1, &mut sag::Poles::new(&s, 1e-9), &[], &uv, &mut tris).unwrap();
    let after = area(&tris);
    let sides: Vec<(u32, u32)> = tris
        .iter()
        .flat_map(|t| (0..3).map(move |k| (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]))))
        .collect();
    assert!(
        sides.contains(&(2, 3)) && !sides.contains(&(0, 1)),
        "{tris:?}"
    );
    assert!(after < 0.5 * before, "{before} -> {after}");
}

/// A disc with a boss standing on it tangent inside its rim: the disc's top is a ring whose inner
/// circle touches the outer one, one loop through the touching vertex. There both circles leave
/// along one tangent, and with the outer chord twice the inner (as their samplings fall) the inner
/// circle's first node lies on the outer's last chord: no triangulation keeps both. That chord is
/// halved, and the ring meshes strictly, to its area (300 pi), not the inner disc's.
#[test]
fn a_ring_touching_itself_at_a_vertex_meshes_strictly() {
    let name = "disc_with_boss_tangent_inside_its_rim";
    let stepped = fixture(name);
    let (_, stats) = ok(plan_at(&stepped, 1e-4, None));
    assert!(
        stats.lenient.is_empty(),
        "{:?}",
        stats.lenient.iter().map(|r| &r.reason).collect::<Vec<_>>()
    );
    // Each face's area and centroid in the body's mesh at a chord of 1e-4.
    let m = evaluated(&body_plan(name, &MeshTolerance::chord(1e-4)));
    let mut faces: Vec<(f64, Vec3)> = vec![(0.0, Vec3::ZERO); stepped.faces.len()];
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let [a, b, c] = t.map(|i| m.positions[i as usize]);
        let ar = (b - a).cross(c - a).len() * 0.5;
        let slot = &mut faces[*f as usize];
        slot.0 += ar;
        slot.1 += (a + b + c) * (ar / 3.0);
    }
    let centroid = |(area, c): (f64, Vec3)| if area > 0.0 { c / area } else { Vec3::ZERO };
    let ring = faces
        .into_iter()
        .find(|f| (centroid(*f).z - 5.0).abs() < 1e-6)
        .unwrap();
    assert!((ring.0 - 300.0 * PI).abs() < 0.05, "ring area {}", ring.0);
}

// A bad face, edge or pcurve in a body: the face (or every face of the edge) is refused, named
// in the stats, and the rest of the body is meshed.

/// The faces of `p` with triangles, by index.
fn meshed_faces(p: &TessPlan) -> Vec<u32> {
    p.faces
        .iter()
        .filter(|f| !f.triangles.is_empty())
        .map(|f| f.face)
        .collect()
}

/// Face `face`'s first pcurve in `b`, moved `du` along u: off its edge.
fn move_pcurve(b: &mut brep::Brep, face: usize, du: f64) -> u32 {
    let Loop::Edges { edges } = &mut b.faces[face].loops[0] else {
        panic!("an edge loop")
    };
    let oe = &mut edges[0];
    match &mut oe.pcurve {
        brep::Curve2::Spline { points, .. } => {
            for p in points.iter_mut() {
                p[0] += du;
            }
        }
        brep::Curve2::RationalSpline { homogeneous, .. } => {
            for p in homogeneous.iter_mut() {
                p[0] += du * p[2];
            }
        }
    }
    oe.edge
}

/// A box one of whose faces has a pcurve 1 mm off its edge: that face is refused, naming the
/// face and the edge, and the other five are meshed. The pcurve's error was the whole body's.
#[test]
fn a_face_whose_pcurve_is_off_its_edge_is_refused_and_the_rest_meshed() {
    let mut b = fixture("box_10x6x4");
    let edge = move_pcurve(&mut b, 2, 1.0);
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    assert_eq!(stats.unmeshed.len(), 1, "{stats:?}");
    let refused = &stats.unmeshed[0];
    assert_eq!(refused.face, 2);
    assert!(
        refused.reason.contains(&format!(
            "face 2 (plane): edge {edge}: node 0: its pcurve is"
        )),
        "{}",
        refused.reason
    );
    assert_eq!(meshed_faces(&p), vec![0, 1, 3, 4, 5]);
    assert_eq!(p.faces.len(), 6);
    let (m, refused) = p.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    assert!(!m.triangles.is_empty() && !m.tri_face.contains(&2));
}

/// A box with an edge whose interval is reversed: the edge cannot be sampled, and both faces it
/// bounds are refused, each naming itself and the edge, with no nodes for the edge. The other
/// four faces are meshed. The edge's error was the whole body's.
#[test]
fn an_edge_that_cannot_be_sampled_refuses_both_its_faces_by_name() {
    let mut b = fixture("box_10x6x4");
    let ei = 3;
    b.edges[ei].interval = [1.0, 0.0];
    let mut faces: Vec<usize> = b.edges[ei].uses.iter().map(|u| u.face as usize).collect();
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces.len(), 2);
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    let refused: Vec<usize> = stats.unmeshed.iter().map(|r| r.face).collect();
    assert_eq!(refused, faces, "{stats:?}");
    for r in &stats.unmeshed {
        assert!(
            r.reason.contains(&format!(
                "face {} (plane): tessellation: edge {ei}: interval [1, 0]",
                r.face
            )),
            "{}",
            r.reason
        );
    }
    let meshed: Vec<u32> = (0..6u32)
        .filter(|f| !faces.contains(&(*f as usize)))
        .collect();
    assert_eq!(meshed_faces(&p), meshed);
    assert!(p.edges[ei].is_empty());
    assert!(
        p.edges
            .iter()
            .enumerate()
            .all(|(e, n)| e == ei || n.len() >= 2)
    );
}

/// An edge that bounds no face (a free edge beside a box) and cannot be sampled is left out of
/// the plan and named in the stats with why; the box is meshed whole, not refused as a body.
#[test]
fn a_free_edge_that_cannot_be_sampled_is_named_and_the_body_meshed() {
    let mut b = fixture("box_10x6x4");
    let mut free = b.edges[3].clone();
    free.uses.clear();
    free.interval = [1.0, 0.0];
    b.edges.push(free);
    let ei = b.edges.len() - 1;
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    assert!(stats.unmeshed.is_empty(), "{stats:?}");
    assert_eq!(stats.unsampled_edges.len(), 1, "{stats:?}");
    assert_eq!(stats.unsampled_edges[0].edge, ei);
    assert!(
        stats.unsampled_edges[0]
            .reason
            .contains(&format!("tessellation: edge {ei}: interval [1, 0]")),
        "{}",
        stats.unsampled_edges[0].reason
    );
    assert_eq!(meshed_faces(&p), (0..6u32).collect::<Vec<u32>>());
    assert!(p.edges[ei].is_empty());
    assert!(p.edge_faces[ei].is_empty());
    assert!(p.edges[..ei].iter().all(|n| n.len() >= 2));
}

/// An edge whose uses list no face but which the faces' loops still run (an unvalidated
/// record), and which cannot be sampled (a spline without points): both faces that run it are
/// refused, each naming it, and the other four are meshed. It was taken for a free edge, and
/// each face's loop closed with a chord across the gap.
#[test]
fn an_edge_left_out_of_its_uses_but_in_its_faces_loops_refuses_them() {
    let mut b = fixture("box_10x6x4");
    let ei = 3;
    let mut faces: Vec<usize> = b.edges[ei].uses.iter().map(|u| u.face as usize).collect();
    faces.sort_unstable();
    faces.dedup();
    b.edges[ei].uses.clear();
    // A spline with no control points: its pcurves and interval are still the line's, so only
    // its sampling fails.
    b.edges[ei].curve = Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: Vec::new(),
    };
    b.edges[ei].nurbs = None;
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    let refused: Vec<usize> = stats.unmeshed.iter().map(|r| r.face).collect();
    assert_eq!(refused, faces, "{stats:?}");
    for r in &stats.unmeshed {
        assert!(r.reason.contains(&format!("edge {ei}")), "{}", r.reason);
    }
    assert!(stats.unsampled_edges.is_empty(), "{stats:?}");
    let meshed: Vec<u32> = (0..6u32)
        .filter(|f| !faces.contains(&(*f as usize)))
        .collect();
    assert_eq!(meshed_faces(&p), meshed);
    assert_eq!(
        p.edge_faces[ei],
        faces.iter().map(|&f| f as u32).collect::<Vec<u32>>()
    );
}

/// A body every face of which is refused (a sphere of one face, its pcurve off its edge) is the
/// first face's error, not an empty plan.
#[test]
fn a_body_none_of_whose_faces_plan_is_the_first_faces_error() {
    let mut b = fixture("sphere_r12");
    assert_eq!(b.faces.len(), 1);
    move_pcurve(&mut b, 0, 0.5);
    match plan(&b, &MeshTolerance::chord(0.01), size_of("sphere_r12")) {
        Err(e) => assert!(e.to_string().contains("face 0 (sphere): edge"), "{e}"),
        Ok((p, stats)) => panic!(
            "planned {} faces, unmeshed {:?}",
            meshed_faces(&p).len(),
            stats.unmeshed
        ),
    }
}

/// A face with a loop of edges too short to bound anything (here none): the face is refused,
/// naming the loop, and the rest of the body is meshed. The loop was left out, and the face
/// meshed without it.
#[test]
fn a_face_with_a_loop_of_fewer_than_two_nodes_is_refused_naming_the_loop() {
    let mut b = fixture("box_10x6x4");
    b.faces[4].loops.push(Loop::Edges { edges: Vec::new() });
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    let reasons: Vec<(usize, &str)> = stats
        .unmeshed
        .iter()
        .map(|r| (r.face, r.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        vec![(
            4,
            "the operation failed: not supported yet: tessellation: face 4 (plane): loop 1 has 0 nodes (0 edge uses)"
        )]
    );
    assert_eq!(meshed_faces(&p), vec![0, 1, 2, 3, 5]);
}

/// A point a whole number of periods along from inside a face's loops is inside them however
/// many periods the loops span: a strip winding 100 turns has the point's 70th copy inside it.
/// Past 64 periods the copies were not looked through, and the point was "not inside"; past
/// [`MAX_PERIODS`] it is an error, never "not inside".
#[test]
fn a_point_inside_a_face_winding_many_periods_is_found() {
    let per = std::f64::consts::TAU;
    // A long strip u in [0, 100 periods], v in [0, 1], and a point at u = 0.5 (its copy 70
    // periods along lies inside).
    let strip = |turns: f64| {
        vec![
            [0.0, 0.0],
            [turns * per, 0.0],
            [turns * per, 1.0],
            [0.0, 1.0],
        ]
    };
    let long = strip(100.0);
    let polys = [long.as_slice()];
    let bbox = [(0.0, 100.0 * per), (0.0, 1.0)];
    let q = [0.5 - 70.0 * per, 0.5];
    assert_eq!(within_loops(&polys, bbox, [Some(per), None], q), Ok(true));
    // Off the strip across it: not inside, at any copy.
    assert_eq!(
        within_loops(&polys, bbox, [Some(per), None], [0.5, 1.5]),
        Ok(false)
    );
    let longer = strip(5000.0);
    let polys = [longer.as_slice()];
    let bbox = [(0.0, 5000.0 * per), (0.0, 1.0)];
    assert!(matches!(
        within_loops(&polys, bbox, [Some(per), None], q),
        Err(TooManyCopies(n)) if n > 4096.0
    ));
}

/// A plan keeps the record's order: face `i` of the plan is the record's face `i`, and edge `e`'s
/// polyline and faces are the record's edge `e`'s, the polyline from its start vertex to its end
/// vertex and the faces in the order of the edge's uses.
#[test]
fn faces_and_edges_keep_the_record_order() {
    for name in ["pin_cylinder", "box_with_hole", "cylinder_with_cross_hole"] {
        let b = fixture(name);
        let p = body_plan(name, &MeshTolerance::chord(0.01));
        let faces: Vec<u32> = p.faces.iter().map(|f| f.face).collect();
        assert_eq!(
            faces,
            (0..b.faces.len() as u32).collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(p.edges.len(), b.edges.len(), "{name}");
        for (ei, e) in b.edges.iter().enumerate() {
            let mut uses: Vec<u32> = Vec::new();
            for u in &e.uses {
                if !uses.contains(&u.face) {
                    uses.push(u.face);
                }
            }
            assert_eq!(p.edge_faces[ei], uses, "{name}: edge {ei}");
            let vertex = |k: usize| {
                let q = b.vertices[e.vertices[k] as usize].point;
                Vec3::new(q[0], q[1], q[2])
            };
            assert_eq!(
                p.edges[ei].first().copied(),
                Some(vertex(0)),
                "{name}: edge {ei}"
            );
            assert_eq!(
                p.edges[ei].last().copied(),
                Some(vertex(1)),
                "{name}: edge {ei}"
            );
        }
        let m = evaluated(&p);
        assert_eq!(m.edges, p.edges);
        assert_eq!(m.edge_faces, p.edge_faces);
    }
}

/// A coil from SMLib (a 1 mm wire at 20 mm, six turns of pitch 4) cut in two across its axis at
/// z = 12.5, each half meshed at a measuring tolerance (3e-4 mm, 3e-4 of the wire's radius): a
/// half's wall wants more cells than a grid may have, and its cut grid gave up for an
/// under-refined Delaunay (the halves came out 1.5% short of the coil). The cut grid thins its
/// lines to fit, and the halves' meshes add up to the coil's closed form, π r² · 2πR · turns,
/// within 1e-3 (SMLib's wall is a spline within a ten-thousandth of the pitch).
#[test]
fn the_halves_of_a_split_smlib_coil_mesh_as_the_whole() {
    let analytic = PI * 2.0 * PI * 20.0 * 6.0;
    let mut halves = 0.0;
    for name in ["smlib_coil_upper_half", "smlib_coil_lower_half"] {
        let b = fixture(name);
        let (p, stats) = ok(plan(&b, &MeshTolerance::chord(3e-4), size_of(name)));
        assert!(
            stats.unmeshed.is_empty() && stats.lenient.is_empty(),
            "{name}: {stats:?}"
        );
        // The wall (face 0) asks for more grid lines than a grid may have, and its refinement
        // cannot make up the rest within the points it may add: it is meshed, and named with
        // both bounds and the sag its triangles reach, past the tolerance.
        let [over] = stats.over_tolerance.as_slice() else {
            panic!("{name}: {:?}", stats.over_tolerance)
        };
        let [Cap::GridThinned { asked, kept }, Cap::Refine] = over.caps.as_slice() else {
            panic!("{name}: {over:?}")
        };
        assert_eq!(over.face, 0, "{name}: {over:?}");
        assert!(
            asked[0] * asked[1] > grid::MAX_FACE_GRID && kept[0] * kept[1] <= grid::MAX_FACE_GRID,
            "{name}: {over:?}"
        );
        assert!(
            over.sag > 3e-4 && over.tol == 3e-4 && !over.within,
            "{name}: {over:?}"
        );
        let m = evaluated(&p);
        assert_watertight(&m, name);
        halves += volume(&m);
    }
    assert!(
        (halves - analytic).abs() < 1e-3 * analytic,
        "halves {halves} vs {analytic}"
    );
}

/// A face past [`MAX_FACE_NODES`] (its boundary alone, or with what refinement may add) is
/// refused with its count, where every u32 index of it would no longer be proven to fit.
#[test]
fn a_face_past_the_node_cap_is_refused() {
    assert_eq!(too_many_nodes(MAX_FACE_NODES, 0), None);
    assert_eq!(
        too_many_nodes(MAX_FACE_NODES - MAX_REFINE, MAX_REFINE),
        None
    );
    let Some(why) = too_many_nodes(MAX_FACE_NODES + 1, 0) else {
        panic!("refused")
    };
    assert!(
        why.starts_with(&format!("its {} boundary nodes", MAX_FACE_NODES + 1)),
        "{why}"
    );
    let Some(why) = too_many_nodes(MAX_FACE_NODES - MAX_REFINE + 1, MAX_REFINE) else {
        panic!("refused")
    };
    assert!(why.contains("with the 20000 refinement may add"), "{why}");
    assert!(too_many_nodes(usize::MAX, MAX_REFINE).is_some());
}

/// A cylinder face without its NURBS form (a label alone, an unvalidated record): the kernel's
/// evaluator keeps it in its closed form, which tess does not mesh, and the face is refused,
/// naming it, through `plan()`; its two end discs are meshed.
#[test]
fn a_face_whose_surface_does_not_evaluate_is_refused_through_plan() {
    let mut b = fixture("cylinder_r10_h5");
    assert!(b.faces[0].nurbs.is_some());
    b.faces[0].nurbs = None;
    let (p, stats) = ok(plan(
        &b,
        &MeshTolerance::chord(0.01),
        size_of("cylinder_r10_h5"),
    ));
    let reasons: Vec<(usize, &str)> = stats
        .unmeshed
        .iter()
        .map(|r| (r.face, r.reason.as_str()))
        .collect();
    assert_eq!(reasons.len(), 1, "{stats:?}");
    assert_eq!(reasons[0].0, 0);
    assert!(
        reasons[0].1.contains(
            "face 0 (cylinder): invalid input: tessellation: a cylinder face without its NURBS form"
        ),
        "{}",
        reasons[0].1
    );
    assert_eq!(meshed_faces(&p), vec![1, 2]);
    assert_eq!(p.faces[0].surface, None);
}

/// A face with a loop of one edge use that leaves a single node (a second loop of one short
/// straight edge on it, 1 µm long, a single segment): its boundary is refused, naming the loop.
#[test]
fn a_face_with_a_loop_of_one_node_is_refused_naming_the_loop() {
    let mut b = fixture("box_10x6x4");
    // Edge 2 runs (10, 0, 4) to (10, 6, 4); face 4 runs it backwards, its pcurve from (0, 4) to
    // (6, 4). A copy 1 µm long from its start, to a vertex of its own.
    let mut short = b.edges[2].clone();
    let (start, end) = ([10.0, 0.0, 4.0], [10.0, 1e-3, 4.0]);
    short.curve = Curve::Line { start, end };
    short.uses.clear();
    let v = u32::try_from(b.vertices.len()).unwrap();
    let mut vertex = b.vertices[short.vertices[0] as usize].clone();
    vertex.point = end;
    b.vertices.push(vertex);
    short.vertices[1] = v;
    let e = u32::try_from(b.edges.len()).unwrap();
    b.edges.push(short);
    let one = brep::OrientedEdge {
        edge: e,
        forward: true,
        pcurve: brep::Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 4.0], [1e-3, 4.0]],
        },
    };
    b.faces[4].loops.push(Loop::Edges { edges: vec![one] });
    // Through plan() the loop's edge is split to three segments first (split_thin_loops), so the
    // boundary is built from the edges as sampled.
    let rules = size::Rules::new(0.01, 12.0, None);
    let nodes: Vec<edges::Nodes> = (0..b.edges.len())
        .map(|e| ok(edges::sample_edge(&b, e, &rules, rules.max_len)))
        .collect();
    assert_eq!(nodes[e as usize].points.len(), 2);
    let face = &b.faces[4];
    let s = ok(surface::eval_surface(face.parameters()));
    match boundary(&b, 4, face, &s, &nodes, &rules) {
        Err(e) => assert!(
            e.to_string()
                .ends_with("face 4 (plane): loop 1 has 1 node (1 edge uses)"),
            "{e}"
        ),
        Ok(fb) => panic!("{} loops", fb.loops.len()),
    }
}

/// A loop's edge that cannot be split where `split_thin_loops` halves it (a spline with no point at
/// its middle, its nodes only its ends): the edge is returned with its error, for every face of
/// it to be refused. Through `plan()` the sampling meets the middle first.
#[test]
fn a_thin_loops_edge_that_cannot_be_split_is_returned_with_why() {
    let mut b = fixture("box_10x6x4");
    // x = 3, z = 40 t, its weight's double root at t = 0.5 (0 / 0 there).
    let mut bad = b.edges[2].clone();
    bad.curve = Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        homogeneous: vec![
            [9.0, 0.0, 0.0, 3.0],
            [-3.0, 0.0, 40.0, -1.0],
            [-3.0, 0.0, -80.0, -1.0],
            [9.0, 0.0, 120.0, 3.0],
        ],
    };
    bad.nurbs = None;
    bad.uses.clear();
    let e = b.edges.len();
    b.edges.push(bad);
    let one = brep::OrientedEdge {
        edge: u32::try_from(e).unwrap(),
        forward: true,
        pcurve: brep::Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 4.0], [1.0, 4.0]],
        },
    };
    b.faces[4].loops.push(Loop::Edges { edges: vec![one] });
    let rules = size::Rules::new(0.01, 12.0, None);
    let mut nodes: Vec<edges::Nodes> = (0..e)
        .map(|k| ok(edges::sample_edge(&b, k, &rules, rules.max_len)))
        .collect();
    nodes.push(edges::Nodes {
        points: vec![Vec3::new(3.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 40.0)],
        params: vec![0.0, 1.0],
        snapped: Vec::new(),
        capped: None,
    });
    let failed = split_thin_loops(&b, &mut nodes, &[false; 6]);
    let [(edge, why)] = failed.as_slice() else {
        panic!("{failed:?}")
    };
    assert_eq!(*edge, e);
    assert!(
        why.to_string()
            .contains(&format!("edge {e}: the curve does not evaluate at t = 0.5")),
        "{why}"
    );
}

/// A face's crossing chords are halved on their edge; an edge that cannot take the new nodes
/// (here past `MAX_EDGE_NODES`) refuses every face of it, each naming the edge, and untangle
/// passes them over. (A box face's boundary made to cross itself by moving one of its nodes.)
#[test]
fn an_edge_untangling_cannot_split_refuses_its_faces() {
    let b = fixture("box_10x6x4");
    let rules = size::Rules::new(0.01, 12.0, None);
    let evals: Vec<Option<EvalSurface>> = b
        .faces
        .iter()
        .map(|f| surface::eval_surface(f.parameters()).ok())
        .collect();
    let mut edge_faces: Vec<Vec<usize>> = vec![Vec::new(); b.edges.len()];
    for (fi, f) in b.faces.iter().enumerate() {
        for oe in face_edges(f) {
            if !edge_faces[oe.edge as usize].contains(&fi) {
                edge_faces[oe.edge as usize].push(fi);
            }
        }
    }
    let mut nodes: Vec<edges::Nodes> = (0..b.edges.len())
        .map(|e| ok(edges::sample_edge(&b, e, &rules, 1.0)))
        .collect();
    let face_boundary = |fi: usize, nodes: &[edges::Nodes]| {
        boundary(
            &b,
            fi,
            &b.faces[fi],
            evals[fi].as_ref().unwrap(),
            nodes,
            &rules,
        )
    };
    let mut boundaries: Vec<Boundary> = (0..b.faces.len())
        .map(|fi| ok(face_boundary(fi, &nodes)))
        .collect();
    // Face 4's loop with its node 1 moved past the face's opposite side, through its middle.
    let uv = &mut boundaries[4].loops[0].uv;
    let m = uv.len() as f64;
    let c = uv
        .iter()
        .fold([0.0, 0.0], |a, q| [a[0] + q[0] / m, a[1] + q[1] / m]);
    uv[1] = [
        c[0] + 1.5 * (c[0] - uv[1][0]),
        c[1] + 1.5 * (c[1] - uv[1][1]),
    ];
    let fb = &boundaries[4];
    let crossing = tangle::crossing(evals[4].as_ref().unwrap(), fb.reach, &fb.loops).unwrap();
    let Some(&(l, k)) = crossing.first() else {
        panic!("the swapped loop crosses itself")
    };
    let e = fb.segments[l][k].0;
    // The edge full: one more node is past the cap.
    let n = &mut nodes[e as usize];
    let (p, t) = (*n.points.last().unwrap(), *n.params.last().unwrap());
    n.points.resize(edges::MAX_EDGE_NODES, p);
    n.params.resize(edges::MAX_EDGE_NODES, t);
    let mut refused = Refused::new(&b);
    let tangled = untangle(
        &b,
        &evals,
        &edge_faces,
        rules.tol,
        &mut nodes,
        &mut boundaries,
        &face_boundary,
        &mut refused,
    );
    for &f in &edge_faces[e as usize] {
        assert!(refused.is(f), "face {f}");
        let why = refused.why[f].as_ref().unwrap().to_string();
        assert!(
            why.contains(&format!("edge {e}: more than 20000 nodes")),
            "{why}"
        );
        assert!(tangled.iter().all(|(t, _)| *t != f), "{tangled:?}");
    }
}

/// A torus (radii 6 and 1.5) at a chord of 2e-4 mm, no facet angle: its grid fits, but its sag
/// refinement adds all the points it may ([`MAX_REFINE`]) with triangles still off the surface,
/// and the face is still meshed and named in [`PlanStats::over_tolerance`] with that bound and
/// the sag its triangles reach, past the tolerance. The refinement stopped there silently.
#[test]
fn a_face_whose_refinement_reaches_its_bound_is_named_with_its_sag() {
    let tol = 2e-4;
    let b = fixture("torus_6_1_5");
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(tol), size_of("torus_6_1_5")));
    assert!(stats.unmeshed.is_empty(), "{stats:?}");
    let [o] = stats.over_tolerance.as_slice() else {
        panic!("{:?}", stats.over_tolerance)
    };
    assert_eq!(
        (o.face, o.caps.as_slice()),
        (0, [Cap::Refine].as_slice()),
        "{o:?}"
    );
    assert!(
        o.sag > tol && o.sag < 10.0 * tol && o.tol == tol && !o.within,
        "{o:?}"
    );
    assert!(!p.faces[0].triangles.is_empty());
}

/// A crowded face's curved edge whose sampling at the coarser angle fails (a spline with no
/// point at its middle, which its strict nodes, filled in here, never reached): the edge keeps
/// its strict nodes, refuses no face, and is named with the coarser sampling's error.
#[test]
fn an_edge_whose_coarser_sampling_fails_keeps_its_nodes_and_is_named() {
    let mut b = fixture("box_10x6x4");
    // x = 3, z = 40 t, its weight's double root at t = 0.5 (0 / 0 there).
    let mut bad = b.edges[2].clone();
    bad.curve = Curve::RationalSpline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        homogeneous: vec![
            [9.0, 0.0, 0.0, 3.0],
            [-3.0, 0.0, 40.0, -1.0],
            [-3.0, 0.0, -80.0, -1.0],
            [9.0, 0.0, 120.0, 3.0],
        ],
    };
    bad.nurbs = None;
    bad.uses.clear();
    let e = b.edges.len();
    b.edges.push(bad);
    let one = brep::OrientedEdge {
        edge: u32::try_from(e).unwrap(),
        forward: true,
        pcurve: brep::Curve2::Spline {
            knots: vec![0.0, 0.0, 1.0, 1.0],
            points: vec![[0.0, 4.0], [1.0, 4.0]],
        },
    };
    b.faces[4].loops.push(Loop::Edges { edges: vec![one] });
    let rules = size::Rules::new(
        0.01,
        12.0,
        Some(size::Turn {
            angle: crate::DISPLAY_ANGLE,
            floor: 1e-5,
        }),
    );
    let mut nodes: Vec<edges::Nodes> = (0..e)
        .map(|k| ok(edges::sample_edge(&b, k, &rules, rules.max_len)))
        .collect();
    // Strict nodes: more segments than a face may have, none at t = 0.5 (an odd count).
    let count = MAX_ANGLE_BOUNDARY + 1;
    let params: Vec<f64> = (0..=count).map(|k| k as f64 / count as f64).collect();
    let points: Vec<Vec3> = params
        .iter()
        .map(|t| Vec3::new(3.0, 0.0, 40.0 * t))
        .collect();
    nodes.push(edges::Nodes {
        points: points.clone(),
        params: params.clone(),
        snapped: Vec::new(),
        capped: None,
    });
    let jobs: Vec<(usize, f64)> = (0..=e).map(|k| (k, rules.max_len)).collect();
    let kept = relax_crowded_faces(&b, &jobs, &rules, &mut nodes, &[false; 6]);
    let [RefusedEdge { edge, reason }] = kept.as_slice() else {
        panic!("{kept:?}")
    };
    assert_eq!(*edge, e);
    assert!(
        reason.contains(&format!("edge {e}: the curve does not evaluate at t = 0.5")),
        "{reason}"
    );
    assert_eq!(nodes[e].params, params);
    assert_eq!(nodes[e].points, points);
}

/// An edge that cannot be sampled, whose uses also list a face that does not run it (a stale
/// use, an unvalidated record): only the two faces whose loops run it are refused; the third
/// is meshed, and is not among the edge's faces. It was refused with them.
#[test]
fn a_stale_use_does_not_make_a_face_an_edges() {
    let mut b = fixture("box_10x6x4");
    let ei = 3;
    let mut faces: Vec<usize> = b.edges[ei].uses.iter().map(|u| u.face as usize).collect();
    faces.sort_unstable();
    faces.dedup();
    let mut stale = b.edges[ei].uses[0];
    stale.face = 5;
    b.edges[ei].uses.push(stale);
    b.edges[ei].curve = Curve::Spline {
        knots: vec![0.0, 0.0, 1.0, 1.0],
        points: Vec::new(),
    };
    b.edges[ei].nurbs = None;
    let (p, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    let refused: Vec<usize> = stats.unmeshed.iter().map(|r| r.face).collect();
    assert_eq!(refused, faces, "{stats:?}");
    assert!(meshed_faces(&p).contains(&5));
    assert!(!p.edge_faces[ei].contains(&5));
}

/// An edge bent sharply at a corner no halving reaches (a polyline turning back on itself at a
/// third of its parameter) is halved at the corner until the depth bound: there its chords follow
/// it to a hair, but no segment across the corner turns less than a quarter turn. The plan names it
/// with that bound, its sag within the tolerance, and how far it exceeds its rules.
#[test]
fn an_edge_stopped_at_its_depth_bound_is_named_within_its_tolerance() {
    let mut b = fixture("box_10x6x4");
    b.edges[2].curve = Curve::Spline {
        knots: vec![0.0, 0.0, 1.0 / 3.0, 1.0, 1.0],
        points: vec![[10.0, 0.0, 4.0], [10.0, 2.0, 20.0], [10.0, 6.0, 4.0]],
    };
    b.edges[2].nurbs = None;
    let (_, stats) = ok(plan(&b, &MeshTolerance::chord(0.01), size_of("box_10x6x4")));
    let [capped] = stats.edges_capped.as_slice() else {
        panic!("{:?}", stats.edges_capped)
    };
    assert_eq!((capped.edge, capped.cap), (2, EdgeCap::Depth), "{capped:?}");
    assert!(capped.within && capped.sag <= capped.tol, "{capped:?}");
    assert!(capped.excess > 1.0, "{capped:?}");
}

/// Refining a spline face moves no triangle count: the memo of the pole classifier only drops the
/// repeated question (the coil took 1,089,434 / 1,091,452 calls at display / export before it).
#[test]
fn the_spline_coil_keeps_its_triangles() {
    let name = "smlib_coil_lower_half";
    let (b, size) = (fixture(name), size_of(name));
    for (t, want) in [
        (MeshTolerance::display_of(size), 30_776),
        (MeshTolerance::export_of(size), 30_808),
    ] {
        let (p, _) = plan(&b, &t, size).unwrap();
        let (m, _) = p.evaluate();
        assert_eq!(m.triangles.len(), want);
    }
}
