//! Booleans: solidcore-ops' ([`solidcore_ops::boolean`]), on copies of the operands.
//!
//! solidcore-ops' booleans take their operands by value, as the kernel does: it edits the first
//! and consumes the second. A SolidCraft body never changes, so every operation here copies its
//! operands first ([`Body::copy`], `SmApiBrepCopy`).

use solidcore::Result as SolidResult;
use solidcore::boolean::api_brep as sm;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use solidcore_ops::boolean as ops;

use super::Body;
use super::call;
use crate::{BooleanOp, Error, Result};

fn operation(op: BooleanOp) -> ops::BooleanOp {
    match op {
        BooleanOp::Union => ops::BooleanOp::Union,
        BooleanOp::Subtract => ops::BooleanOp::Subtract,
        BooleanOp::Intersect => ops::BooleanOp::Intersect,
    }
}

/// What a failure's dump calls the first operands.
const OPERANDS: [&str; 4] = ["a", "b", "c", "d"];

fn operands<'a>(bodies: &[&'a Body]) -> Vec<(&'static str, &'a Body)> {
    bodies.iter().enumerate().map(|(n, &b)| (OPERANDS.get(n).copied().unwrap_or("operand"), b)).collect()
}

/// Fails when a panic left an operand half-edited: it is never used again.
fn usable(call: &'static str, bodies: &[&Body]) -> Result<()> {
    if bodies.iter().any(|b| b.kernel_left_unfinished()) {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    Ok(())
}

/// A copy of each operand, for the operation to consume.
fn copies(bodies: &[&Body]) -> Result<Vec<Body>> {
    bodies.iter().map(|b| call::ops("SmApiBrepCopy", || b.copy())).collect()
}

/// `a` op `b`: `None` when the result is empty, [`Error::NotSolid`] when solid operands give a
/// body that is not closed (solids touching along an edge or at a vertex are closed).
pub(crate) fn boolean(a: &Body, b: &Body, op: BooleanOp) -> Result<Option<Body>> {
    usable("SmApiBoolean", &[a, b])?;
    if op == BooleanOp::Union {
        return call::ops_on("SmApiBoolean", &operands(&[a, b]), || ops::union_copies(a, b));
    }
    let mut c = copies(&[a, b])?.into_iter();
    let (Some(ca), Some(cb)) = (c.next(), c.next()) else { return Err(Error::Argument) };
    call::ops_on("SmApiBoolean", &operands(&[a, b]), || ops::boolean(ca, cb, operation(op)))
}

/// [`boolean`] of `a` and `b` tagged as operands 0 and 1 ([`super::history::tag`]): the tagged
/// copies are the operation's to consume, so they are not copied again. A boolean of tagged
/// copies made them and then copied them both, the larger operand's copy as dear as the
/// boolean on a body of many faces (solidcore#3359). A failure's dump gets `a` and `b`.
pub(crate) fn boolean_tagged(a: &Body, b: &Body, op: BooleanOp) -> Result<Option<Body>> {
    usable("SmApiBoolean", &[a, b])?;
    let ta = super::history::tag(a, 0)?;
    let tb = super::history::tag(b, 1)?;
    call::ops_on("SmApiBoolean", &operands(&[a, b]), || ops::boolean(ta, tb, operation(op)))
}

/// The kernel's operation for [`merged`].
fn kernel_operation(op: BooleanOp) -> BooleanOperation {
    match op {
        BooleanOp::Union => BooleanOperation::Union,
        BooleanOp::Subtract => BooleanOperation::Difference,
        BooleanOp::Intersect => BooleanOperation::Intersection,
    }
}

/// `SmApiBoolean` of `a` and `b` in the kernel `k` (both consumed), with the result's
/// tolerances grown as [`boolean`] grows them.
pub(crate) fn merged(k: &mut Kernel, a: Brep, b: Brep, op: BooleanOp) -> SolidResult<Brep> {
    let mut r = sm::boolean(k, a, b, kernel_operation(op))?;
    // Grow the result's tolerances to cover marginal edge/vertex, vertex/face and
    // edge/face gaps, so an otherwise-sound union is not refused as "Bad Gap" for a
    // sub-micron gap (SolidCraft issue #10, fuzzer seed 73). Tolerances only grow:
    // a body that already passes is unchanged; a marginal failure becomes sound.
    // Offset and shell already grow their results' tolerances this way. First, a vertex
    // a grazing cut left wider than the check allows is centred on its edges' ends.
    solidcore::topology::brep::validation::update_tolerances_after_boolean(k, &mut r.result)?;
    Ok(r.result)
}

/// The union of copies of `bodies`, pairwise, level by level (an odd one carried to the next
/// level as it is): at least one body and at most [`crate::MAX_UNION_BREPS`].
///
/// Not upstream: when the level-by-level union fails with the classification's own typed
/// refusal ([`Error::NotOneFoot`], `compute_point_parameters` finding no single nearest point
/// on a face's surface), the same bodies are united once more on fresh copies, one at a time
/// against the result so far ([`union_one_by_one`]). The union of solids is the same set
/// whichever pairing computes it, and a pair the level scheme cannot classify may classify fine
/// against the accumulated body, so a join of many bodies that fails one pairing is built
/// instead of refused (SolidCraft issue #16552). Any other failure — a deliberate refusal such
/// as the solid leak, an unported step, a cancellation — keeps its own error; so does a failure
/// of the retry.
pub(crate) fn union_all(bodies: &[&Body]) -> Result<Body> {
    if bodies.is_empty() || bodies.len() > crate::MAX_UNION_BREPS {
        return Err(Error::Argument);
    }
    usable("smlib_boolean_union_all", bodies)?;
    let c = copies(bodies)?;
    match call::ops_on("smlib_boolean_union_all", &operands(bodies), || ops::union_all(c)) {
        Ok(b) => Ok(b),
        Err(first @ Error::NotOneFoot { .. }) => {
            let fresh = copies(bodies)?;
            match union_one_by_one(fresh) {
                Ok(b) => Ok(b),
                // the retry's own classification refusal keeps the level scheme's; any other
                // failure of the retry - a deliberate refusal, an unported step, a cancellation -
                // is the answer, not masked by the level scheme's refusal
                Err(Error::NotOneFoot { .. }) => Err(first),
                Err(e) => Err(e),
            }
        }
        Err(first) => Err(first),
    }
}

/// Not upstream: the same bodies united one at a time, each into the result so far. A pair that
/// fails the classification's typed refusal ([`Error::NotOneFoot`]) is put aside until the
/// others are in (a later union may classify the pair that failed now); one that still fails
/// then fails the retry. Any other failure — a deliberate refusal, an unported step — fails the
/// retry with its own error at once, so it is neither deferred nor masked by the level scheme's
/// refusal. Each union works on a copy of the result so far and a copy of the body, so a failed
/// union leaves both ready to try again.
fn union_one_by_one(bodies: Vec<Body>) -> Result<Body> {
    let mut bodies = bodies.into_iter();
    let Some(mut acc) = bodies.next() else { return Err(Error::Argument) };
    let mut deferred = Vec::new();
    for b in bodies {
        match boolean_pair(&acc, &b) {
            Ok(Some(u)) => acc = u,
            Ok(None) => return Err(Error::Empty),
            Err(Error::NotOneFoot { .. }) => deferred.push(b),
            Err(e) => return Err(e),
        }
    }
    for b in deferred {
        acc = boolean_pair(&acc, &b)?.ok_or(Error::Empty)?;
    }
    Ok(acc)
}

/// A union of two bodies on copies, as [`ops::boolean`] gives it (the result is `None` only
/// where a union of nonempty solids never lands: an empty body).
fn boolean_pair(a: &Body, b: &Body) -> Result<Option<Body>> {
    let mut c = copies(&[a, b])?.into_iter();
    let (Some(ca), Some(cb)) = (c.next(), c.next()) else { return Err(Error::Argument) };
    let inputs = [("a", a), ("b", b)];
    call::ops_on("smlib_boolean_union_all", &inputs, || ops::boolean(ca, cb, operation(BooleanOp::Union)))
}

/// [`union_all`] of `bodies` tagged each as its own operand, in order
/// ([`super::history::tag`]): the tagged copies are the union's to consume, so they are not
/// copied again, as in [`boolean_tagged`]. A failure's dump gets `bodies`.
pub(crate) fn union_all_tagged(bodies: &[&Body]) -> Result<Body> {
    if bodies.is_empty() || bodies.len() > crate::MAX_UNION_BREPS {
        return Err(Error::Argument);
    }
    usable("smlib_boolean_union_all", bodies)?;
    let tagged = bodies.iter().enumerate().map(|(i, b)| super::history::tag(b, i)).collect::<Result<Vec<_>>>()?;
    call::ops_on("smlib_boolean_union_all", &operands(bodies), || ops::union_all(tagged))
}

/// `a` op `b` for sheets of planar faces on one plane, on copies: `None` when either has no
/// face or the result is empty.
pub(crate) fn boolean2d(a: &Body, b: &Body, op: BooleanOp) -> Result<Option<Body>> {
    usable("SmApiBoolean2d", &[a, b])?;
    let mut c = copies(&[a, b])?.into_iter();
    let (Some(ca), Some(cb)) = (c.next(), c.next()) else { return Err(Error::Argument) };
    call::ops_on("SmApiBoolean2d", &operands(&[a, b]), || ops::boolean2d(ca, cb, operation(op)))
}

/// `tool`'s edges imprinted on a copy of `body`: `None` when the imprint changed nothing of it
/// (the tool meets no face).
pub(crate) fn imprint(body: &Body, tool: &Body) -> Result<Option<Body>> {
    if std::ptr::eq(body, tool) {
        return Err(Error::Argument);
    }
    usable("SmApiBooleanWithOptions", &[body, tool])?;
    let mut c = copies(&[body, tool])?.into_iter();
    let (Some(cb), Some(ct)) = (c.next(), c.next()) else { return Err(Error::Argument) };
    let (imprinted, changed) = call::ops_on("SmApiBooleanWithOptions", &operands(&[body, tool]), || ops::imprint(cb, ct))?;
    Ok(changed.then_some(imprinted))
}
