//! A sheet turned over through ops (`Body::turn_over`): its record reads each face the other way
//! (`forward` negated, the faces, edges and vertices as they were) and goes round the kernel;
//! turned over twice it is the sheet again; a closed body is refused and left as it was.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use solidcore_ops::surface::ruled;
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

/// `r` turned over through ops.
fn turned(r: &Brep) -> Brep {
    let (body, _) = Body::from_record(r, true).unwrap();
    body.turn_over().unwrap();
    body.to_record().unwrap().0
}

#[test]
fn a_turned_sheet_reads_each_face_the_other_way() {
    let before = sheet();
    let after = turned(&before);
    after.validate_built().unwrap();
    assert_eq!(
        after.faces, before.faces,
        "surfaces, domains and loops kept"
    );
    assert_eq!(after.edges, before.edges);
    assert_eq!(after.vertices, before.vertices);
    let (b, a) = (sides(&before), sides(&after));
    assert!(!b.is_empty());
    assert_eq!(b.len(), a.len());
    for ((fb, wb), (fa, wa)) in b.iter().zip(&a) {
        assert_eq!(fb, fa);
        assert_eq!(*wa, !*wb, "face {fb}'s forward is negated");
    }

    // the turned record goes round the kernel as it is
    let (body, _) = Body::from_record(&after, true).unwrap();
    assert_eq!(body.to_record().unwrap().0, after);
}

#[test]
fn turned_over_twice_a_sheet_is_itself() {
    let before = sheet();
    assert_eq!(turned(&turned(&before)), before);
}

#[test]
fn a_closed_body_is_refused_and_left_as_it_was() {
    let body = make_box([0.0; 3], [1.0, 2.0, 3.0]).unwrap();
    let (before, _) = body.to_record().unwrap();
    assert!(matches!(body.turn_over(), Err(Error::Unsupported(_))));
    assert_eq!(body.to_record().unwrap().0, before);
}
