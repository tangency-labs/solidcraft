//! A control-net edit of one face of a multi-face body (u81): a capped cylinder's or cone's side
//! face with a CV of its top row moved, the cap that shares that row's edge following, the body
//! still closed, and its faces and edges still where they were in the record. A box's top with
//! a corner raised, its vertical edge following. Two lofted panels sewn along an edge: the
//! points of the seam tied across it, moving together, and a seam whose rows have other knots
//! refused by name.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use solidcore_ops::stitch::{open_edges, sew};
use solidcore_ops::surface_cvs::{
    face_net, falloff_weight, move_face_cvs, move_face_net, tied_points, FacePoint, Falloff, Net,
    Picks, SeamMismatch, Shape,
};
use solidcore_ops::sweep::{loft, Curve as Curve3};
use solidcore_ops::{
    blend_ordered, make_box, make_cone, make_cylinder, Blend, BlendOptions, Body, CheckLevel,
    Corners, Error,
};
use solidcore_record::brep::{Brep, Curve, Edge, Loop, Surface};

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

/// Every line edge of `r` runs from its start vertex to its end vertex: no gap at a corner.
fn lines_reach_their_vertices(r: &Brep) {
    for (i, e) in r.edges.iter().enumerate() {
        if let Curve::Line { start, end } = e.curve {
            let [a, b] = e.vertices.map(|v| r.vertices[v as usize].point);
            let at = |t: f64| -> [f64; 3] {
                std::array::from_fn(|k| start[k] + t * (end[k] - start[k]))
            };
            let [t0, t1] = e.interval;
            for (p, v) in [(at(t0), a), (at(t1), b)] {
                for k in 0..3 {
                    assert!((p[k] - v[k]).abs() <= r.tolerance, "edge {i}: {p:?} {v:?}");
                }
            }
        }
    }
}

/// The face of `r` whose edges all have both ends at height `z`.
fn face_at_height(r: &Brep, z: f64) -> usize {
    r.faces
        .iter()
        .position(|f| {
            let Some(Loop::Edges { edges }) = f.loops.first() else {
                return false;
            };
            edges.iter().all(|u| {
                r.edges[u.edge as usize]
                    .vertices
                    .iter()
                    .all(|&v| r.vertices[v as usize].point[2] == z)
            })
        })
        .expect("a face at that height")
}

/// The `(row, column)` of the point of `net` at `p`.
fn index_of(net: &Net, p: [f64; 3]) -> (usize, usize) {
    let n = net
        .points
        .iter()
        .position(|&q| q == p)
        .unwrap_or_else(|| panic!("no CV at {p:?} in {net:?}"));
    (n / net.cols, n % net.cols)
}

/// A box's top face with a corner CV raised along the box's vertical edge there (the corner gap
/// #16577 left): the two top edges at the corner tilt within their walls, and the vertical edge
/// between those walls runs on to the raised corner. The body stays a valid closed solid, its
/// volume the box's and the bilinear top's: the corner's rise times a quarter of the top's area.
#[test]
fn a_box_corner_raised_takes_its_vertical_edge_along() {
    let b = make_box([0.0, 0.0, 0.0], [2.0, 3.0, 4.0]).unwrap();
    let before = record(&b);
    let top = face_at_height(&before, 4.0);
    let net = face_net(&before, top).unwrap();
    let corner = index_of(&net, [0.0, 0.0, 4.0]);
    let after = move_face_cvs(&before, top, &[(corner, [0.0, 0.0, 5.0])]).unwrap();
    lines_reach_their_vertices(&after);
    let body = holds_its_record(&before, &after, top);
    let report = body.check(CheckLevel::Geometry).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    let v = body.volume(1e-9).unwrap();
    assert!((v - (24.0 + 1.5)).abs() < 1e-6, "{v}");
}

/// Whether `p` is within `tol` of `q` in each coordinate.
fn near(p: [f64; 3], q: [f64; 3], tol: f64) -> bool {
    (0..3).all(|k| (p[k] - q[k]).abs() <= tol)
}

/// A box with its top front edge rounded: the top's corner at the round raised along the box's
/// left wall. The round's end arc on that wall ends at the moved vertex, but it is not a line
/// between planar faces (it bounds the round's cylinder), so it cannot run on to the vertex: the
/// edit is refused naming that arc.
#[test]
fn a_corner_raised_where_a_round_ends_is_refused_naming_the_round_s_arc() {
    let b = make_box([0.0, 0.0, 0.0], [2.0, 3.0, 4.0]).unwrap();
    let r = 0.5;
    // the top front edge: the line at y = 0, z = 4, by its record index
    let (before, order) = b.to_record().unwrap();
    let front = before
        .edges
        .iter()
        .position(|e| {
            e.vertices.iter().all(|&v| {
                let p = before.vertices[v as usize].point;
                p[1] == 0.0 && p[2] == 4.0
            })
        })
        .expect("the top front edge");
    blend_ordered(
        &b,
        &order,
        Blend::Round,
        &[front],
        &[r],
        &[],
        Corners::Filled,
        BlendOptions::default(),
    )
    .unwrap();
    let before = record(&b);
    let tol = before.tolerance;
    // the top: the planar face whose vertices are all at z = 4
    let top = before
        .faces
        .iter()
        .enumerate()
        .position(|(n, f)| {
            matches!(
                f.surface,
                Surface::Plane { .. } | Surface::PlaneThrough { .. }
            ) && before
                .edges
                .iter()
                .filter(|e| e.uses.iter().any(|u| u.face as usize == n))
                .all(|e| {
                    e.vertices
                        .iter()
                        .all(|&v| (before.vertices[v as usize].point[2] - 4.0).abs() <= tol)
                })
        })
        .expect("the top");
    // the round's arc on the left wall x = 0: from the top's corner (0, r, 4) down to (0, 0, 4 - r)
    let corner_at = [0.0, r, 4.0];
    let arc = before
        .edges
        .iter()
        .position(|e| {
            !matches!(e.curve, Curve::Line { .. })
                && e.vertices
                    .iter()
                    .any(|&v| near(before.vertices[v as usize].point, corner_at, tol))
        })
        .expect("the round's arc at the corner");
    let net = face_net(&before, top).unwrap();
    let corner = (0..net.points.len())
        .find(|&n| near(net.points[n], corner_at, tol))
        .map(|n| (n / net.cols, n % net.cols))
        .expect("the top's corner point");
    let refused = move_face_cvs(&before, top, &[(corner, [0.0, r, 5.0])]);
    assert!(
        matches!(refused, Err(Error::EdgeLeftBehind { edge }) if edge == arc),
        "{refused:?}, the arc is edge {arc}"
    );
}

/// A cubic along x at height `y`, through four control points (their heights `z`), as a Bezier.
fn cubic(y: f64, z: [f64; 4]) -> Curve3 {
    Curve3::Nurbs {
        degree: 3,
        points: (0..4).map(|i| [i as f64, y, z[i], 1.0]).collect(),
        knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    }
}

/// The seam's cubic, at `y = 1`.
fn seam_curve() -> Curve3 {
    cubic(1.0, [0.0, 0.5, -0.5, 0.0])
}

/// Two panels lofted at quadratic across their three sections (`y` 0 to 1, then 1 to 2), the
/// second starting with `seam` (the first ends with the seam's cubic), sewn along it; with the
/// sewn record, its open edges and the seam edge's record index.
fn sewn_panels(seam: Curve3) -> (Brep, Vec<usize>, usize) {
    let a = loft(
        &[
            vec![cubic(0.0, [0.0; 4])],
            vec![cubic(0.5, [0.2, 0.0, 0.0, 0.2])],
            vec![seam_curve()],
        ],
        2,
        false,
    )
    .unwrap();
    let b = loft(
        &[
            vec![seam],
            vec![cubic(1.5, [0.0, 0.3, 0.3, 0.0])],
            vec![cubic(2.0, [0.0; 4])],
        ],
        2,
        false,
    )
    .unwrap();
    let sewn = sew(&[&a, &b], 1e-6).unwrap();
    let r = record(&sewn.body);
    assert_eq!(r.faces.len(), 2);
    let shared: Vec<usize> = (0..r.edges.len())
        .filter(|&e| r.edges[e].uses.len() == 2)
        .collect();
    let [seam] = shared[..] else {
        panic!("one shared edge: {shared:?}")
    };
    (r, sewn.open_edges, seam)
}

/// The net point of `net` at `(row, column)`.
fn at(net: &Net, (i, j): (usize, usize)) -> [f64; 3] {
    net.points[i * net.cols + j]
}

/// The tied-points call lists the seam once, every point of its row paired with the point of
/// the other face's row at the same place, each point once.
#[test]
fn the_tied_points_of_two_sewn_panels_pair_their_seam_rows() {
    let (r, _, seam) = sewn_panels(seam_curve());
    let tied = tied_points(&r).unwrap();
    assert_eq!(tied.len(), 1, "{tied:?}");
    let t = &tied[0];
    assert_eq!(t.edge, seam);
    let pairs = t.ties.as_ref().unwrap();
    assert_eq!(pairs.len(), 4);
    let nets = t.faces.map(|f| face_net(&r, f).unwrap());
    let mut seen: Vec<FacePoint> = Vec::new();
    for [p, q] in pairs {
        assert_eq!([p.face, q.face], t.faces);
        let (a, b) = (
            at(&nets[0], (p.row, p.column)),
            at(&nets[1], (q.row, q.column)),
        );
        for k in 0..3 {
            assert!((a[k] - b[k]).abs() <= r.tolerance, "{a:?} {b:?}");
        }
        assert_eq!(a[1], 1.0, "on the seam: {a:?}");
        assert!(!seen.contains(p) && !seen.contains(q));
        seen.extend([*p, *q]);
    }
}

/// A seam point of one panel raised: the other panel's twin rises with it, the seam stays shut
/// (the same open edges, the seam still one edge of both faces) and the body is valid.
#[test]
fn a_seam_point_raised_moves_both_panels() {
    let (before, open, seam) = sewn_panels(seam_curve());
    let tied = tied_points(&before).unwrap();
    let [p, q] = tied[0].ties.as_ref().unwrap()[1];
    let net = face_net(&before, p.face).unwrap();
    let mut to = at(&net, (p.row, p.column));
    to[2] += 0.75;
    let after = move_face_cvs(&before, p.face, &[((p.row, p.column), to)]).unwrap();
    assert_eq!(after.faces.len(), 2);
    assert_eq!(after.edges.len(), before.edges.len());
    for (a, b) in before.edges.iter().zip(&after.edges) {
        assert_eq!(uses(a), uses(b));
    }
    assert_eq!(
        at(&face_net(&after, p.face).unwrap(), (p.row, p.column)),
        to
    );
    let twin = at(&face_net(&after, q.face).unwrap(), (q.row, q.column));
    for k in 0..3 {
        assert!(
            (twin[k] - to[k]).abs() <= before.tolerance,
            "{twin:?} {to:?}"
        );
    }
    let (body, _) = Body::from_record(&after, false).unwrap();
    assert_eq!(open_edges(&body).unwrap(), open);
    assert!(!open.contains(&seam));
    let report = body.check(CheckLevel::Geometry).unwrap();
    assert!(report.is_valid(), "{}", report.text);
    // the same edit by a soft move of that point
    let soft = move_face_net(
        &before,
        p.face,
        &Picks::Cvs(vec![(p.row, p.column)]),
        [0.0, 0.0, 0.75],
        None,
    )
    .unwrap();
    let twin = at(&face_net(&soft, q.face).unwrap(), (q.row, q.column));
    for k in 0..3 {
        assert!(
            (twin[k] - to[k]).abs() <= before.tolerance,
            "{twin:?} {to:?}"
        );
    }
}

/// A point inside one panel (its middle section's, off the seam): only that face changes.
#[test]
fn an_inner_point_of_one_panel_moves_that_panel_alone() {
    let (before, _, seam) = sewn_panels(seam_curve());
    let f = before.edges[seam].uses[0].face as usize;
    let other = 1 - f;
    let net = face_net(&before, f).unwrap();
    let inner = (1, 1);
    assert!(net.rows > 2 && net.cols > 2, "{net:?}");
    let mut to = at(&net, inner);
    to[2] += 0.5;
    let after = move_face_cvs(&before, f, &[(inner, to)]).unwrap();
    assert_eq!(after.faces[other], before.faces[other]);
    assert_eq!(after.edges, before.edges);
    assert_ne!(after.faces[f], before.faces[f]);
}

/// The second panel starts on the seam's cubic with a knot inserted at its middle: the same
/// curve, so the panels sew, but their rows along the seam have other knots. Raising a seam point
/// is refused naming the seam.
#[test]
fn panels_whose_seam_rows_have_other_knots_are_refused_naming_the_seam() {
    // the seam's cubic [0, 0.5, -0.5, 0] at x = 0..3 with u = 1/2 inserted: each new point halfway
    // between two old ones
    let halved = Curve3::Nurbs {
        degree: 3,
        points: vec![
            [0.0, 1.0, 0.0, 1.0],
            [0.5, 1.0, 0.25, 1.0],
            [1.5, 1.0, 0.0, 1.0],
            [2.5, 1.0, -0.25, 1.0],
            [3.0, 1.0, 0.0, 1.0],
        ],
        knots: vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
    };
    let (before, open, seam) = sewn_panels(halved);
    assert!(!open.contains(&seam), "the panels sew along the seam");
    let tied = tied_points(&before).unwrap();
    assert_eq!(tied.len(), 1);
    assert!(
        matches!(tied[0].ties, Err(SeamMismatch::KnotCount { .. })),
        "{tied:?}"
    );
    let f = tied[0].faces[0];
    let net = face_net(&before, f).unwrap();
    let on_seam = (0..net.points.len())
        .find(|&n| net.points[n][1] == 1.0)
        .map(|n| (n / net.cols, n % net.cols))
        .expect("a seam point");
    let mut to = at(&net, on_seam);
    to[2] += 0.5;
    let refused = move_face_cvs(&before, f, &[(on_seam, to)]);
    assert!(
        matches!(refused, Err(Error::SeamMismatch { edge, .. }) if edge == seam),
        "{refused:?}"
    );
}

/// A soft move of panel `a`'s point next to the seam, its falloff reaching the seam row: the
/// seam row moves by its weight, the other panel's twins move with it so the seam stays shut,
/// and the other panel's points off the seam stay where they were (the falloff does not cross
/// the seam yet, so the seam is not kept tangent).
#[test]
fn a_falloff_edit_by_a_seam_keeps_it_shut_and_stops_there() {
    let (before, open, seam) = sewn_panels(seam_curve());
    let tied = tied_points(&before).unwrap();
    let pairs = tied[0].ties.as_ref().unwrap();
    let [fa, fb] = tied[0].faces;
    let (net_a, net_b) = (
        face_net(&before, fa).unwrap(),
        face_net(&before, fb).unwrap(),
    );
    // a's seam row: one row index or one column index; the pick its inner neighbour of the
    // second seam point, one index step from it
    let [p, _] = pairs[1];
    let along_rows = pairs.iter().all(|[a, _]| a.row == p.row);
    let inward = |i: usize| if i == 0 { 1 } else { i - 1 };
    let pick = if along_rows {
        (inward(p.row), p.column)
    } else {
        (p.row, inward(p.column))
    };
    let falloff = Some(Falloff {
        radius: 1,
        shape: Shape::Linear,
    });
    let delta = [0.0, 0.0, 0.5];
    let after = move_face_net(&before, fa, &Picks::Cvs(vec![pick]), delta, falloff).unwrap();
    let (moved_a, moved_b) = (face_net(&after, fa).unwrap(), face_net(&after, fb).unwrap());
    // the seam point one step from the pick moved by that step's weight, and its twin with it
    let w = falloff_weight(falloff, 1);
    assert!(w > 0.0 && w < 1.0, "{w}");
    let was = at(&net_a, (p.row, p.column));
    let want: [f64; 3] = std::array::from_fn(|k| was[k] + w * delta[k]);
    assert!(near(
        at(&moved_a, (p.row, p.column)),
        want,
        before.tolerance
    ));
    // every seam point and its twin are still one
    for [x, y] in pairs {
        let (q, t) = (
            at(&moved_a, (x.row, x.column)),
            at(&moved_b, (y.row, y.column)),
        );
        assert!(near(q, t, before.tolerance), "{q:?} {t:?}");
    }
    let (body, _) = Body::from_record(&after, false).unwrap();
    assert_eq!(open_edges(&body).unwrap(), open);
    assert!(!open.contains(&seam));
    // b's points off the seam (its inner row and its far row) did not move
    let on_seam: Vec<(usize, usize)> = pairs.iter().map(|[_, y]| (y.row, y.column)).collect();
    for i in 0..net_b.rows {
        for j in 0..net_b.cols {
            if !on_seam.contains(&(i, j)) {
                assert_eq!(at(&moved_b, (i, j)), at(&net_b, (i, j)), "({i}, {j})");
            }
        }
    }
}
