//! A control-net edit of one face of a multi-face body (u81): a capped cylinder's or cone's side
//! face with a CV of its top row moved, the cap that shares that row's edge following, the body
//! still closed, and its faces and edges still where they were in the record.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use solidcore_ops::surface_cvs::{face_net, move_face_cvs, move_face_net, Net, Picks};
use solidcore_ops::{make_cone, make_cylinder, Body, CheckLevel, Error};
use solidcore_record::brep::{Brep, Edge, Surface};

/// The record of `body`.
fn record(body: &Body) -> Brep {
    body.to_record().unwrap().0
}

/// The index of the face on a cylinder or cone (the side).
fn side(r: &Brep) -> usize {
    r.faces
        .iter()
        .position(|f| matches!(f.surface, Surface::Cylinder { .. } | Surface::Cone { .. }))
        .expect("a side face")
}

/// The CV of the side's net at height `z` farthest along +y (a joint of its quarter arcs, off
/// the seam at +x) and the CVs on either side of it along its row, moved by `delta` together:
/// the three move as one, so the arcs' tangents at the joint stay one line and the surface keeps
/// its G1 continuity there.
fn joint_moves(net: &Net, z: f64, delta: [f64; 3]) -> Vec<((usize, usize), [f64; 3])> {
    let at = |(i, j): (usize, usize)| net.points[i * net.cols + j];
    let (i, j) = (0..net.rows)
        .flat_map(|i| (0..net.cols).map(move |j| (i, j)))
        .filter(|&p| (at(p)[2] - z).abs() < 1e-9)
        .max_by(|&a, &b| at(a)[1].total_cmp(&at(b)[1]))
        .expect("a CV at that height");
    assert!(i > 0 && i + 1 < net.rows, "the joint is inside its row");
    [(i - 1, j), (i, j), (i + 1, j)]
        .into_iter()
        .map(|p| {
            let q = at(p);
            (p, [q[0] + delta[0], q[1] + delta[1], q[2] + delta[2]])
        })
        .collect()
}

/// Each edge's uses as (face, loop, position), sorted.
fn uses(e: &Edge) -> Vec<(u32, u32, u32)> {
    let mut u: Vec<_> = e
        .uses
        .iter()
        .map(|u| (u.face, u.r#loop, u.position))
        .collect();
    u.sort_unstable();
    u
}

/// The edited body: valid, a closed manifold solid, its record the same faces (each on the
/// same kind of surface but the edited one, on a spline) and the same edges, each used by the
/// same faces at the same loop positions.
fn holds_its_record(before: &Brep, after: &Brep, edited: usize) -> Body {
    assert_eq!(after.faces.len(), before.faces.len());
    assert_eq!(after.edges.len(), before.edges.len());
    assert_eq!(after.vertices.len(), before.vertices.len());
    for (i, (a, b)) in before.faces.iter().zip(&after.faces).enumerate() {
        if i == edited {
            assert!(
                matches!(
                    b.surface,
                    Surface::Spline { .. } | Surface::RationalSpline { .. }
                ),
                "{:?}",
                b.surface
            );
        } else {
            assert_eq!(
                std::mem::discriminant(&a.surface),
                std::mem::discriminant(&b.surface),
                "face {i}"
            );
        }
        assert_eq!(a.loops.len(), b.loops.len(), "face {i}");
    }
    for (i, (a, b)) in before.edges.iter().zip(&after.edges).enumerate() {
        assert_eq!(uses(a), uses(b), "edge {i}");
    }
    let (body, _) = Body::from_record(after, false).unwrap();
    // every check but the kinks a CV edit makes: a circle's net is G1 at its joints only while
    // the points around each joint stay in line, which moving one row's points breaks at the
    // next joints (the same edit of a one-face sheet kinks it the same way)
    let report = body.check(CheckLevel::Geometry).unwrap();
    let kinks = |l: &str| l.contains("Bad Surf Discontinuity") || l.contains("Bad CurveKink");
    assert!(
        report.text.lines().all(|l| l.trim().is_empty() || kinks(l)),
        "{}",
        report.text
    );
    assert!(body.is_manifold_solid().unwrap());
    body
}

#[test]
fn a_cylinder_side_cv_moved_in_its_caps_plane_takes_the_cap_along() {
    let cylinder = make_cylinder([0.0, 0.0, 0.0], 2.0, 5.0).unwrap();
    let before = record(&cylinder);
    let f = side(&before);
    let net = face_net(&before, f).unwrap();
    assert_eq!(net.rows * net.cols, net.points.len());
    // the top row's joint at +y pulled out by 1, in the top cap's plane z = 5
    let moves = joint_moves(&net, 5.0, [0.0, 1.0, 0.0]);
    let after = move_face_cvs(&before, f, &moves).unwrap();
    let body = holds_its_record(&before, &after, f);
    // the moved CVs are the edited face's
    let moved = face_net(&after, f).unwrap();
    for &((i, j), to) in &moves {
        assert_eq!(moved.points[i * moved.cols + j], to);
    }
    // the side bulges out at the top and the cap grows with it: more material
    let v0 = std::f64::consts::PI * 4.0 * 5.0;
    let v = body.volume(1e-6).unwrap();
    assert!(v > v0 + 1e-3, "{v} {v0}");
}

#[test]
fn a_cylinder_side_cv_moved_out_of_its_caps_plane_is_refused_in_words() {
    let cylinder = make_cylinder([0.0, 0.0, 0.0], 2.0, 5.0).unwrap();
    let before = record(&cylinder);
    let f = side(&before);
    let net = face_net(&before, f).unwrap();
    let refused = move_face_cvs(&before, f, &joint_moves(&net, 5.0, [0.0, 0.0, 1.0]));
    assert!(
        matches!(&refused, Err(Error::Unsupported(why)) if why.contains("plane")),
        "{refused:?}"
    );
}

/// A move of the bottom row rebuilds the bottom cap only: the top cap and its edge stay as they
/// were.
#[test]
fn a_bottom_row_move_leaves_the_top_cap_as_it_is() {
    let cylinder = make_cylinder([0.0, 0.0, 0.0], 2.0, 5.0).unwrap();
    let before = record(&cylinder);
    let f = side(&before);
    let net = face_net(&before, f).unwrap();
    let after = move_face_cvs(&before, f, &joint_moves(&net, 0.0, [0.0, 0.5, 0.0])).unwrap();
    holds_its_record(&before, &after, f);
    // the cap on the top edge (z = 5), and that edge
    let top = |r: &Brep| {
        let e = r
            .edges
            .iter()
            .position(|e| {
                r.vertices[e.vertices[0] as usize].point[2] == 5.0 && e.vertices[0] == e.vertices[1]
            })
            .expect("the top circle");
        let cap = r.edges[e]
            .uses
            .iter()
            .map(|u| u.face as usize)
            .find(|&n| n != f)
            .expect("the top cap");
        (e, cap)
    };
    let (e, cap) = top(&before);
    assert_eq!(top(&after), (e, cap));
    assert_eq!(before.edges[e], after.edges[e]);
    assert_eq!(before.faces[cap], after.faces[cap]);
}

#[test]
fn a_cone_side_cv_moved_in_its_caps_plane_takes_the_cap_along() {
    let cone = make_cone([0.0, 0.0, 0.0], 3.0, 1.0, 4.0).unwrap();
    let before = record(&cone);
    let f = side(&before);
    let net = face_net(&before, f).unwrap();
    let after = move_face_cvs(&before, f, &joint_moves(&net, 4.0, [0.0, 0.5, 0.0])).unwrap();
    holds_its_record(&before, &after, f);
}

/// The whole top row moved across in its plane (`move_face_net`): the top circle moves without
/// a kink, the cap with it, and the body passes every check.
#[test]
fn the_top_row_moved_across_takes_its_cap_without_a_kink() {
    let cylinder = make_cylinder([0.0, 0.0, 0.0], 2.0, 5.0).unwrap();
    let before = record(&cylinder);
    let f = side(&before);
    let net = face_net(&before, f).unwrap();
    let at = |n: usize| net.points[n][2];
    // the top row: its side's index, along u or along v
    let top = if (0..net.cols).all(|j| at((net.rows - 1) * net.cols + j) == 5.0) {
        Picks::Rows(vec![net.rows - 1])
    } else if (0..net.rows).all(|i| at(i * net.cols + net.cols - 1) == 5.0) {
        Picks::Columns(vec![net.cols - 1])
    } else {
        panic!("no top row in {net:?}")
    };
    let after = move_face_net(&before, f, &top, [0.5, 0.0, 0.0], None).unwrap();
    let body = holds_its_record(&before, &after, f);
    let report = body.check(CheckLevel::Geometry).unwrap();
    assert!(report.is_valid(), "{}", report.text);
}
