//! FFI wrappers for unit `heal.face_props`.
//!
//! Shim: `harness/cpp/shim/heal_face_props.cpp`.
//! Upstream: `source/SMLib/src/SmFaceProps.cpp`, `SmLoopProps.cpp`, `inc/SmLoopProps.h`.
//!
//! | Rust      | shim                        | upstream                                  |
//! |-----------|-----------------------------|-------------------------------------------|
//! | [`run`]   | `sc_heal_face_props_run`    | `SmFaceProps` methods on a one-face B-rep |
//! | [`tree`]  | `sc_heal_face_props_tree`   | `SmLooptreeItem` building and `FindItem`  |
//!
//! Script input (all `f64`): `face_tol`, the plane `O[3] X[3] Y[3] umin vmin umax vmax`, `n_ops`,
//! then per op `op p0 p1 p2 p3`: `0` `SetProps`(stage p0, heal op p1, index p2), `1` `ReSet`(stage
//! p0), `2` `HasProps`(stage p0, face p1 != 0), `3` `HasProblems`(bits p0), `4` `IsPoint3dOnPole`(p0, p1,
//! p2), `5` seed field p0 (index p1, value p2; see [`SEED_FIELDS`]), `6` `SetProps_Gaps` /
//! `Stage2` / `Stage3` directly (which p0 in 1..=3, heal op p1, index p2), `7` `IsPeriodicU` +
//! 2 `IsPeriodicV`. Output per op: the result then [`SNAP_LEN`] snapshot values.
//!
//! Guards: finite values, a plane with non-parallel axes and a non-empty domain, stage and heal
//! op codes in range, field ids in range. The one-face B-rep has a vertex loop only, so the
//! shim never classifies a real loop.

use crate::{call_with_buffer, FfiError};

/// Values per snapshot.
pub const SNAP_LEN: usize = 70;
/// Largest number of ops.
pub const MAX_OPS: usize = 64;
/// Largest number of tree items.
pub const MAX_ITEMS: usize = 32;
/// Number of seedable fields (ids `0..SEED_FIELDS`).
pub const SEED_FIELDS: i64 = 26;

mod sys {
    extern "C" {
        pub fn sc_heal_face_props_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_heal_face_props_tree(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// One script op.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Op {
    /// Op code.
    pub op: i64,
    /// Operands.
    pub p: [f64; 4],
}

/// The face and plane of the one-face B-rep.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceSpec {
    /// Face tolerance.
    pub tol: f64,
    /// Plane origin.
    pub o: [f64; 3],
    /// Plane X axis.
    pub x: [f64; 3],
    /// Plane Y axis.
    pub y: [f64; 3],
    /// Domain `[umin, vmin, umax, vmax]`.
    pub domain: [f64; 4],
}

fn bad<T>(m: &str) -> Result<T, FfiError> {
    Err(FfiError::InvalidArgument(m.into()))
}

/// Checks a script the way the shim needs (anything it would crash on).
pub fn validate(face: &FaceSpec, ops: &[Op]) -> Result<(), FfiError> {
    let vals = face
        .o
        .iter()
        .chain(&face.x)
        .chain(&face.y)
        .chain(&face.domain)
        .chain(core::iter::once(&face.tol));
    if vals.clone().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return bad("face values");
    }
    let cross = [
        face.x[1] * face.y[2] - face.x[2] * face.y[1],
        face.x[2] * face.y[0] - face.x[0] * face.y[2],
        face.x[0] * face.y[1] - face.x[1] * face.y[0],
    ];
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-6 {
        return bad("plane axes");
    }
    if face.domain[2] <= face.domain[0] || face.domain[3] <= face.domain[1] {
        return bad("plane domain");
    }
    if ops.len() > MAX_OPS {
        return bad("op count");
    }
    for o in ops {
        if o.p.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return bad("operand");
        }
        let ok = match o.op {
            0 => (0.0..=6.0).contains(&o.p[0]) && (0.0..=26.0).contains(&o.p[1]),
            1 | 2 => (0.0..=6.0).contains(&o.p[0]),
            3 | 4 | 7 => true,
            5 => (0.0..SEED_FIELDS as f64).contains(&o.p[0]),
            6 => (1.0..=3.0).contains(&o.p[0]) && (0.0..=26.0).contains(&o.p[1]),
            _ => false,
        };
        if !ok || (o.op == 0 || o.op == 6) && o.p[2] < 0.0 {
            return bad("op");
        }
    }
    Ok(())
}

/// Runs the script; returns `[result, snapshot..]` per op.
pub fn run(face: &FaceSpec, ops: &[Op]) -> Result<Vec<f64>, FfiError> {
    validate(face, ops)?;
    let mut input = vec![face.tol];
    input.extend(face.o);
    input.extend(face.x);
    input.extend(face.y);
    input.extend(face.domain);
    input.push(ops.len() as f64);
    for o in ops {
        input.push(o.op as f64);
        input.extend(o.p);
    }
    call_with_buffer(ops.len() * (SNAP_LEN + 1) + 1, |buf, len| unsafe {
        sys::sc_heal_face_props_run(
            input.as_ptr(),
            input.len() as i64,
            buf.as_mut_ptr(),
            buf.len() as i64,
            len,
        )
    })
}

/// A loop tree item: `loop_` (`-1` pole vertex) and `partner` (`-2` none, `-1` pole vertex).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TreeItem {
    /// Loop number or `-1`.
    pub loop_: i64,
    /// Partner loop number, `-1` or `-2`.
    pub partner: i64,
}

/// Builds the tree (`links`: `(kind 0 child / 1 sibling, this, new)`, each `new` linked at most
/// once and no cycles) and runs the `finds` `(start, loop)`; returns `(found, parent)` per find
/// (`-1` none) then the `IsNoArea_Pair` flag per item.
pub fn tree(
    items: &[TreeItem],
    links: &[(i64, i64, i64)],
    finds: &[(i64, i64)],
) -> Result<Vec<f64>, FfiError> {
    tree_validate(items, links, finds)?;
    let mut input = vec![items.len() as f64];
    for it in items {
        input.push(it.loop_ as f64);
        input.push(it.partner as f64);
    }
    input.push(links.len() as f64);
    for &(k, a, c) in links {
        input.extend([k as f64, a as f64, c as f64]);
    }
    input.push(finds.len() as f64);
    for &(s, l) in finds {
        input.extend([s as f64, l as f64]);
    }
    call_with_buffer(2 * finds.len() + items.len() + 1, |buf, len| unsafe {
        sys::sc_heal_face_props_tree(
            input.as_ptr(),
            input.len() as i64,
            buf.as_mut_ptr(),
            buf.len() as i64,
            len,
        )
    })
}

/// The guards of [`tree`].
pub fn tree_validate(
    items: &[TreeItem],
    links: &[(i64, i64, i64)],
    finds: &[(i64, i64)],
) -> Result<(), FfiError> {
    let n = items.len() as i64;
    if items.is_empty() || items.len() > MAX_ITEMS {
        return bad("item count");
    }
    let mut linked = vec![false; items.len()];
    for &(kind, a, c) in links {
        if !(0..=1).contains(&kind) || !(0..n).contains(&a) || !(0..n).contains(&c) || a == c {
            return bad("link");
        }
        let Some(flag) = usize::try_from(c).ok().and_then(|c| linked.get_mut(c)) else {
            return bad("link");
        };
        if *flag {
            return bad("item linked twice");
        }
        *flag = true;
    }
    // no cycles: simulate the pointers AddChild / AddSibling leave; every item is the
    // target of at most one pointer, so there is a cycle exactly when the items reachable
    // from the items nothing points to are not all of them
    let mut child = vec![None::<usize>; items.len()];
    let mut sib = vec![None::<usize>; items.len()];
    for &(kind, a, c) in links {
        let (Ok(a), Ok(c)) = (usize::try_from(a), usize::try_from(c)) else {
            return bad("link");
        };
        let old = if kind == 0 { child.get(a) } else { sib.get(a) }
            .copied()
            .flatten();
        if let Some(slot) = sib.get_mut(c) {
            *slot = old;
        }
        let target = if kind == 0 { child.get_mut(a) } else { sib.get_mut(a) };
        if let Some(slot) = target {
            *slot = Some(c);
        }
    }
    let mut pointed = vec![false; items.len()];
    for t in child.iter().chain(sib.iter()).flatten() {
        if let Some(p) = pointed.get_mut(*t) {
            *p = true;
        }
    }
    let mut stack: Vec<usize> = (0..items.len())
        .filter(|i| !pointed.get(*i).copied().unwrap_or(true))
        .collect();
    let mut reached = 0usize;
    while let Some(i) = stack.pop() {
        reached += 1;
        stack.extend(child.get(i).copied().flatten());
        stack.extend(sib.get(i).copied().flatten());
        if reached > items.len() {
            return bad("cycle");
        }
    }
    if reached != items.len() {
        return bad("cycle");
    }
    for it in items {
        if it.loop_ < -1 || it.loop_ > 1000 || !(-2..=1000).contains(&it.partner) {
            return bad("item");
        }
    }
    for &(s, l) in finds {
        if !(0..n).contains(&s) || !(-1..=1000).contains(&l) {
            return bad("find");
        }
    }
    Ok(())
}
