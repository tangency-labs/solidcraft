//! The substitution replay of the `topology.face_p1` rows WR-469's vertex step moved
//! (tangency-labs/solidcraft#17985).
//!
//! `MakeEdgeInFaceWithVU` grows each end vertex's tolerance to twice its gap, the farthest
//! edge end at the vertex (the new edge's or any other edge's; only if larger). The `wr469-replay` seam (`solidcore`'s
//! `topology::brep::p9::replay`, a feature this crate turns on for its tests only) sees each
//! step's vertex, its tolerance before the step and the gap, and gives the tolerance the
//! step passes. The seam is the kernel's ([`solidcore::Kernel::wr469_replay`]); the
//! runners make their kernels with [`crate::runner_kernel`], which gives each the seam a
//! test here installs on its thread ([`with_seam`]).
//!
//! The tests run every row of `topology.face_p1` with the step answered by the tolerance
//! the vertex already has, which leaves it as the pinned C++ (and main before this change)
//! did:
//! 1. the rows that then do not agree with their committed line (the goldens' own
//!    comparison) are exactly [`MOVED`] (37), and each is then main's line ([`main_line`])
//!    bit for bit. So the step is the only cause of every move in the unit.
//! 2. On each of the 37, run as is and without the step: the encoded row as is is the
//!    committed line; the two records differ only in the UV trim tally (`out[24..26]`,
//!    `n own`: 15 without the step, 2 with it); the bodies have the same edge uses; with
//!    the step every edge's curve end at a vertex the step grew lies within half that
//!    vertex's tolerance (the property the step exists for, `AssertValid`'s test of an edge
//!    end; the step sizes from the farthest end at the vertex and doubling is exact, so no
//!    rounding allowance applies); as the operation leaves the body, exactly the four ends at
//!    the split's own vertices v6 and v7 ([`UNCOVERED_AT_CAPTURE`], sized by the point
//!    merge's zone neighbourhood, tangency-labs/solidcraft#21340) are uncovered, 148 over the 37 rows against 222
//!    without the step: the step covers fewer ends, not all of them; and `check_brep`
//!    level 2 fails on both bodies (with the step
//!    and without it), each geometry's owner taken as the geometry records it (as
//!    `validation::soundness` checks a body), so the failure is the body's own and not the
//!    runner's naming of it. The rows are invalid-input crash reproducers, and the value
//!    that moves is a trim tally, not geometry.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};
use solidcore::curve::curve::Curve;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::object::ObjectRef;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p9::replay::{VertexStep, VertexStepSeam};
use solidcore::topology::{Brep, VertexId};
use solidcore::{EntityRef, Kernel};

use crate::case::{repo_root, Case, CaseContext};
use crate::golden::{default_dir, encode, read};
use crate::UnitDiff;

/// The unit.
const UNIT: &str = "topology.face_p1";

/// Generated cases per unit, as `goldens` runs them (`main.rs` `DEFAULT_SEEDS`).
const SEEDS: usize = 16;

/// The rows the step moved: `MoveSeam` with split on `vase_rev.smb` face 0.
const MOVED: [&str; 37] = [
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

/// Main's encoded `out` of every [`MOVED`] row (main before this change).
fn main_line(name: &str) -> Value {
    json!({
        "case": name, "deferred": null, "error": null, "ints": {}, "topology": null,
        "values": {"out": {"blocks": [[186.0, 186.0, 2397.0]], "n": 28}},
    })
}

thread_local! {
    /// The seam a test here installed on this thread, for [`with_seam`].
    static SEAM: RefCell<Option<VertexStepSeam>> = const { RefCell::new(None) };
}

/// `k` with the seam a test here installed on this thread, if any ([`crate::runner_kernel`]).
pub(crate) fn with_seam(mut k: Kernel) -> Kernel {
    SEAM.with(|s| {
        if let Some(seam) = s.borrow().as_ref() {
            k.wr469_replay = seam.clone();
        }
    });
    k
}

/// How a run answers the step.
#[derive(Clone, Copy, PartialEq)]
enum Step {
    /// The step's own tolerance (this branch).
    Own,
    /// The tolerance the vertex already has (main, the pinned C++).
    Without,
}

/// The (edge, vertex) ends the step leaves uncovered as the operation leaves the body, on
/// every moved row: both ends at v6 (edges 6 and 7) and at v7 (edges 7 and 8), the
/// vertices the edge split made, whose tolerance the point merge sized
/// (`merge_vertex_neighborhood`, the zone neighbourhood of the merged points).
const UNCOVERED_AT_CAPTURE: [(u32, u32); 4] = [(6, 6), (7, 7), (7, 6), (8, 7)];

/// What a run of a `MoveSeam` row leaves.
struct Body {
    /// Every edge use.
    edgeuses: BTreeSet<u32>,
    /// `check_brep` level 2's lines.
    level2: Vec<String>,
    /// Whether level 2 passed.
    valid: bool,
    /// Per edge end as the operation leaves the body: (edge, vertex, the distance from the
    /// edge curve's end to the vertex, the vertex's tolerance).
    ends: Vec<(u32, VertexId, f64, f64)>,
}

impl Body {
    /// The edge ends whose gap is more than half the vertex's tolerance (`AssertValid`'s
    /// test of an edge end against its vertex).
    fn uncovered(&self) -> Vec<(u32, u32)> {
        uncovered(&self.ends)
    }
}

/// The (edge, vertex) of each of `ends` whose gap is more than half the vertex's tolerance.
fn uncovered(ends: &[(u32, VertexId, f64, f64)]) -> Vec<(u32, u32)> {
    ends.iter()
        .filter(|(_, _, gap, tol)| *gap > 0.5 * tol)
        .map(|(e, v, _, _)| (*e, v.index()))
        .collect()
}

/// One run of `case`: the encoded row, the raw `out`, the steps and the result body
/// (a `MoveSeam` row observes one).
fn run(u: &dyn UnitDiff, case: &Case, step: Step) -> (Value, Vec<f64>, Vec<VertexStep>, Vec<Body>) {
    let steps = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&steps);
    let seam = VertexStepSeam::new(Box::new(move |s: VertexStep| {
        seen.lock().unwrap().push(s);
        match step {
            Step::Own => s.own,
            Step::Without => s.before,
        }
    }));
    let bodies = Rc::new(RefCell::new(Vec::new()));
    let into = Rc::clone(&bodies);
    SEAM.with(|s| *s.borrow_mut() = Some(seam));
    let out = crate::trim_hook::with_hook(
        Box::new(move |b: &mut Brep| into.borrow_mut().push(body(b))),
        || u.run_rust(case),
    );
    SEAM.with(|s| *s.borrow_mut() = None);
    let out = out.unwrap_or_else(|e| panic!("{}: {e:?}", case.name));
    let raw = out.values.get("out").cloned().unwrap_or_default();
    let steps = steps.lock().unwrap().clone();
    let bodies = bodies.take();
    (encode(&case.name, &out), raw, steps, bodies)
}

/// The [`Body`] of `b`.
fn body(b: &mut Brep) -> Body {
    use solidcore::topology::brep::validation::{check_brep_with_keys, CheckLevel};
    let edgeuses: BTreeSet<u32> = b.edgeuses().iter().map(|(eu, _)| eu.index()).collect();
    let ends = ends(b);
    // each geometry's owner as the geometry records it (an edge use without a trim: the
    // identity the body names it by), as `validation::soundness` checks a body: the owner
    // tests cannot fail, so what is checked is the body's geometry and topology
    let mut owners = BTreeMap::new();
    for (id, e) in b.edges().iter() {
        if let Some(o) = e.curve.as_ref().and_then(Curve::owner) {
            owners.insert(EntityRef::Edge(id), o);
        }
    }
    for (id, f) in b.faces().iter() {
        if let Some(o) = f.surface.as_ref().and_then(Surface::owner) {
            owners.insert(EntityRef::Face(id), o);
        }
    }
    for (id, u) in b.edgeuses().iter() {
        let o = match u.uv_trim_curve.as_ref() {
            Some(trim) => trim.owner(),
            None => Some(b.owner_identity(EntityRef::Edgeuse(id))),
        };
        if let Some(o) = o {
            owners.insert(EntityRef::Edgeuse(id), o);
        }
    }
    let keys = |e: EntityRef| {
        owners.get(&e).copied().unwrap_or(ObjectRef {
            type_code: 0,
            key: ObjectKey(0),
        })
    };
    let mut k = Kernel::default();
    let report = check_brep_with_keys(&mut k, b, CheckLevel::Level2, &keys).unwrap();
    Body {
        edgeuses,
        valid: report.is_valid(),
        level2: report.lines(),
        ends,
    }
}

/// Per edge end of `b`: (edge, vertex, the distance from the edge curve's end to the
/// vertex, the vertex's tolerance).
fn ends(b: &Brep) -> Vec<(u32, VertexId, f64, f64)> {
    let ek = Kernel::default();
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
            let gap = c.evaluate_point(&ek, at).unwrap().distance(vr.point);
            ends.push((e.index(), v, gap, vr.tolerance));
        }
    }
    ends
}

fn cases(u: &dyn UnitDiff) -> BTreeMap<String, Case> {
    let ctx = CaseContext {
        root: repo_root(),
        seeds: SEEDS,
    };
    u.cases(&ctx)
        .unwrap()
        .into_iter()
        .map(|c| (c.name.clone(), c))
        .collect()
}

fn unit() -> Box<dyn UnitDiff> {
    crate::registry()
        .into_iter()
        .find(|u| u.id() == UNIT)
        .unwrap_or_else(|| panic!("no unit {UNIT}"))
}

/// Without the step, the rows of the unit that do not agree with their committed line (as
/// the goldens compare) are exactly [`MOVED`], and each is then main's line bit for bit.
#[test]
fn without_the_step_only_the_moved_rows_differ_and_they_are_mains() {
    let u = unit();
    let golden = read(&default_dir(), UNIT).unwrap();
    let mut moved = Vec::new();
    for (name, case) in cases(u.as_ref()) {
        let (row, _, _, _) = run(u.as_ref(), &case, Step::Without);
        let line = golden
            .get(&name)
            .unwrap_or_else(|| panic!("{name}: no committed line"));
        // the goldens' own comparison (a committed line may lag the run within it)
        if crate::golden::diff(line, &row, u.abs_floors()).is_some() {
            assert_eq!(
                row,
                main_line(&name),
                "{name}: without the step the row is neither its committed line nor main's"
            );
            moved.push(name);
        }
    }
    assert_eq!(moved, MOVED.map(String::from).to_vec());
}

/// On each moved row: the step grew a vertex, and every edge end at a vertex it grew lies
/// within half that vertex's tolerance (the property WR-469 exists for); the records differ
/// only in the UV trim tally; and level 2 fails on both bodies.
#[test]
fn the_moved_rows_cover_every_end_at_a_grown_vertex_and_fail_level_2_both_ways() {
    let u = unit();
    let golden = read(&default_dir(), UNIT).unwrap();
    let cases = cases(u.as_ref());
    let mut not_committed = Vec::new();
    let mut totals = (0, 0);
    for name in MOVED {
        let case = cases.get(name).unwrap_or_else(|| panic!("no case {name}"));
        let (row, out, steps, bodies) = run(u.as_ref(), case, Step::Own);
        if golden.get(name) != Some(&row) {
            not_committed.push(name);
        }
        let (row0, out0, steps0, bodies0) = run(u.as_ref(), case, Step::Without);
        assert_eq!(
            row0,
            main_line(name),
            "{name}: without the step, not main's line"
        );

        // the runs reach the same steps (vertex, tolerance before, gap), in the same order,
        // up to the first that grows a tolerance: until then the bodies are the same
        let key = |s: &VertexStep| (s.vertex, s.before.to_bits(), s.gap.to_bits());
        let first = steps
            .iter()
            .position(|s| s.own > s.before)
            .unwrap_or_else(|| panic!("{name}: the step grew no vertex: {steps:?}"));
        assert!(
            steps0.len() > first,
            "{name}: without the step the run stops before step {first}"
        );
        assert_eq!(
            steps[..=first].iter().map(key).collect::<Vec<_>>(),
            steps0[..=first].iter().map(key).collect::<Vec<_>>(),
            "{name}: the runs reach different steps before the first growth"
        );

        // the raw records differ only in the UV trim tally (`n own`)
        assert_eq!(out.len(), out0.len(), "{name}: record lengths");
        let diff: Vec<usize> = (0..out.len())
            .filter(|&i| out[i].to_bits() != out0[i].to_bits())
            .collect();
        assert_eq!(diff, vec![24, 25], "{name}: {out:?} against {out0:?}");
        assert_eq!(
            (out[24], out[25], out0[24], out0[25]),
            (2.0, 2.0, 15.0, 15.0),
            "{name}: UV trim tally"
        );

        assert_eq!(
            (bodies.len(), bodies0.len()),
            (1, 1),
            "{name}: bodies observed"
        );
        let (b, b0) = (&bodies[0], &bodies0[0]);
        assert_eq!(
            b.edgeuses, b0.edgeuses,
            "{name}: the bodies' edge uses differ"
        );
        assert!(!b.ends.is_empty(), "{name}: no edge ends");

        // (A) every end at a vertex the step grew lies within half its tolerance
        let grown: BTreeSet<VertexId> = steps
            .iter()
            .filter(|s| s.own > s.before)
            .map(|s| s.vertex)
            .collect();
        let at_grown: Vec<_> = b.ends.iter().filter(|e| grown.contains(&e.1)).collect();
        assert!(
            !at_grown.is_empty(),
            "{name}: no edge end at a grown vertex"
        );
        let bad: Vec<_> = at_grown.iter().filter(|e| e.2 > 0.5 * e.3).collect();
        assert!(
            bad.is_empty(),
            "{name}: edge ends (edge, vertex, gap, tolerance) at a grown vertex {grown:?} not \
             covered: {bad:?}"
        );
        // (B) as the operation leaves the body, the step leaves uncovered exactly the ends
        // at the vertices the edge split made (v6, v7: the point merge sizes their
        // tolerance as its zone neighbourhood, not from the edge ends; tangency-labs/solidcraft#21340), fewer than
        // without it
        assert_eq!(
            b.uncovered(),
            UNCOVERED_AT_CAPTURE.to_vec(),
            "{name}: uncovered (edge, vertex) with the step"
        );
        totals.0 += b.uncovered().len();
        totals.1 += b0.uncovered().len();
        assert!(
            b.uncovered().len() < b0.uncovered().len(),
            "{name}: uncovered with the step {:?}, without {:?}",
            b.uncovered(),
            b0.uncovered()
        );

        for (body, run) in [(b, "with"), (b0, "without")] {
            assert!(
                !body.valid && body.level2.iter().any(|l| l.contains("ValidatePointers")),
                "{name}: level 2 {run} the step does not fail ValidatePointers: {:?}",
                body.level2.iter().take(8).collect::<Vec<_>>()
            );
        }
    }
    assert_eq!(
        totals,
        (4 * MOVED.len(), 6 * MOVED.len()),
        "uncovered ends over the moved rows (with the step, without)"
    );
    assert!(
        not_committed.is_empty(),
        "rows that are not their committed line: {not_committed:?}"
    );
}
