//! CV edits on a sphere, a torus and a tube (a cylinder's wall): the face is first made its
//! surface's exact NURBS form (the kernel's `editable_sheet`), a rational net; a moved CV keeps
//! its weight, every other CV and weight stays where it was, and the points that are one point
//! (a seam's twins, a pole's row) move together, so a closed surface comes back closed, with one
//! seam edge. A rational net is not rebuilt: the kernel has no certified bound for a rational
//! surface's refit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_arithmetic
)]

use solidcore_ops::body::{make_cylinder, make_sphere, make_torus};
use solidcore_ops::surface_cvs::{control_net, move_cvs, move_net, rebuild, Net, Picks};
use solidcore_ops::sweep::{extrude, Curve, DraftCorner};
use solidcore_ops::Error;
use solidcore_record::brep::{Brep, Loop, Surface};

fn sphere() -> Brep {
    make_sphere([0.0, 0.0, 0.0], 5.0)
        .unwrap()
        .to_record()
        .unwrap()
        .0
}

fn torus() -> Brep {
    make_torus([0.0, 0.0, 0.0], 10.0, 3.0)
        .unwrap()
        .to_record()
        .unwrap()
        .0
}

/// A circle of radius 5 about z extruded 10 up, open: one face.
fn tube() -> Brep {
    let circle = Curve::Arc {
        center: [0.0, 0.0, 0.0],
        axis: [0.0, 0.0, 1.0],
        reference: [1.0, 0.0, 0.0],
        radius: 5.0,
        start: 0.0,
        end: std::f64::consts::TAU,
    };
    extrude(
        &[circle],
        [0.0, 0.0, 1.0],
        10.0,
        0.0,
        DraftCorner::Curvature,
        false,
    )
    .unwrap()
    .to_record()
    .unwrap()
    .0
}

/// The edges the record's one face uses twice in its loop: its seams.
fn seams(r: &Brep) -> usize {
    let [face] = r.faces.as_slice() else {
        panic!("one face: {}", r.faces.len());
    };
    let [Loop::Edges { edges }] = face.loops.as_slice() else {
        panic!("one loop of edges: {:?}", face.loops);
    };
    let mut ids: Vec<u32> = edges.iter().map(|e| e.edge).collect();
    ids.sort_unstable();
    ids.windows(2).filter(|w| w[0] == w[1]).count()
}

fn near(a: [f64; 3], b: [f64; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= 1e-9)
}

fn at(net: &Net, (i, j): (usize, usize)) -> [f64; 3] {
    net.points[i * net.cols + j]
}

/// A seam's two twins: a tie of two points, past the net's corners.
fn seam_pair(net: &Net) -> Vec<(usize, usize)> {
    net.ties
        .iter()
        .find(|g| g.len() == 2 && g.iter().all(|&(i, j)| i != 0 || j != 0))
        .cloned()
        .unwrap_or_else(|| panic!("no seam pair in {:?}", net.ties))
}

/// Every point of `after` but those of `moved` is where it was in `before`, every weight
/// unchanged, and each of `moved` at `to`.
fn only_moved(before: &Net, after: &Net, moved: &[(usize, usize)], to: [f64; 3]) {
    assert_eq!((after.rows, after.cols), (before.rows, before.cols));
    assert_eq!(
        after
            .weights
            .iter()
            .map(|w| w.to_bits())
            .collect::<Vec<_>>(),
        before
            .weights
            .iter()
            .map(|w| w.to_bits())
            .collect::<Vec<_>>(),
        "the weights are kept"
    );
    for i in 0..before.rows {
        for j in 0..before.cols {
            let want = if moved.contains(&(i, j)) {
                to
            } else {
                at(before, (i, j))
            };
            assert!(
                near(at(after, (i, j)), want),
                "({i}, {j}) at {:?}, want {want:?}",
                at(after, (i, j))
            );
        }
    }
}

#[test]
fn a_spheres_net_is_its_exact_rational_form_with_its_poles_and_seam_tied() {
    let s = sphere();
    assert!(matches!(s.faces[0].surface, Surface::Sphere { .. }));
    let net = control_net(&s).unwrap();
    assert_eq!((net.rows, net.cols), (9, 5));
    assert_eq!(net.weights.len(), 45);
    let pole = |j: usize| (0..9).map(|i| (i, j)).collect::<Vec<_>>();
    assert!(net.ties.contains(&pole(0)), "{:?}", net.ties);
    assert!(net.ties.contains(&pole(4)), "{:?}", net.ties);
    assert!(net.ties.contains(&vec![(0, 2), (8, 2)]), "{:?}", net.ties);
}

#[test]
fn an_interior_cv_moves_alone_and_the_sphere_keeps_one_seam_edge() {
    let s = sphere();
    let before = control_net(&s).unwrap();
    let p = at(&before, (4, 2));
    let to = [p[0] * 1.2, p[1] * 1.2, p[2] + 0.5];
    let moved = move_cvs(&s, &[((4, 2), to)]).unwrap();
    assert!(matches!(
        moved.faces[0].surface,
        Surface::RationalSpline { .. }
    ));
    only_moved(&before, &control_net(&moved).unwrap(), &[(4, 2)], to);
    assert_eq!(seams(&moved), 1);
}

#[test]
fn a_seam_cv_moves_with_its_twin_and_the_tube_stays_closed() {
    let t = tube();
    assert_eq!(t.faces.len(), 1);
    let before = control_net(&t).unwrap();
    let pair = seam_pair(&before);
    let p = at(&before, pair[0]);
    let to = [p[0] + 1.0, p[1], p[2]];
    let moved = move_cvs(&t, &[(pair[0], to)]).unwrap();
    only_moved(&before, &control_net(&moved).unwrap(), &pair, to);
    assert_eq!(seams(&moved), 1);
}

#[test]
fn a_pole_moves_as_one() {
    let s = sphere();
    let before = control_net(&s).unwrap();
    let p = at(&before, (3, 4));
    let to = [p[0], p[1], p[2] + 2.0];
    let moved = move_cvs(&s, &[((3, 4), to)]).unwrap();
    let row: Vec<(usize, usize)> = (0..before.rows).map(|i| (i, 4)).collect();
    only_moved(&before, &control_net(&moved).unwrap(), &row, to);
    assert_eq!(seams(&moved), 1);
}

#[test]
fn a_torus_seam_pick_moves_its_twin_by_the_same_delta() {
    let t = torus();
    let before = control_net(&t).unwrap();
    let pair = seam_pair(&before);
    let p = at(&before, pair[0]);
    let moved = move_net(&t, &Picks::Cvs(vec![pair[0]]), [0.0, 0.0, 1.0], None).unwrap();
    only_moved(
        &before,
        &control_net(&moved).unwrap(),
        &pair,
        [p[0], p[1], p[2] + 1.0],
    );
    assert_eq!(seams(&moved), 2);
}

#[test]
fn a_rational_net_is_not_rebuilt() {
    assert!(matches!(
        rebuild(&sphere(), 3, 3, 4, 4, 1.0),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn a_body_of_several_faces_is_refused() {
    let c = make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0)
        .unwrap()
        .to_record()
        .unwrap()
        .0;
    assert_eq!(control_net(&c), Err(Error::Argument));
}
