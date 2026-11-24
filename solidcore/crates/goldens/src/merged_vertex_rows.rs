//! The golden rows the point merge's edge-end cover moved (tangency-labs/solidcraft#21340).
//!
//! `PointClassification::merge_into_brep` now grows the merged vertex's tolerance to twice
//! the farthest end of its edges at it, when every such end lies inside the merged
//! neighbourhood (a ball of the merged zone tolerance). Per end the bound is the evaluated
//! end point's distance `d` plus `√3` times the curve's bound on its evaluation's error `E`
//! (`Curve::point_error_bound`), each step rounded up by 4 ulps. Before, the vertex was
//! sized by the merged points' zone neighbourhood only, and an edge end could lie farther
//! than half its tolerance (`AssertValid`'s test of an edge end against its vertex,
//! `docs/validity-and-tolerances.md`).
//!
//! Each test replays the moved rows and holds them to main's rows, which are kept verbatim
//! in `merged_vertex_rows_base.jsonl` (the units' expected lines at this change's base):
//! 1. Vertex tolerance rows (`boolean.topology_intersector_p3`, `topology.brep_p9`): each
//!    moved value is a vertex tolerance `old -> new`. On the bodies the operation leaves,
//!    every vertex holding `new` has its ends within `new / 2`, and one of them (the vertex
//!    the merge sized; the others are its mates in the other body, which take the larger
//!    tolerance of the pair) has its farthest end `d` beyond `old / 2`, so the old value
//!    failed the end test, and `new` is at most twice the end's certified bound `d + √3·E`
//!    plus 16 ulps (the sizing's four roundings up by 4 ulps each). Then the row is
//!    rebuilt with every entry holding a `new` replaced by its `old` and encoded as the
//!    golden encodes it: it must equal main's row exactly (every block's sum, absolute sum
//!    and weighted sum), so no other entry moved and the `old` values are main's.
//! 2. `topology.face_p1` (37 rows, `MoveSeam` with split on `vase_rev.smb` face 0, and one
//!    plain `vase_rev.smb` row): every edge end of the body is within half its vertex's
//!    tolerance (main's body fails this, see `topology_face_p1`'s regression test). The
//!    edge curve tally and the UV trim tally the row records are the body's own counts of
//!    edges with a curve and of their edge uses with a UV trim, read again from the body,
//!    every one its own owner's. The row with the split's edge count one lower, the curve
//!    tally one higher and the UV trim tally at main's [`MAIN_UV_TRIMS`] encodes exactly to
//!    main's row.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;
use solidcore::topology::Brep;
use solidcore::Kernel;

use crate::case::regression_case;
use crate::output::Output;

/// Main's expected lines for the moved cases (`(unit) (json line)` per line).
const BASE: &str = include_str!("merged_vertex_rows_base.jsonl");

/// A vertex tolerance row: (unit, case, [(old, new)]).
type ToleranceRow = (&'static str, &'static str, &'static [(f64, f64)]);

/// The vertex tolerance moves: (unit, case, [(old, new)]), each `new` the value the row
/// now records, `old` main's.
const TOLERANCE_ROWS: [ToleranceRow; 8] = [
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
        "point_merge_7_new_center",
        &[(0.6090170443749475, 1.0180340887499457)],
    ),
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

/// `topology.face_p1`'s `MoveSeam` record (`out`): the index of the split's edge count. The
/// edge curve tally and its own-owner count are `put_owners`' second group, 8 from the end,
/// and the UV trim tally its third, 4 from the end.
const SPLIT_EDGES: usize = 14;

/// Main's UV trim tally (`n` and `own`) on the `topology.face_p1` rows: WR-469's vertex step
/// (tangency-labs/solidcraft#21273) changed it from 15 to 2 there, and with the merge
/// covering the edge ends the step grows no vertex on these rows (`wr469_replay`), so the
/// rows record the body's 15 again.
const MAIN_UV_TRIMS: f64 = 2.0;

/// One edge end: (vertex index, vertex tolerance, the evaluated end's distance to the
/// vertex `d`, the certified bound `d + √3·E`, `None` without an error bound).
type End = (u32, f64, f64, Option<f64>);

/// A body the runner observed: its edge ends, its count of edges with a curve and its count
/// of those edges' edge uses with a UV trim.
struct Body {
    ends: Vec<End>,
    curves: usize,
    uvs: usize,
}

/// Every edge end of `b` (see [`End`]) and its edge curve count.
fn body(b: &Brep) -> Body {
    let k = Kernel::default();
    let mut ends = Vec::new();
    let mut curves = 0;
    let mut uvs = 0;
    let edges = solidcore::topology::brep::p1::edges(b, None).unwrap();
    for e in edges {
        let rec = b.edge(e).unwrap();
        for eu in solidcore::topology::edge::edgeuses(b, e, None, None).unwrap() {
            if b.edgeuse(eu).unwrap().uv_trim_curve.is_some() {
                uvs += 1;
            }
        }
        let Some(c) = rec.curve.as_ref() else {
            continue;
        };
        curves += 1;
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
    Body { ends, curves, uvs }
}

/// Runs `case` of `unit`: its output and each body the runner observes.
fn run(unit: &str, case: &str) -> (Output, Vec<Body>) {
    let u = crate::registry()
        .into_iter()
        .find(|u| u.id() == unit)
        .unwrap_or_else(|| panic!("no unit {unit}"));
    let c = regression_case(u.as_ref(), case);
    let bodies: Rc<RefCell<Vec<Body>>> = Rc::default();
    let into = Rc::clone(&bodies);
    let out = crate::trim_hook::with_hook(
        Box::new(move |b: &mut Brep| into.borrow_mut().push(body(b))),
        || u.run_rust(&c),
    );
    let out = out.unwrap_or_else(|e| panic!("{case}: {e:?}"));
    assert!(
        out.error.is_none() && out.deferred.is_none(),
        "{case}: {:?} {:?}",
        out.error,
        out.deferred
    );
    (out, bodies.take())
}

/// Main's row for `case` of `unit`.
fn base_row(unit: &str, case: &str) -> Value {
    let mut found = None;
    for line in BASE.lines() {
        let (u, json) = line.split_once(' ').unwrap();
        let row: Value = serde_json::from_str(json).unwrap();
        if u == unit && row["case"] == case {
            assert!(found.is_none(), "{unit} {case}: two base rows");
            found = Some(row);
        }
    }
    found.unwrap_or_else(|| panic!("{unit} {case}: no base row"))
}

/// Asserts that `out` with each value `edit` changes encodes to main's row for `case`.
fn assert_rebuilds_base(
    unit: &str,
    case: &str,
    out: &Output,
    edit: impl Fn(&str, usize, f64) -> f64,
) {
    let mut old = out.clone();
    for (key, xs) in &mut old.values {
        for (i, x) in xs.iter_mut().enumerate() {
            *x = edit(key, i, *x);
        }
    }
    let now = crate::golden::encode(case, out);
    let base = base_row(unit, case);
    assert_ne!(now, base, "{unit} {case}: the row did not move");
    assert_eq!(
        crate::golden::encode(case, &old),
        base,
        "{unit} {case}: the row with the moved values put back is not main's"
    );
}

/// Rows 1: each moved tolerance is forced (the old one failed the end test), covers its
/// vertex's farthest end, and is at most twice that end's certified bound plus 16 ulps; and
/// the row with each new tolerance put back to its old one is main's row.
#[test]
fn moved_vertex_tolerances_are_twice_the_certified_farthest_end() {
    let slack = 1.0 + 16.0 * f64::EPSILON;
    for (unit, case, moves) in TOLERANCE_ROWS {
        let (out, bodies) = run(unit, case);
        assert!(!bodies.is_empty(), "{case}: no body observed");
        for &(old, new) in moves {
            // the vertex the merge sized, and its mates in the other body, which take the
            // larger tolerance of the pair (the mates' own rule, unchanged here)
            let mut sized = 0;
            for body in &bodies {
                let mut vertices: Vec<u32> = body
                    .ends
                    .iter()
                    .filter(|(_, tol, _, _)| tol.to_bits() == new.to_bits())
                    .map(|(v, ..)| *v)
                    .collect();
                vertices.sort_unstable();
                vertices.dedup();
                for v in vertices {
                    let at: Vec<&End> = body.ends.iter().filter(|(w, ..)| *w == v).collect();
                    let d = at.iter().map(|e| e.2).fold(0.0_f64, f64::max);
                    let bound = at
                        .iter()
                        .map(|e| e.3.unwrap_or(e.2))
                        .fold(0.0_f64, f64::max);
                    assert!(d <= 0.5 * new, "{case} v{v}: {d} not covered by {new}");
                    if d > 0.5 * old && new <= 2.0 * bound * slack {
                        sized += 1;
                    }
                }
            }
            assert!(
                sized > 0,
                "{case}: no vertex sized to {new} by an end past {old} / 2"
            );
        }
        assert_rebuilds_base(unit, case, &out, |_, _, x| {
            moves
                .iter()
                .find(|(_, new)| new.to_bits() == x.to_bits())
                .map_or(x, |&(old, _)| old)
        });
    }
}

/// Rows 2: every edge end of each `topology.face_p1` body is within half its vertex's
/// tolerance, the row's edge curve and UV trim tallies are the body's, and the row with the
/// split's edge count one lower, the curve tally one higher and the UV trim tally main's is
/// main's.
#[test]
fn moved_move_seam_bodies_cover_every_edge_end() {
    for case in FACE_P1_ROWS {
        let (out, bodies) = run("topology.face_p1", case);
        assert_eq!(bodies.len(), 1, "{case}: one body");
        let body = &bodies[0];
        for (v, tol, d, _) in &body.ends {
            assert!(
                *d <= 0.5 * tol,
                "{case}: an end {d} from v{v}, tolerance {tol}"
            );
        }
        let xs = &out.values["out"];
        let tally = xs.len() - 8;
        assert_eq!(xs[tally], body.curves as f64, "{case}: curve tally");
        assert_eq!(xs[tally + 1], body.curves as f64, "{case}: own curves");
        let uv = tally + 4;
        assert_eq!(xs[uv], body.uvs as f64, "{case}: UV trim tally");
        assert_eq!(xs[uv + 1], body.uvs as f64, "{case}: own UV trims");
        assert_rebuilds_base("topology.face_p1", case, &out, |key, i, x| match (key, i) {
            ("out", SPLIT_EDGES) => x - 1.0,
            ("out", i) if i == tally || i == tally + 1 => x + 1.0,
            ("out", i) if i == uv || i == uv + 1 => MAIN_UV_TRIMS,
            _ => x,
        });
    }
}
