//! CV edits on a flat skin, whose face is a plane (since the skin makes flat walls planes): the
//! face is first rebuilt as the Coons spline of its edges (the kernel's `face_spline_sheet`), so
//! it has a net; moving one interior CV off the plane crowns it, every other CV where the
//! conversion put it and the boundary unchanged.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::surface_cvs::{control_net, move_cvs};
use solidcore_ops::sweep::{loft, Curve};
use solidcore_record::brep::{Brep, Surface};

/// A cubic in the plane z = 0 from x = 0 to 9 at about `y`, wiggling across it.
fn section(y: f64, wiggle: f64) -> Curve {
    Curve::Nurbs {
        degree: 3,
        points: vec![
            [0.0, y, 0.0, 1.0],
            [3.0, y + wiggle, 0.0, 1.0],
            [6.0, y - wiggle, 0.0, 1.0],
            [9.0, y, 0.0, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}

/// The skin, quadratic across, through three coplanar cubics at y = 0, 4 and 8, open: one flat
/// wall.
fn flat_skin() -> Brep {
    let body = loft(
        &[
            vec![section(0.0, 1.0)],
            vec![section(4.0, -0.5)],
            vec![section(8.0, 1.0)],
        ],
        2,
        false,
    )
    .unwrap();
    body.to_record().unwrap().0
}

/// The record's vertices.
fn corners(r: &Brep) -> Vec<[f64; 3]> {
    r.vertices.iter().map(|v| v.point).collect()
}

#[test]
fn a_flat_skin_has_a_net_and_a_moved_cv_crowns_it() {
    let skin = flat_skin();
    assert_eq!(skin.faces.len(), 1);
    assert!(
        matches!(skin.faces[0].surface, Surface::Plane { .. }),
        "the flat skin is a plane face: {:?}",
        skin.faces[0].surface
    );
    let net = control_net(&skin).unwrap();
    assert!(net.rows >= 3 && net.cols >= 3, "{net:?}");
    let k = net.cols + 1; // the point (1, 1): inside the net
    let p = net.points[k];
    let to = [p[0], p[1], p[2] + 2.0];
    let crowned = move_cvs(&skin, &[((1, 1), to)]).unwrap();
    assert_eq!(crowned.faces.len(), 1);
    assert!(
        matches!(crowned.faces[0].surface, Surface::Spline { .. }),
        "{:?}",
        crowned.faces[0].surface
    );
    let after = control_net(&crowned).unwrap();
    assert_eq!((after.rows, after.cols), (net.rows, net.cols));
    for (i, (a, b)) in after.points.iter().zip(&net.points).enumerate() {
        if i == k {
            assert_eq!(*a, to);
        } else {
            assert_eq!(a.map(f64::to_bits), b.map(f64::to_bits), "point {i} moved");
        }
    }
    // the boundary is the skin's: its corners where they were
    let (was, now) = (corners(&skin), corners(&crowned));
    assert_eq!(was.len(), now.len());
    for c in &was {
        assert!(
            now.iter()
                .any(|q| (0..3).all(|i| (q[i] - c[i]).abs() <= 1e-9)),
            "{c:?} not in {now:?}"
        );
    }
}
