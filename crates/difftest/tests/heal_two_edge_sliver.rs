//! `SmApiHealBrep` on a sliver face bounded by two edges that share both vertices.
//!
//! The shape is a 10 mm cube whose top front edge is doubled. The top face has the straight
//! edge B from (0, 0, 10) to (10, 0, 10). The front face (y = 0) has, in its place, a curve A
//! between the same two vertices that sags below B by `sag` at mid-span. A seventh face, the
//! sliver, lies on the front plane between A and B: one loop of the two edges. The B-rep
//! built through `SmBrepConstructor` is closed and manifold.
//!
//! Heal's `Fix_DegenFaces` squeezes the sliver (`SmBrep::SqueezeFace`). That deletes the
//! face, then `SmStitch::StitchEdgesOfVertices` glues A and B, and `Fix_BadSheets` makes
//! the solid region again: the cube, 2 regions, 6 faces, 12 edges, 8 vertices, manifold,
//! volume 1000. The pattern is that of a sliver in a customer STEP body, where the port
//! kept both edges and the healed body had no solid region and volume 0.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use difftest::step::{build_rust_constructor, Record};
use step_import::model::{
    BSplineCurve, Body, BodyKind, Bound, Curve, Edge, Face, Frame, Loop, OrientedEdge, Shell,
    Surface, Vertex, P3,
};

fn frame(origin: P3, x: P3, y: P3) -> Frame {
    let z = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    Frame { origin, x, y, z }
}

fn plane(origin: P3, x: P3, y: P3) -> Surface {
    Surface::Plane {
        frame: frame(origin, x, y),
    }
}

fn line(a: usize, b: usize, p: P3, q: P3) -> Edge {
    let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    Edge {
        step_id: 0,
        start: a,
        end: b,
        curve: Curve::Line {
            origin: p,
            dir: [d[0] / n, d[1] / n, d[2] / n],
            magnitude: 1.0,
        },
        same_sense: true,
        t_start: 0.0,
        t_end: n,
    }
}

/// The quadratic Bezier from `p` to `q` in the plane y = 0 whose middle sits `sag` below
/// the chord (its control point `2 sag` below the chord's midpoint).
fn sagging(a: usize, b: usize, p: P3, q: P3, sag: f64) -> Edge {
    let mid = [
        0.5 * (p[0] + q[0]),
        0.5 * (p[1] + q[1]),
        0.5 * (p[2] + q[2]) - 2.0 * sag,
    ];
    Edge {
        step_id: 0,
        start: a,
        end: b,
        curve: Curve::BSpline(BSplineCurve {
            degree: 2,
            points: vec![p, mid, q],
            weights: None,
            knots: vec![0.0, 1.0],
            mults: vec![3, 3],
        }),
        same_sense: true,
        t_start: 0.0,
        t_end: 1.0,
    }
}

fn oe(edge: usize, forward: bool) -> OrientedEdge {
    OrientedEdge { edge, forward }
}

fn face(surface: Surface, edges: Vec<OrientedEdge>) -> Face {
    Face {
        step_id: 0,
        surface,
        same_sense: true,
        loops: vec![Loop {
            outer: true,
            orientation: true,
            bound: Bound::Edges(edges),
        }],
    }
}

/// The doubled-edge cube. With `b_reversed` edge B runs from (10, 0, 10) to (0, 0, 10),
/// against A, so the glue is `SM_OT_OPPOSITE`. With `b_curved` B is itself a curve
/// sagging `-sag` (above the chord), so neither edge is a line and the glue takes the
/// 5 point check.
fn doubled_edge_cube(sag: f64, b_reversed: bool, b_curved: bool) -> Body {
    let points: Vec<P3> = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [0.0, 0.0, 10.0],
        [10.0, 0.0, 10.0],
        [10.0, 10.0, 10.0],
        [0.0, 10.0, 10.0],
    ];
    let l = |a: usize, b: usize| line(a, b, points[a], points[b]);
    let (b0, b1) = if b_reversed { (5, 4) } else { (4, 5) };
    let edge_b = if b_curved {
        sagging(b0, b1, points[b0], points[b1], -sag)
    } else {
        l(b0, b1)
    };
    let edges = vec![
        l(0, 1),                                  // 0 bottom front
        l(1, 2),                                  // 1 bottom right
        l(2, 3),                                  // 2 bottom back
        l(3, 0),                                  // 3 bottom left
        edge_b,                                   // 4 B: top front, on the top face
        l(5, 6),                                  // 5 top right
        l(6, 7),                                  // 6 top back
        l(7, 4),                                  // 7 top left
        l(0, 4),                                  // 8 vertical front left
        l(1, 5),                                  // 9 vertical front right
        l(2, 6),                                  // 10 vertical back right
        l(3, 7),                                  // 11 vertical back left
        sagging(4, 5, points[4], points[5], sag), // 12 A: on the front face
    ];
    // edge B traversed from (0, 0, 10) to (10, 0, 10)
    let b_fwd = !b_reversed;
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let (nx, ny) = ([-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]);
    let faces = vec![
        // bottom, normal -z
        face(
            plane(points[0], x, ny),
            vec![oe(3, false), oe(2, false), oe(1, false), oe(0, false)],
        ),
        // top, normal +z: B from (0, 0, 10) to (10, 0, 10)
        face(
            plane(points[4], x, y),
            vec![oe(4, b_fwd), oe(5, true), oe(6, true), oe(7, true)],
        ),
        // front (y = 0), normal -y, its top edge A (traversed from (10, 0, 10) back)
        face(
            plane(points[0], x, z),
            vec![oe(0, true), oe(9, true), oe(12, false), oe(8, false)],
        ),
        // right (x = 10), normal +x
        face(
            plane(points[1], y, z),
            vec![oe(1, true), oe(10, true), oe(5, false), oe(9, false)],
        ),
        // back (y = 10), normal +y
        face(
            plane(points[2], nx, z),
            vec![oe(2, true), oe(11, true), oe(6, false), oe(10, false)],
        ),
        // left (x = 0), normal -x
        face(
            plane(points[3], ny, z),
            vec![oe(3, true), oe(8, true), oe(7, false), oe(11, false)],
        ),
        // the sliver on the front plane: along A, then back along B
        face(plane(points[0], x, z), vec![oe(12, true), oe(4, !b_fwd)]),
    ];
    let n = faces.len();
    let body = Body {
        step_id: 0,
        name: None,
        kind: BodyKind::ManifoldSolid,
        unit_mm: 1.0,
        uncertainty_mm: None,
        shells: vec![Shell {
            step_id: 0,
            closed: true,
            orientation: true,
            faces: (0..n).collect(),
        }],
        faces,
        edges,
        vertices: points
            .into_iter()
            .map(|point| Vertex { step_id: 0, point })
            .collect(),
    };
    // Both kernels build from the decoded encoding, as `--step-case` does.
    step_import::flat::decode(&step_import::flat::encode(&body)).unwrap()
}

fn stage<'a>(r: &'a Record, name: &str) -> &'a [f64] {
    r.stages
        .iter()
        .find(|(n, _)| n == name)
        .map_or_else(|| panic!("no stage {name}: {r:?}"), |(_, v)| v.as_slice())
}

/// The cases: (name, sag, B reversed, B curved).
const CASES: [(&str, f64, bool, bool); 4] = [
    ("line", 5e-7, false, false),
    ("line reversed", 5e-7, true, false),
    ("curves", 5e-7, false, true),
    ("curves reversed", 5e-7, true, true),
];

fn assert_healed(name: &str, r: &Record) {
    assert!(r.deferred.is_none(), "{name}: {r:?}");
    // the constructor's B-rep: closed and manifold, 7 faces, 13 edges, 8 vertices
    let construct = stage(r, "construct");
    assert_eq!(
        construct[1], 0.0,
        "{name}: a constructor call failed: {construct:?}"
    );
    assert_eq!(stage(r, "counts"), [2.0, 2.0, 7.0, 13.0, 8.0], "{name}");
    assert_eq!(stage(r, "manifold"), [0.0, 1.0], "{name}");
    // healed: the sliver squeezed, A and B glued, the solid region made again
    assert_eq!(stage(r, "heal"), [0.0], "{name}");
    assert_eq!(
        stage(r, "healed_counts"),
        [2.0, 2.0, 6.0, 12.0, 8.0],
        "{name}"
    );
    assert_eq!(stage(r, "healed_manifold"), [0.0, 1.0], "{name}");
    let v = stage(r, "healed_volume");
    assert_eq!(v[0], 0.0, "{name}");
    assert!(
        (v[1] - 1000.0).abs() <= 1e-9 * 1000.0,
        "{name}: volume {}",
        v[1]
    );
}

#[test]
fn heal_glues_the_two_edges_of_a_squeezed_sliver_face() {
    for (name, sag, b_reversed, b_curved) in CASES {
        let body = doubled_edge_cube(sag, b_reversed, b_curved);
        let r = build_rust_constructor(&body);
        assert_healed(name, &r);
    }
}

/// The same cases on the C++ kernel: the same healed body, stage for stage.
#[cfg(feature = "cpp")]
#[test]
fn heal_of_a_two_edge_sliver_face_matches_cpp() {
    use difftest::step::{build_cpp_constructor, first_difference};
    for (name, sag, b_reversed, b_curved) in CASES {
        let body = doubled_edge_cube(sag, b_reversed, b_curved);
        let cpp = build_cpp_constructor(&body).unwrap();
        assert_healed(&format!("{name} (C++)"), &cpp);
        let rust = build_rust_constructor(&body);
        assert_eq!(first_difference(&cpp, &rust), None, "{name}");
    }
}
