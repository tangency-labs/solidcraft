//! FFI wrappers for unit `blend.fillet_solver_p1`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_solver_p1.cpp` (`sc_blend_fillet_solver_p1_eval`).
//! Upstream: `source/SMLib/src/SmFilletSolver.cpp`.
//!
//! Surface and curve blocks are those of unit `topology.edgeuse_p1` (`1 O[3] N[3]` an
//! `SmPlane`, `0 oob form n m p q r s cpts u v` an `SmBSplineSurface`; a curve
//! `dim n p m cpts knots`). The solver is a minimal `SmFilletSolver` subclass (every
//! virtual is the base version), no edge uses or executive, orientations `1.0`, saved
//! offsets `0.0`. Arrays are `nx x[nx] nf f[nf] has_j rows cols` (a zero-filled
//! `rows x cols` Jacobian when `has_j`).
//!
//! | [`Op`]       | upstream | parameters |
//! |--------------|----------|------------|
//! | `State`      | constructor, getters, `SetSurface`, `SetSurfacesToZeroOffset`, `ReloadSurfaceOffsets`, `GetOffsetsData`, `SetOffsetsData`, `SetExtensionFactor`, `SetReverseTrim`, `SetStatus` | `tol ang tang surfA surfB has0 off0 has1 off1 o0 o1 k`, then `k` x `cmd a b` |
//! | `Extension`  | `SetupOffsetExtension` | `ext surf u0 v0 u1 v1` |
//! | `Sphere`     | `SmFilletSphereSolveENFO::Evaluate` | `c[3] r adjusts load found surfs arrays` |
//! | `RailUv`     | `SmRailUVCurveIntersectENFO::Evaluate` | `rail load found has_line [start end dim] curve t0 t1 arrays` (`has_line`: an `SmLine` UV curve replaces `curve`; `load`:the shim solver's deterministic `LoadJacobian`, answering `found`) |
//! | `SurfValues` | `ComputeSurfaceValues` | `setup surfs uv0[2] uv1[2]` (`setup`: the shim solver's `SetupOffsetValues` succeeds) |
//! | `UvVectors`  | `ComputeUVVectors` | `du0 dv0 du1 dv1 crv_deriv` (each `[3]`), `uvd0[2] uvd1[2]` |
//! | `Split`      | `SplitFilletGeom` with empty interval arrays | `index` |
//! | `Finish`     | `FinishConstruction` | `surf has0 has1 off` |
//! | `Fsg`        | the fillet surface generators: constructors, `GetFilletSurfaceGeneratorType`, the circular members, `GetBlendScale`, `GetContinuity`, the base `CreateSurface` | `kind a b c d` (kinds below) |
//! | `Misc`       | `SmTol::GetAngTolRad(const SmFilletSolver*)` (`0 has ang`), the base `LoadInitialValues` (`1 do2 rail idx2`) | |
//!
//! `surfs` is `surf has0 off0 has1 off1`: `SmOffsetSurface`s of one base surface installed
//! with `SetSurface`. `Fsg` kinds: `0` the base, `1` linear, `2`/`3` circular (defaults /
//! `a b c d` = approximate, tolerance percent, mirror, complement), `4`/`5` blend curve
//! (defaults / scale `a`, continuity `b`). `load`/`found` select the shim solver's
//! deterministic `LoadJacobian` as for `RailUv`.
//!
//! `State` commands: `0` zero offsets, `1` reload offsets, `2` save offsets data, `3`
//! restore it, `4` `SetSurface(a, new SmOffsetSurface(b, base[a]))`, `5`
//! `SetExtensionFactor(a)`, `6` `SetReverseTrim(a != 0)`, `7` `SetStatus(a)`.
//!
//! Output: `State` the 21 constructor values, then per command the state (per rail
//! `present key offset orientation saved`, then `ext reverse status`); `Extension` `0`
//! and the domain; `Sphere`, `RailUv` `status found f[nf]` and the Jacobian; others the
//! status (`Split` plus the geometry count, `UvVectors` plus both UV derivatives); `Fsg`
//! `type approximate tolerance% mirror complement scale continuity create_status` (`-1`
//! without a `CreateSurface` call); `Misc` the tolerance, or `status do2 idx2
//! surface2_null n_periodic n_guess`.
//!
//! [`eval_static`] runs [`StaticOp`]s on the copy of `SmFilletSolver.cpp` compiled by
//! `harness/cpp/shim/blend_fillet_solver_p1_static.cpp` (`sc_blend_fillet_solver_p1_static_eval`;
//! B-rep blocks and traversal-order indices of `topology.edgeuse_p1`; `-2` marks a call the
//! shim skipped because upstream dereferences `NULL` there):
//!
//! | [`StaticOp`]    | upstream | parameters | output |
//! |-----------------|----------|------------|--------|
//! | `FindNext`      | `sm_FindNextGoodInterval` | `n start cls[n]` (mid point classes) | `status found index` (`999` when not found) |
//! | `CheckDomains`  | `sm_CheckEdgeCurveDomains` | `block edge before after` | `0 ret curve` |
//! | `Analytic`      | `sm_TestAnalyticSurface` | `surface` | `ret` |
//! | `AnalyticRev`   | `sm_TestAnalyticSurface` on an `SmCone` / `SmCylinder` / `SmSphere` / `SmTorus` | `kind origin[3] x[3] y[3] p[4]` (kind 0 `SmCone::CreateCanonical(bot, top, height)`, 1 `SmCylinder(radius, start, end, height)` + `MakeNurb`, 2 `SmSphere::CreateCanonical(radius)`, 3 `SmTorus::CreateCanonical(major, minor)`) | `status ret` (`ret` only when built) |
//! | `RailRail`      | `SmRailRailIntersectENFO::Evaluate` | `rail other load1 found1 load2 found2 arrays` | `status found f J` |
//! | `Clipped`       | `SmFindClippedRadiusEFO` constructor and `Evaluate` | `idx dist tol base0 base1 has0 off0 has1 off1 o0 o1 dom0[4] dom1[4] param iso uv0[2] uv1[2] eval radius` (offset surface `i` offsets surface `base_i`) | the members, then with `eval` `status F F' found sUV[idx][2] sUV[1-idx][2] guess off0 off1` (`-2` without both offset surfaces) |
//! | `Convexity`     | `sm_TestSideEdgeConvexity` (the library's) | `block edge tangential` (`edge` `-1`: a fresh `SmEdge`) | `0 ret` |
//! | `TrimRail`      | `sm_TrimRail` | `block edge t0 t1` | `0 zt[3] knots status curve uv uv_mate` (`zt`: `SmTol::GetZoneTol3d` of the three curves before the trim; `knots`: `n knots[n] mults[n]` of the 3D curve, `-1` without) |
//! | `Queries`       | `GetVertex(0/1)`, `FindIndexOfRailOnFace`, `FindIndexOfRailXSideEdgeuse` | `block eu0 eu1 face side` | `0 v0 v1 on_face x_side` |
//! | `Rollover`      | `sm_TestManifoldTangentRollover` | `block edge tol` | `0 tangent opposite` (`-2`: no primary edge use, radial or mate, or an edge use without face use) |
//!
//! A curve is `1 t0 t1 P(t0) P(mid) P(t1)` or `-1`; a B-rep that cannot be built gives
//! `-1 status`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid
//! blocks (see `topology.edgeuse_p1`), finite numbers, at most [`MAX_ARRAY`] array
//! entries and [`MAX_COMMANDS`] commands, rails `0`/`1`, statuses of `SmFilStatus`,
//! `Sphere` with `nf >= rows` and, with a loading solver, the first offset surface,
//! `nx >= 2`, `nf >= 1` and a Jacobian of at least 1 row and 2 columns (the second
//! surface and `nx >= 4` when the plane adjusts: after a successful `LoadJacobian`
//! upstream dereferences both surfaces, reads `crX[0..2]`, `crX[2..4]` and writes `rF[0]`,
//! `J[0][0..2]`); `SurfValues` with both surfaces when `SetupOffsetValues` succeeds;
//! `RailUv` with `nx >= 3`, `nf >= 2` and a Jacobian of at
//! least 2 rows, 3 columns and `rows <= nf`; `Split` only for geometry index 0 (the
//! solver has one geometry: `m_vFilletGeoms[i]` past it is out of bounds; non-empty
//! intervals reach `GetRailVertex` on the constructor's rail-less geometry, a `NULL`
//! dereference). [`eval_static`]: `RailRail` rails `0`/`1` and `rows <= nf`; `Clipped`
//! surface index `0`/`1` (upstream indexes `sUV[1 - lSurfIndex]`); `FindNext` classes of
//! `SmPointClassificationType`; `Convexity` only for a fresh edge or an edge of a file or
//! face B-rep; indices inside the B-rep's export.

use crate::units::topology_edgeuse_p1::{self as eu1, bad, brep_block, curve, surface, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest array length accepted by [`eval`].
pub const MAX_ARRAY: usize = 32;
/// Largest number of `State` commands accepted by [`eval`].
pub const MAX_COMMANDS: usize = 32;
/// Largest `SmFilStatus` value.
const MAX_STATUS: usize = 12;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// Constructor, getters and the offset bookkeeping script.
    State,
    /// `SetupOffsetExtension`.
    Extension,
    /// `SmFilletSphereSolveENFO::Evaluate`.
    Sphere,
    /// `SmRailUVCurveIntersectENFO::Evaluate`.
    RailUv,
    /// `ComputeSurfaceValues`.
    SurfValues,
    /// `ComputeUVVectors`.
    UvVectors,
    /// `SplitFilletGeom` with empty interval arrays.
    Split,
    /// `FinishConstruction`.
    Finish,
    /// The fillet surface generators.
    Fsg,
    /// `SmTol::GetAngTolRad(const SmFilletSolver*)`, the base `LoadInitialValues`.
    Misc,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::State => 0,
            Op::Extension => 1,
            Op::Sphere => 2,
            Op::RailUv => 3,
            Op::SurfValues => 4,
            Op::UvVectors => 5,
            Op::Split => 6,
            Op::Finish => 7,
            Op::Fsg => 8,
            Op::Misc => 9,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_solver_p1_static_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_solver_p1_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// `(nx, nf, Jacobian rows and columns)` of an array block.
type ArraySizes = (usize, usize, Option<(usize, usize)>);

fn arrays(rd: &mut Reader<'_>) -> Result<ArraySizes, FfiError> {
    let nx = rd.index(MAX_ARRAY)?;
    rd.finites(nx)?;
    let nf = rd.index(MAX_ARRAY)?;
    rd.finites(nf)?;
    let has_j = rd.flag()?;
    let rows = rd.index(MAX_ARRAY)?;
    let cols = rd.index(MAX_ARRAY)?;
    Ok((nx, nf, has_j.then_some((rows, cols))))
}

fn validate_state(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.finites(3)?;
    surface(rd)?;
    surface(rd)?;
    for _ in 0..2 {
        rd.flag()?;
        rd.finite()?;
    }
    rd.finites(2)?;
    let k = rd.index(MAX_COMMANDS)?;
    for _ in 0..k {
        let cmd = rd.index(7)?;
        match cmd {
            4 => {
                rd.index(1)?;
                rd.finite()?;
            }
            7 => {
                rd.index(MAX_STATUS)?;
                rd.finite()?;
            }
            _ => {
                rd.finites(2)?;
            }
        }
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::State => validate_state(&mut rd)?,
        Op::Extension => {
            rd.finite()?;
            surface(&mut rd)?;
            rd.finites(4)?;
        }
        Op::Sphere => {
            rd.finites(4)?;
            let adjusts = rd.flag()?;
            let load = rd.flag()?;
            rd.flag()?;
            let has = surfaces(&mut rd)?;
            let (nx, nf, j) = arrays(&mut rd)?;
            if let Some((rows, _)) = j {
                if nf < rows {
                    return Err(bad("sphere: rF shorter than the Jacobian"));
                }
            }
            let small_j = j.is_some_and(|(rows, cols)| rows < 1 || cols < 2);
            if load && (!has[0] || nx < 2 || nf < 1 || small_j || (adjusts && (!has[1] || nx < 4)))
            {
                return Err(bad(
                    "sphere: a loading solver needs its surfaces, crX, rF[0], J[0][0..2]",
                ));
            }
        }
        Op::RailUv => {
            rd.index(1)?;
            rd.flag()?;
            rd.flag()?;
            // an SmLine UV curve: start, end (finite) and its dimension, 2 or 3
            if rd.flag()? {
                rd.finites(6)?;
                if rd.index(3)? < 2 {
                    return Err(bad("rail-uv: line dimension must be 2 or 3"));
                }
            }
            curve(&mut rd)?;
            rd.finites(2)?;
            let (nx, nf, j) = arrays(&mut rd)?;
            if nx < 3 || nf < 2 {
                return Err(bad("rail-uv: need nx >= 3 and nf >= 2"));
            }
            if let Some((rows, cols)) = j {
                if rows < 2 || cols < 3 || nf < rows {
                    return Err(bad(
                        "rail-uv: Jacobian needs rows >= 2, cols >= 3, rows <= nf",
                    ));
                }
            }
        }
        Op::SurfValues => {
            let setup = rd.flag()?;
            let has = surfaces(&mut rd)?;
            rd.finites(4)?;
            if setup && !(has[0] && has[1]) {
                return Err(bad("surf-values: both offset surfaces are dereferenced"));
            }
        }
        Op::UvVectors => {
            rd.finites(15 + 4)?;
        }
        Op::Split => {
            rd.index(0)?;
        }
        Op::Finish => {
            surface(&mut rd)?;
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::Fsg => {
            let kind = rd.index(5)?;
            rd.finite()?;
            if kind == 5 {
                rd.index(MAX_ARRAY)?;
            } else {
                rd.finite()?;
            }
            rd.finites(2)?;
        }
        Op::Misc => {
            if rd.index(1)? == 0 {
                rd.flag()?;
                rd.finite()?;
            } else {
                rd.index(MAX_ARRAY)?;
                rd.index(1)?;
                rd.index(MAX_ARRAY)?;
            }
        }
    }
    rd.done()
}

/// `surf has0 off0 has1 off1`; returns which surfaces are present.
fn surfaces(rd: &mut Reader<'_>) -> Result<[bool; 2], FfiError> {
    surface(rd)?;
    let h0 = rd.flag()?;
    rd.finite()?;
    let h1 = rd.flag()?;
    rd.finite()?;
    Ok([h0, h1])
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed surface and curve blocks, every
        // index, count and flag the shim passes to upstream, the array sizes upstream
        // indexes); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_blend_fillet_solver_p1_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// The routines of the static shim (`harness/cpp/shim/blend_fillet_solver_p1_static.cpp`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticOp {
    /// `sm_FindNextGoodInterval`.
    FindNext,
    /// `sm_CheckEdgeCurveDomains`.
    CheckDomains,
    /// `sm_TestAnalyticSurface`.
    Analytic,
    /// `SmRailRailIntersectENFO::Evaluate`.
    RailRail,
    /// The `SmFindClippedRadiusEFO` constructor and `Evaluate`.
    Clipped,
    /// `sm_TestSideEdgeConvexity`.
    Convexity,
    /// `sm_TrimRail`.
    TrimRail,
    /// `GetVertex`, `FindIndexOfRailOnFace`, `FindIndexOfRailXSideEdgeuse`.
    Queries,
    /// `sm_TestAnalyticSurface` on a cone, cylinder, sphere or torus.
    AnalyticRev,
    /// `sm_TestManifoldTangentRollover`.
    Rollover,
}

impl StaticOp {
    fn code(self) -> i32 {
        match self {
            StaticOp::FindNext => 0,
            StaticOp::CheckDomains => 1,
            StaticOp::Analytic => 2,
            StaticOp::RailRail => 3,
            StaticOp::Clipped => 4,
            StaticOp::Convexity => 5,
            StaticOp::TrimRail => 6,
            StaticOp::Queries => 7,
            StaticOp::AnalyticRev => 8,
            StaticOp::Rollover => 9,
        }
    }

    /// Whether the op starts with a B-rep block.
    #[must_use]
    pub fn has_brep(self) -> bool {
        matches!(
            self,
            StaticOp::CheckDomains
                | StaticOp::Convexity
                | StaticOp::TrimRail
                | StaticOp::Queries
                | StaticOp::Rollover
        )
    }
}

/// `SM_PC_UNKNOWN ..= SM_PC_NOTHING`.
const POINT_CLASSES: std::ops::RangeInclusive<usize> = 100..=110;

fn validate_static_args(
    op: StaticOp,
    rd: &mut Reader<'_>,
    counts: [usize; 10],
    block_kind: f64,
) -> Result<(), FfiError> {
    let [_, _, _, n_f, _, _, n_eu, n_e, _, _] = counts;
    match op {
        StaticOp::FindNext => {
            let n = rd.index(MAX_ARRAY)?;
            rd.index(MAX_ARRAY)?;
            for _ in 0..n {
                if !POINT_CLASSES.contains(&rd.index(*POINT_CLASSES.end())?) {
                    return Err(bad("find-next: not an SmPointClassificationType"));
                }
            }
        }
        StaticOp::CheckDomains => {
            rd.below(n_e)?;
            rd.flag()?;
            rd.flag()?;
        }
        StaticOp::Analytic => {
            surface(rd)?;
        }
        StaticOp::RailRail => {
            rd.index(1)?;
            rd.index(1)?;
            for _ in 0..4 {
                rd.flag()?;
            }
            let (_, nf, j) = arrays(rd)?;
            if let Some((rows, _)) = j {
                if nf < rows {
                    return Err(bad("rail-rail: rF shorter than the Jacobian"));
                }
            }
        }
        StaticOp::Clipped => {
            rd.index(1)?;
            rd.finites(2)?;
            surface(rd)?;
            surface(rd)?;
            for _ in 0..2 {
                rd.flag()?;
                rd.finite()?;
            }
            rd.finites(2 + 8)?;
            rd.index(4)?;
            rd.finites(1 + 4)?;
            rd.flag()?;
            rd.finite()?;
        }
        StaticOp::Convexity => {
            if rd.num()? == -1.0 {
                // a fresh edge
            } else {
                rd.at -= 1;
                if block_kind != 0.0 && block_kind != 2.0 {
                    return Err(bad("convexity: only edges of file or face B-reps"));
                }
                rd.below(n_e)?;
            }
            rd.flag()?;
        }
        StaticOp::TrimRail => {
            rd.below(n_e)?;
            rd.finites(2)?;
        }
        StaticOp::Rollover => {
            rd.below(n_e)?;
            rd.finite()?;
        }
        StaticOp::Queries => {
            rd.below_or_none(n_eu)?;
            rd.below_or_none(n_eu)?;
            rd.below_or_none(n_f)?;
            rd.below(n_eu)?;
        }
        StaticOp::AnalyticRev => {
            rd.index(3)?;
            let v = rd.finites(9)?;
            let [_, _, _, x0, x1, x2, y0, y1, y2] = v[..] else {
                return Err(bad("analytic: placement"));
            };
            // the placement's axes: long enough and far from parallel (as unit
            // surface.cone_p1 requires)
            let (lx, ly) = (
                (x0 * x0 + x1 * x1 + x2 * x2).sqrt(),
                (y0 * y0 + y1 * y1 + y2 * y2).sqrt(),
            );
            let c = [x1 * y2 - x2 * y1, x2 * y0 - x0 * y2, x0 * y1 - x1 * y0];
            let lc = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
            if lx < 1e-6 || ly < 1e-6 || lc < 1e-3 * lx * ly {
                return Err(bad("analytic: degenerate or parallel axes"));
            }
            rd.finites(4)?;
        }
    }
    Ok(())
}

fn raw_static(op: StaticOp, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let c_path = std::ffi::CString::new(path).map_err(|_| bad("path with NUL"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: c_path is a valid NUL-terminated string and input is valid for
        // input.len() reads, holding what the shim reads for `op` (validated by
        // eval_static: well-formed blocks, indices inside the B-rep's export, the array
        // sizes upstream indexes; a B-rep block alone when the B-rep cannot be built,
        // after which the shim reads nothing); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_solver_p1_static_eval(
                op.code(),
                c_path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs a [`StaticOp`] on `input` (layouts in the module docs; `path` is the `.smb` file
/// of a file B-rep block, else empty).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval_static(op: StaticOp, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    let mut counts = [0; 10];
    let mut block_kind = -1.0;
    if op.has_brep() {
        block_kind = input.first().copied().unwrap_or(-1.0);
        brep_block(&mut rd, path)?;
        let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
        let export = eu1::eval(eu1::Op::Export, path, block)?;
        let Some(c) = eu1::counts(&export) else {
            // the B-rep cannot be built: the shim reports the same failure
            return raw_static(op, path, block);
        };
        counts = c;
    }
    validate_static_args(op, &mut rd, counts, block_kind)?;
    rd.done()?;
    raw_static(op, path, input)
}
