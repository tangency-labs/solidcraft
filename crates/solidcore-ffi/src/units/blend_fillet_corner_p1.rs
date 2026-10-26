//! FFI wrappers for unit `blend.fillet_corner_p1`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_corner_p1.cpp` (`sc_blend_fillet_corner_p1_helper`),
//! which compiles `source/SMLib/src/SmFilletCorner.cpp` to reach its file-static helpers.
//!
//! | Op                     | Upstream                                  | Input after the op                 | Output |
//! |------------------------|-------------------------------------------|------------------------------------|--------|
//! | [`Op::Planar`]         | `sm_CreatePlanarSurface`                  | `n p(3).. tol` (`n <= 6`)          | `status` [surface block] |
//! | [`Op::Revolution`]     | `sm_CreateSrfOfRevolution`                | `n curve.. major tol` (`1..=8` 3D curves) | `status` [surface block] |
//! | [`Op::FilletRadius`]   | `sm_GetFilletRadius`                      | `p1 n1 p2 n2 orient`               | `radius span` |
//! | [`Op::SeamOut`]        | `sm_CheckSeamOut`                         | `surface u v tol closure`          | `ret u v` |
//! | [`Op::Closure`]        | `sm_SetSurfaceClosure`                    | `surface u0 v0 u1 v1`              | `closure` |
//! | [`Op::G1`]             | `sm_TestG1Continuity`                     | block, `v e1 e2 tol`               | `0 flag has_angle angle` |
//! | [`Op::FaceByTwo`]      | `sm_GetFaceByTwoEdgeuses`                 | block, `eu1 eu2 eu3`               | `0 face` |
//! | [`Op::Tangential`]     | `sm_CheckTangentialEdgeuse`               | block, `eu tol`                    | `0 flag` |
//! | [`Op::Concavity`]      | `sm_CheckFaceComplimentaryCornerConcavity`| block, `v eu1 eu2 ref`             | `0 flag` |
//! | [`Op::Nx1Degeneracy`]  | `SmFilletNx1Corner::CheckNx1Degeneracy`   | block, `v eu`                      | `0 ret` (the raw `SmBoolean` return: the flag, or the status `SER` returns through it) |
//!
//! Curves and surfaces are unit `topology.edgeuse_p1`'s curve and surface blocks
//! (`dim n p m cpts(x y z w) knots`; `1 O N` for an `SmPlane`, else the B-spline surface
//! block); a surface block in the output is the shim's `put_surface` (`1 O X Y scale
//! step_domain` for an `SmPlane`, `0 oob form n m p q r s cpts u v` for a B-spline
//! surface). `status` is `0` for `SM_SUCCESS`, else the `SmStatus`; `closure` is the
//! `SmSurfParamType` code.
//!
//! The topology ops ([`Op::is_brep`]) start with `topology.edgeuse_p1`'s B-rep block
//! (`path` is the `.smb` file of a file block) and address the built B-rep's vertices,
//! edges and edge uses by their index in that unit's export (`-1` = NULL where the
//! routine accepts it). Their output starts with `0`, or is `-1 status` when the B-rep
//! could not be built; a result of `-2` means "refused": the routine would dereference
//! null (an edge without a curve, an edge use without a radial, vertex use or edge).
//!
//! Inputs are validated before the call: the blocks, every index range, finite and
//! bounded coordinates, a non-empty curve array with the major arc inside it (upstream
//! indexes it unchecked) and closure codes `0..=4`.
//!
//! The `SmFilletCorner` methods ([`corner`], shim
//! `harness/cpp/shim/blend_fillet_corner_p1_corner.inc` compiled inside
//! `blend_fillet_geom_p1.cpp`, `sc_blend_fillet_corner_p1_corner`) run on a corner built on
//! unit `blend.fillet_geom_p1`'s pseudo-B-rep model (layout in
//! [`super::blend_fillet_geom_p1`]), as unit `blend.fillet_corner_p3`'s corner ops do. After
//! the block: `kind v_edge v_sub` (the derived class `0..=10` in the order of
//! `solidcore::blend::fillet_corner::p3::FilletCornerKind`; the filleted vertex as the
//! start (`0`) / end (`1`) vertex of target wire edge `v_edge`, `-1` none; `v_edge <= -2`:
//! vertex `v_sub` of target face `-(v_edge + 2)` in `SmFace::GetVertices` order), the solver and
//! all edge uses (`has n (edge which)..`: target wire edge, `0` its primary edge use / `1`
//! the mate; `has = 0` for NULL), the corner's fillet vertices (`n v..`) and fillet edges
//! (`n e..`), the flags `bevel blending chamfer degenerate ext_patch adjusted`, then
//! `setback approx tang`, the original faces (`n (edge face)..`: `SetOriginalFace` of a
//! pseudo edge to a target face), then the op's own input:
//!
//! | [`CornerOp`]              | upstream                                  | input after the corner | output after `0` |
//! |---------------------------|-------------------------------------------|------------------------|------------------|
//! | [`CornerOp::Peek`]        | the data access methods, predicates and setters (`SetBevel`, `SetCalcAsConstRad`, `SetChamfer`, `AddFilletVertex` first) | `set_bevel set_const_rad set_chamfer add_vertex set_g1 set_status prefill nk kinds..` (`-1` = not called; chamfer only for the `NxN` kinds, `m_bG1Case` only for the closed Nx1 / Nx2 kinds; `prefill` starts the `GetFilletEdges` / `GetFilletVertices` arrays with the model's first edge / vertex) | `tol n edges.. n verts.. v_edge v_sub setback status type type_string_ok is_kind_of.. bevel blending degenerate ext_patch const_rad tangent` |
//! | [`CornerOp::Solvers`]     | `GetTangentFilletSolver`, `GetFilletSolvers` | `ns (edge which approx tang).. q` (solvers registered for target edge uses; `q == ns` an unregistered solver) | `tangent n solvers..` (registration indices, `-1` NULL) |
//! | [`CornerOp::ExtSurface`]  | `AddExtendedOriginalSurface`, `GetExtendedSurface` | `na (face s).. nq faces..` (a plane with natural domain `[0, s]²` per add) | per query `umax vmax` (`-1 -1` NULL) |
//! | [`CornerOp::Reinit`]      | `ReInitializeCornerEdges`                 |                        | the model export |
//! | [`CornerOp::Squeeze`]     | `SqueezeEdge`                             | `e`                    | `status` corner-members model export (as `blend.fillet_corner_p3`'s corner record) |
//! | [`CornerOp::RailRail`]    | `SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve` | `curve` (3D)  | `ret` (the raw `SmBoolean` return) |
//! | [`CornerOp::VertGeom`]    | `CalcCornerVertGeom`                      |                        | `status` corner-members model export |
//! | [`CornerOp::EdgeGeom`]    | `CalcCornerEdgeGeom`                      |                        | `status` corner-members model export |
//! | [`CornerOp::FixDegeneracy`] | `CheckAndFixDegeneracy`                 | `nt topo..` (the executive's topology edges, model edges) | `ret ntopo topo..` corner-members model export |
//! | [`CornerOp::EndPatch`]    | `CreateEndPatch`                          |                        | `status` corner-members model export |
//!
//! A `CheckAndFixDegeneracy` input is rejected by [`validate_corner`] when its deletions
//! would free a model object twice or dereference a freed one: a repeated corner vertex or
//! edge, or a corner vertex with a mate slot still pointing at an earlier-deleted corner
//! vertex when its own destructor runs (each destructor clears one slot per backlink, so
//! the deletions are simulated in order), on a corner whose vertices are all processed
//! (upstream's harmless unprocessed-vertex return; its far-vertex return is not modelled,
//! so such repeats are refused as well).
//!
//! A record of `-2` is "refused" (upstream would dereference null, or free an object twice:
//! a squeezed non-rail edge with both ends at the deleted vertex, or a squeezed closed edge
//! whose surviving vertex is the deleted one; for `GetTangentFilletSolver` of the closed
//! Nx1 and Nx2 corners, unit `blend.fillet_corner_p2`, a curveless filleted edge that
//! `sm_TestG1Continuity` would evaluate), `-3` "not run" (the Rust port of a callee is a
//! stub: `MakeFaceBrep` for an end patch with an original face).

use crate::{call_with_buffer, FfiError};

use super::blend_fillet_geom_p1::{
    curve as p1_curve, validate_block, Block, Reader as P1Reader, MAX_LIST, MAX_MAGNITUDE,
};
use super::topology_edgeuse_p1::{self as eu1, bad, brep_block, curve_up_to, surface, Reader};

/// The largest point count of [`Op::Planar`] (upstream handles 3 and 4; others fail).
pub const MAX_PLANAR_POINTS: usize = 6;
/// The largest curve count of [`Op::Revolution`].
pub const MAX_CURVES: usize = 8;
/// The largest control point index of an [`Op::Revolution`] curve.
pub const MAX_CURVE_INDEX: usize = 16;
/// The largest `SmSurfParamType` code (`SM_SP_NEITHER`).
pub const MAX_CLOSURE: usize = 4;

/// The helper run by [`eval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `sm_CreatePlanarSurface`.
    Planar = 0,
    /// `sm_CreateSrfOfRevolution`.
    Revolution = 1,
    /// `sm_GetFilletRadius`.
    FilletRadius = 2,
    /// `sm_CheckSeamOut`.
    SeamOut = 3,
    /// `sm_SetSurfaceClosure`.
    Closure = 4,
    /// `sm_TestG1Continuity`.
    G1 = 5,
    /// `sm_GetFaceByTwoEdgeuses`.
    FaceByTwo = 6,
    /// `sm_CheckTangentialEdgeuse`.
    Tangential = 7,
    /// `sm_CheckFaceComplimentaryCornerConcavity`.
    Concavity = 8,
    /// `SmFilletNx1Corner::CheckNx1Degeneracy`.
    Nx1Degeneracy = 9,
}

impl Op {
    /// Every op, in code order.
    pub const ALL: [Self; 10] = [
        Self::Planar,
        Self::Revolution,
        Self::FilletRadius,
        Self::SeamOut,
        Self::Closure,
        Self::G1,
        Self::FaceByTwo,
        Self::Tangential,
        Self::Concavity,
        Self::Nx1Degeneracy,
    ];

    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// The op with shim code `code`, `None` outside `0..=9`.
    #[must_use]
    pub fn from_code(code: i64) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|op| i64::from(op.code()) == code)
    }

    /// Whether the input starts with a B-rep block.
    #[must_use]
    pub fn is_brep(self) -> bool {
        matches!(
            self,
            Self::G1 | Self::FaceByTwo | Self::Tangential | Self::Concavity | Self::Nx1Degeneracy
        )
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_corner_p1_helper(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_blend_fillet_corner_p1_corner(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_blend_fillet_corner_p1_helper(
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

/// The geometry ops' arguments.
fn geom_args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Planar => {
            let n = rd.index(MAX_PLANAR_POINTS)?;
            for _ in 0..n {
                rd.vec3()?;
            }
            rd.finite()?;
        }
        Op::Revolution => {
            let n = rd.index(MAX_CURVES)?;
            if n == 0 {
                return Err(bad(
                    "revolution: no curves (upstream indexes the empty array)",
                ));
            }
            for _ in 0..n {
                if rd.input.get(rd.at).copied() != Some(3.0) {
                    return Err(bad("revolution: curves must be 3D"));
                }
                curve_up_to(rd, MAX_CURVE_INDEX)?;
            }
            rd.below(n)?;
            rd.finite()?;
        }
        Op::FilletRadius => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            rd.finite()?;
        }
        Op::SeamOut => {
            surface(rd)?;
            rd.finite()?;
            rd.finite()?;
            rd.finite()?;
            rd.index(MAX_CLOSURE)?;
        }
        Op::Closure => {
            surface(rd)?;
            rd.finites(4)?;
        }
        Op::G1 | Op::FaceByTwo | Op::Tangential | Op::Concavity | Op::Nx1Degeneracy => {
            return Err(bad("not a geometry op"));
        }
    }
    Ok(())
}

/// The topology ops' arguments; `c` holds the export's counts (`[.., edge uses (6),
/// edges (7), vertex uses (8), vertices (9)]`).
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let (n_eu, n_e, n_v) = (c[6], c[7], c[9]);
    match op {
        Op::G1 => {
            rd.below_or_none(n_v)?;
            rd.below_or_none(n_e)?;
            rd.below_or_none(n_e)?;
            rd.finite()?;
        }
        Op::FaceByTwo => {
            rd.below_or_none(n_eu)?;
            rd.below_or_none(n_eu)?;
            rd.below_or_none(n_eu)?;
        }
        Op::Tangential => {
            rd.below(n_eu)?;
            rd.finite()?;
        }
        Op::Concavity => {
            rd.below(n_v)?;
            rd.below(n_eu)?;
            rd.below(n_eu)?;
            rd.below_or_none(n_eu)?;
        }
        Op::Nx1Degeneracy => {
            rd.below(n_v)?;
            rd.below(n_eu)?;
        }
        Op::Planar | Op::Revolution | Op::FilletRadius | Op::SeamOut | Op::Closure => {
            return Err(bad("not a topology op"));
        }
    }
    Ok(())
}

/// Validates `input` for `op` without running it.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; for the topology ops, also the
/// errors of building and exporting the B-rep (which runs the shim of unit
/// `topology.edgeuse_p1`).
pub fn validate(op: Op, path: &str, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    if !op.is_brep() {
        geom_args(op, &mut rd)?;
        return rd.done();
    }
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = eu1::eval(eu1::Op::Export, path, block)?;
    let Some(c) = eu1::counts(&export) else {
        // the B-rep cannot be built: the shim reports it; the arguments are not checked
        return Ok(());
    };
    brep_args(op, &mut rd, c)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); `path` is the `.smb` file of a
/// file B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, path, input)?;
    raw(op, path, input)
}

// ---- the SmFilletCorner methods on the pseudo-B-rep model ----------------------------------

/// The corner-method op run by [`corner`] (layouts in the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerOp {
    /// The data access methods, predicates and setters.
    Peek = 0,
    /// `GetTangentFilletSolver` and `GetFilletSolvers`.
    Solvers = 1,
    /// `AddExtendedOriginalSurface` and `GetExtendedSurface`.
    ExtSurface = 2,
    /// `ReInitializeCornerEdges`.
    Reinit = 3,
    /// `SqueezeEdge`.
    Squeeze = 4,
    /// `SmFilletConcaveNxNCorner::TestRailRailInterpolationCurve`.
    RailRail = 5,
    /// `CalcCornerVertGeom`.
    VertGeom = 6,
    /// `CalcCornerEdgeGeom`.
    EdgeGeom = 7,
    /// `CheckAndFixDegeneracy`.
    FixDegeneracy = 8,
    /// `CreateEndPatch`.
    EndPatch = 9,
    /// `MakeFaceBrep` on given corner surfaces.
    MakeFace = 10,
    /// `InsertIntersectionTopology` with given corner surfaces (`status table corner model`,
    /// `table` the executive topology intersector's intersection table).
    Insert = 11,
    /// `MakeConstantRadiusFillet` with a solver per model geometry.
    ConstRad = 12,
}

impl CornerOp {
    /// Every corner op, in code order.
    pub const ALL: [Self; 13] = [
        Self::Peek,
        Self::Solvers,
        Self::ExtSurface,
        Self::Reinit,
        Self::Squeeze,
        Self::RailRail,
        Self::VertGeom,
        Self::EdgeGeom,
        Self::FixDegeneracy,
        Self::EndPatch,
        Self::MakeFace,
        Self::Insert,
        Self::ConstRad,
    ];
}

/// The largest number of registered solvers, added surfaces, queries and type codes.
pub const MAX_CORNER_LIST: usize = 16;

/// `-2^63` and `2^63`: the type codes the shim casts to `int64_t` must lie in
/// `[-2^63, 2^63)` (the cast of anything else is undefined).
const INT64_MIN_F64: f64 = -9_223_372_036_854_775_808.0;
const INT64_MAX_PLUS_ONE_F64: f64 = 9_223_372_036_854_775_808.0;

/// An edge use list `has n (edge which)..` of target wire edges.
fn corner_eu_list(rd: &mut P1Reader<'_>, nt: usize) -> Result<(), FfiError> {
    rd.flag()?;
    let n = rd.index(MAX_LIST)?;
    for _ in 0..n {
        rd.opt_index(nt)?
            .ok_or_else(|| bad("edge use: target edge -1"))?;
        rd.index(1)?;
    }
    Ok(())
}

/// Validates the input of [`corner`] (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely.
pub fn validate_corner(op: CornerOp, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = P1Reader {
        cursor: crate::reader::Cursor::new(input),
        bound: MAX_MAGNITUDE,
    };
    let b = validate_block(&mut rd)?;
    let kind = rd.index(10)?;
    // `v_edge <= -2`: vertex `v_sub` of target face `-(v_edge + 2)` (`GetVertices`
    // order; an index past its vertices is refused by the shim)
    let v_face = rd.num()?;
    if v_face <= -2.0 {
        if v_face.fract() != 0.0 || -(v_face + 2.0) >= b.nf as f64 {
            return Err(bad(format!("bad face vertex face {v_face} (len {})", b.nf)));
        }
        rd.index(MAX_LIST)?;
    } else {
        if v_face != -1.0 && (v_face.fract() != 0.0 || v_face < 0.0 || v_face >= b.nt as f64) {
            return Err(bad(format!("bad optional index {v_face} (len {})", b.nt)));
        }
        let v_sub = rd.index(1)?;
        if v_face == -1.0 && v_sub != 0 {
            return Err(bad("vertex sub without edge"));
        }
    }
    corner_eu_list(&mut rd, b.nt)?;
    corner_eu_list(&mut rd, b.nt)?;
    let nv = rd.index(MAX_LIST)?;
    let mut verts = Vec::with_capacity(nv);
    for _ in 0..nv {
        verts.push(rd.opt_index(b.nv())?.ok_or_else(|| bad("vertex -1"))?);
    }
    let ne = rd.index(MAX_LIST)?;
    let mut edges = Vec::with_capacity(ne);
    for _ in 0..ne {
        edges.push(rd.opt_index(b.ne())?.ok_or_else(|| bad("edge -1"))?);
    }
    for _ in 0..6 {
        rd.flag()?;
    }
    rd.finites(3)?;
    let no = rd.index(MAX_LIST)?;
    for _ in 0..no {
        rd.opt_index(b.ne())?
            .ok_or_else(|| bad("original face: edge -1"))?;
        rd.opt_index(b.nf)?
            .ok_or_else(|| bad("original face: face -1"))?;
    }
    match op {
        CornerOp::Peek => {
            // the setters: -1 (not called), 0 or 1
            for _ in 0..3 {
                rd.opt_index(2)?;
            }
            rd.opt_index(b.nv())?;
            rd.opt_index(2)?; // m_bG1Case
            rd.opt_index(13)?; // SetStatus (SmFilStatus)
            rd.flag()?; // prefill
            let nk = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..nk {
                // the shim reads each type code as an `int64_t` (a plain cast): only
                // representable values, any of them (unknown codes test `IsKindOf` too)
                let code = rd.finite()?;
                if !(INT64_MIN_F64..INT64_MAX_PLUS_ONE_F64).contains(&code) {
                    return Err(bad("type code outside the int64 range"));
                }
            }
        }
        CornerOp::Solvers => {
            let ns = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..ns {
                rd.opt_index(b.nt)?
                    .ok_or_else(|| bad("solver: target edge -1"))?;
                rd.index(1)?;
                rd.finites(2)?;
            }
            rd.index(ns)?;
        }
        CornerOp::ExtSurface => {
            let na = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..na {
                rd.opt_index(b.nf)?
                    .ok_or_else(|| bad("extended surface: face -1"))?;
                if rd.finite()? <= 0.0 {
                    return Err(bad("extended surface: domain size must be positive"));
                }
            }
            let nq = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..nq {
                rd.opt_index(b.nf)?
                    .ok_or_else(|| bad("extended surface query: face -1"))?;
            }
        }
        CornerOp::Reinit | CornerOp::VertGeom | CornerOp::EdgeGeom | CornerOp::EndPatch => {}
        CornerOp::Squeeze => {
            rd.opt_index(b.ne())?
                .ok_or_else(|| bad("squeeze: edge -1"))?;
        }
        CornerOp::RailRail => {
            if kind != 10 {
                return Err(bad(
                    "rail-rail test: the corner must be a concave NxN corner",
                ));
            }
            p1_curve(&mut rd, 3, "rail-rail curve")?;
        }
        CornerOp::MakeFace | CornerOp::Insert => {
            let ns = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..ns {
                super::blend_fillet_geom_p1::surface(&mut rd)?;
            }
        }
        CornerOp::ConstRad => {
            let ng = rd.index(b.geom_surfaces.len())?;
            if ng != b.geom_surfaces.len() {
                return Err(bad("constant radius: one solver kind per model geometry"));
            }
            for _ in 0..ng {
                // -1 none, 0 circular, 1 blend curve, 2 linear, 3 base, 4 no generator
                rd.opt_index(5)?;
                let scale = rd.finite()?;
                if scale <= 0.0 {
                    return Err(bad("constant radius: blend scale must be positive"));
                }
                rd.index(2)?;
            }
        }
        CornerOp::FixDegeneracy => {
            let nt = rd.index(MAX_CORNER_LIST)?;
            for _ in 0..nt {
                rd.opt_index(b.ne())?
                    .ok_or_else(|| bad("topology edge -1"))?;
            }
            degeneracy_deletions(&b, &verts, &edges)?;
        }
    }
    rd.done()
}

/// `CheckAndFixDegeneracy` past its early-outs (every corner vertex processed; the
/// distance test is not modelled here, a far vertex's harmless return is refused too)
/// deletes every corner edge and every corner vertex but the first, then uses the first:
/// a repeated edge or vertex is freed twice (a repeated first vertex is the survivor,
/// used after its free). `~SmFilletVertex` reads each of its two mate slots: a slot still
/// holding an earlier-freed vertex is a use after free. For each slot it clears **one**
/// slot of that mate pointing back (the first, else the second), so a vertex survives the
/// deletion of a mate only if the mate's slots point back at it at least as often as its
/// own slots point at the mate. The deletions are simulated in order on the model's mate
/// slots; an offending input is refused before the native call.
fn degeneracy_deletions(b: &Block, verts: &[usize], edges: &[usize]) -> Result<(), FfiError> {
    if !verts
        .iter()
        .all(|&v| b.vertex_processed.get(v).copied().unwrap_or(false))
    {
        return Ok(());
    }
    if has_repeat(verts) {
        return Err(bad("degeneracy: repeated corner vertex"));
    }
    if has_repeat(edges) {
        return Err(bad("degeneracy: repeated corner edge"));
    }
    let mut mates = b.vertex_mates.clone();
    let mut freed = Vec::new();
    for &v in verts.iter().skip(1) {
        for slot in 0..2 {
            let Some(m) = mates.get(v).and_then(|s| s.get(slot)).copied().flatten() else {
                continue;
            };
            if freed.contains(&m) {
                return Err(bad(
                    "degeneracy: mate slot pointing at an earlier-deleted corner vertex",
                ));
            }
            // m_pMate[j]->SetMate(0 or 1, NULL): the first of the mate's slots pointing back
            if let Some(x) = mates
                .get_mut(m)
                .and_then(|back| back.iter_mut().find(|x| **x == Some(v)))
            {
                *x = None;
            }
        }
        freed.push(v);
    }
    Ok(())
}

fn has_repeat(ids: &[usize]) -> bool {
    ids.iter()
        .enumerate()
        .any(|(i, id)| ids.get(..i).is_some_and(|s| s.contains(id)))
}

/// Runs the corner-method op `op` (layout in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for an input upstream could not handle safely;
/// [`FfiError::Status`] for a shim error.
pub fn corner(op: CornerOp, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_corner(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads (validated above); buf is valid
        // for buf.len() writes; len is a valid pointer.
        unsafe {
            sys::sc_blend_fillet_corner_p1_corner(
                op as i32,
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

    #[test]
    fn op_codes_round_trip() {
        for op in Op::ALL {
            assert_eq!(Op::from_code(i64::from(op.code())), Some(op));
        }
        assert_eq!(Op::from_code(10), None);
        assert_eq!(Op::from_code(-1), None);
    }

    /// A `peek` input on an empty model and a bare corner, with the given type codes.
    fn peek_input(kinds: &[f64]) -> Vec<f64> {
        // the model block: no targets, faces, fillet B-rep edges, vertices, geometries, edges
        let mut v = vec![0.0; 6];
        // kind, no filleted vertex; no solver / all edge uses; no vertices / edges
        v.extend([0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        // the flags, setback, tolerances, no original faces
        v.extend([0.0; 10]);
        // peek: no setters, no added vertex, no G1 / status, no prefill, the type codes
        v.extend([-1.0, -1.0, -1.0, -1.0, -1.0, -1.0, 0.0, kinds.len() as f64]);
        v.extend_from_slice(kinds);
        v
    }

    #[test]
    fn peek_type_codes_must_be_int64_representable() {
        // guard-only: nothing here reaches the shim
        assert!(validate_corner(CornerOp::Peek, &peek_input(&[16036.0, 0.0, -7.5])).is_ok());
        assert!(validate_corner(CornerOp::Peek, &peek_input(&[9.0e18])).is_ok());
        assert!(
            validate_corner(CornerOp::Peek, &peek_input(&[-9_223_372_036_854_775_808.0])).is_ok()
        );
        assert!(
            validate_corner(CornerOp::Peek, &peek_input(&[9_223_372_036_854_775_808.0])).is_err()
        );
        assert!(validate_corner(CornerOp::Peek, &peek_input(&[1.0e19])).is_err());
        assert!(validate_corner(CornerOp::Peek, &peek_input(&[-1.0e150])).is_err());
        assert!(validate_corner(CornerOp::Peek, &peek_input(&[f64::INFINITY])).is_err());
    }

    /// A `FixDegeneracy` input on a model of bare vertices (status `status`, mates
    /// `mates`) and `ne` curveless edges between the first two, with the corner vertices
    /// `verts`, edges `edges` and no topology edges.
    fn degeneracy_input(
        status: f64,
        mates: &[[f64; 2]],
        ne: usize,
        verts: &[f64],
        edges: &[f64],
    ) -> Vec<f64> {
        // the model block: no targets, faces, fillet B-rep edges; the vertices
        let mut v = vec![0.0, 0.0, 0.0, mates.len() as f64];
        for _ in mates {
            // point, SmFilletVertexType, SmFilStatus, corner tag, SM_PC_UNKNOWN, no
            // classification object, no face, t u v
            v.extend([
                0.0, 0.0, 0.0, 0.0, status, -1.0, 100.0, 0.0, -1.0, 0.0, -1.0,
            ]);
            v.extend([0.0; 3]);
        }
        // no geometries; the edges: vertices 0 1, type, status, corner tag, no curve, no
        // interval, no fillet B-rep edges, no uv curves / geometries, the tolerances
        v.push(0.0);
        v.push(ne as f64);
        for _ in 0..ne {
            v.extend([
                0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, -1.0, 0.0, -1.0, 0.0, -1.0,
            ]);
            v.extend([0.0; 4]);
        }
        // the mates
        for m in mates {
            v.extend(*m);
        }
        // kind, no filleted vertex; no solver / all edge uses; the vertices and edges
        v.extend([0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, verts.len() as f64]);
        v.extend_from_slice(verts);
        v.push(edges.len() as f64);
        v.extend_from_slice(edges);
        // the flags, setback, tolerances, no original faces; no topology edges
        v.extend([0.0; 10]);
        v.push(0.0);
        v
    }

    #[test]
    fn degeneracy_refuses_repeated_deletions_and_one_way_mates() {
        // guard-only: nothing here reaches the shim
        let ok = |i: &[f64]| validate_corner(CornerOp::FixDegeneracy, i).is_ok();
        let none = [[-1.0, -1.0]; 3];
        assert!(ok(&degeneracy_input(
            1.0,
            &none,
            1,
            &[0.0, 1.0, 2.0],
            &[0.0]
        )));
        // a repeated vertex (also the survivor), on processed vertices only
        assert!(!ok(&degeneracy_input(1.0, &none, 0, &[0.0, 1.0, 1.0], &[])));
        assert!(!ok(&degeneracy_input(1.0, &none, 0, &[0.0, 1.0, 0.0], &[])));
        assert!(ok(&degeneracy_input(0.0, &none, 0, &[0.0, 1.0, 1.0], &[])));
        // a repeated edge, unless an unprocessed vertex returns before the deletions;
        // an empty vertex list deletes the edges before the (refused) survivor lookup
        assert!(!ok(&degeneracy_input(1.0, &none, 1, &[0.0], &[0.0, 0.0])));
        assert!(!ok(&degeneracy_input(1.0, &none, 2, &[], &[1.0, 0.0, 1.0])));
        assert!(ok(&degeneracy_input(1.0, &none, 2, &[0.0], &[1.0, 0.0])));
        assert!(ok(&degeneracy_input(0.0, &none, 1, &[0.0], &[0.0, 0.0])));
        // vertex 2 links to vertex 1 one-way: 1 is deleted first, 2's destructor
        // dereferences it; reciprocal links are cleared by the first destructor
        let one_way = [[-1.0, -1.0], [-1.0, -1.0], [1.0, -1.0]];
        assert!(!ok(&degeneracy_input(
            1.0,
            &one_way,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        let mutual = [[-1.0, -1.0], [-1.0, 2.0], [1.0, -1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &mutual,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        // both slots of 2 point at 1: 1's destructor clears one slot per backlink, so a
        // single backlink leaves 2's second slot dangling; two backlinks clear both
        let doubled_once = [[-1.0, -1.0], [2.0, -1.0], [1.0, 1.0]];
        assert!(!ok(&degeneracy_input(
            1.0,
            &doubled_once,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        let doubled_twice = [[-1.0, -1.0], [2.0, 2.0], [1.0, 1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &doubled_twice,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        // a self link is cleared by the vertex's own destructor
        let self_link = [[-1.0, -1.0], [1.0, 1.0], [2.0, -1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &self_link,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        // a chain 3 -> 2 -> 1 with single backlinks, deleted 1, 2, 3: each destructor
        // clears the next vertex's slot before that vertex is destroyed
        let chain = [[-1.0, -1.0], [2.0, -1.0], [1.0, 3.0], [2.0, -1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &chain,
            0,
            &[0.0, 1.0, 2.0, 3.0],
            &[]
        )));
        // the earlier-deleted vertex links one-way to the later one: it clears nothing,
        // but the later one holds no dangling pointer
        let reverse = [[-1.0, -1.0], [2.0, -1.0], [-1.0, -1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &reverse,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        // the survivor and vertices outside the corner are not deleted
        let to_survivor = [[-1.0, -1.0], [-1.0, -1.0], [0.0, -1.0]];
        assert!(ok(&degeneracy_input(
            1.0,
            &to_survivor,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
        assert!(ok(&degeneracy_input(1.0, &one_way, 0, &[0.0, 2.0], &[])));
        // deleting 2 before 1 with 2 -> 1 one-way: 1 is still alive then
        assert!(ok(&degeneracy_input(
            1.0,
            &one_way,
            0,
            &[0.0, 2.0, 1.0],
            &[]
        )));
        // an unprocessed vertex: upstream returns before any deletion
        assert!(ok(&degeneracy_input(
            0.0,
            &one_way,
            0,
            &[0.0, 1.0, 2.0],
            &[]
        )));
    }

    #[test]
    fn rejects_bad_geometry_inputs() {
        // too many planar points
        assert!(validate(Op::Planar, "", &[7.0]).is_err());
        // no curves
        assert!(validate(Op::Revolution, "", &[0.0, 0.0, 1e-6]).is_err());
        // trailing input
        assert!(validate(Op::FilletRadius, "", &[0.0; 14]).is_err());
        assert!(validate(Op::FilletRadius, "", &[0.0; 13]).is_ok());
        // closure code out of range
        let plane = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0];
        let mut v = plane.to_vec();
        v.extend([0.0, 0.0, 1e-6, 5.0]);
        assert!(validate(Op::SeamOut, "", &v).is_err());
    }
}
