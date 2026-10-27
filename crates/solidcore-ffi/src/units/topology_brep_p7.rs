//! FFI wrappers for unit `topology.brep_p7`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p7.cpp` (`sc_topology_brep_p7_eval`).
//! Upstream: `source/SMLib/src/SmBrep.cpp`.
//!
//! A curve block is `dim n p m cpts(x y z w)*(n+1) knots*(m+1)` (an `SmBSplineCurve`;
//! `w = NOW` non-rational, `z = NOZ` in 2D), a surface block `1 O[3] N[3]` (an `SmPlane`) or `0 oob form n
//! m p q r s cpts knots_u knots_v` (an `SmBSplineSurface`), as `sc_topology_fixture.h`
//! reads them. Outputs start with the status (`0` success, else the `SmStatus`) unless
//! noted; `-2` is "upstream would dereference null" (not called), `-3` "not run".
//!
//! | [`Op`]              | input | output |
//! |---------------------|-------|--------|
//! | `Read`              | `n byte*n file_type rebuild` | `status [counts]` (`SmBrep::ReadFromFile` of the bytes) |
//! | `MakeFaceWithCurves`| `surface u0 v0 u1 v1 orient region_null crv_on_surf nloops count* has3d has_uv ncurves (curve3d? curveuv? orient)* npts pt[3]*` | `-2 \| status [face shell region counts]` (`-2`: `crv_on_surf` TRUE with 3D but no UV curves, where upstream dereferences the null UV array) |
//! | `MakeFace`          | `kind side orient region_null` (`kind` 0: null surface, 1: a square wire loop on `z = 0`) | `-3 \| status [face shell region counts]` (`-3`: the square's lines cannot be made) |
//! | `Glue`              | `e1 e2 orient gap nesting`, B-rep block | `0 (-2 \| status counts edge_tols vertex_tols)` (tolerances sorted) (`-2`: `SM_OT_SAME`, equal intervals and one edge without curve) (`-1 status` when the B-rep build fails) |
//! | `Copy`              | `n use_merge`, B-rep block | `0 status nfaces counts` |
//! | `MergeSelf`         | `face tol ncurves (raw curve3d raw curveuv1 raw curveuv2)*`, B-rep block | `0 (-2 \| status nedges counts edges*)` (UV curves 2D) |
//!
//! `counts` are the region, shell, face, loop, edge and vertex counts of the B-rep made.
//! Each returned edge contains its used interval, optional endpoint positions, curve
//! type (or -1), and, when present, its natural interval and five evaluated samples
//! with statuses; its edge uses follow in list order with each UV trim's type,
//! natural interval and five UV samples if present. Raw flags restore supplied knots
//! after construction; distinct raw knots must be more than `1e-12` apart (UB-266).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: clamped
//! B-splines of degree 1 to 5 with at most 33 control points, positive (or `NOW`)
//! weights and finite values, planes with a non-zero normal, loop counts that add up to
//! the curves, indices inside the B-rep's export. For `Read` it applies the guard of
//! unit `topology.brep_data` to the bytes (a rejected stream is
//! [`FfiError::InvalidArgument`]) and, for bytes that are not exactly one of the shipped
//! ASCII test files, runs the read in a fresh child process
//! ([`crate::isolate::fresh_process`]): `MakeTopologyFromData` uses the cross-reference
//! indices of such a stream unchecked (`plan/upstream-issues.md` UB-164), and a read that
//! crashes there, or runs past [`READ_SECONDS`], is [`FfiError::Status`]`(`[`SC_ERR_CHILD_CRASHED`](crate::SC_ERR_CHILD_CRASHED)`)`.

use super::topology_brep_data as bd;
use super::topology_vertexuse as fixture;
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e12;
/// Largest number of curves (and loops, loop points).
pub const MAX_CURVES: usize = 64;
/// Largest file size of `Read`.
pub const MAX_BYTES: usize = 1 << 22;

/// `NOW`: the weight of a non-rational control point.
const NOW: f64 = -f64::MAX;
/// `NOZ`: the z of a 2D control point.
const NOZ: f64 = f64::MAX;

/// The run time in seconds of a modified stream's read in its child process (a shipped
/// file reads in well under a second).
pub const READ_SECONDS: u32 = 30;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Read = 1,
    MakeFaceWithCurves = 2,
    MakeFace = 3,
    Glue = 4,
    Copy = 5,
    MergeSelf = 6,
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p7_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn signed(&mut self, max: usize) -> Result<Option<usize>, FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(None);
        }
        self.at -= 1;
        self.index(max).map(Some)
    }

    fn orient(&mut self) -> Result<(), FfiError> {
        // SM_OT_UNKNOWN (0), SM_OT_SAME (1) or SM_OT_OPPOSITE (2): upstream tests for
        // SAME and treats anything else as the other branch
        self.index(2).map(|_| ())
    }
}

/// Checks a clamped, non-decreasing knot vector of degree `p` with a non-empty domain.
fn check_knots(knots: &[f64], p: usize) -> Result<(), FfiError> {
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("no knots"));
    };
    if b.partial_cmp(&a) != Some(core::cmp::Ordering::Greater) {
        return Err(bad("empty knot domain"));
    }
    let lo = knots.iter().take(p + 1).all(|&k| k == a);
    let hi = knots.iter().rev().take(p + 1).all(|&k| k == b);
    if !lo || !hi {
        return Err(bad("knots not clamped"));
    }
    Ok(())
}

fn points(rd: &mut Reader<'_>, count: usize, dim2: bool) -> Result<(), FfiError> {
    for _ in 0..count {
        rd.finite()?;
        rd.finite()?;
        if dim2 {
            // a 2D control point has z = NOZ
            if rd.num()? != NOZ {
                return Err(bad("2D control point z must be NOZ"));
            }
        } else {
            rd.finite()?;
        }
        let w = rd.num()?;
        if w != NOW && !(w.is_finite() && w > 0.0) {
            return Err(bad("weight must be NOW or positive"));
        }
    }
    Ok(())
}

fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve dimension 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n, m = n+p+1"));
    }
    points(rd, n + 1, dim == 2)?;
    let knots = (0..=m)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&knots, p)
}

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if rd.index(1)? == 1 {
        for _ in 0..3 {
            rd.finite()?;
        }
        let n = [rd.finite()?, rd.finite()?, rd.finite()?];
        // (SmPlane's precondition: LengthSquared() > SM_EFF_ZERO_SQ, SmPlane.cpp)
        if n.iter().map(|c| c * c).sum::<f64>() <= 1.0e-24 {
            return Err(bad("zero plane normal"));
        }
        return Ok(());
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    points(rd, (n + 1) * (m + 1), false)?;
    let u = (0..=r)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..=s)
        .map(|_| rd.finite())
        .collect::<Result<Vec<_>, _>>()?;
    check_knots(&u, p)?;
    check_knots(&v, q)
}

/// The shipped ASCII test files (`upstream/TestFiles/pt_TestFiles`) whose exact bytes the
/// native read is run on without the B-rep data check.
macro_rules! test_file {
    ($f:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../upstream/TestFiles/pt_TestFiles/",
            $f
        ))
        .as_slice()
    };
}

const TRUSTED_FILES: [&[u8]; 15] = [
    test_file!("Mouse.smb"),
    test_file!("Endcap.smb"),
    test_file!("box_with_large_fillet.smb"),
    test_file!("cone.smb"),
    test_file!("disk.smb"),
    test_file!("ellips_ts2.smb"),
    test_file!("plane_circ.smb"),
    test_file!("plane_square.smb"),
    test_file!("sphere.smb"),
    test_file!("surf_extrude.smb"),
    test_file!("surf_rev.smb"),
    test_file!("surf_sphere150.smb"),
    test_file!("torus.smb"),
    test_file!("toruswvy.smb"),
    test_file!("vase_rev.smb"),
];

/// Whether `bytes`, read as `binary` or not, are exactly one of [`TRUSTED_FILES`].
fn trusted(bytes: &[u8], binary: bool) -> bool {
    !binary && TRUSTED_FILES.contains(&bytes)
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input or an index outside the B-rep;
/// [`FfiError::Status`] for a shim failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Read => {
            let n = rd.index(MAX_BYTES)?;
            let mut bytes = Vec::new();
            for _ in 0..n {
                bytes.push(u8::try_from(rd.index(255)?).map_err(|_| bad("byte"))?);
            }
            let binary = rd.flag()?;
            rd.flag()?;
            rd.done()?;
            let trust = trusted(&bytes, binary);
            // the B-rep data guard
            bd::guard(&bd::Request {
                mode: bd::Mode::ReadFile,
                binary,
                version: 0,
                dim: 0,
                persist: false,
                source: 0,
                pre: 0,
                data: bytes,
                upstream_file: trust,
            })?;
            if !trust {
                // MakeTopologyFromData uses a modified stream's indices unchecked (UB-164):
                // its read runs in a fresh child process, which some such indices also
                // keep looping: one that runs past READ_SECONDS is ended as crashed
                return crate::isolate::fresh_process_within(READ_SECONDS, || raw(op, path, input));
            }
        }
        Op::MakeFaceWithCurves => {
            surface(&mut rd)?;
            for _ in 0..4 {
                rd.finite()?;
            }
            rd.orient()?;
            rd.flag()?;
            rd.index(2)?;
            let nl = rd.index(MAX_CURVES)?;
            let mut total = 0usize;
            for _ in 0..nl {
                total += rd.index(MAX_CURVES)?;
            }
            let has3d = rd.flag()?;
            let hasuv = rd.flag()?;
            let nc = rd.index(MAX_CURVES)?;
            // (without curve arrays upstream fails before reading the counts)
            // (upstream reads only the first `total` curves: extra ones are unused)
            if (has3d || hasuv) && total > nc {
                return Err(bad("loop counts exceed the curves"));
            }
            for _ in 0..nc {
                if has3d {
                    curve(&mut rd)?;
                }
                if hasuv {
                    curve(&mut rd)?;
                }
                rd.orient()?;
            }
            let np = rd.index(MAX_CURVES)?;
            for _ in 0..(3 * np) {
                rd.finite()?;
            }
            rd.done()?;
        }
        Op::MakeFace => {
            let kind = rd.index(1)?;
            // (kind 1: the shim builds the square's lines, which need distinct ends; kind 0
            // does not use the side)
            let side = rd.finite()?;
            if kind == 1 && side.partial_cmp(&0.0) != Some(core::cmp::Ordering::Greater) {
                return Err(bad("square side must be positive"));
            }
            rd.orient()?;
            rd.flag()?;
            rd.done()?;
        }
        Op::Glue | Op::Copy | Op::MergeSelf => {
            // the topology.vertexuse wrapper validates the block itself and exports
            let (block, args) = split_block(op, input)?;
            if op == Op::MergeSelf {
                merge_self_curves(input)?;
            }
            let export = fixture::eval(fixture::Op::Export, path, block)?;
            let Some(c) = fixture::counts(&export) else {
                return Ok(export);
            };
            let [_, _, _, n_f, _, _, _, n_e, ..] = c;
            let mut ar = Reader::new(args);
            match op {
                Op::Glue => {
                    for _ in 0..2 {
                        if let Some(i) = ar.signed(MAX_INDEX << 20)? {
                            if i >= n_e {
                                return Err(bad("edge index out of range"));
                            }
                        }
                    }
                    ar.orient()?;
                    ar.finite()?;
                    ar.flag()?;
                }
                Op::Copy => {
                    ar.index(1 << 20)?;
                    ar.flag()?;
                }
                _ => {
                    if ar.index(1 << 20)? >= n_f {
                        return Err(bad("face index out of range"));
                    }
                    ar.finite()?;
                    // (the curves: merge_self_curves)
                    ar.at = args.len();
                }
            }
            ar.done()?;
        }
    }
    raw(op, path, input)
}

/// Validates `MergeSelf`'s curves (`face tol ncurves (curve3d curveuv1 curveuv2)*`, the
/// UV curves 2D); the length of its arguments.
fn merge_self_curves(input: &[f64]) -> Result<usize, FfiError> {
    let mut rd = Reader::new(input);
    rd.num()?;
    rd.num()?;
    let nc = rd.index(MAX_CURVES)?;
    for _ in 0..nc {
        for uv in [false, true, true] {
            let raw = rd.flag()?;
            let start = rd.at;
            if uv && rd.num()? != 2.0 {
                return Err(bad("UV curve dimension must be 2"));
            }
            rd.at = start;
            curve(&mut rd)?;
            if raw {
                let block = input.get(start..rd.at).ok_or_else(|| bad("curve block"))?;
                let n = *block.get(1).ok_or_else(|| bad("curve count"))? as usize;
                let p = *block.get(2).ok_or_else(|| bad("curve degree"))? as usize;
                let knot_start = n
                    .checked_add(1)
                    .and_then(|n| n.checked_mul(4))
                    .and_then(|n| n.checked_add(4))
                    .ok_or_else(|| bad("curve size overflow"))?;
                let knots = block.get(knot_start..).ok_or_else(|| bad("curve knots"))?;
                let (&first, rest) = knots.split_first().ok_or_else(|| bad("no knots"))?;
                let mut unique = first;
                let mut multiplicity = 1;
                for &knot in rest {
                    if knot == unique {
                        multiplicity += 1;
                    } else {
                        if knot - unique <= 1.0e-12 || multiplicity > p + 1 {
                            return Err(bad("unsafe raw knot spacing or multiplicity"));
                        }
                        unique = knot;
                        multiplicity = 1;
                    }
                }
                if multiplicity > p + 1 {
                    return Err(bad("unsafe raw knot multiplicity"));
                }
            }
        }
    }
    Ok(rd.at)
}

/// The number of op arguments before the B-rep block of a B-rep op.
fn n_args(op: Op, input: &[f64]) -> Result<usize, FfiError> {
    Ok(match op {
        Op::Glue => 5,
        Op::Copy => 2,
        Op::MergeSelf => merge_self_curves(input)?,
        _ => 0,
    })
}

/// Splits a B-rep op's input into its arguments (first) and the B-rep block.
fn split_block(op: Op, input: &[f64]) -> Result<(&[f64], &[f64]), FfiError> {
    let n = n_args(op, input)?;
    Ok((
        input.get(n..).ok_or_else(|| bad("input too short"))?,
        input.get(..n).ok_or_else(|| bad("input too short"))?,
    ))
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(256, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_topology_brep_p7_eval(
                op as i32,
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A degree-1 line curve block of dimension `dim`.
    fn line(dim: f64) -> Vec<f64> {
        let z = if dim == 2.0 { NOZ } else { 0.0 };
        let mut v = vec![dim, 1.0, 1.0, 3.0];
        v.extend([0.0, 0.0, z, NOW, 1.0, 1.0, z, NOW]);
        v.extend([0.0, 0.0, 1.0, 1.0]);
        v
    }

    fn merge_self_args(curves: &[(f64, f64, f64)]) -> Vec<f64> {
        let mut v = vec![0.0, 1.0e-5, curves.len() as f64];
        for &(c3, u1, u2) in curves {
            v.push(0.0);
            v.extend(line(c3));
            v.push(0.0);
            v.extend(line(u1));
            v.push(0.0);
            v.extend(line(u2));
        }
        v
    }

    #[test]
    fn merge_self_curves_measures_the_arguments() {
        assert_eq!(merge_self_curves(&merge_self_args(&[])), Ok(3));
        let one = merge_self_args(&[(3.0, 2.0, 2.0)]);
        assert_eq!(merge_self_curves(&one), Ok(one.len()));
        // a 2D 3D curve is what upstream's SmCurve* accepts
        let flat = merge_self_args(&[(2.0, 2.0, 2.0), (3.0, 2.0, 2.0)]);
        assert_eq!(merge_self_curves(&flat), Ok(flat.len()));
        // the B-rep block follows the arguments
        let mut with_block = one.clone();
        with_block.extend([7.0, 8.0]);
        assert_eq!(merge_self_curves(&with_block), Ok(one.len()));
        assert_eq!(n_args(Op::MergeSelf, &with_block), Ok(one.len()));
    }

    #[test]
    fn merge_self_curves_rejects_3d_uv_curves_and_too_many_curves() {
        assert!(merge_self_curves(&merge_self_args(&[(3.0, 3.0, 2.0)])).is_err());
        assert!(merge_self_curves(&merge_self_args(&[(3.0, 2.0, 3.0)])).is_err());
        assert!(merge_self_curves(&[0.0, 1.0e-5, (MAX_CURVES + 1) as f64]).is_err());
        // truncated: the count promises a curve that is not there
        assert!(merge_self_curves(&[0.0, 1.0e-5, 1.0]).is_err());
    }

    #[test]
    fn merge_self_raw_knots_require_safe_spacing() {
        let raw = |middle: f64| {
            let mut v = vec![0.0, 1.0e-5, 1.0];
            for dim in [3.0, 2.0, 2.0] {
                let z = if dim == 2.0 { NOZ } else { 0.0 };
                v.extend([1.0, dim, 3.0, 1.0, 5.0]);
                for x in [0.0, 0.3, 0.7, 1.0] {
                    v.extend([x, 0.0, z, NOW]);
                }
                v.extend([0.0, 0.0, 0.5, middle, 1.0, 1.0]);
            }
            v
        };
        assert!(merge_self_curves(&raw(0.5 + 2.0e-12)).is_ok());
        assert!(merge_self_curves(&raw(0.5 + 0.5e-12)).is_err());
    }
}
