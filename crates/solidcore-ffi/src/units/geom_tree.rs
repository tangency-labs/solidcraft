//! FFI wrappers for unit `geom.tree`.
//!
//! Shim: `harness/cpp/shim/geom_tree.cpp`.
//! Upstream: `source/SMLib/src/SmTree.cpp` (`SmTree`, `SmTreeNode`, `SmObjectList`).
//!
//! | Rust          | shim                                                       |
//! |---------------|------------------------------------------------------------|
//! | [`spatial`]   | `sc_geom_tree_spatial_ints`, `sc_geom_tree_spatial_floats` |
//! | [`subdiv`]    | `sc_geom_tree_subdiv`                                      |
//!
//! Both run a small program against an upstream tree; the encodings are
//! documented in the shim and validated here before any call.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
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

/// Runs a subdivision-tree program over `desc` (9 ints per node, see the shim);
/// returns its output stream.
pub fn subdiv(desc: &[i64], prog: &[i64]) -> Result<Vec<i64>, FfiError> {
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
            0 | 1 | 4 | 5 => 1,
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
        if sz >= 1 && !(0..n_nodes).contains(&at(a, 0)?) {
            return Err(bad("node index"));
        }
        if op == 2 && (!(0..=3).contains(&at(a, 2)?) || !obj(at(a, 3)?)) {
            return Err(bad("set operands"));
        }
        i += 1 + sz;
    }
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
