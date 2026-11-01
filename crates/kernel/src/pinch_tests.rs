//! Outlines that touch themselves: a circle tangent to a side, a corner on an edge, corners
//! meeting. Joins over such a contact fuse into one solid, cuts that leave pieces meeting only
//! along a line give separate shells, and near-tangent numbers from the sketch solver (a
//! circle 1e-13 mm past the side) behave like exact ones.

use solidcraft_geom::{Loop2, Plane, Region2, Vec2, Vec3};

use crate::*;

fn prism(outer: Loop2, z0: f64, z1: f64) -> Body {
    extrude(&Plane::XY, &[Region2 { outer, holes: vec![] }], z0, z1).unwrap().pop().unwrap()
}

fn poly(pts: &[[f64; 2]]) -> Loop2 {
    Loop2::polygon(&pts.iter().map(|p| Vec2::new(p[0], p[1])).collect::<Vec<_>>())
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Loop2 {
    poly(&[[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn disc(cx: f64, cy: f64, r: f64) -> Loop2 {
    Loop2::circle(Vec2::new(cx, cy), r)
}

fn assert_solid(r: &Body, volume: f64, rel: f64, shells: usize, what: &str) {
    let v = r.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    assert!((v - volume).abs() <= volume * rel, "{what}: volume {v} vs {volume}");
    assert_eq!(r.shell_count(), shells, "{what}: shells");
    assert!(r.validity().is_empty(), "{what}: {:?}", r.validity());
}

/// The reported case: a disc on a 31 × 29 block, tangent to its back side and overhanging the
/// other three, joined exactly, whether the tangency is exact or the solver's (1.3e-13 past).
/// The solver's numbers used to fail every route: a split line traced on the disc's face ended
/// one chord's sag short of the rim and was dropped, so the contact faces never matched.
#[test]
fn disc_tangent_to_a_side_and_overhanging_joins() {
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    for (cy, r) in [(4.0, 25.0), (4.392830461839724, 24.607169538160402), (4.0, 25.0 + 1e-9), (4.0, 25.0 - 1e-9)] {
        let top = prism(disc(18.5, cy, r), 10., 20.);
        let want = 8990.0 + std::f64::consts::PI * r * r * 10.0;
        for (a, b) in [(&block, &top), (&top, &block)] {
            let res = boolean(a, b, BoolOp::Union);
            let res = res.unwrap().unwrap();
            assert_solid(&res, want, 1e-5, 1, &format!("disc at {cy}, r {r}"));
        }
    }
}

/// A wall splitting a disc's face where the rim crosses it between two chords of the trace
/// mesh: the split happens however the mesh falls.
#[test]
fn split_line_ending_past_the_last_chord_still_splits() {
    let top = prism(disc(18.5, 4.392830461839724, 24.607169538160402), 10., 20.);
    let bottom =
        (0..top.face_count()).find(|i| matches!(face_surface(&top, *i), Some(FaceSurface::Plane { normal, .. }) if normal.z < -0.9)).unwrap();
    let wall = SplitTool::Plane(Plane { origin: Vec3::ZERO, x: Vec3::X, y: Vec3::Z });
    let (split, _) = split_faces_with_map(&top, &[bottom], &wall).unwrap();
    assert_eq!(split.face_count(), top.face_count() + 1);
}

/// Lines only: a triangle with its apex on the square's back edge, its base overhanging, and a
/// diamond with its corners on the square's edge midpoints, each standing on the square.
#[test]
fn corner_on_an_edge_joins() {
    let square = prism(rect(0., 0., 20., 20.), 0., 10.);
    let triangle = prism(poly(&[[-5., -5.], [25., -5.], [10., 20.]]), 10., 20.);
    let diamond = prism(poly(&[[10., 0.], [20., 10.], [10., 20.], [0., 10.]]), 10., 20.);
    for (tool, want) in [(&triangle, 4000.0 + 3750.0), (&diamond, 4000.0 + 2000.0)] {
        let res = boolean(&square, tool, BoolOp::Union).unwrap().unwrap();
        assert_solid(&res, want, 1e-9, 1, "corner on an edge");
    }
}

/// A cut leaving pieces that meet only along a line gives one shell per piece, not one shell
/// pinched along that line: a diamond cut through a square leaves four corner posts, a disc
/// tangent to the block's back side two.
#[test]
fn cut_leaving_pieces_meeting_along_a_line_gives_separate_shells() {
    let square = prism(rect(0., 0., 20., 20.), 0., 10.);
    let diamond = prism(poly(&[[10., 0.], [20., 10.], [10., 20.], [0., 10.]]), -1., 11.);
    let res = boolean(&square, &diamond, BoolOp::Cut).unwrap().unwrap();
    assert_solid(&res, 2000.0, 1e-9, 4, "diamond through a square");
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    let tool = prism(disc(18.5, 4., 25.), -1., 11.);
    let res = boolean(&block, &tool, BoolOp::Cut).unwrap().unwrap();
    // Material left: the block's area outside the disc, as two corner posts 10 mm high.
    let n = 400_000;
    let dx = 31.0 / n as f64;
    let outside: f64 = (0..n)
        .map(|i| {
            let x = (i as f64 + 0.5) * dx - 18.5;
            let h = (625.0 - x * x).max(0.0).sqrt();
            (29.0 - (4.0 + h).min(29.0)).max(0.0) * dx
        })
        .sum();
    assert_solid(&res, outside * 10.0, 1e-4, 2, "disc tangent to the back side, cut through");
}

/// A hole tangent to a side from inside is one piece (the material goes round the hole), and a
/// disc standing on a larger one, tangent to its rim from inside, joins.
#[test]
fn hole_touching_the_outline_stays_one_piece() {
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    let hole = prism(disc(5., 14.5, 5.), -1., 11.);
    let res = boolean(&block, &hole, BoolOp::Cut).unwrap().unwrap();
    assert_solid(&res, (899.0 - std::f64::consts::PI * 25.0) * 10.0, 1e-5, 1, "hole tangent to a side");
    let small = prism(disc(-2., 0., 10.), 0., 10.);
    let big = prism(disc(0., 0., 12.), 10., 20.);
    let res = boolean(&small, &big, BoolOp::Union).unwrap().unwrap();
    assert_solid(&res, std::f64::consts::PI * (100.0 + 144.0) * 10.0, 1e-5, 1, "disc tangent inside a larger one");
}

/// A Ø40 base with a Ø20 boss on it touching its rim from inside at (20, 0), and a Ø10 hole at
/// (15, 0) touching both rims there: the material on either side of the hole meets along the
/// line x = 20, y = 0 only (a zero-thickness curved wall). The hole cut through both, the boss
/// only and the base only, all exact (no tool nudge); then the same turned 30° and mirrored.
#[test]
fn hole_tangent_inside_a_stepped_boss_cuts_exactly() {
    use std::f64::consts::PI;
    for (turn, flip) in [(0.0, 1.0), (PI / 6.0, 1.0), (0.0, -1.0)] {
        let at = |x: f64, y: f64| Vec2::new(x * turn.cos() - flip * y * turn.sin(), x * turn.sin() + flip * y * turn.cos());
        let circle = |x: f64, r: f64| Loop2::circle(at(x, 0.), r);
        let base = prism(circle(0., 20.), 0., 5.);
        let boss = prism(circle(10., 10.), 5., 15.);
        let stepped = boolean(&base, &boss, BoolOp::Union).unwrap().unwrap();
        for (z0, z1, want) in [(-1., 20., 2000. + 1000. - 25. * 15.), (6., 20., 2000. + 1000. - 25. * 9.), (-1., 4., 2000. + 1000. - 25. * 4.)] {
            let what = format!("hole z {z0}…{z1}, turned {turn}, flip {flip}");
            let hole = prism(circle(15., 5.), z0, z1);
            let res = boolean(&stepped, &hole, BoolOp::Cut);
            let res = res.unwrap_or_else(|e| panic!("{what}: {e}")).unwrap();
            assert_solid(&res, PI * want, 1e-5, 1, &what);
        }
    }
}

/// A square hole in a stepped block whose side lies on the block's side: planar walls meeting
/// along a line at no angle are not a zero-thickness wall but one flat side, and a hole tangent
/// to the side of a square boss on a block cuts exactly.
#[test]
fn hole_tangent_to_a_flat_side_of_a_stepped_block_cuts_exactly() {
    let block = prism(rect(0., 0., 40., 40.), 0., 5.);
    let boss = prism(rect(10., 10., 40., 30.), 5., 15.);
    let stepped = boolean(&block, &boss, BoolOp::Union).unwrap().unwrap();
    let hole = prism(disc(35., 20., 5.), -1., 20.);
    let res = boolean(&stepped, &hole, BoolOp::Cut);
    let res = res.unwrap().unwrap();
    assert_solid(&res, 8000. + 6000. - std::f64::consts::PI * 25. * 15., 1e-5, 1, "hole tangent to a flat side");
}

/// A stepped block cut by a disc tangent to its back side from inside leaves two posts meeting
/// only along the tangent line: the stepped prism route keeps them apart (its edge sheets bound
/// the posts, as in the single-height case above) rather than sewing one shell pinched along the
/// line. A diamond hole with a corner on the side of a stepped block is one piece: the material
/// goes round the hole.
#[test]
fn stepped_cuts_touching_along_a_line_keep_their_pieces() {
    let block = prism(rect(0., 0., 31., 29.), 0., 10.);
    let back = prism(rect(0., 20., 25., 29.), 10., 20.);
    let stepped = boolean(&block, &back, BoolOp::Union).unwrap().unwrap();
    let tool = prism(disc(18.5, 4., 25.), -1., 21.);
    let res = boolean(&stepped, &tool, BoolOp::Cut);
    let res = res.unwrap().unwrap();
    let n = 400_000;
    let dx = 31.0 / n as f64;
    let post = |x: f64| (29.0 - (4.0 + (625.0 - (x - 18.5).powi(2)).max(0.0).sqrt()).min(29.0)).max(0.0);
    let volume: f64 = (0..n)
        .map(|i| {
            let x = (i as f64 + 0.5) * dx;
            post(x) * dx * if x < 25.0 { 20.0 } else { 10.0 }
        })
        .sum();
    let v = res.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume;
    assert!((v - volume).abs() <= volume * 1e-4, "posts: volume {v} vs {volume}");

    let block = prism(rect(0., 0., 40., 40.), 0., 5.);
    let boss = prism(rect(10., 10., 40., 30.), 5., 15.);
    let stepped = boolean(&block, &boss, BoolOp::Union).unwrap().unwrap();
    let diamond = prism(poly(&[[30., 20.], [35., 15.], [40., 20.], [35., 25.]]), -1., 20.);
    let res = boolean(&stepped, &diamond, BoolOp::Cut);
    let res = res.unwrap().unwrap();
    assert_solid(&res, 8000. + 6000. - 50. * 15., 1e-9, 1, "diamond hole with a corner on a stepped side");
}

/// A row of holes each touching the next (a 5 mm pattern of Ø5 holes) through a stepped plate:
/// the walls between them have no thickness, and the cut is exact.
#[test]
fn row_of_touching_holes_through_a_stepped_plate_cuts_exactly() {
    let plate = prism(rect(0., 0., 30., 35.), 0., 7.5);
    let notch = prism(rect(0., 30., 22.5, 50.), 2.5, 25.);
    let mut body = boolean(&plate, &notch, BoolOp::Cut).unwrap().unwrap();
    for x in [15.45, 20.45, 25.45] {
        let hole = prism(disc(x, 15.7, 2.5), -5., 7.5);
        let res = boolean(&body, &hole, BoolOp::Cut);
        body = res.unwrap().unwrap();
    }
    assert_solid(&body, 7312.5 - 3.0 * std::f64::consts::PI * 6.25 * 7.5, 1e-5, 1, "touching holes");
}
