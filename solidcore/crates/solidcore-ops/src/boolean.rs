//! Booleans: two bodies' union, difference or intersection (`SmApiBoolean`), a list's union
//! level by level, the boolean of planar sheets on one plane (`SmApiBoolean2d`), and imprinting
//! one body's edges on another (`SmApiBooleanWithOptions(SM_BO_IMPRINT)`).
//!
//! Each takes its operands by value, as the kernel does: it edits the first and consumes the
//! second. An application that keeps its operands copies them first ([`Body::copy`]). While a
//! failure callback is installed, the operands are written before the call (`run_taking`), so a
//! failure reports them as they came in.
//!
//! **What a solid's result must be** is the caller's choice ([`Contract`], every family's). By default a
//! boolean tests its operands and its result for closure only ([`Contract::Closed`]): solid
//! operands (closed, manifold) must give a closed result, and nothing is proved, so a boolean
//! costs the boolean. [`Contract::Proved`] (the `_with` calls) also proves each operand and
//! the result valid and facing out, refuses a result it cannot prove, and keeps the proof on
//! the result for the next proved operation that takes it (#14830).

use solidcore::boolean::apart::{union_if_apart, Apart};
use solidcore::boolean::api_brep::{self as sm, Boolean2dOutcome};
use solidcore::boolean::merge::MergeOptions;
use solidcore::boolean::operand_tolerances::matched_tolerances;
use solidcore::error::boolean::BooleanFailure;
use solidcore::error::topology::TopologyFailure;
use solidcore::error::Reason;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::primitive::primitive_creation::p1::Boolean2d;
use solidcore::topology::api_files::rebuild_native;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::validation::update_tolerances_after_boolean;
use solidcore::Result as KernelResult;
use solidcore::{Brep, SolidError};

use crate::body::{finish_under, Body, Contract, Expect, Outward, Proof};
use crate::measure::{locate_points, Located};
use crate::session::{read_locked, run_on, run_taking};
use crate::{Error, Result};

/// Which boolean.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BooleanOp {
    /// Everything in either.
    Union,
    /// The first less the second.
    Subtract,
    /// What is in both.
    Intersect,
}

/// What a call gave before it is a result.
enum Done {
    /// The result, with the proof [`finish_proved`] made on it.
    Body(Box<Brep>, Option<Outward>),
    Empty,
    Refused(Error),
}

impl crate::session::Answer for Done {
    fn refusal(&self) -> Option<Error> {
        match self {
            Done::Refused(e) => Some(e.clone()),
            _ => None,
        }
    }
}

/// The result contract of a boolean under `contract` ([`finish_under`]), solids touching along
/// an edge or at a vertex allowed; under [`Contract::Proved`] the result keeps the proof its
/// solid test made, for the next operation that takes it as an operand.
fn finished(k: &mut Kernel, r: Brep, expect: Expect, contract: Contract) -> KernelResult<Done> {
    Ok(match finish_under(k, r, expect.touching(), contract)? {
        Ok((b, outward)) => Done::Body(Box::new(b), outward),
        Err(Error::Empty) => Done::Empty,
        Err(e) => Done::Refused(e),
    })
}

/// The tolerance update a boolean's result gets before the result contract checks it
/// ([`update_tolerances_after_boolean`]): a vertex a grazing cut left wider than the check
/// allows is centred on its edges' ends, then marginal edge/vertex, vertex/face and edge/face
/// gaps are covered by growing tolerances. Only a vertex wider than the check allows moves;
/// elsewhere tolerances only grow. Without it, the validity check of [`finished`] refuses a
/// sound result for a sub-micron gap. The update can itself fail with the kernel's error, which
/// then comes back in place of the [`Error::NotSolidBecause`] the check would have given.
fn after_boolean(k: &mut Kernel, r: &mut Brep) -> KernelResult<()> {
    update_tolerances_after_boolean(k, r).map(|_| ())
}

/// What the operands make a boolean's result: a solid when every operand is one under
/// `contract` ([`Expect::under`]). Each comes with the proof its body held
/// ([`Body::take_held`]), which [`Contract::Proved`] takes where it stands.
fn expect_of(
    k: &mut Kernel,
    operands: &mut [(Brep, Proof)],
    contract: Contract,
) -> KernelResult<Expect> {
    if contract == Contract::Proved && operands.len() >= 2 {
        let taken: Vec<(Brep, Proof)> = operands
            .iter_mut()
            .map(|(b, p)| (std::mem::take(b), std::mem::take(p)))
            .collect();
        let checked = crate::session::run_parallel(k, taken, |wk, (mut b, proof)| {
            let is_solid = Expect::under(wk, &mut b, proof, contract)? != Expect::FACES;
            Ok((b, is_solid))
        })?;
        let mut all = true;
        for ((dst_b, _), (b, is_solid)) in operands.iter_mut().zip(checked) {
            *dst_b = b;
            all &= is_solid;
        }
        return Ok(if all { Expect::SOLID } else { Expect::FACES });
    }
    let mut all = true;
    for (b, proof) in operands.iter_mut() {
        all &= Expect::under(k, b, std::mem::take(proof), contract)? != Expect::FACES;
    }
    Ok(if all { Expect::SOLID } else { Expect::FACES })
}

/// What a failure report calls the first operands.
const OPERANDS: [&str; 4] = ["a", "b", "c", "d"];

fn operands(bodies: &[Body]) -> Vec<(&'static str, &Body)> {
    bodies
        .iter()
        .enumerate()
        .map(|(n, b)| (OPERANDS.get(n).copied().unwrap_or("operand"), b))
        .collect()
}

/// Fails when a panic left an operand half-edited: it is never used again.
fn usable(call: &'static str, bodies: &[Body]) -> Result<()> {
    if bodies.iter().any(Body::is_poisoned) {
        return Err(Error::Internal(format!(
            "{call}: the body was left unfinished by an earlier panic"
        )));
    }
    Ok(())
}

/// The kernel's body taken out of `b`, leaving it empty, with the proof or template it held.
fn take(b: &Body) -> (Brep, Proof) {
    b.take_held()
}

fn operation(op: BooleanOp) -> BooleanOperation {
    match op {
        BooleanOp::Union => BooleanOperation::Union,
        BooleanOp::Subtract => BooleanOperation::Difference,
        BooleanOp::Intersect => BooleanOperation::Intersection,
    }
}

fn result(d: Done, empty: Result<Option<Body>>) -> Result<Option<Body>> {
    match d {
        Done::Body(b, outward) => Ok(Some(Body::held(*b, outward))),
        Done::Empty => empty,
        Done::Refused(e) => Err(e),
    }
}

/// `a` op `b` (`SmApiBoolean`) under [`Contract::Closed`]: `None` when the result is empty,
/// [`Error::NotSolid`] when solid operands give a body that is not closed (solids touching
/// along an edge or at a vertex are closed). An empty difference or intersection of solids is
/// [`Error::EmptyRefuted`] where a vertex of an operand shows the result holds material.
///
/// # Errors
/// [`Error::NotSolid`]; [`Error::EmptyRefuted`]; the kernel's error.
pub fn boolean(a: Body, b: Body, op: BooleanOp) -> Result<Option<Body>> {
    boolean_with(a, b, op, Contract::Closed)
}

/// [`boolean`] under `contract`.
///
/// # Errors
/// As [`boolean`]; under [`Contract::Proved`], [`Error::NotSolidBecause`] too for a result of
/// proved solids that is not proved valid and facing out.
pub fn boolean_with(a: Body, b: Body, op: BooleanOp, contract: Contract) -> Result<Option<Body>> {
    let both = [a, b];
    usable("SmApiBoolean", &both)?;
    let d = run_taking("SmApiBoolean", &operands(&both), |k| {
        let [a, b] = &both;
        let mut operands = [take(a), take(b)];
        let expect = expect_of(k, &mut operands, contract)?;
        let [(mut ca, _), (mut cb, _)] = operands;
        matched_tolerances(k, &mut ca, &mut cb)?;
        let witnesses = Witnesses::of(k, &ca, &cb, op, expect)?;
        let mut r = if op == BooleanOp::Union {
            sm::boolean_union_constrained(k, ca, cb)?
        } else {
            sm::boolean(k, ca, cb, operation(op))?.result
        };
        after_boolean(k, &mut r)?;
        Ok(match finished(k, r, expect, contract)? {
            Done::Empty => witnesses.refute(k),
            d => d,
        })
    })?;
    result(d, Ok(None))
}

/// Union copies of two bodies under [`Contract::Closed`]. For a failed radial search at a
/// shared edge between solid operands, retry in reverse order and, if needed, with rebuilt native
/// operands. The inputs and the attributes that carry their provenance are kept.
pub fn union_copies(a: &Body, b: &Body) -> Result<Option<Body>> {
    let inputs = [("a", a), ("b", b)];
    if inputs.iter().any(|(_, body)| body.is_poisoned()) {
        return Err(Error::Internal(
            "SmApiBoolean: the body was left unfinished by an earlier panic".into(),
        ));
    }
    let d = run_on("SmApiBoolean", &inputs, |k| {
        let ca = read_locked(a, |a| brep_copy(k, a))?;
        let cb = read_locked(b, |b| brep_copy(k, b))?;
        let mut operands = [(ca, Proof::default()), (cb, Proof::default())];
        let expect = expect_of(k, &mut operands, Contract::Closed)?;
        let [(ca, _), (cb, _)] = operands;
        let first = union_pair(k, ca, cb, expect, Contract::Closed);
        let Err(original) = first else { return first };
        if !expect.solid() || !radial_sector_failure(&original) {
            return Err(original);
        }

        // The merge copies faces into its first operand. Its radial search can succeed in
        // the other direction at the same shared edge.
        let cb = read_locked(b, |b| brep_copy(k, b))?;
        let ca = read_locked(a, |a| brep_copy(k, a))?;
        let reverse = union_pair(k, cb, ca, expect, Contract::Proved);
        if matches!(
            reverse,
            Ok(Done::Body(..)) | Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ) || reverse
            .as_ref()
            .is_err_and(|e| e.root_cause().is_not_certified())
        {
            return reverse;
        }
        let constrained_forward = (|| {
            let ca = read_locked(a, |a| brep_copy(k, a))?;
            let cb = read_locked(b, |b| brep_copy(k, b))?;
            union_pair_constrained(k, ca, cb, expect, Contract::Proved)
        })();
        if matches!(
            constrained_forward,
            Ok(Done::Body(..)) | Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ) || constrained_forward
            .as_ref()
            .is_err_and(|e| e.root_cause().is_not_certified())
        {
            return constrained_forward;
        }
        let rebuilt_reverse = (|| {
            let cb = read_locked(b, |b| rebuild_native(k, b))?;
            let ca = read_locked(a, |a| rebuild_native(k, a))?;
            union_pair_constrained(k, cb, ca, expect, Contract::Proved)
        })();
        if matches!(
            rebuilt_reverse,
            Ok(Done::Body(..)) | Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
        ) || rebuilt_reverse
            .as_ref()
            .is_err_and(|e| e.root_cause().is_not_certified())
        {
            return rebuilt_reverse;
        }
        let rebuilt_forward = (|| {
            let ca = read_locked(a, |a| rebuild_native(k, a))?;
            let cb = read_locked(b, |b| rebuild_native(k, b))?;
            union_pair_constrained(k, ca, cb, expect, Contract::Proved)
        })();
        match rebuilt_forward {
            done @ (Ok(Done::Body(..))
            | Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })) => done,
            Err(e) if e.root_cause().is_not_certified() => Err(e),
            _ => Err(original),
        }
    })?;
    result(d, Ok(None))
}

fn radial_sector_failure(error: &SolidError) -> bool {
    error.failure().is_some_and(|failure| {
        failure.operation == "boolean::merge::p2::make_faceuse"
            && matches!(
                failure.reason,
                Reason::Boolean(BooleanFailure::NoRadialEdgeUse)
            )
            && error.root_cause().failure().is_some_and(|cause| {
                matches!(
                    cause.reason,
                    Reason::Topology(TopologyFailure::NoRadialCandidate)
                )
            })
    })
}

fn union_pair(
    k: &mut Kernel,
    mut a: Brep,
    mut b: Brep,
    expect: Expect,
    contract: Contract,
) -> KernelResult<Done> {
    matched_tolerances(k, &mut a, &mut b)?;
    let mut r = sm::boolean_union(k, a, b)?;
    after_boolean(k, &mut r)?;
    finished(k, r, expect, contract)
}

fn union_pair_constrained(
    k: &mut Kernel,
    mut a: Brep,
    mut b: Brep,
    expect: Expect,
    contract: Contract,
) -> KernelResult<Done> {
    matched_tolerances(k, &mut a, &mut b)?;
    let mut r = sm::boolean_union_constrained(k, a, b)?;
    after_boolean(k, &mut r)?;
    finished(k, r, expect, contract)
}

/// Intersect copies of two bodies under [`Contract::Closed`]. For a failed radial search at a
/// shared edge between solid operands, retry in reverse order and, if needed, with rebuilt native
/// operands in both orders. The inputs and the attributes that carry their provenance are kept.
pub fn intersect_copies(a: &Body, b: &Body) -> Result<Option<Body>> {
    let inputs = [("a", a), ("b", b)];
    if inputs.iter().any(|(_, body)| body.is_poisoned()) {
        return Err(Error::Internal(
            "SmApiBoolean: the body was left unfinished by an earlier panic".into(),
        ));
    }
    let d = run_on("SmApiBoolean", &inputs, |k| {
        let ca = read_locked(a, |a| brep_copy(k, a))?;
        let cb = read_locked(b, |b| brep_copy(k, b))?;
        let mut operands = [(ca, Proof::default()), (cb, Proof::default())];
        let expect = expect_of(k, &mut operands, Contract::Closed)?;
        let [(ca, _), (cb, _)] = operands;
        let first = intersect_pair(k, ca, cb, expect, Contract::Closed);
        let Err(original) = first else { return first };
        if !expect.solid() || !radial_sector_failure(&original) {
            return Err(original);
        }

        // The merge copies faces into its first operand. Its radial search can succeed in
        // the other direction at the same shared edge, or after rebuilding native radial links.
        let cb = read_locked(b, |b| brep_copy(k, b))?;
        let ca = read_locked(a, |a| brep_copy(k, a))?;
        let reverse = intersect_pair(k, cb, ca, expect, Contract::Proved);
        if is_settled_intersect(&reverse) {
            return reverse;
        }
        let rebuilt_ab = (|| {
            let ca = read_locked(a, |a| rebuild_native(k, a))?;
            let cb = read_locked(b, |b| rebuild_native(k, b))?;
            intersect_pair(k, ca, cb, expect, Contract::Proved)
        })();
        if is_settled_intersect(&rebuilt_ab) {
            return rebuilt_ab;
        }
        let rebuilt_ba = (|| {
            let cb = read_locked(b, |b| rebuild_native(k, b))?;
            let ca = read_locked(a, |a| rebuild_native(k, a))?;
            intersect_pair(k, cb, ca, expect, Contract::Proved)
        })();
        if is_settled_intersect(&rebuilt_ba) {
            return rebuilt_ba;
        }
        Err(original)
    })?;
    result(d, Ok(None))
}

fn is_settled_intersect(outcome: &KernelResult<Done>) -> bool {
    matches!(
        outcome,
        Ok(Done::Body(..) | Done::Empty)
            | Err(SolidError::Cancelled { .. } | SolidError::OutOfFuel { .. })
    ) || outcome
        .as_ref()
        .is_err_and(|e| e.root_cause().is_not_certified())
}

fn intersect_pair(
    k: &mut Kernel,
    mut a: Brep,
    mut b: Brep,
    expect: Expect,
    contract: Contract,
) -> KernelResult<Done> {
    matched_tolerances(k, &mut a, &mut b)?;
    let witnesses = Witnesses::of(k, &a, &b, BooleanOp::Intersect, expect)?;
    let mut r = sm::boolean_intersection(k, a, b)?;
    after_boolean(k, &mut r)?;
    Ok(match finished(k, r, expect, contract)? {
        Done::Empty => witnesses.refute(k),
        d => d,
    })
}

/// Each operand's vertices with a copy of the other operand, which the boolean consumes, and
/// the side of it (material or void) on which a vertex proves the result is not empty.
struct Witnesses(Vec<(Vec<[f64; 3]>, Brep, bool)>);

impl Witnesses {
    fn of(k: &mut Kernel, a: &Brep, b: &Brep, op: BooleanOp, expect: Expect) -> KernelResult<Self> {
        if !expect.solid() {
            return Ok(Self(Vec::new()));
        }
        let pairs: &[(&Brep, &Brep, bool)] = match op {
            BooleanOp::Union => &[],
            BooleanOp::Subtract => &[(a, b, false)],
            BooleanOp::Intersect => &[(a, b, true), (b, a, true)],
        };
        let mut held = Vec::new();
        for &(from, against, material) in pairs {
            held.push((vertices(from)?, brep_copy(k, against)?, material));
        }
        Ok(Self(held))
    }

    fn refute(self, k: &Kernel) -> Done {
        for (points, against, material) in &self.0 {
            let located = locate_points(k, against, points);
            let on_its_side = |l: &std::result::Result<Located, _>| matches!(l, Ok(Located::InRegion { material: Some(m), .. }) if m == material);
            if let Some((&at, _)) = points.iter().zip(located).find(|(_, l)| on_its_side(l)) {
                return Done::Refused(Error::EmptyRefuted { at });
            }
        }
        Done::Empty
    }
}

fn vertices(b: &Brep) -> KernelResult<Vec<[f64; 3]>> {
    b.vertex_list()
        .iter()
        .map(|&v| b.vertex(v).map(|v| [v.point.x, v.point.y, v.point.z]))
        .collect()
}

/// The union of `bodies` under [`Contract::Closed`], with solids touching along edges or at
/// vertices allowed. Certified-disjoint components and hub-and-spoke spoke groups are merged side
/// by side in linear time ([`sm::combine_disjoint_breps`]) without intersection work, while
/// overlapping components pair spatially in 3D Morton order ([`sm::union_all_breps`]): a plate and
/// 25 pegs that each stand in it but meet no other peg batch all 25 pegs side by side first and
/// take a single boolean against the plate instead of 5 (#21438). Each union result gets
/// [`after_boolean`]'s tolerance update, as [`boolean`] does, so a later level unions a
/// repaired operand and the result passes the same check a chain of single unions does. A
/// single body makes no union and is untouched.
///
/// # Errors
/// [`Error::Argument`] for no body; [`Error::Empty`] for an empty result; [`Error::NotSolid`];
/// the kernel's error.
pub fn union_all(bodies: Vec<Body>) -> Result<Body> {
    union_all_with(bodies, Contract::Closed)
}

/// [`union_all`] under `contract`. Each pairwise result gets [`after_boolean`]'s update as
/// it is made, so a later level unions repaired operands.
///
/// # Errors
/// As [`union_all`]; under [`Contract::Proved`], [`Error::NotSolidBecause`] too for a result of
/// proved solids that is not proved valid and facing out.
// The operands are consumed: their bodies are taken out under the kernel's lock.
#[allow(clippy::needless_pass_by_value)]
pub fn union_all_with(bodies: Vec<Body>, contract: Contract) -> Result<Body> {
    if bodies.is_empty() {
        return Err(Error::Argument);
    }
    usable("union_all", &bodies)?;
    let d = run_taking("union_all", &operands(&bodies), |k| {
        let mut held: Vec<(Brep, Proof)> = bodies.iter().map(take).collect();
        let expect = expect_of(k, &mut held, contract)?;
        let breps: Vec<Brep> = held.into_iter().map(|(b, _)| b).collect();
        match sm::union_all_breps_with(k, breps, |kernel, pairs| {
            crate::session::run_parallel(kernel, pairs, |wk, (a, b)| union_step(wk, a, b))
        })? {
            Some(r) => finished(k, r, expect, contract),
            None => Ok(Done::Empty),
        }
    })?;
    result(d, Err(Error::Empty))?.ok_or(Error::Empty)
}

/// One pairwise union step for [`union_all_with`]: bodies proved apart unite side by side, while
/// any other pair is `SmApiBoolean`'s union. Either result gets [`after_boolean`]'s update, so
/// both routes leave the same tolerances for the next level.
fn union_step(k: &mut Kernel, mut a: Brep, mut b: Brep) -> KernelResult<Brep> {
    matched_tolerances(k, &mut a, &mut b)?;
    let mut r = match union_if_apart(k, a, b)? {
        Apart::United(r) => *r,
        Apart::NotProved(a, b) => sm::boolean(k, *a, *b, BooleanOperation::Union)?.result,
    };
    after_boolean(k, &mut r)?;
    Ok(r)
}

/// `a` op `b` for sheets of planar faces on one plane (`SmApiBoolean2d`): `None` when either has
/// no face (where the kernel succeeds without a result) or the result is empty.
///
/// # Errors
/// The kernel's error.
pub fn boolean2d(a: Body, b: Body, op: BooleanOp) -> Result<Option<Body>> {
    let both = [a, b];
    usable("SmApiBoolean2d", &both)?;
    let d = run_taking("SmApiBoolean2d", &operands(&both), |k| {
        let [a, b] = &both;
        let ((ca, _), (cb, _)) = (take(a), take(b));
        if ca.face_list().is_empty() || cb.face_list().is_empty() {
            return Ok(Done::Empty);
        }
        let kind = match op {
            BooleanOp::Union => Boolean2d::Union,
            BooleanOp::Subtract => Boolean2d::Difference,
            BooleanOp::Intersect => Boolean2d::Intersection,
        };
        match sm::boolean_2d(k, ca, cb, kind)? {
            Boolean2dOutcome::Result(r) => finished(k, r, Expect::FACES, Contract::Closed),
            Boolean2dOutcome::Unassigned { .. } => Ok(Done::Empty),
        }
    })?;
    result(d, Ok(None))
}

/// The topology counts an imprint can change: faces, edges, vertices.
fn counts(b: &Brep) -> [usize; 3] {
    [
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
    ]
}

/// `tool`'s edges imprinted on `body` (`SmApiBooleanWithOptions(SM_BO_IMPRINT)`: cookie cutter
/// off, imprinting on, post-process off, imprint-and-classify off, keep other on) under
/// [`Contract::Closed`]: the result needs only faces left, and the imprint changed `body` when
/// its face count moved (a face cut). When it changed nothing the body comes back as the
/// kernel left it, never dropped.
///
/// # Errors
/// [`Error::Empty`] for an empty result; the kernel's error.
pub fn imprint(body: Body, tool: Body) -> Result<(Body, bool)> {
    imprint_with(body, tool, Contract::Closed)
}

/// [`imprint`] under `contract`. Under [`Contract::Proved`] the result has the result contract
/// ([`finish_proved`]: a proved solid in gives a proved solid out), and the imprint changed
/// `body` when its face, edge or vertex count moved (a face cut, an edge split).
///
/// # Errors
/// As [`imprint`]; under [`Contract::Proved`], [`Error::NotSolidBecause`] when a solid body
/// comes out not proved a solid.
pub fn imprint_with(body: Body, tool: Body, contract: Contract) -> Result<(Body, bool)> {
    let both = [body, tool];
    usable("SmApiBooleanWithOptions", &both)?;
    let mut before = [0; 3];
    let d = run_taking("SmApiBooleanWithOptions", &operands(&both), |k| {
        let [body, tool] = &both;
        let ((mut a, proof), (mut t, _)) = (take(body), take(tool));
        before = counts(&a);
        // an imprint under the default contract needs only faces left of its result
        let expect = match contract {
            Contract::Closed => Expect::FACES,
            Contract::Proved => Expect::under(k, &mut a, proof, contract)?,
        };
        matched_tolerances(k, &mut a, &mut t)?;
        let options = MergeOptions::new(false, true, false, false, true);
        let r = sm::boolean_with_options(k, a, t, BooleanOperation::Imprint, options)?.result;
        finished(k, r, expect, contract)
    })?;
    match d {
        Done::Body(b, outward) => {
            let after = counts(&b);
            let changed = match contract {
                Contract::Closed => after[0] != before[0],
                Contract::Proved => after != before,
            };
            Ok((Body::held(*b, outward), changed))
        }
        Done::Empty => Err(Error::Empty),
        Done::Refused(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use super::*;
    use crate::session::{self, set_failure_hook, Failure};
    use crate::{make_box, make_cylinder, CheckLevel};
    use solidcore_record::brep::Brep as Record;

    fn all_fixes() {
        session::set_thread_fixes(solidcore::kernel::UpstreamFixes::NAMES.iter().copied()).unwrap();
    }

    #[test]
    fn sheets_with_no_face_give_no_result() {
        all_fixes();
        let a = Body::new(Brep::new());
        let b = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        assert!(boolean2d(a, b, BooleanOp::Union).unwrap().is_none());
    }

    /// Two disjoint boxes intersect to no result (`Ok(None)`), not an error, with the tolerance
    /// update in place: the empty result has no vertex, so there is no gap to close, and the
    /// empty contract holds through the new call.
    #[test]
    fn disjoint_boxes_intersect_to_no_result_with_the_update_in_place() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let b = make_box([3.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        assert!(boolean(a, b, BooleanOp::Intersect).unwrap().is_none());
    }

    /// Disjoint solids unioned through `union_all` with the tolerance update in place: the body
    /// holds both boxes, its volume is the closed form, and its proof stands.
    #[test]
    fn disjoint_boxes_union_all_measures_the_closed_form_with_the_update_in_place() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let b = make_box([3.0, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap();
        let u = union_all(vec![a, b]).unwrap();
        let v = u.volume(1e-8).unwrap();
        assert!((v - 2.0).abs() < 2.0 * 1e-9, "{v}");
    }

    /// Overlapping boxes unioned through `union_all` with the tolerance update in place: the
    /// shared region is not double-counted, so the update did not bend the geometry.
    #[test]
    fn overlapping_boxes_union_all_measures_the_closed_form_with_the_update_in_place() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap();
        let b = make_box([1.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap();
        let u = union_all(vec![a, b]).unwrap();
        let v = u.volume(1e-8).unwrap();
        assert!((v - 3.0).abs() < 3.0 * 1e-9, "{v}");
    }

    /// Five overlapping boxes through `union_all`: three pairing levels, one operand carried
    /// across a level unpaired. Each pairwise result gets the tolerance update as it is made,
    /// as smlib's boolean does, so the next level unions repaired operands; the staircase keeps
    /// its closed-form volume (five unit boxes, each overlapping the next by half).
    #[test]
    fn a_five_box_chain_union_all_measures_the_closed_form_with_the_update_per_pair() {
        all_fixes();
        let boxes = (0..5)
            .map(|i| make_box([f64::from(i) * 0.5, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap())
            .collect();
        let u = union_all(boxes).unwrap();
        let v = u.volume(1e-8).unwrap();
        assert!((v - 3.0).abs() < 3.0 * 1e-9, "{v}");
    }

    /// One body through `union_all` makes no boolean, so the tolerance update does not touch it:
    /// its volume comes back as it went in.
    #[test]
    fn a_single_body_union_all_is_not_repaired() {
        all_fixes();
        let a = make_box([0.0, 0.0, 0.0], [2.0, 1.0, 1.0]).unwrap();
        let u = union_all(vec![a]).unwrap();
        let v = u.volume(1e-8).unwrap();
        assert!((v - 2.0).abs() < 2.0 * 1e-9, "{v}");
    }

    /// Many overlapping boxes through parallel level-by-level `union_all_with(Contract::Proved)`
    /// produce the exact closed-form volume and counts deterministically for even and odd counts.
    #[test]
    fn parallel_union_all_is_exact_and_deterministic() {
        all_fixes();
        for n in [16_u32, 17_u32] {
            let make = || {
                (0..n)
                    .map(|i| make_box([f64::from(i) * 0.5, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap())
                    .collect::<Vec<_>>()
            };
            let u1 = union_all_with(make(), Contract::Proved).unwrap();
            let u2 = union_all_with(make(), Contract::Proved).unwrap();
            let want = 1.0 + f64::from(n - 1) * 0.5;
            let v1 = u1.volume(1e-8).unwrap();
            let v2 = u2.volume(1e-8).unwrap();
            assert!((v1 - want).abs() < 1e-7, "n={n}: {v1} vs {want}");
            assert_eq!(u1.counts().unwrap(), u2.counts().unwrap());
            assert!((v1 - v2).abs() < 1e-12, "n={n}: {v1} vs {v2}");
        }
    }

    /// Parallel `union_all` stops cleanly and returns `Error::Cancelled` when cancelled.
    #[test]
    fn parallel_union_all_respects_cancellation() {
        all_fixes();
        let boxes: Vec<Body> = (0..12_u32)
            .map(|i| make_box([f64::from(i) * 0.5, 0.0, 0.0], [1.0, 1.0, 1.0]).unwrap())
            .collect();
        let token = solidcore::kernel::CancelToken::new();
        token.cancel();
        let res = crate::cancel::within(&token, || union_all(boxes));
        assert!(matches!(res, Err(Error::Cancelled)), "{res:?}");
    }

    /// `bodies` united as `union_all` pairs them, each pair by [`boolean`] (a boolean every
    /// time, as before solids apart were put side by side): level by level, an odd one carried.
    fn paired_unions(mut level: Vec<Body>) -> Body {
        while level.len() > 1 {
            let mut next = Vec::new();
            let mut it = level.into_iter();
            while let Some(a) = it.next() {
                next.push(match it.next() {
                    Some(b) => boolean(a, b, BooleanOp::Union).unwrap().unwrap(),
                    None => a,
                });
            }
            level = next;
        }
        level.pop().unwrap()
    }

    /// `union_all` of `bodies()` runs `booleans` booleans and makes `apart` unions side by side,
    /// and gives what the pairwise booleans give: the same volume (within the integration's
    /// accuracy), faces, edges and vertices, pieces, and a body the full check passes. Returns
    /// its volume.
    fn same_as_paired_unions(bodies: impl Fn() -> Vec<Body>, booleans: u64, apart: u64) -> f64 {
        use solidcore::kernel::WorkCounters;
        let (operands, again) = (bodies(), bodies());
        let (u, ran) = session::counted(WorkCounters::booleans, || union_all(operands).unwrap());
        let (_, beside) =
            session::counted(WorkCounters::apart_unions, || union_all(again).unwrap());
        assert_eq!(
            (ran, beside),
            (booleans, apart),
            "booleans, unions side by side"
        );
        let want = paired_unions(bodies());
        let (v, w) = (u.volume(1e-10).unwrap(), want.volume(1e-10).unwrap());
        assert!((v - w).abs() <= 1e-9 * w, "volume {v}, pairwise {w}");
        assert_eq!(
            u.counts().unwrap(),
            want.counts().unwrap(),
            "faces, edges, vertices"
        );
        assert_eq!(
            u.solid_count().unwrap(),
            want.solid_count().unwrap(),
            "pieces"
        );
        assert_eq!(u.check(CheckLevel::All).unwrap().failures, 0);
        assert_eq!(want.check(CheckLevel::All).unwrap().failures, 0);
        v
    }

    fn cube(at: [f64; 3]) -> Body {
        make_box(at, [1.0, 1.0, 1.0]).unwrap()
    }

    /// A 50 x 50 x 10 plate and 25 pegs (radius 2, height 12 from z = 8) on a 10 mm grid: each
    /// peg stands in the plate and meets no other. Hub-and-spoke batching combines all 25 apart
    /// pegs side by side first (24 apart unions) and runs a single boolean against the plate,
    /// where blind sequential pairing took 5 booleans and 20 apart unions (#21438). Each peg adds
    /// its 10 mm above the plate: 25000 + 25 pi 2^2 10.
    #[test]
    fn pegs_on_a_plate_take_one_boolean_per_level() {
        all_fixes();
        let bodies = || {
            let mut b = vec![make_box([0.0, 0.0, 0.0], [50.0, 50.0, 10.0]).unwrap()];
            for i in 0..5 {
                for j in 0..5 {
                    let at = [10.0 * f64::from(i) + 5.0, 10.0 * f64::from(j) + 5.0, 8.0];
                    b.push(make_cylinder(at, 2.0, 12.0).unwrap());
                }
            }
            b
        };
        let v = same_as_paired_unions(bodies, 1, 24);
        let want = 25_000.0 + 25.0 * std::f64::consts::PI * 4.0 * 10.0;
        assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
    }

    /// Ten cylinders that meet nothing are one body of ten pieces with no boolean.
    #[test]
    fn solids_all_apart_join_without_a_boolean() {
        all_fixes();
        let bodies = || {
            (0..10)
                .map(|i| make_cylinder([10.0 * f64::from(i), 0.0, 0.0], 2.0, 5.0).unwrap())
                .collect()
        };
        let v = same_as_paired_unions(bodies, 0, 9);
        let want = 10.0 * std::f64::consts::PI * 4.0 * 5.0;
        assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
    }

    /// Four fins whose inner ends overlap each other inside a hub (#15913's shape): no pair is
    /// apart, so every union is a boolean, as before. The pairwise booleans are the reference.
    #[test]
    fn fins_meeting_each_other_inside_a_hub_are_all_united_by_booleans() {
        all_fixes();
        let bodies = || {
            vec![
                make_cylinder([0.0, 0.0, 0.0], 10.0, 10.0).unwrap(),
                make_box([-1.0, -0.5, 2.0], [21.0, 1.0, 6.0]).unwrap(),
                make_box([-20.0, -0.5, 2.0], [21.0, 1.0, 6.0]).unwrap(),
                make_box([-0.5, -1.0, 2.0], [1.0, 21.0, 6.0]).unwrap(),
                make_box([-0.5, -20.0, 2.0], [1.0, 21.0, 6.0]).unwrap(),
            ]
        };
        same_as_paired_unions(bodies, 4, 0);
    }

    /// A unit cube touching another at a face, an edge or a vertex is not apart from it, so they
    /// are united by a boolean. A third cube far from both then goes beside them with no boolean,
    /// except after the edge: two cubes along an edge share an edge of four faces, which the
    /// boolean's manifold test refuses (it takes its non-manifold path for them), so the far cube
    /// is united with them by a boolean, as before. Two cubes at a vertex share no edge and pass
    /// that test.
    #[test]
    fn solids_touching_at_a_face_an_edge_or_a_vertex_are_united_by_a_boolean() {
        all_fixes();
        for (touching, booleans) in [
            ([1.0, 0.0, 0.0], 1),
            ([1.0, 1.0, 0.0], 2),
            ([1.0, 1.0, 1.0], 1),
        ] {
            let bodies = || vec![cube([0.0; 3]), cube(touching), cube([5.0, 0.0, 0.0])];
            let v = same_as_paired_unions(bodies, booleans, 2 - booleans);
            assert!((v - 3.0).abs() <= 3e-9, "{touching:?}: {v}");
        }
    }

    /// A cube inside a larger one is not apart from it: they are united by a boolean (the
    /// larger whole), and a cube far from both then goes beside it with none.
    #[test]
    fn a_solid_inside_another_is_united_with_it_by_a_boolean() {
        all_fixes();
        let bodies = || {
            vec![
                make_box([0.0; 3], [4.0, 4.0, 4.0]).unwrap(),
                cube([1.0, 1.0, 1.0]),
                cube([10.0, 0.0, 0.0]),
            ]
        };
        let v = same_as_paired_unions(bodies, 1, 1);
        assert!((v - 65.0).abs() <= 65e-9, "{v}");
    }

    /// The pegs under the proved contract (as an extrude of many profiles unites them): the
    /// two pegs paired with each other go side by side and get the tolerance update as a
    /// boolean's result does; the result is proved a solid.
    #[test]
    fn pegs_on_a_plate_under_the_proved_contract() {
        all_fixes();
        let mut bodies = vec![make_box([0.0, 0.0, 0.0], [30.0, 10.0, 10.0]).unwrap()];
        for i in 0..3 {
            bodies.push(make_cylinder([10.0 * f64::from(i) + 5.0, 5.0, 8.0], 2.0, 12.0).unwrap());
        }
        let (u, ran) = session::counted(solidcore::kernel::WorkCounters::booleans, || {
            union_all_with(bodies, Contract::Proved).unwrap()
        });
        assert_eq!(ran, 2);
        let want = 3000.0 + 3.0 * std::f64::consts::PI * 4.0 * 10.0;
        let v = u.volume(1e-10).unwrap();
        assert!((v - want).abs() <= 1e-9 * want, "{v} {want}");
        assert_eq!(u.check(CheckLevel::All).unwrap().failures, 0);
    }

    /// Copies of `a` and `b`: whether the kernel proves them apart, and, when it does, their
    /// union by each route, side by side ([`union_if_apart`]) and by `SmApiBoolean` on fresh
    /// copies, each with its record.
    fn both_routes(a: &Body, b: &Body) -> Option<[(Body, Record); 2]> {
        let route = |beside: bool| {
            let d = run_on("both_routes", &[("a", a), ("b", b)], |k| {
                let ca = read_locked(a, |a| brep_copy(k, a))?;
                let cb = read_locked(b, |b| brep_copy(k, b))?;
                let united = match union_if_apart(k, ca, cb)? {
                    Apart::United(r) => *r,
                    Apart::NotProved(..) => return Ok(Done::Empty),
                };
                let r = if beside {
                    united
                } else {
                    let ca = read_locked(a, |a| brep_copy(k, a))?;
                    let cb = read_locked(b, |b| brep_copy(k, b))?;
                    sm::boolean(k, ca, cb, BooleanOperation::Union)?.result
                };
                Ok(Done::Body(Box::new(r), None))
            })
            .unwrap();
            result(d, Ok(None)).unwrap().map(|u| {
                let record = u.to_record().unwrap().0;
                (u, record)
            })
        };
        Some([route(true)?, route(false)?])
    }

    fn moved(b: Body, by: [f64; 3]) -> Body {
        let [x, y, z] = by;
        b.transform(&[
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [x, y, z, 1.0],
        ])
        .unwrap();
        b
    }

    /// A 10 mm cube with a 6 mm cavity in its middle (two shells, one region of material).
    fn hollow_cube(at: [f64; 3]) -> Body {
        let outer = make_box(at, [10.0; 3]).unwrap();
        let inner = make_box([at[0] + 2.0, at[1] + 2.0, at[2] + 2.0], [6.0; 3]).unwrap();
        boolean(outer, inner, BooleanOp::Subtract).unwrap().unwrap()
    }

    /// A unit cube whose B-rep, edges and vertices all have tolerance `tolerance`.
    fn cube_at_tolerance(at: [f64; 3], tolerance: f64) -> Body {
        let (mut r, _) = cube(at).to_record().unwrap();
        r.tolerance = tolerance;
        r.edges.iter_mut().for_each(|e| e.tolerance = tolerance);
        r.vertices.iter_mut().for_each(|v| v.tolerance = tolerance);
        Body::from_record(&r, true).unwrap().0
    }

    /// The vertices of a record as points, sorted, with their tolerances.
    fn sorted_vertices(r: &Record) -> Vec<([f64; 3], f64)> {
        let mut v: Vec<([f64; 3], f64)> =
            r.vertices.iter().map(|v| (v.point, v.tolerance)).collect();
        v.sort_by(|p, q| {
            p.0.iter()
                .zip(&q.0)
                .map(|(a, b)| a.total_cmp(b))
                .find(|o| o.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        v
    }

    /// Solids apart united side by side and by a boolean give the same body, its entities in
    /// another order: the boolean copies the second operand's faces one by one, the side by
    /// side union copies its topology as it stands. The same faces, edges and vertices, the
    /// same vertices bit for bit near the origin (more than 1000 from it the boolean works on
    /// the operands moved near the origin and back, so to the rounding of that move), the same
    /// volume and area, and a body the full check passes. Boxes, a cylinder and a sphere, a
    /// cone and a torus, two swept parts of B-spline faces, and a hollow cube first and second
    /// (its cavity's shell copied, and set void again by nesting).
    #[test]
    fn solids_apart_united_side_by_side_are_what_a_boolean_gives() {
        all_fixes();
        let hoop = || {
            Body::from_smb(
                include_bytes!("../tests/data/issue_20408_hoop.smb").to_vec(),
                false,
            )
            .unwrap()
        };
        let pairs: Vec<(&str, Body, Body)> = vec![
            ("boxes", cube([0.0; 3]), cube([3.0, 0.0, 0.0])),
            (
                "cylinder and sphere",
                make_cylinder([0.0; 3], 2.0, 5.0).unwrap(),
                crate::make_sphere([10.0, 0.0, 0.0], 3.0).unwrap(),
            ),
            (
                "cone and torus",
                crate::make_cone([0.0; 3], 2.0, 1.0, 4.0).unwrap(),
                crate::make_torus([0.0, 20.0, 0.0], 5.0, 1.0).unwrap(),
            ),
            ("swept parts", hoop(), moved(hoop(), [0.0, 0.0, 100.0])),
            (
                "a hollow cube and a cube",
                hollow_cube([0.0; 3]),
                cube([20.0, 0.0, 0.0]),
            ),
            (
                "boxes far from the origin",
                cube([5000.0, 0.0, 0.0]),
                cube([5003.0, 0.0, 0.0]),
            ),
        ];
        for (name, a, b) in pairs {
            let Some([(beside, br), (merged, mr)]) = both_routes(&a, &b) else {
                panic!("{name}: not proved apart");
            };
            assert_eq!(beside.counts().unwrap(), merged.counts().unwrap(), "{name}");
            assert_eq!(
                (br.regions.len(), br.shells.len()),
                (mr.regions.len(), mr.shells.len()),
                "{name}: regions, shells"
            );
            let (vb, vm) = (sorted_vertices(&br), sorted_vertices(&mr));
            if name.ends_with("origin") {
                for ((p, _), (q, _)) in vb.iter().zip(&vm) {
                    assert!(
                        p.iter().zip(q).all(|(x, y)| (x - y).abs() <= 1e-9),
                        "{name}: {p:?} {q:?}"
                    );
                }
            } else {
                assert_eq!(vb, vm, "{name}: vertices");
            }
            let (v, w) = (beside.volume(1e-10).unwrap(), merged.volume(1e-10).unwrap());
            assert!((v - w).abs() <= 1e-12 * w, "{name}: volume {v} {w}");
            let (s, t) = (beside.area(1e-10).unwrap(), merged.area(1e-10).unwrap());
            assert!((s - t).abs() <= 1e-12 * t, "{name}: area {s} {t}");
            assert_eq!(
                beside.solid_count().unwrap(),
                merged.solid_count().unwrap(),
                "{name}"
            );
            assert_eq!(beside.check(CheckLevel::All).unwrap().failures, 0, "{name}");
        }
    }

    /// Two unit cubes are proved apart only past the margin, the larger intersection tolerance of
    /// the two (twice the zone tolerance, 2e-5 at the default 1e-5), over boxes already grown by
    /// the zone tolerance of twice each entity's tolerance (2e-5 each side): a gap of 1e-3 is
    /// apart, one of 4e-5 (no more than the grown boxes' 4e-5 overlap) and a face shared are not.
    #[test]
    fn solids_are_proved_apart_only_past_their_tolerances() {
        all_fixes();
        for (gap, apart) in [(1e-3, true), (4e-5, false), (0.0, false)] {
            let a = cube([0.0; 3]);
            let b = cube([1.0 + gap, 0.0, 0.0]);
            assert_eq!(both_routes(&a, &b).is_some(), apart, "gap {gap}");
        }
    }

    /// Cubes 2 mm apart at B-rep tolerances 1e-4 and 1e-5 are not put side by side: the second
    /// cube's faces and edges would sit in a B-rep of the first's tolerance below it, which the
    /// check refuses (`SmBrep` tests #6 and #60), where a boolean reconciles the two. Their union
    /// is a boolean, as before, and passes the check.
    #[test]
    fn solids_at_different_tolerances_are_united_by_a_boolean() {
        all_fixes();
        let a = cube_at_tolerance([0.0; 3], 1e-4);
        let b = cube([3.0, 0.0, 0.0]);
        assert!(both_routes(&a, &b).is_none());
        let (u, ran) = session::counted(solidcore::kernel::WorkCounters::booleans, || {
            union_all(vec![a, b]).unwrap()
        });
        assert_eq!(ran, 1);
        assert_eq!(u.check(CheckLevel::All).unwrap().failures, 0);
    }

    /// A cube in the cavity of a hollow cube lies inside the hollow cube's outer shell box, so
    /// it is not apart from it: a boolean unites them, as before, and the far cube then goes
    /// beside them. 1000 - 216 + 1 + 1.
    #[test]
    fn a_solid_in_the_cavity_of_another_is_united_with_it_by_a_boolean() {
        all_fixes();
        let bodies = || {
            vec![
                hollow_cube([0.0; 3]),
                cube([4.5; 3]),
                cube([30.0, 0.0, 0.0]),
            ]
        };
        let v = same_as_paired_unions(bodies, 1, 1);
        assert!((v - 786.0).abs() <= 786e-9, "{v}");
    }

    /// A 10 mm cube with the three edges at its origin corner rounded r 2: the corner is a ball
    /// patch, a surface with no certified box here, so the cube is proved apart from nothing,
    /// not even a cube 20 mm away, and their union is a boolean, as before; the same cube
    /// unrounded is proved apart from it.
    #[test]
    fn a_solid_with_a_face_without_a_certified_box_is_proved_apart_from_nothing() {
        use solidcore::geom::vector3d::Point3;
        use solidcore::query::api_queries::edge_closest_point;
        all_fixes();
        let block = make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        let edges: Vec<usize> = {
            let k = Kernel::default();
            let b = block.lock();
            [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 5.0]]
                .iter()
                .map(|p| {
                    let p = Point3::new(p[0], p[1], p[2]);
                    b.edge_list()
                        .iter()
                        .position(|&e| {
                            edge_closest_point(&k, &b, e, p)
                                .is_ok_and(|c| c.closest.distance < 1e-9)
                        })
                        .unwrap()
                })
                .collect()
        };
        crate::blend(
            &block,
            crate::Blend::Round,
            &edges,
            &[2.0; 3],
            &[],
            crate::Corners::Filled,
        )
        .unwrap();
        let record = block.to_record().unwrap().0;
        assert!(
            record.faces.iter().any(|f| matches!(
                f.surface,
                solidcore_record::brep::Surface::BallPatch { .. }
                    | solidcore_record::brep::Surface::Blend { .. }
            )),
            "{:?}",
            record.faces.iter().map(|f| &f.surface).collect::<Vec<_>>()
        );
        assert!(both_routes(&block, &cube([30.0, 0.0, 0.0])).is_none());
        // the same cube unrounded is proved apart from it: the ball patch is the reason
        let plain = make_box([0.0; 3], [10.0, 10.0, 10.0]).unwrap();
        assert!(both_routes(&plain, &cube([30.0, 0.0, 0.0])).is_some());
    }

    /// A boolean that fails hands the failure callback its operands as they came in, not the
    /// empty bodies the kernel leaves once it has taken them.
    #[test]
    fn a_failed_boolean_reports_its_operands_as_they_came() {
        type Seen = Vec<(&'static str, std::result::Result<Vec<u8>, String>)>;
        let _turn = session::tests::HOOKED
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        all_fixes();
        let seen: Arc<Mutex<Seen>> = Arc::default();
        let s = Arc::clone(&seen);
        set_failure_hook(Some(Arc::new(move |f: &Failure<'_>| {
            if f.call == "SmApiBoolean" {
                s.lock().unwrap().extend(f.inputs.iter().cloned());
            }
        })));
        let a = make_box([0.0, 0.0, 0.0], [1.0, 2.0, 3.0]).unwrap();
        // an operand with a face without its surface: the kernel refuses it
        let b = make_box([0.5, 0.5, 0.5], [1.0, 1.0, 1.0]).unwrap();
        {
            let mut k = b.lock();
            let f = *k.face_list().first().unwrap();
            k.face_mut(f).unwrap().surface = None;
        }
        let r = boolean(a, b, BooleanOp::Union);
        set_failure_hook(None);
        assert!(r.is_err(), "{r:?}");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert_eq!(seen[0].0, "a");
        let back = Body::from_smb(seen[0].1.clone().unwrap(), false).unwrap();
        assert_eq!(back.counts().unwrap(), [6, 12, 8]);
    }

    #[test]
    fn intersect_copies_measures_overlap_clears_flush_contact_and_keeps_inputs() {
        all_fixes();
        let a = cube([0.0, 0.0, 0.0]);
        let overlap = cube([0.5, 0.0, 0.0]);
        let flush = cube([1.0, 0.0, 0.0]);
        let apart = cube([5.0, 0.0, 0.0]);

        let hit = intersect_copies(&a, &overlap).unwrap().unwrap();
        assert!((hit.volume(1e-10).unwrap() - 0.5).abs() < 1e-9);
        assert!(intersect_copies(&a, &flush).unwrap().is_none());
        assert!(intersect_copies(&a, &apart).unwrap().is_none());
        // Both input bodies remain intact after every call.
        assert!((a.volume(1e-10).unwrap() - 1.0).abs() < 1e-9);
        assert!((overlap.volume(1e-10).unwrap() - 1.0).abs() < 1e-9);
        assert!((flush.volume(1e-10).unwrap() - 1.0).abs() < 1e-9);
    }
}
