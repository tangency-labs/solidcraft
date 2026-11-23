//! A sheet turned over through ops (`surface::reverse_sheet`, the kernel's `SmFace::SwapUV` on
//! each face): each face still used as it was in its shell, its surface's u and v swapped, its
//! normal in its shell the other way at the same point; each edge the same edge through the
//! returned map; the record round-tripping; turned over twice it is the sheet again, each edge
//! back at its own number; a closed body refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::face_surface::FaceSurface;
use solidcore_ops::surface::{reverse_sheet, ruled};
use solidcore_ops::{make_box, Body, Error};
use solidcore_record::brep::{Brep, Curve, Edge, Loop, OrientedEdge, Shell, Surface};

/// A bumped sheet: the ruled sheet between two cubics.
fn sheet() -> Brep {
    let row = |y: f64, z: f64| Curve::Spline {
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        points: vec![[0.0, y, 0.0], [3.0, y, z], [6.0, y, -z], [9.0, y, 0.0]],
    };
    ruled(&row(0.0, 2.0), &row(8.0, 1.0), false).unwrap()
}

/// Each face's side in its shell: `(face, forward)`, in shell order.
fn sides(r: &Brep) -> Vec<(u32, bool)> {
    r.shells
        .iter()
        .flat_map(|s| match s {
            Shell::Faces { faces, .. } => faces.iter().map(|f| (f.face, f.forward)).collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// The point of face `face` at `uv`.
fn point(r: &Brep, face: u32, uv: [f64; 2]) -> [f64; 3] {
    FaceSurface::of(r, face as usize)
        .unwrap()
        .point(uv)
        .unwrap()
}

/// The normal face `face` has in its shell at `uv`: the surface's natural normal, turned round
/// where the shell uses the face backward.
fn shell_normal(r: &Brep, face: u32, uv: [f64; 2]) -> [f64; 3] {
    let forward = sides(r)
        .iter()
        .find(|(f, _)| *f == face)
        .map(|(_, fw)| *fw)
        .unwrap();
    let n = FaceSurface::of(r, face as usize)
        .unwrap()
        .normal(uv)
        .unwrap();
    if forward {
        n
    } else {
        [-n[0], -n[1], -n[2]]
    }
}

#[test]
fn turned_over_each_face_s_surface_is_swapped_and_its_shell_normal_negated() {
    let before = sheet();
    let (after, edges) = reverse_sheet(&before).unwrap();
    after.validate_built().unwrap();
    assert_eq!(sides(&after), sides(&before), "each face used as it was");
    assert_eq!(after.faces.len(), before.faces.len());
    for (i, (fb, fa)) in before.faces.iter().zip(&after.faces).enumerate() {
        let f = i as u32;
        assert_eq!(
            fa.domain,
            [fb.domain[1], fb.domain[0]],
            "face {i}'s box swapped"
        );
        // the spline's net transposed, bit for bit
        let (
            Surface::Spline {
                u_knots: ub,
                v_knots: vb,
                points: pb,
            },
            Surface::Spline {
                u_knots: ua,
                v_knots: va,
                points: pa,
            },
        ) = (&fb.surface, &fa.surface)
        else {
            panic!("face {i}: {:?}", fa.surface);
        };
        assert_eq!((ua, va), (vb, ub));
        for (r, row) in pb.iter().enumerate() {
            for (c, p) in row.iter().enumerate() {
                assert_eq!(pa[c][r], *p);
            }
        }
        // at the same point, the normal in its shell is the other way
        let d = fb.domain;
        for (s, t) in [(0.3, 0.6), (0.5, 0.5), (0.8, 0.1)] {
            let u = (1.0 - s) * d[0][0] + s * d[0][1];
            let v = (1.0 - t) * d[1][0] + t * d[1][1];
            let (pb, pa) = (point(&before, f, [u, v]), point(&after, f, [v, u]));
            let (nb, na) = (
                shell_normal(&before, f, [u, v]),
                shell_normal(&after, f, [v, u]),
            );
            for k in 0..3 {
                assert!((pb[k] - pa[k]).abs() < 1e-9, "{pb:?} {pa:?}");
                assert!(
                    (nb[k] + na[k]).abs() < 1e-12,
                    "face {i}'s normal at {pb:?} is negated: {nb:?} then {na:?}"
                );
            }
        }
    }
    // each edge is the same edge, at the index the map gives, one to one
    assert_eq!(edges.len(), before.edges.len());
    assert_eq!(after.edges.len(), before.edges.len());
    let mut seen = edges.clone();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), edges.len(), "the map is one to one");
    let ends = |r: &Brep, e: &Edge| e.vertices.map(|v| r.vertices[v as usize].point);
    for (i, &j) in edges.iter().enumerate() {
        let (eb, ea) = (&before.edges[i], &after.edges[j as usize]);
        assert_eq!(ea.curve, eb.curve, "edge {i} -> {j}");
        assert_eq!(ea.interval, eb.interval, "edge {i} -> {j}");
        assert_eq!(ends(&after, ea), ends(&before, eb), "edge {i} -> {j}");
    }
}

#[test]
fn a_turned_sheet_round_trips_through_the_record() {
    let (after, _) = reverse_sheet(&sheet()).unwrap();
    let (body, _) = Body::from_record(&after, true).unwrap();
    let (again, _) = body.to_record().unwrap();
    assert_eq!(again, after);
}

#[test]
fn turned_over_twice_a_sheet_is_itself() {
    let before = sheet();
    let (once, first) = reverse_sheet(&before).unwrap();
    let (twice, second) = reverse_sheet(&once).unwrap();
    for (i, &j) in first.iter().enumerate() {
        assert_eq!(
            second[j as usize] as usize, i,
            "edge {i} back at its own number"
        );
    }
    assert_eq!(sides(&twice), sides(&before));
    assert_eq!(twice.edges, before.edges);
    let ids = |e: &[OrientedEdge]| e.iter().map(|o| (o.edge, o.forward)).collect::<Vec<_>>();
    for (fb, fa) in before.faces.iter().zip(&twice.faces) {
        assert_eq!(
            (&fa.surface, &fa.nurbs, fa.domain),
            (&fb.surface, &fb.nurbs, fb.domain)
        );
        // the same loops (each pcurve mirrored about u = v twice, so compared by its edges)
        assert_eq!(fa.loops.len(), fb.loops.len());
        for (la, lb) in fa.loops.iter().zip(&fb.loops) {
            match (la, lb) {
                (Loop::Edges { edges: ea }, Loop::Edges { edges: eb }) => {
                    assert_eq!(ids(ea), ids(eb));
                }
                _ => assert_eq!(la, lb),
            }
        }
    }
}

/// `r` with every face of its open shells used backward, as a record from before the side
/// lived in the surface writes a turned sheet (Reverse Normal's `forward: false`).
fn used_backward(r: &Brep) -> Brep {
    let mut loose = r.clone();
    for s in &mut loose.shells {
        if let Shell::Faces { faces, .. } = s {
            for f in faces {
                f.forward = false;
            }
        }
    }
    for e in &mut loose.edges {
        e.uses.clear();
    }
    loose.canonical().unwrap().brep
}

/// A sheet whose face its record uses backward (`forward: false` in its open shell, #13640)
/// comes back from the kernel with its side: the face used forward on its surface turned over,
/// the same point of it with the same normal in its shell (the backward one). Its edges and
/// vertices are where they were. Turned over by ops, it faces as the record did not.
#[test]
fn a_face_used_backward_keeps_its_side_through_the_kernel() {
    let front = sheet();
    let back = used_backward(&front);
    back.validate_built().unwrap();
    assert_eq!(sides(&back), vec![(0, false)]);
    let (body, _) = Body::from_record(&back, true).unwrap();
    let (again, _) = body.to_record().unwrap();
    again.validate_built().unwrap();
    assert_eq!(
        sides(&again),
        vec![(0, true)],
        "every face of an open shell used forward"
    );
    let d = back.faces[0].domain;
    for (s, t) in [(0.3, 0.6), (0.5, 0.5), (0.8, 0.1)] {
        let u = (1.0 - s) * d[0][0] + s * d[0][1];
        let v = (1.0 - t) * d[1][0] + t * d[1][1];
        // the surface turned over: its u and v swapped
        let (pb, pa) = (point(&back, 0, [u, v]), point(&again, 0, [v, u]));
        let (nb, na) = (
            shell_normal(&back, 0, [u, v]),
            shell_normal(&again, 0, [v, u]),
        );
        let nf = shell_normal(&front, 0, [u, v]);
        for k in 0..3 {
            assert!((pb[k] - pa[k]).abs() < 1e-9, "{pb:?} {pa:?}");
            assert!(
                (nb[k] - na[k]).abs() < 1e-12,
                "the side kept at {pb:?}: {nb:?} then {na:?}"
            );
            assert!(
                (nf[k] + na[k]).abs() < 1e-12,
                "the other side from the front's: {nf:?}, {na:?}"
            );
        }
    }
    // numbered as the record numbers them: each vertex where it was, each edge between the
    // same vertices along the same curve
    let points = |r: &Brep| r.vertices.iter().map(|v| v.point).collect::<Vec<_>>();
    assert_eq!(points(&again), points(&back));
    let ends = |r: &Brep| {
        r.edges
            .iter()
            .map(|e| (e.vertices, e.curve.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ends(&again), ends(&back));
    // turned over by ops, the record's front again
    let (turned, _) = reverse_sheet(&back).unwrap();
    let n = shell_normal(
        &turned,
        0,
        [d[0][0] * 0.5 + d[0][1] * 0.5, d[1][0] * 0.5 + d[1][1] * 0.5],
    );
    let f = shell_normal(
        &front,
        0,
        [d[0][0] * 0.5 + d[0][1] * 0.5, d[1][0] * 0.5 + d[1][1] * 0.5],
    );
    for k in 0..3 {
        assert!((n[k] - f[k]).abs() < 1e-12, "{n:?} vs {f:?}");
    }
}

#[test]
fn a_closed_body_is_refused() {
    let (cube, _) = make_box([0.0; 3], [10.0; 3]).unwrap().to_record().unwrap();
    assert!(matches!(reverse_sheet(&cube), Err(Error::Unsupported(_))));
}
