//! A sheet turned over through ops: each face's normal in its shell the other way at the same
//! point, the record round-tripping, turned over twice it is the sheet again, and a closed body
//! refused.

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
use solidcore_record::brep::{Brep, Curve, Loop, Shell};

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
fn turned_over_each_face_s_normal_is_the_other_way_at_the_same_point() {
    let before = sheet();
    let after = reverse_sheet(&before).unwrap();
    // Each face keeps its surface and domain; each loop runs the other way round: its edges in
    // reverse order (as a cycle, from wherever it starts), each used the other way.
    assert_eq!(after.faces.len(), before.faces.len());
    for (fb, fa) in before.faces.iter().zip(&after.faces) {
        assert_eq!(
            (&fa.surface, &fa.nurbs, fa.domain),
            (&fb.surface, &fb.nurbs, fb.domain),
            "the surface and domain are kept"
        );
        assert_eq!(fa.loops.len(), fb.loops.len());
        for (lb, la) in fb.loops.iter().zip(&fa.loops) {
            let (Loop::Edges { edges: eb }, Loop::Edges { edges: ea }) = (lb, la) else {
                assert_eq!(lb, la, "a vertex loop is kept");
                continue;
            };
            let was: Vec<(u32, bool)> = eb.iter().map(|e| (e.edge, e.forward)).collect();
            let back: Vec<(u32, bool)> = ea.iter().rev().map(|e| (e.edge, !e.forward)).collect();
            let same_cycle = (0..was.len().max(1)).any(|k| {
                let mut turned = back.clone();
                turned.rotate_left(k.min(turned.len()));
                turned == was
            });
            assert!(
                same_cycle,
                "the loop runs the other way: {was:?} then {back:?}"
            );
        }
    }
    assert_eq!(after.edges.len(), before.edges.len(), "the edges are kept");
    let (b, a) = (sides(&before), sides(&after));
    assert_eq!(b.len(), a.len());
    for ((fb, wb), (fa, wa)) in b.iter().zip(&a) {
        assert_eq!(fb, fa);
        assert_eq!(*wa, !*wb, "face {fb}");
    }
    let uv = {
        let d = before.faces[0].domain;
        [0.3 * d[0][0] + 0.7 * d[0][1], 0.6 * d[1][0] + 0.4 * d[1][1]]
    };
    let (nb, na) = (shell_normal(&before, 0, uv), shell_normal(&after, 0, uv));
    for k in 0..3 {
        assert!((nb[k] + na[k]).abs() < 1e-12, "{nb:?} {na:?}");
    }
}

#[test]
fn a_turned_sheet_round_trips_through_the_record() {
    let after = reverse_sheet(&sheet()).unwrap();
    let (body, _) = Body::from_record(&after, true).unwrap();
    let (again, _) = body.to_record().unwrap();
    assert_eq!(again, after);
}

#[test]
fn turned_over_twice_a_sheet_is_itself() {
    let before = sheet();
    let twice = reverse_sheet(&reverse_sheet(&before).unwrap()).unwrap();
    assert_eq!(twice, before);
}

#[test]
fn a_closed_body_is_refused() {
    let (cube, _) = make_box([0.0; 3], [10.0; 3]).unwrap().to_record().unwrap();
    assert!(matches!(reverse_sheet(&cube), Err(Error::Unsupported(_))));
}
