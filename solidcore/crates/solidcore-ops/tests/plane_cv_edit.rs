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

/// Each record edge's end points, by index.
fn edge_ends(r: &Brep) -> Vec<[[f64; 3]; 2]> {
    r.edges
        .iter()
        .map(|e| e.vertices.map(|v| r.vertices[v as usize].point))
        .collect()
}

fn same(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= 1e-9)
}

/// A CV edit changes no topology, so the record lists the sheet's edges as it did: a client
/// that names an edge by its index (a skin from it, an align's master) keeps the same edge. An
/// interior CV moved leaves every edge's ends where they were, edge by edge; a corner CV moved
/// moves that corner of the two edges at it, and only that.
#[test]
fn a_cv_edit_keeps_the_records_edge_order() {
    let skin = flat_skin();
    let was = edge_ends(&skin);
    assert_eq!(was.len(), 4);
    let net = control_net(&skin).unwrap();

    let p = net.points[net.cols + 1];
    let crowned = move_cvs(&skin, &[((1, 1), [p[0], p[1], p[2] + 2.0])]).unwrap();
    let now = edge_ends(&crowned);
    assert_eq!(now.len(), was.len());
    for (i, (w, n)) in was.iter().zip(&now).enumerate() {
        let kept = (same(w[0], n[0]) && same(w[1], n[1])) || (same(w[0], n[1]) && same(w[1], n[0]));
        assert!(kept, "edge {i}: {w:?} became {n:?}");
    }

    // the net's corner (0, 0) is a corner of the sheet: lift it
    let corner = net.points[0];
    let lifted = [corner[0], corner[1], corner[2] + 1.5];
    let moved = move_cvs(&skin, &[((0, 0), lifted)]).unwrap();
    let now = edge_ends(&moved);
    assert_eq!(now.len(), was.len());
    let at = |p: [f64; 3]| if same(p, corner) { lifted } else { p };
    for (i, (w, n)) in was.iter().zip(&now).enumerate() {
        let w = [at(w[0]), at(w[1])];
        let kept = (same(w[0], n[0]) && same(w[1], n[1])) || (same(w[0], n[1]) && same(w[1], n[0]));
        assert!(kept, "edge {i}: {w:?} became {n:?}");
    }
    // and again: the edited sheet edited keeps its order too
    let again = move_cvs(&moved, &[((1, 1), [p[0], p[1], p[2] + 2.0])]).unwrap();
    for (i, (w, n)) in edge_ends(&moved).iter().zip(&edge_ends(&again)).enumerate() {
        let kept = (same(w[0], n[0]) && same(w[1], n[1])) || (same(w[0], n[1]) && same(w[1], n[0]));
        assert!(kept, "edge {i}: {w:?} became {n:?}");
    }
}
