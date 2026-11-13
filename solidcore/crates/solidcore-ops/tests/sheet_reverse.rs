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
use solidcore_record::brep::{Brep, Curve, Shell};

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
fn turned_over_each_face_s_forward_and_shell_normal_are_negated() {
    let before = sheet();
    let after = reverse_sheet(&before).unwrap();
    assert_eq!(
        after.faces, before.faces,
        "each face's surface, domain and loops are kept"
    );
    assert_eq!(after.edges, before.edges, "the edges are kept");
    let (b, a) = (sides(&before), sides(&after));
    assert!(!b.is_empty());
    assert_eq!(b.len(), a.len());
    for ((fb, wb), (fa, wa)) in b.iter().zip(&a) {
        assert_eq!(fb, fa);
        assert_eq!(*wa, !*wb, "face {fb}'s forward is negated");
        let d = before.faces[*fb as usize].domain;
        for (s, t) in [(0.3, 0.6), (0.5, 0.5), (0.8, 0.1)] {
            let uv = [
                (1.0 - s) * d[0][0] + s * d[0][1],
                (1.0 - t) * d[1][0] + t * d[1][1],
            ];
            let (nb, na) = (
                shell_normal(&before, *fb, uv),
                shell_normal(&after, *fb, uv),
            );
            for k in 0..3 {
                assert!(
                    (nb[k] + na[k]).abs() < 1e-12,
                    "face {fb}'s shell normal at {uv:?} is negated: {nb:?} then {na:?}"
                );
            }
        }
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
