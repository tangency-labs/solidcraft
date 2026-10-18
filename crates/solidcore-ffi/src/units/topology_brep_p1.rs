//! FFI wrappers for unit `topology.brep_p1`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p1.cpp` (`sc_topology_brep_p1_eval`).
//! Upstream: `source/SMLib/inc/SmBrep.h`, `source/SMLib/src/SmBrep.cpp`.
//!
//! Every op starts with the B-rep block of unit `topology.edgeuse_p1`
//! ([`super::topology_edgeuse_p1`]: `.smb` file, wire edge, face or the `prog_test`
//! wire network; the shim builds a fresh `SmBrep` per call). Indices refer to the
//! export's traversal order; a target is `ty idx` with `ty` an `SmTopoTypes.h` code.
//! Every output starts with `0`, or with `-1 status` when the B-rep could not be built
//! (nothing else follows). `-2` in a status slot is "upstream would dereference null
//! (or recurse without bound)": the shim did not call it.
//!
//! | [`Op`]         | parameters after the block                       | outputs |
//! |----------------|--------------------------------------------------|---------|
//! | `Export`       |                                                  | `inf_region editing size_estimate ratio head_is_infinite n_v (vertex use list)*n_v face_list edge_list vertex_list`, then the graph export of `topology.edgeuse_p1` (wire shells exported completely) |
//! | `Lists`        | `which attr_id n_targets (ty idx)*n`             | `status`, the list (`n idx*n`); `which`: 0 regions, 1 faces, 2 edges, 3 vertices, 4 wire edges, 5 topological edges, 6 topological vertices, 7 surfaces (as face indices), 8 curves (as edge indices), 9 shells; the attribute `attr_id` (-1 none) is attached to the targets first |
//! | `Query`        | `ty idx`                                         | `num_faces live tol est_raw est ratio_raw ratio inf_region type kind_of*9 len_type_string len_class_string editing validate_only` |
//! | `SetSize`      | `kind value`                                     | kind 0 `SetThisModelSizeEstimate`: `returned_tol tol est_raw est ctx_est ctx_tol`; kind 1 `SetThisLargeSmallSizeRatio`: `ratio_raw ratio ctx_ratio` |
//! | `SetInf`       | `set_idx mode reset_idx flag unsure_idx`         | `unsure_idx`'s void flag set to `UNSURE` (2) unless -1, `SetInfiniteRegion(set_idx)` unless -2 (-1 null), then mode 1 `ResetInfiniteRegion(reset_idx, flag)` / 2 `SetRegionIsVoidFlagsForNestedSolids`: `status inf_region regions(n idx*n) [is_void(n flag*n) on success of mode 1/2]` |
//! | `Degenerate`   | `tol`                                            | `flag` |
//! | `AddDeleted`   | `n`                                              | `count` of `m_pDeletedSurfaces` after adding `n` planes |
//! | `Vote`         | `n idx*n`                                        | `regions(n idx*n) votes(n v*n) highest` |
//! | `ObjMap`       | `n_set (oty oi cty ci copy rev)*n_set n_get (ty idx)*n_get` | per get: `type idx copy_map_idx reverse_map_idx` as one list |
//! | `AddAttrs`     | `ty idx n_attrs id*n_attrs n_idx index*n_idx`    | `status`, the object's attribute ids (`n id*n`) |
//! | `ReflectEu`    | `edge`                                           | `status`, the edge use list (`n idx*n`) |
//! | `ReflectShell` | `face`                                           | `status`, then `shell1 shell2` and per shell its face use count (`n c*n`, -1 for a wire shell) |
//! | `RmLoopVu`     | `vu`                                             | `status vertex_list_size [remaining(n idx*n)]` |
//! | `MarkLoops`    | `loop level`                                     | `status loops(n idx*n)` |
//! | `SwapFaceUv`   | `face mark premark`                              | `0 marked u0 v0 u1 v1 [B-rep]` (an unmarked face is swapped: the `put_healed` B-rep follows) |
//! | `CheckUv`      | `eu`                                             | `status` |
//! | `LockBox`      |                                                  | `0` |
//! | `FindSide`     | `curve P[3] D[3] tol`                            | `status right_hand_side` (`sm_FindSideOfCurve`) |
//! | `FindSector`   | `face has_guess u v P[3] D[3]`                   | `status [distance region]` (`sm_FindSectorOfFace`) |
//! | `UpdateIvls`   | `vertex`                                         | `status vertex_tol n (t0 t1 edge_tol)*n` over the vertex's edges |
//! | `Redistribute` | `edge has_uv seam u0 v0 u1 v1`                   | `0 swapped`, or `-3` where the Rust side defers (`sm_RedistributeSeamUVTrimCurves`) |
//! | `SplitUv`      | `eu_orig eu_new P[3] t scaled_zero orient for_tess` | `status`, both edge uses' states (as `topology.edgeuse_p1`'s `SetUv`), both edge tolerances (`sm_MakeVertexSplitUVTrimCurve`) |
//! | `BrepIntersect`| `tol ang want_points`                            | `status n_curves n_points` (with itself; `-3` when the B-rep has faces) |
//! | `CornerBlend`  | `n eu*n has_tan nt tan*nt tol ang`               | `status n_surfaces` (`-3` for two or more edge uses) |
//! | `EdgeEdgeBlend`| `flag eu1 eu2 dir`                               | `status [surface block]` (ruled surfaces; `-3` for a blend) |
//! | `ParallelProj` | `curve V[3] has_dist dist has_tol tol has_ang ang want_3d want_uv want_faces` | `status [n3d (curve id)*n3d nuv (curve id)*nuv nf face*nf]` |
//! | `Heal`         | `op make_uv`                                     | `status [healed B-rep]` (`SmBrep::HealBrep` always returns `SM_SUCCESS`; the healed B-rep follows: the `Export` extras, then the shim's `put_healed` fields) |
//!
//! A curve id is `-1` (none) or `1 t0 t1 P(t0) P(mid) P(t1)`; an edge use state is `-1`
//! or `1 curve_id owner has_fail fail_status fail_brep vu_face_gap_init`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: the B-rep
//! block rules of `topology.edgeuse_p1`, indices inside the B-rep's export (it builds
//! the B-rep once more to count), attribute indices inside their array, finite values.

use crate::units::topology_edgeuse_p1::{bad, brep_block, curve, Reader, MAX_LIST};
use crate::{call_with_buffer, FfiError};

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Lists = 1,
    Query = 2,
    SetSize = 3,
    SetInf = 4,
    Degenerate = 5,
    AddDeleted = 6,
    Vote = 7,
    ObjMap = 8,
    AddAttrs = 9,
    ReflectEu = 10,
    ReflectShell = 11,
    RmLoopVu = 12,
    MarkLoops = 13,
    SwapFaceUv = 14,
    CheckUv = 15,
    LockBox = 16,
    FindSide = 17,
    FindSector = 18,
    UpdateIvls = 19,
    Redistribute = 20,
    SplitUv = 21,
    BrepIntersect = 22,
    CornerBlend = 23,
    EdgeEdgeBlend = 24,
    ParallelProj = 25,
    Heal = 26,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p1_eval(
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

/// Number of scalar extras before the vertex use lists of an `Export`.
pub const N_EXTRA_SCALARS: usize = 5;

/// `SmSurface_TYPE`: a `Lists` target naming the surface of face `idx`.
pub const SURFACE_TYPE: i64 = 15001;
/// `SmCurve_TYPE`: a `Lists` target naming the curve of edge `idx`.
pub const CURVE_TYPE: i64 = 14001;

/// `SmTopoTypes.h` codes of the targets, with the export slot of their kind.
pub const TARGET_TYPES: [(i64, usize); 10] = [
    (16012, 0), // SmRegion_TYPE
    (16013, 1), // SmShell_TYPE
    (16022, 2), // SmFaceuse_TYPE
    (16002, 3), // SmFace_TYPE
    (16004, 4), // SmLoopuse_TYPE
    (16003, 5), // SmLoop_TYPE
    (16005, 6), // SmEdgeuse_TYPE
    (16016, 7), // SmEdge_TYPE
    (16006, 8), // SmVertexuse_TYPE
    (16021, 9), // SmVertex_TYPE
];

/// The entity counts of an export (regions, shells, face uses, faces, loop uses,
/// loops, edge uses, edges, vertex uses, vertices), and where the graph export starts
/// (after the status and the extras). `None` for a build failure.
#[must_use]
pub fn counts(export: &[f64]) -> Option<([usize; 10], usize)> {
    if export.first() != Some(&0.0) {
        return None;
    }
    let n_v = *export.get(1 + N_EXTRA_SCALARS)? as usize;
    let mut at = 2 + N_EXTRA_SCALARS;
    // the vertex use lists, then the face, edge and vertex lists
    for _ in 0..n_v + 3 {
        let n = *export.get(at)? as usize;
        at += 1 + n;
    }
    let mut c = [0usize; 10];
    for (slot, v) in c.iter_mut().zip(export.get(at + 1..at + 11)?) {
        *slot = *v as usize;
    }
    Some((c, at))
}

/// A `ty idx` target: a listed type with an index inside its count or `-1`; any other
/// (small, integral) type code names no target.
fn target(rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let t = rd.num()?;
    let i = rd.num()?;
    if i.fract() != 0.0 || i < -1.0 {
        return Err(bad("bad target index"));
    }
    let slot = TARGET_TYPES
        .iter()
        .find(|(code, _)| *code as f64 == t)
        .map(|&(_, s)| s)
        .or(if t == SURFACE_TYPE as f64 {
            Some(3)
        } else if t == CURVE_TYPE as f64 {
            Some(7)
        } else {
            None
        });
    if let Some(slot) = slot {
        let n = c.get(slot).copied().unwrap_or(0);
        if i >= 0.0 && i as usize >= n {
            return Err(bad("target index out of range"));
        }
    } else if t.fract() != 0.0 || !(0.0..=1.0e6).contains(&t) {
        return Err(bad("bad target type"));
    }
    Ok(())
}

fn attribute_id(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let v = rd.num()?;
    if v.fract() != 0.0 || !(-1.0..=1.0e6).contains(&v) {
        return Err(bad("bad attribute id"));
    }
    Ok(())
}

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [n_r, _, _, n_f, _, n_l, n_eu, n_e, n_vu, n_v] = c;
    match op {
        Op::Export | Op::LockBox => {}
        Op::Lists => {
            rd.index(9)?;
            attribute_id(rd)?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                target(rd, c)?;
            }
        }
        Op::Query => target(rd, c)?,
        Op::SetSize => {
            rd.index(1)?;
            rd.finite()?;
        }
        Op::SetInf => {
            let s = rd.num()?;
            if s.fract() != 0.0 || s < -2.0 || (s >= 0.0 && s as usize >= n_r) {
                return Err(bad("bad infinite region selector"));
            }
            rd.index(2)?;
            rd.below_or_none(n_r)?;
            rd.index(2)?;
            // the region set UNSURE first, or -1
            rd.below_or_none(n_r)?;
            // `ResetInfiniteRegion(region, SM_VS_INFINITEREGION_SWAP)` dereferences the
            // old infinite region (`SmBrep.cpp:8630`) when it is null (set -1, or -2 on a
            // B-rep without one): the shim stops before that call and reports `-2`, as
            // the Rust port does, so the input stays comparable here.
        }
        Op::Degenerate => {
            rd.finite()?;
        }
        Op::AddDeleted => {
            rd.index(16)?;
        }
        Op::Vote => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                // a null region (-1) is a valid vote upstream
                rd.below_or_none(n_r)?;
            }
        }
        Op::ObjMap => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                target(rd, c)?;
                target(rd, c)?;
                rd.flag()?;
                rd.flag()?;
            }
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                target(rd, c)?;
            }
        }
        Op::AddAttrs => {
            target(rd, c)?;
            let n_attrs = rd.index(MAX_LIST)?;
            for _ in 0..n_attrs {
                let v = rd.num()?;
                if v.fract() != 0.0 || !(0.0..=1.0e6).contains(&v) {
                    return Err(bad("bad attribute id"));
                }
            }
            let n_idx = rd.index(MAX_LIST)?;
            for _ in 0..n_idx {
                // sm_AddAttributes reads rAttributes[index] unchecked
                rd.below(n_attrs)?;
            }
        }
        Op::ReflectEu => {
            rd.below(n_e)?;
        }
        Op::ReflectShell => {
            rd.below(n_f)?;
        }
        Op::RmLoopVu => {
            rd.below(n_vu)?;
        }
        Op::MarkLoops => {
            rd.below(n_l)?;
            rd.index(1_000_000)?;
        }
        Op::SwapFaceUv => {
            rd.below(n_f)?;
            let m = rd.num()?;
            if ![1.0, 2.0, 4.0, 8.0, 16.0].contains(&m) {
                return Err(bad("bad mark type"));
            }
            rd.flag()?;
        }
        Op::CheckUv => {
            rd.below(n_eu)?;
        }
        Op::FindSide => {
            curve(rd)?;
            rd.vec3()?;
            rd.nonzero3()?;
            rd.finite()?;
        }
        Op::FindSector => {
            rd.below(n_f)?;
            rd.flag()?;
            rd.finite()?;
            rd.finite()?;
            rd.vec3()?;
            rd.vec3()?;
        }
        Op::UpdateIvls => {
            rd.below(n_v)?;
        }
        Op::Redistribute => {
            rd.below(n_e)?;
            rd.flag()?;
            // SmSurfParamType: SM_SP_U (0) .. SM_SP_VMAX (8); SM_SP_OUT (9) is not a seam
            // kind the Rust runner maps
            rd.index(8)?;
            rd.finites(4)?;
        }
        Op::SplitUv => {
            rd.below(n_eu)?;
            rd.below(n_eu)?;
            rd.vec3()?;
            rd.finite()?;
            rd.finite()?;
            rd.index(2)?;
            rd.flag()?;
        }
        Op::BrepIntersect => {
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
        }
        Op::CornerBlend => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(n_eu)?;
            }
            let has_tan = rd.flag()?;
            let nt = rd.index(MAX_LIST)?;
            for _ in 0..nt {
                rd.flag()?;
            }
            // upstream indexes pOptIsTangent[ii] per edge use
            if has_tan && nt < n {
                return Err(bad("fewer tangency flags than edge uses"));
            }
            rd.finite()?;
            rd.finite()?;
        }
        Op::EdgeEdgeBlend => {
            rd.index(3)?;
            rd.below_or_none(n_eu)?;
            rd.below_or_none(n_eu)?;
            rd.index(2)?;
        }
        Op::ParallelProj => {
            curve(rd)?;
            rd.nonzero3()?;
            rd.flag()?;
            rd.finite()?;
            // a used approximation or angle tolerance must be positive (the projector
            // subdivides to it)
            let has_tol = rd.flag()?;
            let tol = rd.finite()?;
            let has_ang = rd.flag()?;
            let ang = rd.finite()?;
            if (has_tol && tol <= 0.0) || (has_ang && ang <= 0.0) {
                return Err(bad("projection tolerances must be positive"));
            }
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Heal => {
            // SmHealerOpType: SM_HO_NONE (0) ..= SM_HO_ALL (26)
            rd.index(26)?;
            rd.flag()?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_brep_p1_eval(
                op.code(),
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

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, path, block)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some((c, _)) = counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    brep_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}
