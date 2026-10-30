//! Booleans on solidcore. The kernel consumes its operands, so each call runs on copies and the
//! inputs stay as they were.
use std::sync::Mutex;

use solidcore::boolean::api_brep::{boolean_difference, boolean_intersection, boolean_union};
use solidcore::primitive::primitive_creation::p1::{Boolean2d, PrimitiveCreation};
use solidcore::topology::Brep;
use solidcore::topology::brep::p2::ArenaNotifyState;
use solidcore::topology::topology_traverser::Marks;

use super::ops::{BOOLEAN_INTERSECT, BOOLEAN_SUBTRACT, BOOLEAN_UNION};
use super::rs::{State, body, copy_in, state, status};
use super::{ERR_ARG, ERR_EMPTY, ERR_EXCEPTION, Handle};

/// A result with no faces is empty: a disjoint intersection, a body wholly subtracted away.
/// Otherwise it goes out as a copy: the merge leaves the geometry owners named by the operands'
/// identities, and a copy names them all in one key domain again.
pub(super) fn result(s: &mut State, b: Brep) -> Result<Handle, i32> {
    if b.face_list().is_empty() {
        return Err(ERR_EMPTY);
    }
    let (b, domain) = copy_in(s, &b)?;
    Ok(Handle { brep: Mutex::new(b), domain })
}

/// A real copy of `h` (`api_brep_copy`): never `Brep::clone`, which would reset the geometry owners.
pub(super) fn copy_of(s: &mut State, h: &Handle) -> Result<(Brep, u64), i32> {
    copy_in(s, &body(h))
}

fn combine(s: &mut State, a: Brep, b: Brep, op: i32) -> Result<Brep, i32> {
    match op {
        BOOLEAN_UNION => boolean_union(&mut s.kernel, a, b),
        BOOLEAN_SUBTRACT => boolean_difference(&mut s.kernel, a, b),
        BOOLEAN_INTERSECT => boolean_intersection(&mut s.kernel, a, b),
        _ => return Err(ERR_ARG),
    }
    .map_err(|e| status(&e))
}

pub fn boolean(a: &Handle, b: &Handle, op: i32) -> Result<Handle, i32> {
    let mut s = state();
    // One lock at a time: `a` and `b` may be the same body.
    let (a, _) = copy_of(&mut s, a)?;
    let (b, _) = copy_of(&mut s, b)?;
    let r = combine(&mut s, a, b, op)?;
    result(&mut s, r)
}

pub fn boolean_union_all(breps: &[&Handle]) -> Result<Handle, i32> {
    if breps.is_empty() {
        return Err(ERR_ARG);
    }
    let mut s = state();
    let mut level = Vec::with_capacity(breps.len());
    for h in breps {
        let (c, _) = copy_of(&mut s, h)?;
        level.push(c);
    }
    // Pairwise, level by level, so no body takes part in more than about log2(n) unions.
    while level.len() > 1 {
        let mut next = Vec::with_capacity(level.len().div_ceil(2));
        let mut it = level.into_iter();
        while let Some(a) = it.next() {
            next.push(match it.next() {
                Some(b) => combine(&mut s, a, b, BOOLEAN_UNION)?,
                None => a,
            });
        }
        level = next;
    }
    let r = level.pop().ok_or(ERR_EMPTY)?;
    result(&mut s, r)
}

/// The separate solid regions (the void ones are not counted).
pub fn solid_count(h: &Handle) -> Result<i64, i32> {
    let _kernel = state();
    let b = body(h);
    let infinite = b.infinite_region();
    let n = b.regions().iter().filter(|(id, r)| Some(*id) != infinite && r.is_void == 0).count();
    i64::try_from(n).map_err(|_| ERR_EXCEPTION)
}

/// `SmApiBoolean2d` on sheets of coplanar faces. Runs on copies; empty when either sheet has no
/// faces or nothing is left.
pub fn boolean2d(a: &Handle, b: &Handle, op: i32) -> Result<Handle, i32> {
    let op = match op {
        BOOLEAN_UNION => Boolean2d::Union,
        BOOLEAN_SUBTRACT => Boolean2d::Difference,
        BOOLEAN_INTERSECT => Boolean2d::Intersection,
        _ => return Err(ERR_ARG),
    };
    let mut s = state();
    let (mut a, _) = copy_of(&mut s, a)?;
    let (mut b, _) = copy_of(&mut s, b)?;
    let mut notify = ArenaNotifyState::new(&a);
    notify.seed(&b);
    let mut env = notify.env(None);
    let (mut marks_a, mut marks_b) = (Marks::default(), Marks::default());
    let mut out = None;
    PrimitiveCreation::new(a.infinite_region(), 0, true)
        .boolean_2d(&mut s.kernel, &mut a, &mut b, op, &mut out, (&mut marks_a, &mut marks_b), &mut env)
        .map_err(|e| status(&e))?;
    let r = out.ok_or(ERR_EMPTY)?;
    result(&mut s, r)
}
