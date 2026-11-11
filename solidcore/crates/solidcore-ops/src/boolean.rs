//! Booleans: two bodies' union, difference or intersection (`SmApiBoolean`), a list's union
//! level by level, the boolean of planar sheets on one plane (`SmApiBoolean2d`), and imprinting
//! one body's edges on another (`SmApiBooleanWithOptions(SM_BO_IMPRINT)`).
//!
//! Each takes its operands by value, as the kernel does: it edits the first and consumes the
//! second. An application that keeps its operands copies them first ([`Body::copy`]). While a
//! failure callback is installed, the operands are written before the call (`run_taking`), so a
//! failure reports them as they came in.

use solidcore::boolean::api_brep::{self as sm, Boolean2dOutcome};
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::primitive::primitive_creation::p1::Boolean2d;
use solidcore::Brep;
use solidcore::Result as KernelResult;

use crate::body::{finish, Body, Expect};
use crate::session::run_taking;
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
    Body(Box<Brep>),
    Empty,
    Refused(Error),
}

impl crate::session::Answer for Done {
    fn refusal(&self) -> Option<String> {
        match self {
            Done::Refused(e) => Some(e.to_string()),
            _ => None,
        }
    }
}

/// The result contract of a boolean ([`finish`]): faces left, and a solid in gives a solid out,
/// solids touching along an edge or at a vertex allowed.
fn finished(k: &mut Kernel, r: Brep, expect: Expect) -> KernelResult<Done> {
    Ok(match finish(k, r, expect.touching())? {
        Ok(b) => Done::Body(Box::new(b)),
        Err(Error::Empty) => Done::Empty,
        Err(e) => Done::Refused(e),
    })
}

/// What the operands make a boolean's result: a solid when every operand is one.
fn expect_of(k: &mut Kernel, operands: &mut [Brep]) -> KernelResult<Expect> {
    let mut all = true;
    for b in operands.iter_mut() {
        all &= Expect::of(k, b)? != Expect::FACES;
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

/// The kernel's body taken out of `b`, leaving it empty.
fn take(b: &Body) -> Brep {
    std::mem::replace(&mut *b.lock(), Brep::new())
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
        Done::Body(b) => Ok(Some(Body::new(*b))),
        Done::Empty => empty,
        Done::Refused(e) => Err(e),
    }
}

/// `a` op `b` (`SmApiBoolean`): `None` when the result is empty, [`Error::NotSolid`] when solid
/// operands give a body that is not closed (solids touching along an edge or at a vertex are
/// closed).
///
/// # Errors
/// [`Error::NotSolid`]; the kernel's error.
pub fn boolean(a: Body, b: Body, op: BooleanOp) -> Result<Option<Body>> {
    let both = [a, b];
    usable("SmApiBoolean", &both)?;
    let d = run_taking("SmApiBoolean", &operands(&both), |k| {
        let [a, b] = &both;
        let mut operands = [take(a), take(b)];
        let expect = expect_of(k, &mut operands)?;
        let [ca, cb] = operands;
        let r = sm::boolean(k, ca, cb, operation(op))?;
        finished(k, r.result, expect)
    })?;
    result(d, Ok(None))
}

/// The union of `bodies`, pairwise level by level (an odd one carried to the next level as it
/// is), each union `SmApiBoolean`'s, with solids touching along edges or at vertices allowed.
///
/// # Errors
/// [`Error::Argument`] for no body; [`Error::Empty`] for an empty result; [`Error::NotSolid`];
/// the kernel's error.
// The operands are consumed: their bodies are taken out under the kernel's lock.
#[allow(clippy::needless_pass_by_value)]
pub fn union_all(bodies: Vec<Body>) -> Result<Body> {
    if bodies.is_empty() {
        return Err(Error::Argument);
    }
    usable("union_all", &bodies)?;
    let d = run_taking("union_all", &operands(&bodies), |k| {
        let mut level: Vec<Brep> = bodies.iter().map(take).collect();
        let expect = expect_of(k, &mut level)?;
        while level.len() > 1 {
            let mut next = Vec::new();
            let mut it = level.into_iter();
            while let Some(a) = it.next() {
                match it.next() {
                    Some(b) => next.push(sm::boolean(k, a, b, BooleanOperation::Union)?.result),
                    None => next.push(a),
                }
            }
            level = next;
        }
        match level.pop() {
            Some(r) => finished(k, r, expect),
            None => Ok(Done::Empty),
        }
    })?;
    result(d, Err(Error::Empty))?.ok_or(Error::Empty)
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
        let (ca, cb) = (take(a), take(b));
        if ca.face_list().is_empty() || cb.face_list().is_empty() {
            return Ok(Done::Empty);
        }
        let kind = match op {
            BooleanOp::Union => Boolean2d::Union,
            BooleanOp::Subtract => Boolean2d::Difference,
            BooleanOp::Intersect => Boolean2d::Intersection,
        };
        match sm::boolean_2d(k, ca, cb, kind)? {
            Boolean2dOutcome::Result(r) => finished(k, r, Expect::FACES),
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
/// off, imprinting on, post-process off, imprint-and-classify off, keep other on), with the
/// result contract ([`finish`]: faces left, a solid in gives a solid out), and whether the
/// imprint changed `body`'s topology (a face cut, an edge split: its face, edge or vertex
/// count moved). When it changed nothing the body comes back as the kernel left it, never
/// dropped.
///
/// # Errors
/// [`Error::Empty`] for an empty result; [`Error::NotSolid`] when a solid body comes out not
/// closed; the kernel's error.
pub fn imprint(body: Body, tool: Body) -> Result<(Body, bool)> {
    let both = [body, tool];
    usable("SmApiBooleanWithOptions", &both)?;
    let mut before = [0; 3];
    let d = run_taking("SmApiBooleanWithOptions", &operands(&both), |k| {
        let [body, tool] = &both;
        let (mut a, t) = (take(body), take(tool));
        before = counts(&a);
        let expect = Expect::of(k, &mut a)?;
        let options = MergeOptions::new(false, true, false, false, true);
        let r = sm::boolean_with_options(k, a, t, BooleanOperation::Imprint, options)?.result;
        finished(k, r, expect)
    })?;
    match d {
        Done::Body(b) => {
            let changed = counts(&b) != before;
            Ok((Body::new(*b), changed))
        }
        Done::Empty => Err(Error::Empty),
        Done::Refused(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use super::*;
    use crate::make_box;
    use crate::session::{self, set_failure_hook, Failure};

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
}
