//! A face whose edge is an intersection curve that does not evaluate (#1252): meshing it neither
//! panics nor gives non-finite points.

use solidcraft_geom::brep::{self as b, Curve, Curve2, Surface};

/// A plane at height `z` with its parameters along x and y.
fn plane(z: f64) -> Surface {
    Surface::Plane { origin: [0.0, 0.0, z], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] }
}

/// A triangular sheet at z = 0.5 whose first edge is the intersection of two planes: the parallel
/// planes `z = 0` and `z = 1` when `support_gap` is `1.0` (the projection onto both never
/// converges), the same plane twice when it is `0.0` (the leader lies on both and always
/// evaluates).
fn triangle_with_intersection_edge(support_gap: f64) -> b::Brep {
    let corners = [[0.0, 0.0, 0.5], [1.0, 0.0, 0.5], [0.5, 1.0, 0.5]];
    let uv = [[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]];
    let traced = Curve::SurfaceIntersection {
        surfaces: [Box::new(plane(0.5 - support_gap / 2.0)), Box::new(plane(0.5 + support_gap / 2.0))],
        leader: Box::new(Curve::Polyline { points: vec![corners[0], corners[1]] }),
        tolerance: 0.01,
    };
    let curve = |k: usize| if k == 0 { traced.clone() } else { Curve::Line { start: corners[k], end: corners[(k + 1) % 3] } };
    let edges = (0..3)
        .map(|k| b::Edge { nurbs: None, curve: curve(k), interval: [0.0, 1.0], vertices: [k as u32, ((k + 1) % 3) as u32], tolerance: 1e-6, uses: Vec::new() })
        .collect();
    let uses = (0..3)
        .map(|k| b::OrientedEdge {
            edge: k as u32,
            forward: true,
            pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![uv[k], uv[(k + 1) % 3]] },
        })
        .collect();
    let face = b::Face { nurbs: None, surface: plane(0.5), domain: [[0.0, 1.0], [0.0, 1.0]], loops: vec![b::Loop::Edges { edges: uses }] };
    let loose = b::Brep {
        version: b::VERSION,
        tolerance: 1e-6,
        regions: vec![b::Region { shells: vec![0], void: false }],
        shells: vec![b::Shell::Faces { faces: vec![b::OrientedFace { face: 0, forward: true }], wires: Vec::new() }],
        faces: vec![face],
        edges,
        vertices: corners.iter().map(|p| b::Vertex { point: *p, tolerance: 1e-6 }).collect(),
    };
    loose.canonical().unwrap().brep
}

/// Meshing an edge whose intersection curve does not evaluate (#1252): the two supports here are
/// parallel planes, so the projection never converges. Nothing may panic (wasm aborts on one, and
/// the native guard prints the message first), and a mesh that comes back is finite.
#[test]
fn meshing_an_edge_that_does_not_evaluate_neither_panics_nor_goes_non_finite() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    for gap in [1.0, 0.0] {
        let me = std::thread::current().id();
        let count = Arc::new(AtomicUsize::new(0));
        let c2 = count.clone();
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |_| {
            if std::thread::current().id() == me {
                c2.fetch_add(1, Ordering::SeqCst);
            }
        }));
        let meshed = std::panic::catch_unwind(|| {
            crate::Body::from_record(triangle_with_intersection_edge(gap)).and_then(|body| body.mesh(&crate::MeshTolerance::chord(0.01)))
        });
        std::panic::set_hook(prev);
        assert_eq!(count.load(Ordering::SeqCst), 0, "gap {gap}: panics while meshing");
        if let Ok(Ok(mesh)) = meshed {
            assert!(mesh.positions.iter().all(|p| p.is_finite()), "gap {gap}");
        }
    }
}
