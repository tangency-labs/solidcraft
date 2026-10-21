//! Modifiers' histories (fillets, chamfers, shell, draft, offset, split, delete, surfaces): every
//! face and edge origin checked against the geometry of the operands (in tests only: the history
//! itself is recorded, never sampled), and the exact origins asserted where the outcome is known.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::body::Body;
use crate::boolean_record::tests::{self, Reach, changes, to_edge, to_face};
use crate::history::BodyHistory;
use crate::history::{Change, EdgeOrigin, FaceOrigin, History};
use crate::{BoolOp, boolean, box_solid, cylinder, extrude};

const TOL: f64 = 0.05;

const fn face(face: usize) -> FaceOrigin {
    FaceOrigin::Face { operand: 0, face }
}

const fn edge(edge: usize) -> FaceOrigin {
    FaceOrigin::Edge { operand: 0, edge }
}

const fn reach(made: f64) -> Reach {
    Reach { made, moved: 0.0 }
}

/// [`checked`] at this module's mesh tolerance.
fn checked<'a>(r: &'a Body, operands: &[&Body], at: Reach, generated: &dyn Fn(usize) -> bool) -> &'a History {
    tests::checked(r, operands, TOL, at, generated)
}

/// [`checked`] for a modifier of one operand whose every face comes from something of it.
fn verified<'a>(r: &'a Body, operand: &Body, at: Reach) -> &'a History {
    checked(r, &[operand], at, &|_| false)
}

/// The faces whose origins are exactly `origins`.
fn with(h: &History, origins: &[FaceOrigin]) -> Vec<usize> {
    (0..h.faces.len()).filter(|i| h.faces[*i].origins == origins).collect()
}

/// The origins of every face made in place of edges (blends).
fn blends(h: &History) -> Vec<Vec<FaceOrigin>> {
    h.faces.iter().filter(|f| f.origins.iter().any(|o| matches!(o, FaceOrigin::Edge { .. }))).map(|f| f.origins.clone()).collect()
}

/// The index of the edge of `b` nearest `p`.
fn edge_at(b: &Body, p: Vec3) -> usize {
    b.nearest_edge(p, TOL).unwrap().unwrap().0
}

/// The index of the face of `b` holding `p`.
fn face_at(b: &Body, p: Vec3) -> usize {
    let m = b.tessellate(TOL).unwrap();
    (0..b.face_count()).min_by(|x, y| to_face(&m, *x, p).total_cmp(&to_face(&m, *y, p))).unwrap()
}

fn cube() -> Body {
    box_solid(Vec3::ZERO, Vec3::new(40.0, 30.0, 20.0)).unwrap()
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    Loop2::polygon(&[Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)])
}

/// An L-shaped prism (not convex), 20 high.
fn ell() -> Body {
    let outer = Loop2::polygon(&[
        Vec2::ZERO,
        Vec2::new(40.0, 0.0),
        Vec2::new(40.0, 15.0),
        Vec2::new(15.0, 15.0),
        Vec2::new(15.0, 30.0),
        Vec2::new(0.0, 30.0),
    ]);
    extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], 0.0, 20.0).unwrap().pop().unwrap()
}

#[test]
fn fillet_one_edge() {
    let b = cube();
    let at = Vec3::new(20.0, 30.0, 20.0);
    let r = crate::fillet(&b, &[at], 5.0).unwrap();
    let h = verified(&r, &b, reach(5.0));
    assert_eq!(with(h, &[edge(edge_at(&b, at))]).len(), 1);
    // Two faces along the edge and the two at its ends are trimmed, two are untouched.
    assert_eq!(changes(h), [2, 4, 0, 0, 1]);
}

#[test]
fn fillet_an_edge_a_cut_split() {
    let b = cube();
    // A slot through the back leaves two pieces of the top back edge.
    let notch = box_solid(Vec3::new(15.0, 25.0, -5.0), Vec3::new(25.0, 35.0, 25.0)).unwrap();
    let cut = boolean(&b, &notch, BoolOp::Cut).unwrap().unwrap();
    let at = Vec3::new(5.0, 30.0, 20.0);
    let piece = edge_at(&cut, at);
    assert_eq!(cut.history().unwrap().edges[piece].origins, [EdgeOrigin::Edge { operand: 0, edge: edge_at(&b, at) }]);
    let r = crate::fillet(&cut, &[at], 3.0).unwrap();
    let h = verified(&r, &cut, reach(3.0));
    assert_eq!(blends(h), [vec![edge(piece)]]);
    // Through the cut: the blend stands in place of the box's edge, which the cut split.
    let mut through = h.clone();
    through.substitute(0, cut.history().unwrap()).unwrap();
    through.check(&r, &[&b, &notch]).unwrap();
    assert_eq!(blends(&through), [vec![edge(edge_at(&b, at))]]);
}

#[test]
fn fillet_three_edges_at_a_corner() {
    let b = cube();
    let pts = [Vec3::new(20.0, 30.0, 20.0), Vec3::new(40.0, 15.0, 20.0), Vec3::new(40.0, 30.0, 10.0)];
    let r = crate::fillet_radii(&b, &pts.map(|p| (p, 4.0))).unwrap();
    let h = verified(&r, &b, reach(4.0 * 1.5));
    let edges: Vec<usize> = pts.iter().map(|p| edge_at(&b, *p)).collect();
    for e in &edges {
        assert_eq!(with(h, &[edge(*e)]).len(), 1, "{h:?}");
    }
    // The corner patch stands in place of the three edges meeting there.
    let mut corner: Vec<FaceOrigin> = edges.iter().map(|e| edge(*e)).collect();
    corner.sort();
    assert_eq!(with(h, &corner).len(), 1, "{:?}", blends(h));
}

#[test]
fn fillet_a_whole_loop() {
    let b = cube();
    let pts = [Vec3::new(20.0, 0.0, 20.0), Vec3::new(40.0, 15.0, 20.0), Vec3::new(20.0, 30.0, 20.0), Vec3::new(0.0, 15.0, 20.0)];
    let r = crate::fillet(&b, &pts, 3.0).unwrap();
    let h = verified(&r, &b, reach(3.0 * 1.5));
    let mut made: Vec<FaceOrigin> = blends(h).into_iter().flatten().collect();
    made.sort();
    let mut want: Vec<FaceOrigin> = pts.iter().map(|p| edge(edge_at(&b, *p))).collect();
    want.sort();
    assert_eq!(made, want);
}

#[test]
fn fillet_every_edge_of_a_box() {
    let b = cube();
    let pts: Vec<Vec3> = b.edges(TOL).unwrap().iter().map(|e| e.mid).collect();
    let r = crate::fillet(&b, &pts, 2.0).unwrap();
    let h = verified(&r, &b, reach(2.0 * 2.0));
    // Six faces shrunk, twelve cylinders, eight corners of three edges each.
    assert_eq!(changes(h), [0, 6, 0, 0, 20]);
    assert_eq!(blends(h).iter().filter(|o| o.len() == 1).count(), 12);
    assert_eq!(blends(h).iter().filter(|o| o.len() == 3).count(), 8);
}

#[test]
fn fillet_a_rolling_ball_chain() {
    let main = cylinder(Vec3::new(-40.0, 0.0, 0.0), Vec3::X, 15.0, 80.0).unwrap();
    let branch = cylinder(Vec3::ZERO, Vec3::Z, 10.0, 35.0).unwrap();
    let tee = boolean(&main, &branch, BoolOp::Union).unwrap().unwrap();
    let at = Vec3::new(0.0, 10.0, 125f64.sqrt());
    let r = crate::fillet(&tee, &[at], 3.0).unwrap();
    let h = verified(&r, &tee, reach(3.0 * 2.0));
    assert!(!blends(h).is_empty());
    assert!(blends(h).iter().all(|o| o.len() == 1));
}

#[test]
fn fillet_variable_and_chord() {
    let b = cube();
    let at = Vec3::new(20.0, 30.0, 20.0);
    let r = crate::fillet_variable(&b, at, Vec3::new(0.0, 30.0, 20.0), 2.0, 5.0).unwrap();
    let h = verified(&r, &b, reach(5.0));
    assert_eq!(blends(h), [vec![edge(edge_at(&b, at))]]);
    let r = crate::fillet_chord(&b, &[at, Vec3::new(40.0, 15.0, 0.0)], 4.0).unwrap();
    let h = verified(&r, &b, reach(4.0));
    assert_eq!(blends(h).len(), 2);
}

#[test]
fn chamfer_cuts_one_edge() {
    let b = cube();
    let at = Vec3::new(20.0, 30.0, 20.0);
    let r = crate::chamfer(&b, &[at], 4.0).unwrap();
    let h = verified(&r, &b, reach(4.0));
    let chamfer = with(h, &[edge(edge_at(&b, at))]);
    assert_eq!(chamfer.len(), 1, "{h:?}");
    assert_eq!(h.faces[chamfer[0]].change, Change::Generated);
    assert_eq!(r.face_count(), 7);
}

#[test]
fn chamfer_cutters_mitre_at_a_corner() {
    let b = cube();
    let pts = [Vec3::new(20.0, 30.0, 20.0), Vec3::new(40.0, 15.0, 20.0)];
    let r = crate::chamfer(&b, &pts, 3.0).unwrap();
    let h = verified(&r, &b, reach(3.0 * 1.5));
    for p in pts {
        assert_eq!(with(h, &[edge(edge_at(&b, p))]).len(), 1, "{h:?}");
    }
    // The two chamfers meet along the mitre.
    let (a, c) = (edge(edge_at(&b, pts[0])), edge(edge_at(&b, pts[1])));
    assert!(h.edges.iter().any(|e| e.origins == [EdgeOrigin::meet(a, c)]));
}

#[test]
fn chamfer_with_two_sides() {
    let b = cube();
    let at = Vec3::new(20.0, 30.0, 20.0);
    let r = crate::chamfer_sides(&b, &[at], 2.0, crate::ChamferSide::Distance(4.0), false).unwrap();
    let h = verified(&r, &b, reach(4.0));
    assert_eq!(blends(h), [vec![edge(edge_at(&b, at))]]);
}

#[test]
fn delete_a_fillet() {
    let b = cube();
    let r = crate::fillet(&b, &[Vec3::new(20.0, 30.0, 20.0)], 5.0).unwrap();
    let round = with(r.history().unwrap(), &[edge(edge_at(&b, Vec3::new(20.0, 30.0, 20.0)))]);
    let back = crate::delete_faces(&r, &round).unwrap();
    let h = verified(&back, &r, Reach { made: 0.0, moved: 6.0 });
    // Every face is one of the filleted body's, trimmed back out to the sharp edge.
    assert_eq!(h.faces.len(), 6);
    assert!(h.faces.iter().all(|f| f.origins.len() == 1));
    // The faces either side of the round meet again where they met nowhere in its input.
    let (top, back_face) = (face_at(&r, Vec3::new(20.0, 10.0, 20.0)), face_at(&r, Vec3::new(20.0, 30.0, 5.0)));
    let (x, y) = (face(top), face(back_face));
    assert!(h.edges.iter().any(|e| e.origins == [EdgeOrigin::meet(x, y)]));
}

#[test]
fn shell_lines_the_walls_with_offsets() {
    let b = cube();
    let top = Vec3::new(20.0, 15.0, 20.0);
    let r = crate::shell(&b, &[top], 2.0).unwrap();
    let h = verified(&r, &b, reach(2.0));
    // Five inner walls, each an offset of the face it lines; the top is left as a rim.
    let top = face_at(&b, top);
    for f in (0..6).filter(|f| *f != top) {
        let walls = with(h, &[FaceOrigin::Offset { operand: 0, face: f }]);
        assert_eq!(walls.len(), 1, "{h:?}");
        assert_eq!(h.faces[walls[0]].change, Change::Generated);
    }
    assert!(with(h, &[FaceOrigin::Offset { operand: 0, face: top }]).is_empty());
    assert_eq!(changes(h), [0, 6, 0, 0, 5]);
}

#[test]
fn shell_of_a_body_that_is_not_convex() {
    let b = ell();
    let r = crate::shell(&b, &[Vec3::new(5.0, 5.0, 20.0)], 2.0).unwrap();
    // An inner wall runs past its face by the thickness at a concave corner.
    let h = verified(&r, &b, reach(2.0 * 1.5));
    let offsets = h.faces.iter().filter(|f| matches!(f.origins.as_slice(), [FaceOrigin::Offset { .. }])).count();
    assert_eq!(offsets, 7, "{h:?}");
}

/// A shell of a block with a tab joined on a side: the cavity Cut sews the walls in pieces
/// across its slabs and joins them on their common supports, the history carried through.
#[test]
fn shell_of_a_stepped_body_joins_its_walls() {
    let stock = box_solid(Vec3::new(-20.0, -15.0, 0.0), Vec3::new(20.0, 15.0, 24.0)).unwrap();
    let tab = box_solid(Vec3::new(-12.0, -25.0, 8.0), Vec3::new(-4.0, -15.0, 16.0)).unwrap();
    let b = boolean(&stock, &tab, BoolOp::Union).unwrap().unwrap();
    let r = crate::shell(&b, &[Vec3::new(0.0, 0.0, 24.0)], 1.0).unwrap();
    let h = verified(&r, &b, reach(1.0 * 1.5));
    // Every face of the body but the open top is lined by one inner wall.
    let offsets = h.faces.iter().filter(|f| matches!(f.origins.as_slice(), [FaceOrigin::Offset { .. }])).count();
    assert_eq!(offsets, b.face_count() - 1, "{h:?}");
}

/// A shell of a painted block: its faces are used as they are (unhealed), keeping the paint's
/// face order, with the history of that copy. Paint is an authored boundary, so the cavity Cut
/// keeps the side faces it sews across the rim's level in pieces (Split).
#[test]
fn shell_of_a_painted_body() {
    let b = painted_cube();
    let r = crate::shell(&b, &[Vec3::new(20.0, 15.0, 20.0)], 2.0).unwrap();
    let h = verified(&r, &b, reach(2.0));
    assert_eq!(changes(h), [0, 2, 8, 0, 5], "{h:?}");
}

#[test]
fn draft_tilts_faces() {
    let b = cube();
    let side = Vec3::new(40.0, 15.0, 10.0);
    let r = crate::draft(&b, &[side], &Plane::XY, Vec3::Z, 5f64.to_radians()).unwrap();
    let h = verified(&r, &b, Reach { made: 0.0, moved: 2.0 });
    assert_eq!(changes(h), [0, 6, 0, 0, 0]);
    assert_eq!(with(h, &[face(face_at(&b, side))]).len(), 1);
    // Walls of a body that is not convex.
    let l = ell();
    let wall = Vec3::new(40.0, 7.0, 10.0);
    let r = crate::draft(&l, &[wall], &Plane::XY, Vec3::Z, 5f64.to_radians()).unwrap();
    let h = verified(&r, &l, Reach { made: 0.0, moved: 2.0 });
    assert_eq!(with(h, &[face(face_at(&l, wall))]).len(), 1);
}

#[test]
fn offset_moves_a_face() {
    let b = ell();
    let top = Vec3::new(5.0, 5.0, 20.0);
    let r = crate::offset_faces(&b, &[top], 3.0).unwrap();
    let h = verified(&r, &b, Reach { made: 0.0, moved: 3.0 * 1.1 });
    assert_eq!(h.faces.len(), b.face_count());
    assert_eq!(with(h, &[face(face_at(&b, top))]).len(), 1);
    assert!(h.edges.iter().all(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Edge { .. }])));
}

#[test]
fn split_a_face() {
    let b = cube();
    let top = face_at(&b, Vec3::new(20.0, 15.0, 20.0));
    let plane = Plane::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
    let (r, map) = crate::split_faces_with_map(&b, &[top], &crate::SplitTool::Plane(plane)).unwrap();
    let h = verified(&r, &b, reach(0.0));
    // Two pieces of the top; the faces whose edges the split cut are whole, as is the rest.
    assert_eq!(with(h, &[face(top)]), map.iter().enumerate().filter(|(_, f)| **f == top).map(|(i, _)| i).collect::<Vec<_>>());
    assert_eq!(changes(h), [5, 0, 2, 0, 0]);
    for (i, f) in map.iter().enumerate() {
        assert_eq!(h.faces[i].origins, [face(*f)]);
    }
    // The split line runs between pieces of one face: it comes from nothing.
    assert_eq!(h.edges.iter().filter(|e| e.origins.is_empty()).count(), 1);
}

#[test]
fn split_a_body() {
    let b = cube();
    let plane = Plane::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
    let parts = crate::split_body(&b, &crate::SplitTool::Plane(plane)).unwrap();
    assert_eq!(parts.len(), 2);
    for p in &parts {
        let h = checked(p, &[&b], reach(0.0), &|i| p.history().is_some_and(|h| h.faces[i].change == Change::Generated));
        // One section face, new; the rest the box's faces or pieces of them.
        assert_eq!(h.faces.iter().filter(|f| f.origins.is_empty()).count(), 1);
    }
}

#[test]
fn surfaces_keep_their_faces() {
    let b = cube();
    let top = Vec3::new(20.0, 15.0, 20.0);
    let copy = crate::copy_faces(&b, &[top]).unwrap();
    let h = verified(&copy, &b, reach(0.0));
    assert_eq!(h.faces[0].origins, [face(face_at(&b, top))]);
    assert_eq!(h.faces[0].change, Change::Kept);
    // Trimmed: the copy, cut back (its other piece is gone).
    let plane = Plane::new(Vec3::new(10.0, 0.0, 0.0), Vec3::Y, Vec3::Z).unwrap();
    let t = crate::trim(&copy, &crate::SplitTool::Plane(plane), Vec3::new(5.0, 15.0, 20.0)).unwrap();
    let h = verified(&t, &copy, reach(0.0));
    assert_eq!(h.faces[0].origins, [face(0)]);
    assert_eq!(h.faces[0].change, Change::Modified);
    // Extended: the face and its edges moved out.
    let x = crate::extend(&copy, &[Vec3::new(40.0, 15.0, 20.0)], 5.0).unwrap();
    let h = verified(&x, &copy, Reach { made: 0.0, moved: 5.0 * 1.1 });
    assert_eq!(h.faces[0].origins, [face(0)]);
    assert!(h.edges.iter().all(|e| e.origins.len() == 1));
    // Patched: a new face on the body's own edges.
    let open = crate::copy_faces(
        &b,
        &[
            Vec3::new(20.0, 15.0, 0.0),
            Vec3::new(20.0, 0.0, 10.0),
            Vec3::new(20.0, 30.0, 10.0),
            Vec3::new(0.0, 15.0, 10.0),
            Vec3::new(40.0, 15.0, 10.0),
        ],
    )
    .unwrap();
    let rim = [Vec3::new(20.0, 0.0, 20.0), Vec3::new(40.0, 15.0, 20.0), Vec3::new(20.0, 30.0, 20.0), Vec3::new(0.0, 15.0, 20.0)];
    let lid = crate::patch_edges(&open, &rim).unwrap();
    let h = checked(&lid, &[&open], reach(0.0), &|_| true);
    assert_eq!(h.faces[0].change, Change::Generated);
    assert!(h.edges.iter().all(|e| matches!(e.origins.as_slice(), [EdgeOrigin::Edge { operand: 0, .. }])));
    // Stitched: each face the one it was sewn from, the seam the edges sewn together.
    let closed = crate::stitch(&[&open, &lid], 0.0).unwrap();
    let h = checked(&closed, &[&open, &lid], reach(0.0), &|_| false);
    assert!(h.faces.iter().all(|f| f.change == Change::Kept && f.origins.len() == 1));
    assert_eq!(h.edges.iter().filter(|e| e.origins.len() == 2).count(), 4);
    let patch = crate::patch_region(&Plane::XY, &Region2 { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![] }).unwrap();
    assert_eq!(patch.history().unwrap().operands, 0);
}

#[test]
fn thicken_offsets_the_surface() {
    let s = crate::patch_region(&Plane::XY, &Region2 { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![] }).unwrap();
    let t = crate::thicken(&s, 2.0, false).unwrap();
    let h = verified(&t, &s, reach(2.0));
    assert_eq!(with(h, &[face(0)]).len(), 1);
    assert_eq!(with(h, &[FaceOrigin::Offset { operand: 0, face: 0 }]).len(), 1);
    assert_eq!(blends(h).len(), 4);
}

#[test]
fn mirrored_copy_keeps_every_face_and_edge() {
    let b = ell();
    let m = [[-1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [100.0, 0.0, 0.0, 1.0]];
    let r = crate::transform_matrix(&b, m).unwrap();
    let h = r.history().unwrap();
    h.check(&r, &[&b]).unwrap();
    // Each edge is the mirror image of the one it says it is.
    let (rm, bm) = (r.tessellate(TOL).unwrap(), b.tessellate(TOL).unwrap());
    for (j, e) in h.edges.iter().enumerate() {
        let [EdgeOrigin::Edge { edge, .. }] = e.origins[..] else { panic!("edge {j}: {e:?}") };
        for p in &rm.edges[j] {
            assert!(to_edge(&bm, edge, Vec3::new(100.0 - p.x, p.y, p.z)) < 1e-6, "edge {j} is not the mirror of edge {edge}");
        }
    }
}

/// The block with a face painted: an authored boundary, so the cuts leave the faces they split
/// across their slabs in pieces (none of them joined on their common supports).
fn painted_cube() -> Body {
    cube().with_paint(Some(crate::Paint { opacity: 1., faces: vec![crate::FacePaint { face: 0, color: [1., 0., 0.], opacity: 1. }] }))
}

/// The block `b` with box cuts of each common shape: a notch through the back, a pocket, a
/// step along one end, a slot across the top, a hole through, a corner bitten out.
fn box_cuts_of(b: &Body) -> Vec<(&'static str, Body, Body)> {
    [
        ("notch", Vec3::new(15.0, 25.0, -5.0), Vec3::new(25.0, 35.0, 25.0)),
        ("pocket", Vec3::new(10.0, 10.0, 10.0), Vec3::new(30.0, 20.0, 25.0)),
        ("step", Vec3::new(30.0, -5.0, 10.0), Vec3::new(45.0, 35.0, 25.0)),
        ("slot", Vec3::new(15.0, -5.0, 10.0), Vec3::new(25.0, 35.0, 25.0)),
        ("hole", Vec3::new(15.0, 10.0, -5.0), Vec3::new(25.0, 20.0, 25.0)),
        ("bite", Vec3::new(30.0, 20.0, 10.0), Vec3::new(45.0, 35.0, 25.0)),
    ]
    .into_iter()
    .map(|(name, lo, hi)| {
        let tool = box_solid(lo, hi).unwrap();
        let cut = boolean(b, &tool, BoolOp::Cut).unwrap().unwrap();
        (name, cut, tool)
    })
    .collect()
}

/// Meetings with a side of several origins (a face the modifier's heal merged).
fn merged_meetings(h: &History) -> usize {
    h.edges.iter().flat_map(|e| &e.origins).filter(|o| matches!(o, EdgeOrigin::Meet { a, b } if a.single().is_none() || b.single().is_none())).count()
}

/// A fillet right after a box cut. The cut joins the walls it sews in pieces across its slabs
/// on their common supports, so the fillet finds whole faces. Where the block is painted the cut
/// keeps those pieces (it has more faces), the fillet's heal merges them, and the edges where
/// the round runs into such a face meet all of it. Composed with the cut, over the block and
/// the tool.
#[test]
fn fillet_after_box_cuts() {
    let at = Vec3::new(5.0, 0.0, 20.0);
    let plain = box_cuts_of(&cube());
    for (b, painted) in [(cube(), false), (painted_cube(), true)] {
        let cuts = if painted { box_cuts_of(&b) } else { plain.clone() };
        for ((name, cut, tool), (_, whole, _)) in cuts.into_iter().zip(&plain) {
            let r = crate::fillet(&cut, &[at], 2.0).unwrap();
            let h = verified(&r, &cut, reach(2.0 * 1.5));
            assert_eq!(blends(h), [vec![edge(edge_at(&cut, at))]], "{name}");
            let pieces = cut.face_count() > whole.face_count();
            assert_eq!(pieces, painted && name != "notch" && name != "hole", "{name} (painted: {painted})");
            assert_eq!(merged_meetings(h) > 0, pieces, "{name} (painted: {painted}): {h:?}");
            let mut through = h.clone();
            through.substitute(0, cut.history().unwrap()).unwrap();
            through.check(&r, &[&b, &tool]).unwrap();
        }
    }
}

/// A chamfer right after a box cut, its cutter's cut going the prism way along the edge: the
/// cutter's walls meet the cut body's walls where a segment was split off. The cut body's ends
/// have their outlines in collinear edges (pocket, step, bite), each matched by the segment
/// running along it; walls the cut left in pieces across the slabs (step, slot, bite of the
/// painted block) are followed through the levels where one piece gives way to the next. Where
/// the cut joined those pieces, the line between two walls stays in edges split at the levels,
/// and each slab's edge runs along the one over its levels. Composed with the cut, over the
/// block and the tool.
#[test]
fn chamfer_after_box_cuts() {
    let at = Vec3::new(5.0, 0.0, 20.0);
    for (b, painted) in [(cube(), false), (painted_cube(), true)] {
        for (name, cut, tool) in box_cuts_of(&b) {
            let r = crate::chamfer(&cut, &[at], 2.0).unwrap();
            let h = verified(&r, &cut, reach(2.0 * 1.5));
            // On the painted block the chamfer face keeps the boundaries the cut left.
            let faces = with(h, &[edge(edge_at(&cut, at))]).len();
            assert!(if painted { faces >= 1 } else { faces == 1 }, "{name} (painted: {painted}): {h:?}");
            let mut through = h.clone();
            through.substitute(0, cut.history().unwrap()).unwrap();
            through.check(&r, &[&b, &tool]).unwrap();
        }
    }
}

/// Shell, offset faces and thicken through SMLib (`crate::offset_backend`), their histories
/// checked against the geometry as truck's are, and their numbers against the analytic ones.
#[cfg(feature = "smlib")]
mod smlib {
    use super::*;
    use crate::offset_backend;

    fn volume(b: &Body) -> f64 {
        crate::measure(b).unwrap().volume
    }

    #[test]
    fn shell_lines_the_walls_with_offsets() {
        let b = cube();
        let top = Vec3::new(20.0, 15.0, 20.0);
        let r = offset_backend::shell(&b, &[top], 2.0).unwrap();
        // The cavity is 36 x 26 x 18 (the floor 2 thick, the top open).
        assert!((volume(&r) - (24000.0 - 36.0 * 26.0 * 18.0)).abs() < 1e-3, "{}", volume(&r));
        let h = verified(&r, &b, reach(2.0));
        let top = face_at(&b, top);
        for f in (0..6).filter(|f| *f != top) {
            let walls = with(h, &[FaceOrigin::Offset { operand: 0, face: f }]);
            assert_eq!(walls.len(), 1, "{h:?}");
            assert_eq!(h.faces[walls[0]].change, Change::Generated);
        }
        assert!(with(h, &[FaceOrigin::Offset { operand: 0, face: top }]).is_empty());
        assert_eq!(changes(h), [0, 6, 0, 0, 5], "{h:?}");
    }

    #[test]
    fn shell_of_a_body_that_is_not_convex() {
        let b = ell();
        let r = offset_backend::shell(&b, &[Vec3::new(5.0, 5.0, 20.0)], 2.0).unwrap();
        let h = verified(&r, &b, reach(2.0 * 1.5));
        let offsets = h.faces.iter().filter(|f| matches!(f.origins.as_slice(), [FaceOrigin::Offset { .. }])).count();
        assert_eq!(offsets, 7, "{h:?}");
    }

    /// tangency-labs/solidcore#380: a closed shell whose wall leaves no room inside the body is
    /// refused before SMLib's offset sees it, by the body's own inradius (the L's legs are 15
    /// wide, the largest ball fits at the inner corner: 15 * sqrt(2) / (1 + sqrt(2)) = 8.79),
    /// never by its box (whose half extent, 10, the old guard used).
    #[test]
    fn a_wall_with_no_room_inside_is_refused() {
        let b = ell();
        for t in [8.8, 9.5, 12.0] {
            let e = offset_backend::shell(&b, &[], t).unwrap_err();
            assert!(e.to_string().contains("no room"), "{t}: {e}");
        }
        let e = offset_backend::shell(&b, &[Vec3::new(5.0, 5.0, 20.0)], 8.8).unwrap_err();
        assert!(e.to_string().contains("no room"), "{e}");
    }

    /// A hole's wall is stored as two halves; a pick on the line between them opens the whole
    /// wall (kernel-cases shell/4010). The cavity is the plate inset by 2.5 less the hole's
    /// cylinder, open to the hole.
    #[test]
    fn shell_open_at_a_hole_wall_opens_the_whole_wall() {
        let hole = Loop2::circle(Vec2::new(20.0, 15.0), 5.0);
        let plate = extrude(&Plane::XY, &[Region2 { outer: square(0.0, 0.0, 40.0, 30.0), holes: vec![hole] }], 0.0, 12.5).unwrap().pop().unwrap();
        let r = offset_backend::shell(&plate, &[Vec3::new(25.0, 15.0, 6.25)], 2.5).unwrap();
        let pi = std::f64::consts::PI;
        let want = (15000.0 - pi * 25.0 * 12.5) - (35.0 * 25.0 * 7.5 - pi * 25.0 * 7.5);
        // The measure is a mesh volume (within 1e-5 of the true one on this round hole).
        assert!((volume(&r) - want).abs() < want * 1e-5, "{} vs {want}", volume(&r));
        verified(&r, &plate, reach(2.5 * 1.5));
    }

    #[test]
    fn offset_moves_a_face() {
        let b = ell();
        let top = Vec3::new(5.0, 5.0, 20.0);
        let healed = crate::boolean_record::healed(&b).unwrap();
        let chosen = [face_at(&healed, top)];
        let r = offset_backend::offset_faces(&healed, &chosen, 3.0).unwrap();
        // The L's plan is 40 x 15 + 15 x 15 = 825; 23 high.
        assert!((volume(&r) - 825.0 * 23.0).abs() < 1e-3, "{}", volume(&r));
        let h = verified(&r, &healed, Reach { made: 0.0, moved: 3.0 * 1.1 });
        assert_eq!(h.faces.len(), healed.face_count());
        let moved = with(h, &[face(chosen[0])]);
        assert_eq!(moved.len(), 1);
        assert_eq!(h.faces[moved[0]].change, Change::Modified);
        // Into the body by its whole height: nothing is left behind the face.
        let e = offset_backend::offset_faces(&healed, &chosen, -20.0).unwrap_err();
        assert!(e.to_string().contains("no room"), "{e}");
    }

    #[test]
    fn thicken_offsets_the_surface() {
        let s = crate::patch_region(&Plane::XY, &Region2 { outer: square(0.0, 0.0, 10.0, 10.0), holes: vec![] }).unwrap();
        let t = offset_backend::thicken(&s, 2.0, false).unwrap();
        assert!((volume(&t) - 200.0).abs() < 1e-6, "{}", volume(&t));
        // Along the sheet's normal (+Z).
        let m = t.tessellate(TOL).unwrap();
        let (lo, hi) = m.positions.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.z), hi.max(p.z)));
        assert!(lo.abs() < 1e-9 && (hi - 2.0).abs() < 1e-9, "z {lo}..{hi}");
        let h = verified(&t, &s, reach(2.0));
        assert_eq!(with(h, &[face(0)]).len(), 1, "{h:?}");
        assert_eq!(with(h, &[FaceOrigin::Offset { operand: 0, face: 0 }]).len(), 1, "{h:?}");
        assert_eq!(blends(h).len(), 4, "{h:?}");
        let both = offset_backend::thicken(&s, 2.0, true).unwrap();
        let m = both.tessellate(TOL).unwrap();
        let (lo, hi) = m.positions.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.z), hi.max(p.z)));
        assert!((lo + 1.0).abs() < 1e-9 && (hi - 1.0).abs() < 1e-9, "z {lo}..{hi}");
        let h = verified(&both, &s, reach(1.0));
        assert_eq!(with(h, &[FaceOrigin::Offset { operand: 0, face: 0 }]).len(), 2, "{h:?}");
        assert_eq!(blends(h).len(), 4, "{h:?}");
        let against = offset_backend::thicken(&s, -2.0, false).unwrap();
        assert!((volume(&against) - 200.0).abs() < 1e-6);
    }
}
