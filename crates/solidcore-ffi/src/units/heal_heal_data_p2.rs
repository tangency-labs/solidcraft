//! FFI wrappers for unit `heal.heal_data_p2`.
//!
//! Shim: `harness/cpp/shim/heal_heal_data_p2.cpp` (`sc_heal_heal_data_p2_eval`).
//! Upstream: `source/SMLib/src/SmHealData.cpp`, `source/SMLib/src/SmEdgeProps.cpp`,
//! `source/SMLib/src/SmVertexProps.cpp`, `source/SMLib/inc/SmHealProbArray.h`.
//!
//! A B-rep block is one of [`super::topology_vertexuse`]'s (kinds `0..=3`),
//! `4 dx dy dz validate_only init_gaps` (`SmPrimitiveCreation::CreateBox`) or
//! `5 radius swap_uv validate_only init_gaps` (a face on the full
//! `SmSphere::CreateCanonical` domain); `init_gaps` computes the cached gaps first. The shim replaces analytic face surfaces and edge curves other than
//! `SmPlane` / `SmLine` by plain B-spline copies before anything runs.
//!
//! | [`Op`]   | input | output |
//! |----------|-------|--------|
//! | `Export` | block | `0` and the graph of [`super::topology_vertexuse`]'s export (walked with the vertex lists), or `-1 status` |
//! | `VertexLists` | block | `0`, per vertex its vertex use list (`len vu*`), then the cached gap records of every edge use (`init gap eu0 eu1 t0 t1`, `init gap eu t u v`) and vertex use (`init gap vu t`, `init gap vu u v`); or `-1 status` |
//! | `Prob`   | `kind n nops (code a b c)*` | per op `code result` (`-2`: upstream would read past the end or a null member, and nothing follows), then `-100`, the array (`size elem* orig get_op labels`), `coin_pair_cnt orig`, and per object its 18 flags, 2 mixed and 3 tried members |
//! | `Heal`   | block `targets vx vy vz nsteps step*` | `0`, per step `code` and its outputs, then `-100` and the healer's state (layout in the difftest runner); or `-1 status` |
//!
//! `targets` is `nv sel* ne sel* nf sel*` (selectors modulo the B-rep's counts). A
//! `Prob` `kind` is `0` `SmProbArray`, `1` `SmPairProbArray`, `2` `SmMixedProbArray`,
//! `3` `SmTriedProbArray`, over `n` `SmFaceProps`; the op codes are documented with
//! [`check_prob`]. Heal steps are documented with [`check_heal`].
//!
//! The wrapper accepts only what upstream handles without undefined behaviour: valid
//! B-rep blocks, bounded counts and selectors, voxel counts in `1..=MAX_VOXELS`,
//! `FreeTargets` only as the last step (the shim cannot tell freed props from reused
//! addresses afterwards). The shim itself refuses (`-2`, nothing follows) the steps
//! whose upstream path reads freed or null pointers or past an array.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

use super::topology_vertexuse as vu;

/// Largest target list length.
pub const MAX_TARGETS: usize = 512;
/// Largest step count of a heal script.
pub const MAX_STEPS: usize = 64;
/// Largest op count of a problem array script.
pub const MAX_OPS: usize = 64;
/// Largest object count of a problem array script.
pub const MAX_OBJECTS: usize = 8;
/// Largest voxel count per axis.
pub const MAX_VOXELS: usize = 16;
/// Largest "old" count of `SetOldCounts`.
pub const MAX_OLD: usize = 1024;
/// Largest selector.
pub const MAX_SEL: usize = 1 << 30;
/// Largest topology list of a `Get*Props_ForTgts` step.
pub const MAX_TGTS: usize = 64;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Prob = 1,
    Heal = 2,
    VertexLists = 3,
}

mod sys {
    extern "C" {
        pub fn sc_heal_heal_data_p2_eval(
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

struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn int(&mut self, lo: i64, hi: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad integer {v} (range {lo}..={hi})")));
        }
        Ok(v as i64)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_heal_heal_data_p2_eval(
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

/// Checks a B-rep block (all of `block`).
pub(crate) fn check_block(path: &str, block: &[f64]) -> Result<(), FfiError> {
    let kind = block.first().copied().ok_or_else(|| bad("empty block"))?;
    let mut rd = Reader {
        input: block,
        at: 1,
    };
    if kind == 4.0 {
        for _ in 0..3 {
            let d = rd.num()?;
            if !(1e-4..=1e4).contains(&d) {
                return Err(bad("box size out of range"));
            }
        }
        rd.flag()?;
        rd.flag()?;
        return rd.done();
    }
    if kind == 5.0 || kind == 6.0 {
        let r = rd.num()?;
        if !(1e-3..=1e3).contains(&r) {
            return Err(bad("sphere radius out of range"));
        }
        rd.flag()?;
        rd.flag()?;
        rd.flag()?;
        return rd.done();
    }
    // kinds 0..=3: the topology.vertexuse validator (it builds the export once)
    vu::eval(vu::Op::Export, path, block).map(|_| ())
}

/// The graph export of `block` (validated), with the analytic geometry replaced.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed block; [`FfiError::Status`] for a shim
/// failure.
pub fn export(path: &str, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_block(path, block)?;
    raw(Op::Export, path, block)
}

/// Each vertex's vertex use list of `block`'s B-rep, as `len vu*` per vertex in the
/// export's order.
///
/// # Errors
///
/// As [`export`].
pub fn vertex_lists(path: &str, block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_block(path, block)?;
    raw(Op::VertexLists, path, block)
}

/// Checks a problem array script: `kind n nops (code a b c)*`. Codes: `0` `Setup(a,
/// link b or -1)`; `1` `Add(object a, b)` (`b` the set-flag boolean, the
/// `SmMixedLinkType` or the `SmTriedLinkType`); `2` `Remove(object a)`; `3`
/// `RemoveAll`; `4` `ReSet`; `5` `IncOrigProbCount`; `6` object `a`'s flag `b` set to
/// `c`; `7` `SetLabels` preset `a`; `8` `GetAt(a mod size)`; `9` `SetOrigCoinPairCnt(a)`
/// (pair) or `SetOrigProbCnt(a)`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything else.
pub fn check_prob(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let kind = rd.index(3)?;
    let n = rd.index(MAX_OBJECTS)?;
    if n == 0 {
        return Err(bad("no objects"));
    }
    let nops = rd.index(MAX_OPS)?;
    let max_link: i64 = match kind {
        2 => 1,
        3 => 2,
        _ => 17,
    };
    for _ in 0..nops {
        let code = rd.index(9)?;
        match code {
            0 => {
                rd.int(0, 26)?;
                rd.int(-1, max_link)?;
                rd.int(0, 0)?;
            }
            1 => {
                rd.index(n - 1)?;
                match kind {
                    2 => rd.int(0, 5)?,
                    3 => rd.int(0, 3)?,
                    _ => rd.int(0, 1)?,
                };
                rd.int(0, 0)?;
            }
            2 => {
                rd.index(n - 1)?;
                rd.int(0, 0)?;
                rd.int(0, 0)?;
            }
            6 => {
                rd.index(n - 1)?;
                rd.index(17)?;
                rd.index(2)?;
            }
            7 => {
                rd.index(2)?;
                rd.int(0, 0)?;
                rd.int(0, 0)?;
            }
            8 => {
                rd.index(MAX_SEL)?;
                rd.int(0, 0)?;
                rd.int(0, 0)?;
            }
            9 => {
                rd.index(MAX_OLD)?;
                rd.int(0, 0)?;
                rd.int(0, 0)?;
            }
            _ => {
                rd.int(0, 0)?;
                rd.int(0, 0)?;
                rd.int(0, 0)?;
            }
        }
    }
    rd.done()
}

/// Runs a problem array script (see [`check_prob`]).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script; [`FfiError::Status`] for a
/// shim failure.
pub fn prob(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_prob(input)?;
    raw(Op::Prob, "", input)
}

fn targets(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..3 {
        let n = rd.index(MAX_TARGETS)?;
        for _ in 0..n {
            rd.index(MAX_SEL)?;
        }
    }
    Ok(())
}

/// Checks the heal arguments after the block: `targets vx vy vz nsteps step*`. Steps
/// (code and arguments): `0` `Cache_EdgeProps`, `1` `Cache_VertexProps`, `2`
/// `Cache_FaceProps_Gaps`, `3` `Fix_TolSizes`, `4` `Cache_CoinVertices`, `5`
/// `Cache_DegenFaces`, `6` `Cache_CoinEdges`, `7` `Fix_CoinEdges`, `8`
/// `Cache_MissedEdgeXSects`, `9` `Fix_MissedEdgeXSects`, `10` `Fix_UncontainedEdges`,
/// `11` `Fix_BadGaps` (outputs `status new_probs`); `12`/`13`/`14 sel`
/// `Remove{Vertex,Edge,Face}Props_FromLists` of the `sel`-th alive props (outputs
/// `props rm_tgt rm_props`, or `-3` with none alive); `15 which stale n (kind index)*`
/// `Get{Vertex,Edge,Face}Props_ForTgts` (`kind` 0..9 the export's entity kinds;
/// outputs `0 count props*`); `16` `FreeTargets` (last step only); `17 targets`
/// `LoadTargets`; `18 ov oe of` the old counts; `19 op` `m_eDoneHealOp`; `20 sel meta`
/// `SmEdgeProps::ReSet`; `21 sel` `SmVertexProps::ReSet`; `22`
/// `SetupEmptyProbArrays` (steps `16..=22` output `0 0`).
///
/// Steps of unit `heal.heal_data_p1` (outputs `status new_probs` unless noted):
/// `23 flag` `ReSet(flag)` (last step only); `24 sel` `MoveToNotYetProblems` of the
/// `sel`-th alive face props (outputs its index, or `-3` with none alive); `25`
/// `Cache_FaceProps_Stage2`; `26` `Cache_FaceProps_Stage3`; `27` `Fix_BadSheets` (with
/// a fresh mark lock of the B-rep's context); `28 flag` `Make_UVTrimCurves(flag)`; `29`
/// `Fix_InfiniteRegion` (last step only); `30 mode op flag [targets]` `HealBrep` with
/// a null B-rep (`mode` 0), every target (`1`) or the `targets` that follow (`2`),
/// healer op `op` and `bMakeUVTrimCurves` `flag` (modes 1 and 2 last step only; the
/// case ends after the status, without the state); `31` the stage 2/3 snapshot of
/// every registered face props (outputs `n`, per record `0` freed or `1` and its
/// values; layout in the difftest runner of `heal.heal_data_p1`). Steps `25..=28`
/// output `-2` and end the case when a props record they would use has no face (see
/// `solidcore::heal::heal_data::p1`).
/// Steps for `heal.heal_data_p3`: `32 op make` runs `HealTgts`; `33 op nrm
/// [kind selector]* nadd [kind selector]*` runs `UpdateTargetLists` (kind 10 = null);
/// `34..=39` run `Fix_CoinVertices`, `Fix_DegenFaces`, `Fix_DegenEdges`, `Fix_MoveSeam`,
/// `Fix_SplitEdgesAtSeam`, `Fix_SplitFaceAtSeams`; `40 active op` runs the tracker
/// overload with empty lists. `41` seeds the native degenerate-face queue from
/// loaded face targets for the single-vertex-sphere regression. `42 null_face`
/// seeds degenerate/missing/crossed face queues with no crossed direction, only
/// immediately before a final step 38 (or 35/39 for null faces). Step 38
/// also outputs each current face props' crossed direction, flag and tried state.
/// Selectors use the
/// fixture graph's traversal order. Step 33 refuses catch-up into an uninitialized
/// incremental vertex voxel model (`-2`). A nonempty add list also requires every
/// post-removal target count to equal its props count, including for null adds.
/// Remove lookups must fit the corresponding props list. Zero prior vertices or an initialized
/// model remain valid. Catch-up through `Fix_CoinVertices` or later is terminal
/// because it can delete original fixture topology.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for anything else.
pub fn check_heal(args: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input: args, at: 0 };
    targets(&mut rd)?;
    for _ in 0..3 {
        let v = rd.index(MAX_VOXELS)?;
        if v == 0 {
            return Err(bad("zero voxel count"));
        }
    }
    let nsteps = rd.index(MAX_STEPS)?;
    for s in 0..nsteps {
        let code = rd.index(42)?;
        let last = s + 1 == nsteps;
        match code {
            12..=14 | 21 | 24 => {
                rd.index(MAX_SEL)?;
            }
            15 => {
                rd.index(2)?;
                rd.flag()?;
                let n = rd.index(MAX_TGTS)?;
                for _ in 0..n {
                    rd.index(9)?;
                    rd.index(MAX_SEL)?;
                }
            }
            16 => {
                if s + 1 != nsteps {
                    return Err(bad("FreeTargets must be the last step"));
                }
            }
            17 => targets(&mut rd)?,
            18 => {
                rd.index(MAX_OLD)?;
                rd.index(MAX_OLD)?;
                rd.index(MAX_OLD)?;
            }
            19 => {
                rd.index(26)?;
            }
            20 => {
                rd.index(MAX_SEL)?;
                rd.flag()?;
            }
            23 | 29 => {
                if code == 23 {
                    rd.flag()?;
                }
                if !last {
                    return Err(bad("ReSet / Fix_InfiniteRegion must be the last step"));
                }
            }
            28 => {
                rd.flag()?;
            }
            42 => {
                let null_face = rd.flag()?;
                let next = rd.input.get(rd.at).copied();
                let safe_repair =
                    next == Some(38.0) || (null_face && matches!(next, Some(35.0 | 39.0)));
                if s + 2 != nsteps || !safe_repair {
                    return Err(bad(
                        "SeedFaceProblems must immediately precede its final guarded repair",
                    ));
                }
            }
            32 => {
                rd.index(26)?;
                rd.flag()?;
                if !last {
                    return Err(bad("HealTgts must be last"));
                }
            }
            33 => {
                rd.index(26)?;
                for _ in 0..2 {
                    let count = rd.index(MAX_TGTS)?;
                    for _ in 0..count {
                        rd.index(10)?;
                        rd.index(MAX_SEL)?;
                    }
                }
            }
            40 => {
                rd.flag()?;
                rd.index(26)?;
            }
            30 => {
                let mode = rd.index(2)?;
                rd.index(26)?;
                rd.flag()?;
                if mode == 2 {
                    targets(&mut rd)?;
                }
                if mode != 0 && !last {
                    return Err(bad("HealBrep on a B-rep must be the last step"));
                }
            }
            _ => {}
        }
    }
    rd.done()
}

/// Runs a heal script on the B-rep of `block` (see [`check_heal`]).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed block or script;
/// [`FfiError::Status`] for a shim failure.
pub fn heal(path: &str, block: &[f64], args: &[f64]) -> Result<Vec<f64>, FfiError> {
    check_block(path, block)?;
    check_heal(args)?;
    let mut input = block.to_vec();
    input.extend_from_slice(args);
    raw(Op::Heal, path, &input)
}

#[cfg(all(test, feature = "cpp"))]
mod tests {
    use super::*;

    // Exercise the safe public FFI independently of the Rust runner: kernel
    // deferrals must not conceal a crash in the native half of a case.
    fn catch_up(
        all_targets: bool,
        cached: bool,
        done: u8,
        through: u8,
        remove_all: bool,
    ) -> Result<Vec<f64>, FfiError> {
        let mut args = Vec::new();
        for n in [8, 12, 6] {
            args.push(if all_targets { f64::from(n) } else { 0.0 });
            if all_targets {
                args.extend((0..n).map(f64::from));
            }
        }
        args.extend([
            12.0,
            12.0,
            12.0,
            if cached { 6.0 } else { 5.0 },
            0.0,
            1.0,
            2.0,
        ]);
        if cached {
            args.push(4.0);
        }
        args.extend([19.0, f64::from(done), 33.0, f64::from(through)]);
        args.push(if remove_all { 8.0 } else { 0.0 });
        if remove_all {
            for i in 0..8 {
                args.extend([9.0, f64::from(i)]);
            }
        }
        args.extend([3.0, 3.0, 0.0, 7.0, 0.0, 9.0, 0.0]);
        check_heal(&args)?;
        heal("", &[4.0, 1.0, 2.0, 3.0, 0.0, 1.0], &args)
    }

    #[test]
    fn synthetic_face_problems_allow_only_the_guarded_repair_paths() {
        for null in [0.0, 1.0] {
            for repair in [34.0, 35.0, 36.0, 37.0, 38.0, 39.0, 40.0] {
                let args = [0.0, 0.0, 0.0, 12.0, 12.0, 12.0, 2.0, 42.0, null, repair];
                let allowed = repair == 38.0 || (null == 1.0 && (repair == 35.0 || repair == 39.0));
                assert_eq!(check_heal(&args).is_ok(), allowed);
            }
        }
        let extra_step = [0.0, 0.0, 0.0, 12.0, 12.0, 12.0, 3.0, 42.0, 1.0, 38.0, 0.0];
        assert!(check_heal(&extra_step).is_err());
    }

    #[test]
    fn forged_done_state_is_refused_before_native_incremental_voxel_access() -> Result<(), FfiError>
    {
        for op in [6, 8, 9, 16, 26] {
            let out = catch_up(true, false, op, op, false)?;
            assert!(out.ends_with(&[33.0, -2.0]), "op={op}: {out:?}");
        }
        Ok(())
    }

    #[test]
    fn initialized_empty_and_below_coin_catch_up_remain_valid() -> Result<(), FfiError> {
        for (all, cached, done, through, remove_all) in [
            (true, true, 6, 6, false),
            (false, false, 6, 6, false),
            (true, false, 6, 5, false),
            (true, false, 5, 6, false),
            (true, false, 6, 6, true),
        ] {
            let out = catch_up(all, cached, done, through, remove_all)?;
            assert!(out.contains(&-100.0), "valid catch-up was refused: {out:?}");
        }
        Ok(())
    }
    fn box_script(steps: &[f64], count: usize) -> Result<Vec<f64>, FfiError> {
        let mut args = Vec::new();
        for n in [8, 12, 6] {
            args.push(f64::from(n));
            args.extend((0..n).map(f64::from));
        }
        args.extend([12.0, 12.0, 12.0, count as f64]);
        args.extend_from_slice(steps);
        check_heal(&args)?;
        heal("", &[4.0, 1.0, 2.0, 3.0, 0.0, 1.0], &args)
    }

    #[test]
    fn second_review_crash_sequences_are_refused_before_native_update() -> Result<(), FfiError> {
        let scripts: &[(&[f64], usize)] = &[
            (
                &[
                    19.0, 9.0, 33.0, 6.0, 2.0, 9.0, 0.0, 9.0, 1.0, 2.0, 9.0, 0.0, 9.0, 0.0,
                ],
                2,
            ),
            (&[33.0, 0.0, 1.0, 3.0, 0.0, 1.0, 10.0, 0.0], 1),
            (
                &[
                    0.0, 1.0, 19.0, 12.0, 33.0, 9.0, 1.0, 3.0, 0.0, 1.0, 3.0, 0.0,
                ],
                4,
            ),
            (&[12.0, 0.0, 33.0, 0.0, 0.0, 1.0, 9.0, 0.0], 2),
            // Get*Props_ForTgts itself would index past props after an uncached removal.
            (&[12.0, 0.0, 33.0, 0.0, 1.0, 9.0, 7.0, 0.0], 2),
            (&[13.0, 0.0, 33.0, 0.0, 1.0, 7.0, 11.0, 0.0], 2),
            (&[14.0, 0.0, 33.0, 0.0, 1.0, 3.0, 5.0, 0.0], 2),
        ];
        for &(steps, count) in scripts {
            assert!(
                box_script(steps, count)?.ends_with(&[33.0, -2.0]),
                "{steps:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn target_props_guard_preserves_valid_cache_and_remove_add_combinations() -> Result<(), FfiError>
    {
        // 8 cache masks x 3 removed kinds x 5 add kinds x 2 removal paths.
        for cache_mask in 0_u8..8 {
            for (bit, kind, remove_step) in [(0, 7.0, 13.0), (1, 9.0, 12.0), (2, 3.0, 14.0)] {
                for add_kind in [None, Some(10.0), Some(9.0), Some(7.0), Some(3.0)] {
                    for prior_remove in [false, true] {
                        let mut steps = Vec::new();
                        let mut count = 1;
                        for cache in 0_u8..3 {
                            if cache_mask & (1 << cache) != 0 {
                                steps.push(f64::from(cache));
                                count += 1;
                            }
                        }
                        if prior_remove {
                            steps.extend([remove_step, 0.0]);
                            count += 1;
                        }
                        steps.extend([33.0, 0.0]);
                        if prior_remove {
                            steps.push(0.0);
                        } else {
                            steps.extend([1.0, kind, 0.0]);
                        }
                        if let Some(add_kind) = add_kind {
                            steps.extend([1.0, add_kind, 0.0]);
                        } else {
                            steps.push(0.0);
                        }
                        let out = box_script(&steps, count)?;
                        let must_refuse = add_kind.is_some() && cache_mask & (1 << bit) == 0;
                        if must_refuse {
                            assert!(out.ends_with(&[33.0,-2.0]), "mask={cache_mask}, kind={kind}, add={add_kind:?}, prior={prior_remove}");
                        } else {
                            assert!(out.contains(&-100.0), "valid mask={cache_mask}, kind={kind}, add={add_kind:?}, prior={prior_remove}: {out:?}");
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
