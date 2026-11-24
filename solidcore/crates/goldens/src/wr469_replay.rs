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
//! the vertex already has, which leaves it as the pinned C++ did. Since the point merge
//! covers its edge ends (tangency-labs/solidcraft#21340), the step has nothing left to grow
//! on these rows:
//! 1. no row of the unit then differs from its committed line (the goldens' own
//!    comparison): the step moves no row. Before #21340 it moved exactly [`MOVED`] (37),
//!    through the UV trim tally (`out[24..26]`, 15 without the step, 2 with it).
//! 2. On each of the 37, run as is and without the step: the encoded row is the committed
//!    line both ways and the raw records are the same bit for bit; both runs reach the same
//!    steps (vertex, tolerance before, gap) and none grows a tolerance; the bodies have the
//!    same edge uses; every edge's curve end lies within half its vertex's tolerance both
//!    ways (`AssertValid`'s test of an edge end; before #21340 the four ends at the split's
//!    own vertices v6 and v7, sized by the point merge's zone neighbourhood, were not, with
//!    the step or without it); and `check_brep` level 2 still fails `ValidatePointers` on
//!    both bodies, each geometry's owner taken as the geometry records it (as
//!    `validation::soundness` checks a body), so the failure is the body's own and not the
//!    runner's naming of it. The rows are invalid-input crash reproducers.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use serde_json::Value;
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

/// The rows the step moved before tangency-labs/solidcraft#21340: `MoveSeam` with split on
/// `vase_rev.smb` face 0, and one plain `vase_rev.smb` row.
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

/// Without the step, every row of the unit agrees with its committed line (as the goldens
/// compare): the step moves no row.
#[test]
fn without_the_step_no_row_of_the_unit_moves() {
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
            moved.push(name);
        }
    }
    assert!(moved.is_empty(), "rows the step moves: {moved:?}");
}

/// On each row the step moved before #21340: the step grows no vertex and changes nothing,
/// every edge end lies within half its vertex's tolerance both ways, and level 2 fails on
/// both bodies.
#[test]
fn the_moved_rows_take_no_step_cover_every_end_and_fail_level_2_both_ways() {
    let u = unit();
    let golden = read(&default_dir(), UNIT).unwrap();
    let cases = cases(u.as_ref());
    let mut not_committed = Vec::new();
    for name in MOVED {
        let case = cases.get(name).unwrap_or_else(|| panic!("no case {name}"));
        let (row, out, steps, bodies) = run(u.as_ref(), case, Step::Own);
        if golden.get(name) != Some(&row) {
            not_committed.push(name);
        }
        let (row0, out0, steps0, bodies0) = run(u.as_ref(), case, Step::Without);
        assert_eq!(row0, row, "{name}: the step moves the row");
        let bits = |xs: &[f64]| xs.iter().map(|x| x.to_bits()).collect::<Vec<_>>();
        assert_eq!(bits(&out), bits(&out0), "{name}: the step moves the record");

        // both runs reach the same steps (vertex, tolerance before, gap), and none grows
        let key = |s: &VertexStep| (s.vertex, s.before.to_bits(), s.gap.to_bits());
        assert!(!steps.is_empty(), "{name}: the run reaches no step");
        assert_eq!(
            steps.iter().map(key).collect::<Vec<_>>(),
            steps0.iter().map(key).collect::<Vec<_>>(),
            "{name}: the runs reach different steps"
        );
        let grown: Vec<_> = steps.iter().filter(|s| s.own > s.before).collect();
        assert!(grown.is_empty(), "{name}: the step grew {grown:?}");

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
        for (body, run) in [(b, "with"), (b0, "without")] {
            assert!(!body.ends.is_empty(), "{name}: no edge ends {run} the step");
            // every end within half its vertex's tolerance, as the operation leaves the body
            assert_eq!(
                body.uncovered(),
                Vec::<(u32, u32)>::new(),
                "{name}: uncovered (edge, vertex) {run} the step"
            );
            assert!(
                !body.valid && body.level2.iter().any(|l| l.contains("ValidatePointers")),
                "{name}: level 2 {run} the step does not fail ValidatePointers: {:?}",
                body.level2.iter().take(8).collect::<Vec<_>>()
            );
        }
    }
    assert!(
        not_committed.is_empty(),
        "rows that are not their committed line: {not_committed:?}"
    );
}
