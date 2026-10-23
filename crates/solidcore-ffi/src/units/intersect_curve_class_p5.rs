//! Native oracle for `intersect.curve_class_p5` (`SmCurveClass.cpp`).
//!
//! Flat input: `op shape dim_minus_2 ignore flag1 flag2 flag3 target index scale
//! source_zone angle n_intervals n_solutions optional_xsect_tol other_n_intervals`,
//! followed by both
//! classifications' boundaries, then their point-class codes (`3` per interval),
//! then solutions (`type start_t start_other end_t end_other start_gap end_gap`).
//! A negative optional tolerance selects upstream's default. Shapes are line/circle,
//! or a stationary quadratic source/target/both;
//! targets are real line/plane/vertex/face. Counts and native undefined branches are
//! checked before entry. Output contains status, operation outputs, both full interval
//! states and every mutable solution end. No native pointers cross the boundary.
//!
//! **B-rep cases** ([`topo`], shim `harness/cpp/shim/intersect_curve_class_p5_topo.cpp`,
//! `sc_intersect_curve_class_p5_topo`) run on real `SmBrep` objects. The input is a
//! B-rep block of unit `topology.edgeuse_p1` (`path` is the `.smb` file of a file
//! block), then
//!
//! - edits `n (kind sel value)*` (`n <= 64`; the entity `sel` modulo the count of its
//!   kind, in export order): kind 0 face, 1 edge, 2 vertex: the raw zone tolerance set to
//!   `value` (`0..=1e3`); 3: `GetOrCreateUVTrimCurve` on the face's loop edge uses; 4, 5,
//!   6: the raw vertex point's x, y or z moved by `value` (`|value| <= 1e3`);
//! - mates `n (e1 e2)*` (`n <= 16`): with `n > 0` the block is built twice (same edits)
//!   and an `SmTopologyIntersector` relates edge `e1` of the first B-rep to edge `e2` of
//!   the second (`Relate`). No edge may appear twice on one side: the Rust side has no
//!   `Relate`, and a second relation of an edge is a refused `Relate` upstream;
//!
//! then the op's arguments ([`TopoOp`]). Indices are export indices of the first
//! B-rep. A surface block is unit `topology.edgeuse_p1`'s (`1 O[3] N[3]` an `SmPlane`,
//! `0 ...` an `SmBSplineSurface`), a curve block likewise (`SmBSplineCurve`).
//!
//! | [`TopoOp`]   | arguments | output after `0` |
//! |--------------|-----------|------------------|
//! | `Export`     | `which` (`1`, the second B-rep, only with mates) | that B-rep's export (unit `topology.edgeuse_p1`'s layout) |
//! | `EdgeInsert` | `e ok_set ignore tol P[3] Q[3] opt_tol has_srf [surface surface] ns (type t0 t1 t2 t3 g0 g1)*` | `status`, the classification, the solutions, the topology state |
//! | `Vertices`   | `f big uv_ok tol cc_face curve has_srf [surface surface] has_uv1 [curve] has_uv2 [curve] nv v*` | `status is_curve_current`, the curve block, the classification, the topology state |
//!
//! `EdgeInsert` runs `sm_InsertSolutions` on `SmCurveClassification(line, [0, 1], NULL,
//! tol)` of the line segment `P -> Q` (`SmBSplineCurve::CreateLineSegment`), with
//! `SetIgnoreTangency(ignore)`, `SetOKToSetTolerance(ok_set)` and, with `has_srf`,
//! `SetIntersectData(s1, s2, NULL)`, against edge `e` (solutions name the line and the
//! edge; `opt_tol < 0` is the default tolerance). `Vertices` runs `IntersectVertices`
//! of the vertices `v` against face `f`'s surface on `SmCurveClassification(curve,
//! natural interval, uv1, tol)` (a 3D curve), with `SetIntersectData(s1, s2, uv2, TRUE,
//! ti)` when `has_srf` (`ti` the intersector when there are mates) and `SetFace(f)` when
//! `cc_face`. A classification is `size min max` then per interval `min max` and three
//! point classifications `class kind brep index deviation tparam u v U V W params_set
//! sss_flag sss(x y z) presnap src_zone` (object `kind`: 0 none, 1 face, 2 edge, 3 vertex
//! with the B-rep and export index, 4 the classification curve, 5 and 6 the surfaces).
//! The topology state is, per B-rep, every vertex tolerance then per edge use whether it
//! has a UV trim curve. The shim refuses (`-3` alone) an edge without curve or end
//! vertex, a face without surface or surface owner, and with large gaps a vertex
//! without a vertex use on the gap face, all of which upstream dereferences; `-1
//! status` is a failed B-rep build, `-2` a refused `Relate`.

use crate::{call_with_buffer, FfiError};

use super::topology_edgeuse_p1::{self as eu1, bad, brep_block, curve, surface, Reader};

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p5_topo(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_intersect_curve_class_p5_run(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn invalid() -> FfiError {
    FfiError::InvalidArgument("invalid curve_class_p5 input or undefined native path".into())
}

/// Validates a bounded oracle input before native execution.
///
/// Native topology mutation on bare objects and the
/// null-edge short-range tangency bug are excluded. Valid non-topology insertion,
/// extraction, vertex projection and merge skip paths remain covered.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    if input.iter().any(|x| !x.is_finite() || x.abs() > 1e21) {
        return Err(invalid());
    }
    let &[op, shape, dim, ignore, flag1, flag2, flag3, target, index, scale, tol, _angle, n, ns, _opt_tol, other_n] =
        input.first_chunk::<16>().ok_or_else(invalid)?;
    let int = |x: f64, max: u32| -> Result<usize, FfiError> {
        if x < 0.0 || x > f64::from(max) || x.fract() != 0.0 {
            return Err(invalid());
        }
        Ok(x as usize)
    };
    let op = int(op, 6)?;
    int(shape, 4)?;
    int(dim, 1)?;
    for flag in [ignore, flag1, flag2, flag3] {
        int(flag, 1)?;
    }
    let target = int(target, 3)?;
    int(index, 1)?;
    let n = int(n, 32)?;
    let ns = int(ns, 32)?;
    let other_n = int(other_n, 32)?;
    if input.len() != 18 + 4 * (n + other_n) + 7 * ns || scale <= 0.0 || scale > 1e6 || tol < 0.0 {
        return Err(invalid());
    }
    for (start, count) in [(16, n), (17 + n, other_n)] {
        let bounds = input.get(start..start + count + 1).ok_or_else(invalid)?;
        if bounds.windows(2).any(|w| matches!(w,[a,b] if a>=b)) {
            return Err(invalid());
        }
    }
    let at = 18 + n + other_n;
    let class_count = 3 * (n + other_n);
    let classes = input.get(at..at + class_count).ok_or_else(invalid)?;
    for &class in classes {
        let valid = class == 100.0
            || class == 110.0
            || (class == 106.0 && target == 0)
            || (class == 107.0 && target == 1)
            || (class == 104.0 && target == 2)
            || (class == 102.0 && target == 3);
        if !valid || ((op == 4 || op == 5) && (101.0..=104.0).contains(&class)) {
            return Err(invalid());
        }
    }
    if ((op == 0 || op == 1) && n == 0 && ns > 0)
        || (op == 3 && (n == 0 || other_n == 0))
        || (op == 4 && n != other_n)
        || (op == 6 && flag1 != 0.0 && flag2 != 0.0)
        // UB-109: `IntersectVertices` -> `InsertPointClass` -> `FindInterval` reads the
        // first interval of an empty classification (SIGSEGV); the shim only passes
        // a vertex when flag1 is set
        || (op == 6 && n == 0 && flag1 != 0.0)
    {
        return Err(invalid());
    }

    for &[typ, t0, t1, t2, t3, gap0, gap1] in input
        .get(at + class_count..)
        .ok_or_else(invalid)?
        .as_chunks::<7>()
        .0
    {
        int(typ, 4)?;
        let _ = (t0, t1, t2, t3); // Finite curve parameters may span any subdomain.
        if gap0 < 0.0 || gap1 < 0.0 {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Executes a validated oracle input; native guards also reject dynamically unsafe paths.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: input's exact size, every count/code and all native preconditions
        // above are validated; output and length have caller-owned valid storage.
        unsafe {
            sys::sc_intersect_curve_class_p5_run(
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}

/// The B-rep ops of [`topo`] (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopoOp {
    /// Export one B-rep after the edits and mates.
    Export = 0,
    /// `sm_InsertSolutions` against an edge.
    EdgeInsert = 1,
    /// `SmCurveClassification::IntersectVertices`.
    Vertices = 2,
}

/// The most edits of a B-rep case.
pub const MAX_EDITS: usize = 64;
/// The most edge mates of a B-rep case.
pub const MAX_MATES: usize = 16;

fn tolerance(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    if !(0.0..=1e3).contains(&t) {
        return Err(bad("tolerance out of range"));
    }
    Ok(t)
}

/// Validates a B-rep case of [`topo`] without running it.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; the errors of building and
/// exporting the B-rep (unit `topology.edgeuse_p1`'s shim).
pub fn validate_topo(op: TopoOp, path: &str, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    let Some(c) = eu1::counts(&export) else {
        // the B-rep cannot be built: the shim reports it before reading further
        return Ok(());
    };
    let (n_f, n_e, n_v) = (c[3], c[7], c[9]);
    for _ in 0..rd.index(MAX_EDITS)? {
        let kind = rd.index(6)?;
        rd.index(usize::MAX >> 12)?;
        let value = rd.finite()?;
        if (kind <= 2 && !(0.0..=1e3).contains(&value)) || (kind >= 4 && value.abs() > 1e3) {
            return Err(bad("edit value out of range"));
        }
    }
    let n_mates = rd.index(MAX_MATES)?;
    let (mut firsts, mut seconds) = (Vec::new(), Vec::new());
    for _ in 0..n_mates {
        let (e1, e2) = (rd.below(n_e)?, rd.below(n_e)?);
        if firsts.contains(&e1) || seconds.contains(&e2) {
            return Err(bad("an edge mated twice"));
        }
        firsts.push(e1);
        seconds.push(e2);
    }
    match op {
        TopoOp::Export => {
            if rd.flag()? && n_mates == 0 {
                return Err(bad("no second B-rep without mates"));
            }
        }
        TopoOp::EdgeInsert => {
            rd.below(n_e)?;
            rd.flag()?;
            rd.flag()?;
            tolerance(&mut rd)?;
            rd.vec3()?;
            rd.vec3()?;
            rd.finite()?;
            if rd.flag()? {
                surface(&mut rd)?;
                surface(&mut rd)?;
            }
            for _ in 0..rd.index(32)? {
                rd.index(4)?;
                rd.finites(4)?;
                let (g0, g1) = (rd.finite()?, rd.finite()?);
                if g0 < 0.0 || g1 < 0.0 {
                    return Err(bad("negative solution value"));
                }
            }
        }
        TopoOp::Vertices => {
            rd.below(n_f)?;
            rd.flag()?;
            rd.flag()?;
            tolerance(&mut rd)?;
            rd.flag()?;
            if input.get(rd.at) != Some(&3.0) {
                return Err(bad("the classification curve must be 3D"));
            }
            curve(&mut rd)?;
            if rd.flag()? {
                surface(&mut rd)?;
                surface(&mut rd)?;
            }
            for _ in 0..2 {
                if rd.flag()? {
                    if input.get(rd.at) != Some(&2.0) {
                        return Err(bad("a UV curve must be 2D"));
                    }
                    curve(&mut rd)?;
                }
            }
            for _ in 0..rd.index(64)? {
                rd.below(n_v)?;
            }
        }
    }
    rd.done()
}

/// Runs a validated B-rep case (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn topo(op: TopoOp, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_topo(op, path, input)?;
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads (validated above); out is valid
        // for out.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_intersect_curve_class_p5_topo(
                op as i32,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_malformed_lengths_counts_and_null_edge_tangency() {
        assert!(run(&[]).is_err());
        let mut v = vec![
            0., 0., 1., 1., 0., 0., 0., 0., 0., 1., 0., 0., 1., 1., -1., 1., 0., 1., 0., 1., 100.,
            100., 100., 100., 100., 100., 2., 0., 0., 0.001, 0.001, 0., 0.,
        ];
        assert!(run(&v).is_err());
        v[12] = 1e20;
        assert!(run(&v).is_err());
        v[12] = 1.;
        v.pop();
        assert!(run(&v).is_err());
    }
}
