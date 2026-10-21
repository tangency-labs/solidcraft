//! FFI wrappers for unit `query.brep`.
//!
//! Shim: `harness/cpp/shim/query_brep.cpp` (`sc_query_brep_evaluate`,
//! `sc_query_brep_attribute`, `sc_query_brep_box`, `sc_query_brep_classify`).
//! Upstream: `source/SM_API/src/SmApiSurfaces.cpp`, `source/SMLib/inc/SmPolyDecimate.h`,
//! `source/SMLib/src/SmPolyDecimate.cpp`, `source/SMLib/src/SmBrep.cpp`.
//!
//! Notation as in [`super::surface_surface_cache_p3`]: `S` a surface `kind ..` (`0 B`:
//! B-spline; `1 o x y sx sy D`: bounded `SmPlane`; `2 B`: `SmCurveBoundedSurface` over the
//! B-spline), `V` a vector `x y z`. A `ULONG` is its two 32-bit halves `hi lo`, so all 64
//! bits are exact.
//!
//! | [`EvalOp`]    | upstream                          |
//! |---------------|-----------------------------------|
//! | `Point`       | `SmApiEvaluateSurfacePoint`       |
//! | `Normal`      | `SmApiEvaluateSurfaceNormal`      |
//! | `Derivatives` | `SmApiEvaluateSurfaceDerivatives` |
//!
//! [`evaluate`] takes `has_surface [S] u v P Du Dv` (`has_surface = 0`: a `NULL` surface;
//! `P Du Dv` the values the outputs hold before the call) and returns the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`) followed by `P Du Dv` as the entry point leaves them.
//! The surface must be one [`super::surface_surface_cache_p3`] accepts; the parameter and
//! the output values may be anything (the entry points reject a parameter outside the
//! natural domain, NaN included, before evaluating).
//!
//! [`attribute`] takes `id has_behavior behavior set [data]`: `behavior` an
//! `SmAttributeBehaviorType` (`0..=4`, used when `has_behavior`, else the constructor's
//! default), and with `set` the data written before the copy: `class distance error N
//! sort_set sort` (`SmDecimateVertexAttr`), `Q0 Q1 Q2 b c` (`SmQuadricVertexAttr`) or `P
//! error sort_set sort` (`SmQuadricEdgeAttr`); a sort index is written only when
//! `sort_set`. It returns `id behavior type data` of the constructed attribute, then of
//! its `MakeCopy`, the data as `sort class distance error N`, `Q0 Q1 Q2 b c` or `P error
//! sort` (the quadric edge's sort index NaN NaN while unset). Classes are `0..=5`.
//!
//! [`box_steps`] builds a real `SmBrep` from unit `topology.edgeuse_p1`'s B-rep block
//! (`path` names the `.smb` file of a file block; validated and built once through that
//! unit's wrapper first) and runs the [`BoxStep`]s on it in order. The output is, per
//! step: for [`BoxStep::Box`] and [`BoxStep::Tight`] the status and the box `min max`
//! (seeded with [`SEED_BOX`], so what a failing call leaves is compared), for
//! [`BoxStep::Lock`] the status, for [`BoxStep::Unlock`] `0`. At most [`MAX_BOX_STEPS`]
//! steps; a B-rep with a face without surface is rejected by the shim (upstream
//! dereferences it).
//!
//! [`classify`] builds the B-rep the same way and runs `Point3DClassify(P, tol, do_bnd,
//! pc)` with `pc` constructed with `tol`; it returns the status and the
//! [`CLASSIFICATION_LEN`] values `class kind index gap src_tol presnap tparam u v
//! param_set face_kind face_index` of `pc` (objects as `kind index`: `1` face, `2` edge,
//! `3` vertex, `4` region by traversal index, `0 -1` none). The point and the tolerance
//! must be finite (any sign or magnitude: upstream takes them as given; a non-finite one
//! could not end the tree descents).

use super::surface_surface_cache_p1::{bad, Reader};
use super::surface_surface_cache_p3::sized_surface;
use super::topology_edgeuse_p1 as graph;
use crate::{call_with_buffer, FfiError};

/// The entry point run by [`evaluate`]; the discriminant is the shim's op code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EvalOp {
    /// `SmApiEvaluateSurfacePoint`.
    Point = 0,
    /// `SmApiEvaluateSurfaceNormal`.
    Normal = 1,
    /// `SmApiEvaluateSurfaceDerivatives`.
    Derivatives = 2,
}

/// `u v` and the nine output sentinels `P Du Dv` after the surface of an [`evaluate`] record.
const EVALUATE_TAIL_LEN: usize = 11;
/// The status and `P Du Dv` of an [`evaluate`] output.
pub const EVALUATE_LEN: usize = 10;

/// The decimation attribute class run by [`attribute`]; the discriminant is the shim's
/// kind code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum AttrKind {
    /// `SmDecimateVertexAttr`.
    DecimateVertex = 0,
    /// `SmQuadricVertexAttr`.
    QuadricVertex = 1,
    /// `SmQuadricEdgeAttr`.
    QuadricEdge = 2,
}

/// A `ULONG` record entry: its two 32-bit halves.
const ULONG_LEN: usize = 2;
/// `id behavior type` at the start of an [`attribute`] output block.
const ATTRIBUTE_HEADER_LEN: usize = ULONG_LEN + 2;
/// Largest `SmPolyVertexClass` value.
const MAX_VERTEX_CLASS: usize = 5;
/// Largest `SmAttributeBehaviorType` value.
const MAX_BEHAVIOR: usize = 4;

impl AttrKind {
    /// The values of the data written before the copy, without the sort index:
    /// `class distance error N`, `Q0 Q1 Q2 b c` or `P error`.
    const fn data_len(self) -> usize {
        match self {
            Self::DecimateVertex => 6,
            Self::QuadricVertex => 13,
            Self::QuadricEdge => 4,
        }
    }

    /// Whether the class has a sort index.
    const fn has_sort(self) -> bool {
        !matches!(self, Self::QuadricVertex)
    }

    /// The length of one `id behavior type data` output block.
    const fn block_len(self) -> usize {
        let sort = if self.has_sort() { ULONG_LEN } else { 0 };
        ATTRIBUTE_HEADER_LEN + self.data_len() + sort
    }
}

/// A step of [`box_steps`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxStep {
    /// `CalculateBoundingBox(box, approx, tight, expand)`, or with the default arguments.
    Box {
        /// `bApprox`.
        approx: bool,
        /// `bTight`.
        tight: bool,
        /// `bExpandByZoneTol3d`.
        expand: bool,
        /// Call with the default arguments instead.
        defaults: bool,
    },
    /// `CalculateTightBoundingBox`.
    Tight,
    /// `LockBoundingBox(tight)`, or with the default argument.
    Lock {
        /// `bTight`.
        tight: bool,
        /// Call with the default argument instead.
        defaults: bool,
    },
    /// `UnLockBoundingBox`.
    Unlock,
}

/// The shim's step codes.
mod step {
    pub const BOX: f64 = 0.0;
    pub const TIGHT: f64 = 1.0;
    pub const LOCK: f64 = 2.0;
    pub const UNLOCK: f64 = 3.0;
}

/// The status and `min max` of a box step's output.
pub const BOX_LEN: usize = 7;

impl BoxStep {
    /// The step's record `kind a b c defaults`.
    fn record(self) -> [f64; 5] {
        match self {
            Self::Box {
                approx,
                tight,
                expand,
                defaults,
            } => [
                step::BOX,
                flag(approx),
                flag(tight),
                flag(expand),
                flag(defaults),
            ],
            Self::Tight => [step::TIGHT, 0.0, 0.0, 0.0, 0.0],
            Self::Lock { tight, defaults } => [step::LOCK, flag(tight), 0.0, 0.0, flag(defaults)],
            Self::Unlock => [step::UNLOCK, 0.0, 0.0, 0.0, 0.0],
        }
    }

    /// The length of the step's output.
    const fn output_len(self) -> usize {
        match self {
            Self::Box { .. } | Self::Tight => BOX_LEN,
            Self::Lock { .. } | Self::Unlock => 1,
        }
    }
}

/// Largest number of [`box_steps`] steps.
pub const MAX_BOX_STEPS: usize = 8;

/// The box every box step starts from, `min max` (no box computation gives it).
pub const SEED_BOX: [f64; 6] = [-17.0, -19.0, -23.0, 29.0, 31.0, 37.0];

/// The classification values of a [`classify`] output (after the status).
pub const CLASSIFICATION_LEN: usize = 12;

mod sys {
    extern "C" {
        pub fn sc_query_brep_classify(
            path: *const std::ffi::c_char,
            block: *const f64,
            n_block: i64,
            args: *const f64,
            n_args: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_query_brep_box(
            path: *const std::ffi::c_char,
            block: *const f64,
            n_block: i64,
            args: *const f64,
            n_args: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_query_brep_attribute(
            kind: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_query_brep_evaluate(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// Fails unless `out` has the `want` values the shim writes.
fn checked_len(out: Vec<f64>, want: usize, what: &str) -> Result<Vec<f64>, FfiError> {
    if out.len() == want {
        Ok(out)
    } else {
        Err(bad(format!("{what}: unexpected output length")))
    }
}

/// Checks an [`evaluate`] input record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate_evaluate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    if rd.flag()? {
        sized_surface(&mut rd)?;
    }
    // u v P Du Dv: anything (see the module docs)
    for _ in 0..EVALUATE_TAIL_LEN {
        rd.num()?;
    }
    rd.done()
}

/// Runs `op` on `input` (layout in the module docs); returns the status and `P Du Dv`.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn evaluate(op: EvalOp, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_evaluate(input)?;
    let out = call_with_buffer(EVALUATE_LEN, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: a well-formed clamped surface or a plane with
        // independent axes, or none); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_query_brep_evaluate(
                op as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    checked_len(out, EVALUATE_LEN, "evaluate")
}

/// A `ULONG`: two integer halves in `0..2^32`.
fn ulong(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    for _ in 0..ULONG_LEN {
        let v = rd.num()?;
        if v.fract() != 0.0 || !(0.0..=f64::from(u32::MAX)).contains(&v) {
            return Err(bad(format!("bad ULONG half {v}")));
        }
    }
    Ok(())
}

/// Checks an [`attribute`] input record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate_attribute(kind: AttrKind, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    ulong(&mut rd)?; // id
    rd.flag()?; // has_behavior
    rd.index(MAX_BEHAVIOR)?;
    if rd.flag()? {
        let mut values = kind.data_len();
        if kind == AttrKind::DecimateVertex {
            rd.index(MAX_VERTEX_CLASS)?;
            values -= 1;
        }
        for _ in 0..values {
            rd.num()?;
        }
        if kind.has_sort() {
            rd.flag()?;
            ulong(&mut rd)?;
        }
    }
    rd.done()
}

/// Constructs and copies a `kind` attribute from `input` (layout in the module docs);
/// returns both `id behavior type data` blocks.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn attribute(kind: AttrKind, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate_attribute(kind, input)?;
    let want = 2 * kind.block_len();
    let out = call_with_buffer(want, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads (validated above: a known class and behaviour, a defined vertex class,
        // exact halves for the ULONG fields); buf is valid for buf.len() writes and the
        // shim honours the capacity; len is valid.
        unsafe {
            sys::sc_query_brep_attribute(
                kind as i32,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    checked_len(out, want, "attribute")
}

/// The signature of the shim entry points that build the fixture B-rep and run `args` on
/// it.
type FixtureFn = unsafe extern "C" fn(
    *const std::ffi::c_char,
    *const f64,
    i64,
    *const f64,
    i64,
    *mut f64,
    i64,
    *mut i64,
) -> i32;

/// Validates and builds the B-rep block once through the graph wrapper, then runs `f`
/// with `args` on the shim's B-rep; returns the `want` output values.
fn on_fixture(
    f: FixtureFn,
    path: &str,
    block: &[f64],
    args: &[f64],
    want: usize,
    what: &str,
) -> Result<Vec<f64>, FfiError> {
    let export = graph::eval(graph::Op::Export, path, block)?;
    if graph::counts(&export).is_none() {
        return Err(bad("native fixture construction failed"));
    }
    let path = std::ffi::CString::new(path).map_err(|_| bad("path contains NUL"))?;
    let n_block = i64::try_from(block.len()).map_err(|_| bad("block too large"))?;
    let out = call_with_buffer(want, |buf, len| {
        // SAFETY: the block was validated (and built) by the graph wrapper and the
        // callers validate `args`; all buffers and the C string stay live for the call,
        // and the shim honours `cap`.
        unsafe {
            f(
                path.as_ptr(),
                block.as_ptr(),
                n_block,
                args.as_ptr(),
                args.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })?;
    checked_len(out, want, what)
}

/// Runs `steps` on the B-rep of `block` (see the module docs); returns every step's
/// output in order.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the graph wrapper rejects, a B-rep that
/// cannot be built, no or too many steps; the shim's status (also for a face without
/// surface).
pub fn box_steps(path: &str, block: &[f64], steps: &[BoxStep]) -> Result<Vec<f64>, FfiError> {
    if steps.is_empty() || steps.len() > MAX_BOX_STEPS {
        return Err(bad("box steps: count"));
    }
    // `seed n` then the step records
    let mut args = SEED_BOX.to_vec();
    args.push(steps.len() as f64);
    for s in steps {
        args.extend(s.record());
    }
    let want = steps.iter().map(|s| s.output_len()).sum();
    on_fixture(sys::sc_query_brep_box, path, block, &args, want, "box steps")
}

/// Classifies `point` against the B-rep of `block` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the graph wrapper rejects, a B-rep that
/// cannot be built, a non-finite point or tolerance; the shim's status (also for a face
/// without surface).
pub fn classify(
    path: &str,
    block: &[f64],
    point: [f64; 3],
    tol: f64,
    do_boundary: bool,
) -> Result<Vec<f64>, FfiError> {
    if point.iter().any(|x| !x.is_finite()) {
        return Err(bad("classify: point"));
    }
    if !tol.is_finite() {
        return Err(bad("classify: tolerance"));
    }
    let [x, y, z] = point;
    let args = [x, y, z, tol, flag(do_boundary)];
    on_fixture(
        sys::sc_query_brep_classify,
        path,
        block,
        &args,
        1 + CLASSIFICATION_LEN,
        "classify",
    )
}
