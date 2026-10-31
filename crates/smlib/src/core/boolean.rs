//! Booleans on solidcore: the C API's own logic round `SM_API`, ported from the fork's
//! `capi/src/smlib_ops.cpp` (`smlib_boolean_with`, `smlib_boolean2d`,
//! `smlib_boolean_union_all`, `finish`, `closed_nonmanifold`) and
//! `capi/src/smlib_imprint.cpp` (`smlib_imprint`).
//!
//! As there, every operation works on copies of its operands (`SmApiBrepCopy`): `SM_API` edits
//! the first operand in place and consumes the second, and a SolidCraft body never changes.

use std::sync::PoisonError;

use solidcore::Result as SolidResult;
use solidcore::SolidError;
use solidcore::boolean::api_brep::{self as sm, Boolean2dOutcome};
use solidcore::boolean::merge::MergeOptions;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::kernel::Kernel;
use solidcore::primitive::primitive_creation::p1::Boolean2d;
use solidcore::topology::brep::p3::brep_copy;
use solidcore::topology::brep::{p1, p2};
use solidcore::topology::{Brep, edge};

use super::{Body, run};
use crate::{BooleanOp, Error, Result};

/// What an operation's `SM_API` call gave, before it is a SolidCraft result: the capi's
/// statuses besides success.
enum Done {
    Body(Box<Brep>),
    /// `SMLIB_ERR_EMPTY`.
    Empty,
    /// `SMLIB_ERR_NOT_SOLID`.
    NotSolid,
    /// `SMLIB_ERR_NO_CUT` (`smlib_imprint`).
    NoCut,
}

/// `GetNumFaces`.
fn num_faces(b: &Brep) -> usize {
    b.face_list().len()
}

/// `closed_nonmanifold` (smlib_ops.cpp): a closed body whose solids touch only along edges or at
/// vertices: at least two regions, no shell vertices, no wire edge, and four edgeuses (two faces,
/// both sides) per face round every edge. As there, an edge whose query fails (`e == nullptr`, a
/// failed `GetEdgeuses`) makes it false: [`edge_closes`].
fn closed_nonmanifold(r: &Brep) -> SolidResult<bool> {
    if p1::regions(r, None)?.len() < 2 {
        return Ok(false);
    }
    if !p2::shell_vertices(r, None)?.is_empty() {
        return Ok(false);
    }
    for e in p1::edges(r, None)? {
        if !edge_closes(edge::is_wire(r, e), edge::edgeuses(r, e, None, None).map(|u| u.len())) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The capi's test of one edge in `closed_nonmanifold`, given its `IsWire` and the count of its
/// `GetEdgeuses`: not a wire, and four edgeuses per face round it. Either query failing is
/// false, not an error: the capi returns false where `GetEdgeuses` fails.
pub(super) fn edge_closes(wire: SolidResult<bool>, uses: SolidResult<usize>) -> bool {
    matches!(wire, Ok(false)) && matches!(uses, Ok(n) if n >= 4 && n % 4 == 0)
}

/// `finish` (smlib_ops.cpp): the result must have faces, and a solid input must give a closed
/// manifold solid or (`nonmanifold`) a closed one whose solids touch along an edge or at a vertex.
fn finish(r: Brep, solid_in: bool, nonmanifold: bool) -> SolidResult<Done> {
    if num_faces(&r) == 0 {
        return Ok(Done::Empty);
    }
    if solid_in && !p2::is_manifold_solid(&r)? && !(nonmanifold && closed_nonmanifold(&r)?) {
        return Ok(Done::NotSolid);
    }
    Ok(Done::Body(Box::new(r)))
}

/// Copies of `bodies`, in order (a body given twice is copied twice, as the capi copies each
/// operand), all made under the kernel's lock.
fn copies(k: &mut Kernel, bodies: &[&Body]) -> SolidResult<Vec<Brep>> {
    // Under the kernel's lock (`run` holds it), as every body lock is: two never deadlock.
    let mut out = Vec::with_capacity(bodies.len());
    for (n, b) in bodies.iter().enumerate() {
        let g = b.0.lock().unwrap_or_else(PoisonError::into_inner);
        let c = brep_copy(k, &g)?;
        drop(g);
        out.push(c);
    }
    Ok(out)
}

fn result(d: Done, empty: Result<Option<Body>>) -> Result<Option<Body>> {
    match d {
        Done::Body(b) => Ok(Some(Body::new(*b))),
        Done::Empty => empty,
        Done::NotSolid => Err(Error::NotSolid),
        Done::NoCut => Ok(None),
    }
}

fn operation(op: BooleanOp) -> BooleanOperation {
    match op {
        BooleanOp::Union => BooleanOperation::Union,
        BooleanOp::Subtract => BooleanOperation::Difference,
        BooleanOp::Intersect => BooleanOperation::Intersection,
    }
}

/// Whether every body is a manifold solid (`IsManifoldSolid` of each copy).
fn all_solid(bodies: &[Brep]) -> SolidResult<bool> {
    for b in bodies {
        if !p2::is_manifold_solid(b)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Fails when a panic left one of `bodies` half-edited: it is never used again.
fn usable(call: &'static str, bodies: &[&Body]) -> Result<()> {
    if bodies.iter().any(|b| b.0.is_poisoned()) {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    Ok(())
}

/// `smlib_boolean` (`smlib_boolean_with` with `SMLIB_RESULT_NONMANIFOLD`): `SmApiBoolean` on
/// copies of `a` and `b`; `None` when the result is empty (`SMLIB_ERR_EMPTY`), and
/// [`Error::NotSolid`] when solid operands give a body that is not closed.
pub(crate) fn boolean(a: &Body, b: &Body, op: BooleanOp) -> Result<Option<Body>> {
    usable("SmApiBoolean", &[a, b])?;
    let d = run("SmApiBoolean", |k| {
        let mut c = copies(k, &[a, b])?.into_iter();
        let (Some(ca), Some(cb)) = (c.next(), c.next()) else {
            return Err(SolidError::invalid_input("smlib_boolean", "no operand"));
        };
        let solid_in = p2::is_manifold_solid(&ca)? && p2::is_manifold_solid(&cb)?;
        let r = sm::boolean(k, ca, cb, operation(op))?;
        finish(r.result, solid_in, true)
    })?;
    result(d, Ok(None))
}

/// `smlib_boolean_union_all`: copies of `bodies` unioned pairwise, level by level (an odd one
/// carried to the next level as it is), each union `SmApiBoolean`'s; then `finish` with
/// non-manifold results allowed. An empty result is [`Error::Empty`].
pub(crate) fn union_all(bodies: &[&Body]) -> Result<Body> {
    if bodies.is_empty() || bodies.len() > crate::MAX_UNION_BREPS {
        return Err(Error::Argument);
    }
    usable("smlib_boolean_union_all", bodies)?;
    let d = run("smlib_boolean_union_all", |k| {
        let mut level = copies(k, bodies)?;
        let solid_in = all_solid(&level)?;
        while level.len() > 1 {
            let mut next = Vec::with_capacity(level.len().div_ceil(2));
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
            Some(r) => finish(r, solid_in, true),
            None => Ok(Done::Empty),
        }
    })?;
    result(d, Err(Error::Empty))?.ok_or(Error::Empty)
}

/// `smlib_boolean2d`: `SmApiBoolean2d` (`SmPrimitiveCreation::Boolean2D`) on copies of `a` and
/// `b`, sheets of planar faces on one plane; `None` when either has no face (where
/// `SmApiBoolean2d` succeeds without a result) or the result is empty.
pub(crate) fn boolean2d(a: &Body, b: &Body, op: BooleanOp) -> Result<Option<Body>> {
    usable("SmApiBoolean2d", &[a, b])?;
    let d = run("SmApiBoolean2d", |k| {
        let mut c = copies(k, &[a, b])?.into_iter();
        let (Some(ca), Some(cb)) = (c.next(), c.next()) else {
            return Err(SolidError::invalid_input("smlib_boolean2d", "no operand"));
        };
        if num_faces(&ca) == 0 || num_faces(&cb) == 0 {
            return Ok(Done::Empty);
        }
        let kind = match op {
            BooleanOp::Union => Boolean2d::Union,
            BooleanOp::Subtract => Boolean2d::Difference,
            BooleanOp::Intersect => Boolean2d::Intersection,
        };
        match sm::boolean_2d(k, ca, cb, kind)? {
            Boolean2dOutcome::Result(r) => finish(r, false, false),
            Boolean2dOutcome::Unassigned { .. } => Ok(Done::Empty),
        }
    })?;
    result(d, Ok(None))
}

/// `smlib_imprint`: `SmApiBooleanWithOptions(SM_BO_IMPRINT)` on copies of `body` and `tool`,
/// imprinting and keeping the tool (cookie cutter off, imprinting on, post-process off,
/// imprint-and-classify off, keep other on). `None` when no face was cut
/// (`SMLIB_ERR_NO_CUT`); an empty result is [`Error::Empty`]; the same body twice is
/// [`Error::Argument`].
pub(crate) fn imprint(body: &Body, tool: &Body) -> Result<Option<Body>> {
    if std::ptr::eq(body, tool) {
        return Err(Error::Argument);
    }
    usable("SmApiBooleanWithOptions", &[body, tool])?;
    let d = run("SmApiBooleanWithOptions", |k| {
        let mut c = copies(k, &[body, tool])?.into_iter();
        let (Some(a), Some(t)) = (c.next(), c.next()) else {
            return Err(SolidError::invalid_input("smlib_imprint", "no operand"));
        };
        let before = num_faces(&a);
        let options = MergeOptions::new(false, true, false, false, true);
        let r = sm::boolean_with_options(k, a, t, BooleanOperation::Imprint, options)?.result;
        let after = num_faces(&r);
        Ok(if after == 0 {
            Done::Empty
        } else if after == before {
            Done::NoCut
        } else {
            Done::Body(Box::new(r))
        })
    })?;
    match d {
        Done::Empty => Err(Error::Empty),
        d => result(d, Err(Error::Empty)),
    }
}
