//! A boundary matched to another surface's: a 2 degree kink taken out at G1, a curvature jump
//! taken out at G2 on a curved neighbour, how far one row and three rows carry a G0 move, and the
//! refusals.

use super::*;

/// A surface of degrees `(p, q)` on clamped Bezier knots through the rows of points `rows` (each
/// along v).
fn bezier(rows: &[&[[f64; 3]]]) -> Surface {
    let p = rows.len() - 1;
    let q = rows[0].len() - 1;
    let net: Vec<Vec<CPoint>> = rows
        .iter()
        .map(|r| {
            r.iter()
                .map(|c| CPoint::from_point(Vec3::new(c[0], c[1], c[2])))
                .collect()
        })
        .collect();
    let knots = |d: usize| {
        let mut k = vec![0.0; d + 1];
        k.extend(vec![1.0; d + 1]);
        k
    };
    Surface::new(net, p, q, knots(p), knots(q)).unwrap()
}

/// A flat 10 by 10 sheet in `z = h`, from `x = 0` (its `u = 0` side) to `x = 10`.
fn flat(h: f64) -> Surface {
    bezier(&[
        &[[0.0, 0.0, h], [0.0, 10.0, h]],
        &[[10.0, 0.0, h], [10.0, 10.0, h]],
    ])
}

fn rows(n: usize, blend: f64) -> MatchRows {
    MatchRows { rows: n, blend }
}

/// A neighbour coming up to `x = 0` (its `u = 1` side) from `x < 0`, 2 degrees off the flat
/// sheet's plane: matched at G1, the kink goes and the edge stays.
#[test]
fn a_two_degree_kink_is_taken_out_at_g1() {
    let a = 2.0_f64.to_radians();
    let (c, s) = (10.0 * a.cos(), 10.0 * a.sin());
    let neighbour = bezier(&[
        &[[-c, 0.0, s], [-c, 10.0, s]],
        &[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
    ]);
    let m = match_boundary(
        &flat(0.0),
        Boundary::Left,
        &neighbour,
        Boundary::Right,
        MatchOrder::G1,
        rows(2, 0.5),
    )
    .unwrap();
    assert!((m.before.angle - a).abs() < 1e-9, "{:?}", m.before);
    assert!(m.after.angle < 1e-9, "{:?}", m.after);
    assert!(m.after.gap < 1e-9, "{:?}", m.after);
    assert!(m.moved[0] < 1e-12, "the edge stays: {:?}", m.moved);
    assert!(m.moved[1] > 0.0, "{:?}", m.moved);
}

/// A curved neighbour (a parabolic arch, its end tangent level with the flat sheet, its end
/// curvature 1/25): the flat sheet is G1 to it already; matched at G2 its curvature across the
/// seam is the arch's.
#[test]
fn a_curvature_jump_is_taken_out_at_g2() {
    let arch = bezier(&[
        &[[-10.0, 0.0, -2.0], [-10.0, 10.0, -2.0]],
        &[[-5.0, 0.0, 0.0], [-5.0, 10.0, 0.0]],
        &[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
    ]);
    let m = match_boundary(
        &flat(0.0),
        Boundary::Left,
        &arch,
        Boundary::Right,
        MatchOrder::G2,
        rows(3, 0.5),
    )
    .unwrap();
    assert!(m.before.angle < 1e-9, "{:?}", m.before);
    assert!((m.before.curvature - 0.04).abs() < 1e-9, "{:?}", m.before);
    assert!(m.after.curvature < 1e-9, "{:?}", m.after);
    assert!(m.after.angle < 1e-9, "{:?}", m.after);
    assert!(m.after.gap < 1e-9, "{:?}", m.after);
    // only the third row moves, along the normal (z)
    assert!(m.moved[0] < 1e-12 && m.moved[1] < 1e-12, "{:?}", m.moved);
    assert!(m.moved[2] > 0.0, "{:?}", m.moved);
}

/// The flat sheet 1 above its neighbour's edge, matched at G0: with one row only the edge row
/// moves; with three and a blend of a half the next rows follow by a half and a quarter.
#[test]
fn rows_and_blend_set_how_far_a_move_spreads() {
    let below = bezier(&[
        &[[-10.0, 0.0, 0.0], [-10.0, 10.0, 0.0]],
        &[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
    ]);
    let one = match_boundary(
        &flat(1.0),
        Boundary::Left,
        &below,
        Boundary::Right,
        MatchOrder::G0,
        rows(1, 0.5),
    )
    .unwrap();
    assert!((one.moved[0] - 1.0).abs() < 1e-12, "{:?}", one.moved);
    assert_eq!((one.moved[1], one.moved[2]), (0.0, 0.0));
    assert!(one.after.gap < 1e-12, "{:?}", one.after);
    let three = match_boundary(
        &flat(1.0),
        Boundary::Left,
        &below,
        Boundary::Right,
        MatchOrder::G0,
        rows(3, 0.5),
    )
    .unwrap();
    let want = [1.0, 0.5, 0.25];
    for (got, want) in three.moved.iter().zip(want) {
        assert!((got - want).abs() < 1e-12, "{:?}", three.moved);
    }
    assert!(three.after.gap < 1e-12, "{:?}", three.after);
}

/// The sides turn: the same G1 match from the flat sheet's `v = 0` side (`Bottom`) to a
/// neighbour along `y < 0` gives the same seam.
#[test]
fn a_bottom_boundary_matches_as_a_left_one() {
    let a = 2.0_f64.to_radians();
    let (c, s) = (10.0 * a.cos(), 10.0 * a.sin());
    // the flat sheet with u along x and v along y: its v = 0 side is y = 0
    let sheet = bezier(&[
        &[[0.0, 0.0, 0.0], [0.0, 10.0, 0.0]],
        &[[10.0, 0.0, 0.0], [10.0, 10.0, 0.0]],
    ]);
    let neighbour = bezier(&[
        &[[0.0, -c, s], [0.0, 0.0, 0.0]],
        &[[10.0, -c, s], [10.0, 0.0, 0.0]],
    ]);
    let m = match_boundary(
        &sheet,
        Boundary::Bottom,
        &neighbour,
        Boundary::Top,
        MatchOrder::G1,
        rows(2, 0.0),
    )
    .unwrap();
    assert!((m.before.angle - a).abs() < 1e-9, "{:?}", m.before);
    assert!(m.after.angle < 1e-9, "{:?}", m.after);
    // the result has the sheet's own orientation: its v = 0 side still at y = 0
    let r = m.surface.parameter_bounds();
    let p = point(&m.surface, r.ul, r.vb).unwrap();
    assert!((p - Vec3::new(0.0, 0.0, 0.0)).length() < 1e-12, "{p:?}");
}

/// The refusals: a target whose boundary is one point, rows below the order, a blend out of
/// range.
#[test]
fn degenerate_targets_and_bad_rows_are_refused() {
    let point_side = bezier(&[
        &[[-10.0, 0.0, 0.0], [-10.0, 10.0, 0.0]],
        &[[0.0, 5.0, 0.0], [0.0, 5.0, 0.0]],
    ]);
    let refused = |r| matches!(r, Err(SolidError::InvalidInput { .. }));
    assert!(refused(match_boundary(
        &flat(0.0),
        Boundary::Left,
        &point_side,
        Boundary::Right,
        MatchOrder::G1,
        rows(2, 0.5),
    )));
    assert!(refused(match_boundary(
        &flat(0.0),
        Boundary::Left,
        &flat(0.0),
        Boundary::Right,
        MatchOrder::G2,
        rows(2, 0.5),
    )));
    assert!(refused(match_boundary(
        &flat(0.0),
        Boundary::Left,
        &flat(0.0),
        Boundary::Right,
        MatchOrder::G0,
        rows(1, 1.5),
    )));
}

// ---- a boundary matched to its own mirror ------------------------------------------------------

/// The plane `y = 0`, the side plane of a car: its point and normal.
const SIDE: (Vec3, Vec3) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0));

/// A deck coming up to `y = 0` (its `u = 0` side) 20 degrees off square to the plane: its first
/// column of control points on the plane, the next rising 20 degrees from the plane's normal.
fn leaning_deck() -> Surface {
    let a = 20.0_f64.to_radians();
    let (c, s) = (10.0 * a.cos(), 10.0 * a.sin());
    bezier(&[
        &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [20.0, 0.0, 0.0]],
        &[[0.0, c, s], [10.0, c, s], [20.0, c, s]],
        &[
            [0.0, 2.0 * c, 2.0 * s + 1.0],
            [10.0, 2.0 * c, 2.0 * s + 2.0],
            [20.0, 2.0 * c, 2.0 * s + 1.0],
        ],
    ])
}

/// The deck leaves the plane 20 degrees off square, so it meets its mirror at a 40 degree
/// crease. Matched at G1 the next row leaves square to the plane: the seam with the mirror has no
/// gap, no crease and no curvature jump (a surface square to its mirror plane is curvature
/// continuous with its mirror), and the edge row stays where it was.
#[test]
fn a_boundary_leaning_off_its_mirror_plane_is_made_square_to_it() {
    let (o, n) = SIDE;
    let m = match_plane(
        &leaning_deck(),
        Boundary::Left,
        o,
        n,
        MatchOrder::G1,
        rows(2, 0.5),
    )
    .unwrap();
    assert!(
        (m.before.angle - 40.0_f64.to_radians()).abs() < 1e-9,
        "{:?}",
        m.before
    );
    assert!(m.after.gap < 1e-12, "{:?}", m.after);
    assert!(m.after.angle < 1e-12, "{:?}", m.after);
    assert!(m.after.curvature < 1e-9, "{:?}", m.after);
    assert!(
        m.moved[0] < 1e-12,
        "the edge stays on the plane: {:?}",
        m.moved
    );
    assert!(m.moved[1] > 0.0, "{:?}", m.moved);
}

/// A curved deck, its edge 1 off the plane and leaning: matched at G2 (the same rows as G1, the
/// curvature following by symmetry) the edge is on the plane and the seam with its mirror holds
/// G2, measured.
#[test]
fn a_curved_boundary_off_its_plane_meets_its_mirror_at_g2() {
    let deck = bezier(&[
        &[[0.0, 1.0, 0.0], [10.0, 1.5, 2.0], [20.0, 1.0, 0.0]],
        &[[2.0, 6.0, 1.0], [10.0, 6.0, 3.0], [18.0, 6.0, 1.0]],
        &[[3.0, 12.0, 0.0], [10.0, 12.0, 2.0], [17.0, 12.0, 0.0]],
    ]);
    let (o, n) = SIDE;
    let m = match_plane(&deck, Boundary::Left, o, n, MatchOrder::G2, rows(3, 0.5)).unwrap();
    assert!(m.before.gap > 0.9, "{:?}", m.before);
    assert!(m.after.gap < 1e-12, "{:?}", m.after);
    assert!(m.after.angle < 1e-12, "{:?}", m.after);
    assert!(m.after.curvature < 1e-9, "{:?}", m.after);
}

/// The refusals: a normal of no length, a boundary that runs along the plane without leaving it
/// (a sheet lying in its mirror plane has no seam with its mirror), a boundary that leaves the
/// plane to both sides, and rows below what the order fixes.
#[test]
fn degenerate_planes_and_boundaries_are_refused() {
    let refused = |r| matches!(r, Err(SolidError::InvalidInput { .. }));
    let (o, n) = SIDE;
    assert!(refused(match_plane(
        &leaning_deck(),
        Boundary::Left,
        o,
        Vec3::new(0.0, 0.0, 0.0),
        MatchOrder::G1,
        rows(2, 0.5),
    )));
    // flat(0) lies in z = 0: its x = 0 side runs along the plane z = 0 without leaving it
    assert!(refused(match_plane(
        &flat(0.0),
        Boundary::Left,
        o,
        Vec3::new(0.0, 0.0, 1.0),
        MatchOrder::G1,
        rows(2, 0.5),
    )));
    let crossing = bezier(&[
        &[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
        &[[0.0, 5.0, 0.0], [10.0, -5.0, 0.0]],
    ]);
    assert!(refused(match_plane(
        &crossing,
        Boundary::Left,
        o,
        n,
        MatchOrder::G1,
        rows(2, 0.5)
    )));
    assert!(refused(match_plane(
        &leaning_deck(),
        Boundary::Left,
        o,
        n,
        MatchOrder::G1,
        rows(1, 0.5)
    )));
}
