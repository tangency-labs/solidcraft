//! FFI wrappers for unit `geom.tree`.
//!
//! Shim: `harness/cpp/shim/geom_tree.cpp`.
//! Upstream: `source/SMLib/src/SmTree.cpp` (`SmTree`, `SmTreeNode`, `SmObjectList`).
//!
//! | Rust          | shim                                                       |
//! |---------------|------------------------------------------------------------|
//! | [`spatial`]   | `sc_geom_tree_spatial_ints`, `sc_geom_tree_spatial_floats` |
//! | [`subdiv`]    | `sc_geom_tree_subdiv`                                      |
//! | [`subdiv_patches`] | `sc_geom_tree_subdiv_patches`                         |
//! | [`patch_lookup`] | `sc_geom_tree_patch_lookup`                              |
//!
//! Both run a small program against an upstream tree; the encodings are
//! documented in the shim and validated here before any call.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_tree_patch_lookup(
            prog: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tree_spatial_ints(
            prog: *const f64,
            n: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tree_spatial_floats(
            prog: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tree_subdiv(
            desc: *const i64,
            n_nodes: i64,
            prog: *const i64,
            n_prog: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tree_subdiv_patches(
            desc: *const i64,
            n_nodes: i64,
            prog: *const i64,
            n_prog: i64,
            scale: f64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Largest object id + 1 (size of the shim's fake object array).
pub const MAX_OBJECTS: i64 = 4096;

/// Number of operands of spatial op `op`, or `None` for an unknown op.
pub fn spatial_op_size(op: i64) -> Option<usize> {
    Some(match op {
        0 | 4..=6 => 6,
        1 => 5,
        2 => 8,
        3 => 7,
        7 | 9 => 0,
        8 => 1,
        _ => return None,
    })
}

fn at<T: Copy>(s: &[T], i: usize) -> Result<T, FfiError> {
    s.get(i).copied().ok_or_else(|| bad("truncated program"))
}

fn obj_ok(v: f64) -> bool {
    v.fract() == 0.0 && v >= -1.0 && v < MAX_OBJECTS as f64
}

fn validate_spatial(prog: &[f64]) -> Result<(), FfiError> {
    let mut i = 0usize;
    let mut point = false;
    while i < prog.len() {
        let op = at(prog, i)?;
        if op.fract() != 0.0 || !(0.0..=9.0).contains(&op) {
            return Err(bad("op"));
        }
        let op = op as i64;
        let sz = spatial_op_size(op).ok_or_else(|| bad("op"))?;
        if i + 1 + sz > prog.len() {
            return Err(bad("truncated program"));
        }
        let a = prog
            .get(i + 1..i + 1 + sz)
            .ok_or_else(|| bad("truncated program"))?;
        if (op <= 1) != (i == 0) {
            return Err(bad("constructor must be first and only"));
        }
        match op {
            1 => point = true,
            2 => {
                if point {
                    return Err(bad("AddToSpatialTree on a point tree"));
                }
                let (o, g) = (at(a, 6)?, at(a, 7)?);
                if !obj_ok(o) || !(0.0..=3.0).contains(&g) || g.fract() != 0.0 {
                    return Err(bad("add operands"));
                }
            }
            3 if !obj_ok(at(a, 6)?) => return Err(bad("rm object")),
            8 if at(a, 0)?.fract() != 0.0 => return Err(bad("node index")),
            _ => {}
        }
        i += 1 + sz;
    }
    if prog.is_empty() {
        return Err(bad("empty program"));
    }
    Ok(())
}

/// Runs a spatial-tree program; returns its integer and float output streams.
pub fn spatial(prog: &[f64]) -> Result<(Vec<i64>, Vec<f64>), FfiError> {
    validate_spatial(prog)?;
    let n = prog.len() as i64;
    let ints = call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: prog valid for n reads; buf valid for buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_tree_spatial_ints(
                prog.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    let floats = call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: as above.
        unsafe {
            sys::sc_geom_tree_spatial_floats(
                prog.as_ptr(),
                n,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    Ok((ints, floats))
}

/// Rejects node links upstream cannot walk. The neighbour searches
/// (`GetLeftNeighbors` .. `GetTopNeighbors`, `SmTree.cpp:3027-3240`) recurse up
/// through `m_pParent` and into `m_pParent->m_pChild1/2`, the `Get*Side` searches
/// (`SmTree.cpp:3244-3510`) recurse into `m_pChild1` and `m_pChild2`, and
/// `PropagateToParents` (`SmTree.cpp:2749`) reads the sibling's data, all without
/// null or cycle checks: a node with one child, a parent that does not list the
/// node, a child whose parent is another node, or cyclic parent links crash
/// (null dereference or unbounded recursion). Accepted: every node has no child
/// or two distinct children, parent and child links agree, and every parent
/// chain ends. Trees built by upstream (and the generator) always have this shape.
fn well_formed(desc: &[i64], n_nodes: i64) -> Result<(), FfiError> {
    let link = |k: i64, j: usize| -> Result<i64, FfiError> {
        let k = usize::try_from(k).map_err(|_| bad("node link"))?;
        at(desc, k * 9 + j)
    };
    for k in 0..n_nodes {
        let (parent, c1, c2) = (link(k, 0)?, link(k, 1)?, link(k, 2)?);
        if (c1 < 0) != (c2 < 0) || (c1 >= 0 && c1 == c2) {
            return Err(bad("node with one child"));
        }
        for c in [c1, c2] {
            if c >= 0 && link(c, 0)? != k {
                return Err(bad("child whose parent is another node"));
            }
        }
        if parent >= 0 && link(parent, 1)? != k && link(parent, 2)? != k {
            return Err(bad("parent that does not list the node"));
        }
        // the parent chain ends within n_nodes steps
        let mut cur = parent;
        let mut steps = 0;
        while cur >= 0 {
            steps += 1;
            if steps > n_nodes {
                return Err(bad("cyclic parent links"));
            }
            cur = link(cur, 0)?;
        }
    }
    Ok(())
}

/// Checks a subdivision descriptor (9 ints per node) and its program.
fn validate_subdiv(desc: &[i64], prog: &[i64]) -> Result<(), FfiError> {
    if desc.is_empty() || !desc.len().is_multiple_of(9) {
        return Err(bad("desc"));
    }
    let n_nodes = (desc.len() / 9) as i64;
    let in_nodes = |v: i64| (-1..n_nodes).contains(&v);
    let obj = |v: i64| (-1..MAX_OBJECTS).contains(&v);
    for d in desc.as_chunks::<9>().0 {
        let f = |j| at(d, j);
        if !(in_nodes(f(0)?) && in_nodes(f(1)?) && in_nodes(f(2)?)) {
            return Err(bad("node link"));
        }
        if !(0..=8).contains(&f(3)?) || !(0..=2).contains(&f(4)?) || !(0..=3).contains(&f(6)?) {
            return Err(bad("node enum"));
        }
        if !obj(f(7)?) || !obj(f(8)?) {
            return Err(bad("node object"));
        }
    }
    well_formed(desc, n_nodes)?;
    let mut i = 0usize;
    while i < prog.len() {
        let op = at(prog, i)?;
        let sz = match op {
            0 | 1 | 4..=7 => 1,
            2 => 4,
            3 => 0,
            _ => return Err(bad("op")),
        };
        if i + 1 + sz > prog.len() {
            return Err(bad("truncated program"));
        }
        let a = prog
            .get(i + 1..i + 1 + sz)
            .ok_or_else(|| bad("truncated program"))?;
        if sz >= 1 && op != 7 && !(0..n_nodes).contains(&at(a, 0)?) {
            return Err(bad("node index"));
        }
        if op == 2 && (!(0..=3).contains(&at(a, 2)?) || !obj(at(a, 3)?)) {
            return Err(bad("set operands"));
        }
        if op == 7 && !obj(at(a, 0)?) {
            return Err(bad("owner surface"));
        }
        i += 1 + sz;
    }
    Ok(())
}

/// Runs a subdivision-tree program, returning presence flags and node relations.
pub fn subdiv(desc: &[i64], prog: &[i64]) -> Result<Vec<i64>, FfiError> {
    validate_subdiv(desc, prog)?;
    let n_nodes = (desc.len() / 9) as i64;
    call_with_buffer(256, |buf: &mut [i64], len| {
        // SAFETY: desc/prog valid for their lengths (checked above); buf valid for
        // buf.len() writes; len valid.
        unsafe {
            sys::sc_geom_tree_subdiv(
                desc.as_ptr(),
                n_nodes,
                prog.as_ptr(),
                prog.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Validate a real-patch lookup fixture: `ns owner`, `ns * 16` homogeneous control
/// point values (bilinear 2x2 surfaces), `nn`, `nn * (parent aux patch surface)`,
/// `nq`, query node indices. Each parent precedes its child; the native walk is
/// bounded to 64 nodes. Surface and patch indices use -1 for absence.
pub fn validate_patch_lookup(prog: &[f64]) -> Result<(), FfiError> {
    let mut cursor = 0;
    let mut number = || {
        let v = at(prog, cursor)?;
        cursor += 1;
        Ok::<_, FfiError>(v)
    };
    let integer = |v: f64, lo: i64, hi: i64| -> Result<i64, FfiError> {
        if !v.is_finite() || v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad("patch fixture index/count"));
        }
        Ok(v as i64)
    };
    let ns = integer(number()?, 1, 16)?;
    integer(number()?, -1, ns - 1)?;
    for _ in 0..ns * 4 {
        for _ in 0..3 {
            let v = number()?;
            if !v.is_finite() || v.abs() > 1e50 {
                return Err(bad("patch coordinate"));
            }
        }
        let weight = number()?;
        if !weight.is_finite() || !(1e-6..=1e6).contains(&weight) {
            return Err(bad("patch weight"));
        }
    }
    let nn = integer(number()?, 1, 64)?;
    let mut children = [0; 64];
    for i in 0..nn {
        let parent = integer(number()?, -1, i - 1)?;
        if parent >= 0 {
            let count = children
                .get_mut(parent as usize)
                .ok_or_else(|| bad("patch parent"))?;
            *count += 1;
            if *count > 2 {
                return Err(bad("nonbinary patch tree"));
            }
        }
        let aux = integer(number()?, 0, 6)?;
        if ![0, 3, 4, 6].contains(&aux) {
            return Err(bad("patch aux type"));
        }
        let patch = integer(number()?, -1, ns - 1)?;
        integer(number()?, -1, ns - 1)?;
        if aux != 3 && patch != -1 {
            return Err(bad("patch requires Bezier aux"));
        }
    }
    let nq = integer(number()?, 1, 128)?;
    for _ in 0..nq {
        integer(number()?, 0, nn - 1)?;
    }
    if cursor != prog.len() {
        return Err(bad("trailing patch fixture data"));
    }
    Ok(())
}

/// Run `GetFirstBezierPatch` on real patches and temporary B-spline surfaces.
/// Returns, per query, the selected pool index (-1 absent), then the selected
/// NURB's `n m p q r s`, homogeneous control net and U/V knot arrays.
pub fn patch_lookup(prog: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_patch_lookup(prog)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: validation checks every count, index and record before the shim
        // reads it. The shim honours the output capacity; all buffers live here.
        unsafe {
            sys::sc_geom_tree_patch_lookup(
                prog.as_ptr(),
                prog.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Exports each non-null `GetFirstBezierPatch` result as native NURBS data:
/// `n m p q r s`, homogeneous control points (V fastest), then U and V knots.
/// The shim constructs rational bilinear patches using the documented seed recipe
/// in `geom_tree.cpp`; `scale` multiplies their Cartesian coordinates.
pub fn subdiv_patches(desc: &[i64], prog: &[i64], scale: f64) -> Result<Vec<f64>, FfiError> {
    validate_subdiv(desc, prog)?;
    if !scale.is_finite() {
        return Err(bad("patch scale"));
    }
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: validated, well-formed node links and complete program; all buffers
        // are valid for their supplied lengths, and len is a valid output pointer.
        unsafe {
            sys::sc_geom_tree_subdiv_patches(
                desc.as_ptr(),
                (desc.len() / 9) as i64,
                prog.as_ptr(),
                prog.len() as i64,
                scale,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
