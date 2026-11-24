//! The golden rows the point merge's edge-end cover moved (tangency-labs/solidcraft#21340).
//!
//! `PointClassification::merge_into_brep` now grows the merged vertex's tolerance to twice
//! the farthest end of its edges at it: per end, the evaluated end point's distance `d`
//! plus `√3` times the curve's bound on its evaluation's error `E`
//! (`Curve::point_error_bound`), each step rounded up by 4 ulps; `d` alone for a curve
//! with no bound. Before, the vertex was sized by the merged points' zone neighbourhood
//! only, and an edge end could lie farther than half its tolerance (`AssertValid`'s test of
//! an edge end against its vertex, `docs/validity-and-tolerances.md`).
//!
//! The tests replay every moved row of the three units that move and check, on the bodies
//! the operation leaves:
//! 1. `boolean.topology_intersector_p3` (7 rows) and `topology.brep_p9` (4 rows): each
//!    moved value is a vertex tolerance `old -> new`; every vertex holding `new` has its
//!    ends within `new / 2`, and one of them (the vertex the merge sized; the others are
//!    its mates in the other body, which take the larger tolerance of the pair) has its
//!    farthest end `d` beyond `old / 2` (the old tolerance failed the test: the move is
//!    forced), with `new` no larger than twice the end's certified bound `d + √3·E` plus
//!    16 ulps (the sizing's four roundings up by 4 ulps each). The
//!    `topology.brep_p9` rows' merged vertex moves to the classified point `p` while its
//!    edges keep their ends at the origin, so `d = |p|` there (a closed form checked too).
//! 2. `topology.face_p1` (37 rows, `MoveSeam` with split on `vase_rev.smb` face 0, and the
//!    one plain `vase_rev.smb` row): every edge end of the body is within half its vertex's
//!    tolerance. The moved values are the split's edge count (1 -> 2) and the body's edge
//!    curve tally (12 -> 11); before the change the split's vertices failed the end test.

use std::cell::RefCell;
use std::rc::Rc;

use solidcore::topology::Brep;
use solidcore::Kernel;

use crate::case::regression_case;

/// The vertex tolerance moves: (unit, case, [(old, new)]), each `new` the value the row
/// now records, `old` main's.
const TOLERANCE_ROWS: [(&str, &str, &[(f64, f64)]); 11] = [
    (
        "boolean.topology_intersector_p3",
        "gen0012",
        &[
            (6.404311934977263, 12.803875079777875),
            (7.142616126087225, 14.280483461997958),
        ],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0017",
        &[
            (6.4043119349772235, 12.803875079777951),
            (7.071073311865475, 14.14212662373102),
        ],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0115",
        &[
            (2.523574537176181, 5.047129074352894),
            (3.5143882263587067, 7.028756452717949),
        ],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0165",
        &[
            (5.000055, 10.000090000000071),
            (5.000055, 10.00009000000009),
        ],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0187",
        &[(5.0009999999999994, 10.00000000000054)],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0207",
        &[(5.0000050499999995, 9.99999010000054)],
    ),
    (
        "boolean.topology_intersector_p3",
        "gen0243",
        &[
            (2.93969447868463, 5.879368957369961),
            (6.793148519900026, 13.586277039800766),
        ],
    ),
    (
        "topology.brep_p9",
        "point_merge_7_near_end_sss_ignored",
        &[(1e-7, 41.23105625617673)],
    ),
    (
        "topology.brep_p9",
        "point_merge_7_near_start_sss_ignored",
        &[(1e-7, 48.989794855663696)],
    ),
    (
        "topology.brep_p9",
        "point_merge_7_new_center",
        &[(0.6090170443749475, 1.0180340887499457)],
    ),
    (
        "topology.brep_p9",
        "point_merge_7_sss_point",
        &[(0.1, 0.7211102550928484)],
    ),
];

/// The `topology.brep_p9` rows' classified point, to which the merged vertex moves.
const P9_POINTS: [(&str, [f64; 3]); 4] = [
    ("point_merge_7_near_end_sss_ignored", [20.0, 3.0, 4.0]),
    ("point_merge_7_near_start_sss_ignored", [10.0, -10.0, 20.0]),
    (
        "point_merge_7_new_center",
        [0.22763934258568186, 0.4552786851713637, 0.0],
    ),
    ("point_merge_7_sss_point", [0.2, 0.3, 0.0]),
];

/// The `topology.face_p1` rows that moved.
const FACE_P1_ROWS: [&str; 37] = [
    "guard_vase_rev_m0_c01_00",
    "guard_vase_rev_m0_c01_01",
    "guard_vase_rev_m0_c01_10",
    "guard_vase_rev_m0_c01_11",
    "guard_vase_rev_m0_c10_00",
    "guard_vase_rev_m0_c10_01",
    "guard_vase_rev_m0_c10_10",
    "guard_vase_rev_m0_c10_11",
    "guard_vase_rev_m0_c11_00",
    "guard_vase_rev_m0_c11_01",
    "guard_vase_rev_m0_c11_10",
    "guard_vase_rev_m0_c11_11",
    "guard_vase_rev_m1_c01_00",
    "guard_vase_rev_m1_c01_01",
    "guard_vase_rev_m1_c01_10",
    "guard_vase_rev_m1_c01_11",
    "guard_vase_rev_m1_c10_00",
    "guard_vase_rev_m1_c10_01",
    "guard_vase_rev_m1_c10_10",
    "guard_vase_rev_m1_c10_11",
    "guard_vase_rev_m1_c11_00",
    "guard_vase_rev_m1_c11_01",
    "guard_vase_rev_m1_c11_10",
    "guard_vase_rev_m1_c11_11",
    "guard_vase_rev_m3_c01_00",
    "guard_vase_rev_m3_c01_01",
    "guard_vase_rev_m3_c01_10",
    "guard_vase_rev_m3_c01_11",
    "guard_vase_rev_m3_c10_00",
    "guard_vase_rev_m3_c10_01",
    "guard_vase_rev_m3_c10_10",
    "guard_vase_rev_m3_c10_11",
    "guard_vase_rev_m3_c11_00",
    "guard_vase_rev_m3_c11_01",
    "guard_vase_rev_m3_c11_10",
    "guard_vase_rev_m3_c11_11",
    "vase_rev.smb_1_f5_op9",
];

/// One edge end: (vertex tolerance, the evaluated end's distance to the vertex `d`, the
/// certified bound `d + √3·E`, `None` without an error bound), per vertex index.
type End = (u32, f64, f64, Option<f64>);

/// Every edge end of `b` (see [`End`]).
fn ends(b: &Brep) -> Vec<End> {
    let k = Kernel::default();
    let mut ends = Vec::new();
    for (e, rec) in b.edges().iter() {
        let Some(c) = rec.curve.as_ref() else {
            continue;
        };
        let (s, t) = solidcore::topology::edge::start_end_vertices(b, e).unwrap();
        let iv = solidcore::topology::edge::interval(b, e).unwrap();
        for (v, at) in [(s, iv.min()), (t, iv.max())] {
            let Some(v) = v else { continue };
            let vr = b.vertex(v).unwrap();
            let d = c.evaluate_point(&k, at).unwrap().distance(vr.point);
            let bound = c.point_error_bound(at).map(|err| d + 3.0_f64.sqrt() * err);
            ends.push((v.index(), vr.tolerance, d, bound));
        }
    }
    ends
}

/// Runs `case` of `unit`, returning every end of each body the runner observes.
fn run(unit: &str, case: &str) -> Vec<Vec<End>> {
    let u = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .unwrap_or_else(|| panic!("no unit {unit}"));
    let c = regression_case(u.as_ref(), case);
    let bodies: Rc<RefCell<Vec<Vec<End>>>> = Rc::default();
    let into = Rc::clone(&bodies);
    let out = crate::trim_hook::with_hook(
        Box::new(move |b: &mut Brep| into.borrow_mut().push(ends(b))),
        || u.run_rust(&c),
    );
    out.unwrap_or_else(|e| panic!("{case}: {e:?}"));
    bodies.take()
}

/// Rows 1: each moved tolerance is forced (the old one failed the end test), covers its
/// vertex's farthest end, and is at most twice that end's certified bound plus 16 ulps.
#[test]
fn moved_vertex_tolerances_are_twice_the_certified_farthest_end() {
    let slack = 1.0 + 16.0 * f64::EPSILON;
    for (unit, case, moves) in TOLERANCE_ROWS {
        let bodies = run(unit, case);
        assert!(!bodies.is_empty(), "{case}: no body observed");
        for &(old, new) in moves {
            // the vertex the merge sized, and its mates in the other body, which take the
            // larger tolerance of the pair (the mates' own rule, unchanged here)
            let mut sized = 0;
            for body in &bodies {
                let mut vertices: Vec<u32> = body
                    .iter()
                    .filter(|(_, tol, _, _)| tol.to_bits() == new.to_bits())
                    .map(|(v, ..)| *v)
                    .collect();
                vertices.sort_unstable();
                vertices.dedup();
                for v in vertices {
                    let at: Vec<&End> = body.iter().filter(|(w, ..)| *w == v).collect();
                    let d = at.iter().map(|e| e.2).fold(0.0_f64, f64::max);
                    let bound = at
                        .iter()
                        .map(|e| e.3.unwrap_or(e.2))
                        .fold(0.0_f64, f64::max);
                    assert!(d <= 0.5 * new, "{case} v{v}: {d} not covered by {new}");
                    if d > 0.5 * old && new <= 2.0 * bound * slack {
                        if unit == "topology.brep_p9" {
                            let p = P9_POINTS.iter().find(|(n, _)| *n == case).unwrap().1;
                            let closed = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                            assert!(
                                (d - closed).abs() <= 4.0 * f64::EPSILON * closed,
                                "{case}: end {d}, |p| {closed}"
                            );
                        }
                        sized += 1;
                    }
                }
            }
            assert!(
                sized > 0,
                "{case}: no vertex sized to {new} by an end past {old} / 2"
            );
        }
    }
}

/// Rows 2: every edge end of each `topology.face_p1` body is within half its vertex's
/// tolerance.
#[test]
fn moved_move_seam_bodies_cover_every_edge_end() {
    for case in FACE_P1_ROWS {
        let bodies = run("topology.face_p1", case);
        assert_eq!(bodies.len(), 1, "{case}: one body");
        for (v, tol, d, _) in &bodies[0] {
            assert!(
                *d <= 0.5 * tol,
                "{case}: an end {d} from v{v}, tolerance {tol}"
            );
        }
    }
}
