//! Runner for unit `topology.brep_p7`. C++: `solidcore_ffi::units::topology_brep_p7`.
//! Rust: `solidcore::topology::brep::p7`.
//!
//! **Ops.** `read` (`SmBrep::ReadFromFile` of an upstream `.smb` file, as read or with
//! bytes truncated / overwritten), `make_face_with_curves` (planes and B-spline surfaces
//! with loops of B-spline curves, 3D only, UV only or both, and the input errors),
//! `make_face` (a null surface, or a square wire loop built in the shim), `glue`
//! (`GlueEdgesGeneral` of edge pairs of a B-rep of unit `topology.vertexuse`), `copy`
//! (`CopyFaces` of the first faces into a new B-rep) and `merge_self`
//! (`MergeSelfIntersectionCurves` with and without curves, including raw-knot splines).
//! The output includes status and topology counts, and for returned edges their used
//! intervals, endpoint vertices, curve types, natural intervals, point samples and
//! each edge use's UV trim type, interval and samples when present.
//! The raw-knot plane fixtures construct their B-reps independently on each side.
//! Both sides produce one record (`out`) laid out as in the ffi module; statuses are `SmStatus` values (a Rust
//! `InvalidInput` is `SM_ERR_INVALID_INPUT`, `1007`).
//!
//! **Guards.** A `read` stream goes through the stream guard of unit `topology.brep_data`
//! on both sides (its pure-Rust stream simulator, no C++ call; a rejected stream is an
//! `err("guard")` case). A modified stream (not byte-identical to a shipped file) has its
//! C++ read run in a fresh child process (`solidcore_ffi::isolate`):
//! `MakeTopologyFromData` uses its cross-reference indices unchecked and crashes on some
//! of them (`plan/upstream-issues/UB-164--*.md`), and a child that crashes, or runs past
//! `READ_SECONDS`, makes the case `rejected`. The streams that survive are compared;
//! the Rust side always runs its own read (no C++ verdict decides it). The other
//! shim guards answer `-2` (upstream would dereference null). A `MakeFaceWithCurves` with
//! a curve on surface, 3D curves and no UV curves that gets past its own input checks
//! crashes upstream (`plan/upstream-issues.md` UB-165): `run_cpp` rejects it from the
//! inputs and the Rust side's checks before calling the shim (whose `-2` guard stays as a
//! safety net), so neither side runs. The other two `-2` guards would only echo that
//! constant on both sides, so the Rust side defers them: `cpp-guard:null-curve`
//! (`GlueEdgesGeneral` with `SM_OT_SAME`, equal intervals and one edge without curve),
//! `cpp-guard:null-surface`
//! (`MergeSelfIntersectionCurves` on a face without surface).
//!
//! Raw-knot fixtures reject positive spans at or below `1e-12` (UB-266); their
//! comparison does not establish that the input curves retain raw knots after copying
//! inside the operation. No kernel clone change is implied by this coverage.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::are_same;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::{p10, p7};
use solidcore::topology::database_io::FileType;
use solidcore::topology::edge as ed;
use solidcore::topology::{Brep, EdgeId, FaceId, RegionId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_data as bd;
use solidcore_ffi::units::topology_brep_p7::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse as fixture;
use solidcore_ffi::FfiError;

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;
/// "Not run": the shim keeps the call away from upstream (`-3`).
const NOT_RUN: f64 = -3.0;
/// The shim's `SC_ERR_ARG` (a loop it could not close).
const SC_ERR_ARG: f64 = -1002.0;
/// `NOW`: the weight of a non-rational control point.
const NOW: f64 = -f64::MAX;
/// `NOZ`: the z of a 2D control point.
const NOZ: f64 = f64::MAX;

/// The `.smb` files of the cases (the readable ASCII files of unit `topology.vertexuse`).
const FILES: [&str; 15] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
    "sphere.smb",
    "surf_extrude.smb",
    "surf_rev.smb",
    "surf_sphere150.smb",
    "torus.smb",
    "toruswvy.smb",
    "vase_rev.smb",
];

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve (`w = NOW` non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvIn {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A surface: a plane or a B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    Bspline {
        net: Vec<Vec<[f64; 4]>>,
        p: usize,
        q: usize,
        u: Vec<f64>,
        v: Vec<f64>,
    },
}

/// One curve of a face: its 3D and/or UV curve and its orientation code (`1` SAME, `2`
/// OPPOSITE).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaceCurveIn {
    #[serde(default)]
    c3: Option<CrvIn>,
    #[serde(default)]
    uv: Option<CrvIn>,
    orient: u8,
}

/// A change made to a file's bytes before reading.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Damage {
    /// Keep the first `n` bytes.
    Truncate(usize),
    /// Overwrite the byte at `at` (modulo the length) with `byte`.
    Overwrite { at: usize, byte: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Read {
        file: String,
        #[serde(default)]
        damage: Option<Damage>,
        #[serde(default)]
        rebuild: bool,
    },
    MakeFaceWithCurves {
        surface: SrfIn,
        dom: [f64; 4],
        orient: u8,
        region_null: bool,
        /// `pbOptCrvOnSurf`: 0 null, 1 FALSE, 2 TRUE.
        crv_on_surf: u8,
        loops: Vec<usize>,
        has3d: bool,
        has_uv: bool,
        curves: Vec<FaceCurveIn>,
        #[serde(default)]
        points: Vec<[f64; 3]>,
    },
    MakeFace {
        kind: u8,
        side: f64,
        orient: u8,
        region_null: bool,
    },
    Glue {
        brep: BrepIn,
        e1: i64,
        e2: i64,
        orient: u8,
        gap: f64,
        nesting: bool,
    },
    Copy {
        brep: BrepIn,
        n: u64,
        merge: bool,
    },
    MergeSelf {
        brep: BrepIn,
        face: i64,
        tol: f64,
        #[serde(default)]
        curves: Vec<SelfCurveIn>,
    },
}

/// One self-intersection curve of `merge_self`: its 3D curve and its (2D) UV curves on
/// the two sheets.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelfCurveIn {
    c3: CrvIn,
    uv1: CrvIn,
    uv2: CrvIn,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    raw_knots: bool,
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn push_curve(v: &mut Vec<f64>, c: &CrvIn) {
    let n = c.cpts.len().saturating_sub(1);
    let m = c.u.len().saturating_sub(1);
    v.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        v.extend(p);
    }
    v.extend(&c.u);
}

fn push_surface(v: &mut Vec<f64>, s: &SrfIn) {
    match s {
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline {
            net,
            p,
            q,
            u,
            v: vk,
        } => {
            let n = net.len().saturating_sub(1);
            let m = net.first().map_or(0, |r| r.len().saturating_sub(1));
            v.extend([0.0, 0.0, 0.0]);
            v.extend([n as f64, m as f64, *p as f64, *q as f64]);
            v.extend([
                (u.len().saturating_sub(1)) as f64,
                (vk.len().saturating_sub(1)) as f64,
            ]);
            for row in net {
                for pt in row {
                    v.extend(pt);
                }
            }
            v.extend(u);
            v.extend(vk);
        }
    }
}

/// The B-rep block (`validate_only` and `init_gaps` off).
fn brep_block(b: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    let mut out = Vec::new();
    match b {
        BrepIn::File { rebuild, .. } => out.extend([0.0, flag(*rebuild)]),
        BrepIn::Wire { curve, t0, t1 } => {
            out.push(1.0);
            shared::push_curve(&mut out, curve)?;
            out.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            out.push(2.0);
            shared::push_surface(&mut out, surface)?;
            out.extend(dom);
        }
        BrepIn::Network => out.push(3.0),
    }
    out.extend([0.0, 0.0]);
    Ok(out)
}

/// Whether `bytes` are exactly the test file `file` (a damage that changed nothing): the
/// FFI runs the native read without the B-rep data check on exactly the shipped files.
fn is_test_file(file: &str, bytes: &[u8]) -> Result<bool, HarnessError> {
    Ok(read_bytes(file, None)? == bytes)
}

/// The bytes of a read case.
fn read_bytes(file: &str, damage: Option<Damage>) -> Result<Vec<u8>, HarnessError> {
    let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
    let mut bytes = std::fs::read(&p).map_err(|e| harness(format!("{}: {e}", p.display())))?;
    match damage {
        None => {}
        Some(Damage::Truncate(n)) => bytes.truncate(n),
        Some(Damage::Overwrite { at, byte }) => {
            let len = bytes.len();
            if len > 0 {
                if let Some(b) = bytes.get_mut(at % len) {
                    *b = byte;
                }
            }
        }
    }
    Ok(bytes)
}

/// The `topology.brep_data` stream guard of the bytes (its pure-Rust stream simulator; no
/// C++ call): `false` when it rejects them.
fn guard_accepts(bytes: &[u8], upstream_file: bool) -> Result<bool, HarnessError> {
    let req = bd::Request {
        mode: bd::Mode::ReadFile,
        // every read case is ASCII: the encoder sends `binary = 0` to the FFI
        binary: false,
        version: 0,
        dim: 0,
        persist: false,
        source: 0,
        pre: 0,
        data: bytes.to_vec(),
        upstream_file,
    };
    match bd::guard(&req) {
        Ok(()) => Ok(true),
        Err(FfiError::InvalidArgument(_)) => Ok(false),
        Err(e) => Err(harness(format!("guard: {e}"))),
    }
}

/// The positions of the last digit of every integer token of the topology records of an
/// ASCII `.smb` stream (the lines from `//Region Data` up to `//Vertex Data`, comments
/// skipped): the cross-reference indices, flags and counts.
fn topology_index_digits(bytes: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut inside = false;
    let mut start = 0;
    for line in bytes.split(|&b| b == b'\n') {
        let at = start;
        start += line.len() + 1;
        if line.starts_with(b"//Region Data") {
            inside = true;
        } else if line.starts_with(b"//Vertex Data") {
            break;
        }
        if !inside || line.starts_with(b"//") {
            continue;
        }
        let mut i = 0;
        while i < line.len() {
            if line.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
                continue;
            }
            let t0 = i;
            while i < line.len() && !line.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            let tok = line.get(t0..i).unwrap_or_default();
            if tok.last().is_some_and(u8::is_ascii_digit)
                && tok.iter().all(|&c| c.is_ascii_digit() || c == b'-')
            {
                out.push(at + i - 1);
            }
        }
    }
    out
}

/// A cached export with its key (the `.smb` path and the block's bits).
type CachedExport = (String, Vec<u64>, Rc<Vec<f64>>);

thread_local! {
    /// The last few exports (both sides of a case export the same B-rep).
    static EXPORTS: RefCell<Vec<CachedExport>> = const { RefCell::new(Vec::new()) };
}

fn export_of(brep: &BrepIn) -> Result<Rc<Vec<f64>>, HarnessError> {
    let path = shared::path_of(brep)?;
    let block = brep_block(brep)?;
    let bits: Vec<u64> = block.iter().map(|x| x.to_bits()).collect();
    if let Some(e) = EXPORTS.with(|c| {
        c.borrow()
            .iter()
            .find(|(p, b, _)| *p == path && *b == bits)
            .map(|(_, _, e)| Rc::clone(e))
    }) {
        return Ok(e);
    }
    let e = Rc::new(
        fixture::eval(fixture::Op::Export, &path, &block)
            .map_err(|e| harness(format!("c++ export: {e}")))?,
    );
    EXPORTS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= 4 {
            c.remove(0);
        }
        c.push((path, bits, Rc::clone(&e)));
    });
    Ok(e)
}

/// The inputs with selectors resolved against the B-rep, and its export; `Err(output)`
/// when the case ends early (the B-rep did not build, or has nothing to select).
#[allow(clippy::type_complexity)]
fn resolve(i: &Inputs) -> Result<Result<(Inputs, Option<Rc<Vec<f64>>>), Output>, HarnessError> {
    let (Inputs::Glue { brep, .. } | Inputs::Copy { brep, .. } | Inputs::MergeSelf { brep, .. }) =
        i
    else {
        return Ok(Ok((i.clone(), None)));
    };
    if let Inputs::MergeSelf { face, curves, .. } = i {
        if curves.iter().any(|c| c.raw_knots) {
            if *face != 0
                || !matches!(
                    brep,
                    BrepIn::Face {
                        surface: shared::SrfIn::Plane { .. },
                        ..
                    }
                )
            {
                return Err(harness("raw merge requires face 0 of a plane fixture"));
            }
            return Ok(Ok((i.clone(), None)));
        }
    }
    let ex = export_of(brep)?;
    let Some(c) = fixture::counts(&ex) else {
        return Ok(Err(Output::new().values("out", ex.to_vec())));
    };
    let [_, _, _, n_f, _, _, _, n_e, ..] = c;
    let mut r = i.clone();
    match &mut r {
        Inputs::Glue { e1, e2, .. } => {
            *e1 = shared::sel(*e1, n_e);
            *e2 = shared::sel(*e2, n_e);
        }
        Inputs::MergeSelf { face, .. } => {
            if n_f == 0 {
                return Ok(Err(Output::err("no_face")));
            }
            *face = shared::sel((*face).max(0), n_f);
        }
        _ => {}
    }
    Ok(Ok((r, Some(ex))))
}

// ---- C++ side ---------------------------------------------------------------------------

fn encode(i: &Inputs) -> Result<(Op, String, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let mut path = String::new();
    let op = match i {
        Inputs::Read {
            file,
            damage,
            rebuild,
        } => {
            let bytes = read_bytes(file, *damage)?;
            v.push(bytes.len() as f64);
            v.extend(bytes.iter().map(|&b| f64::from(b)));
            v.extend([0.0, flag(*rebuild)]);
            Op::Read
        }
        Inputs::MakeFaceWithCurves {
            surface,
            dom,
            orient,
            region_null,
            crv_on_surf,
            loops,
            has3d,
            has_uv,
            curves,
            points,
        } => {
            push_surface(&mut v, surface);
            v.extend(dom);
            v.extend([
                f64::from(*orient),
                flag(*region_null),
                f64::from(*crv_on_surf),
            ]);
            v.push(loops.len() as f64);
            v.extend(loops.iter().map(|&n| n as f64));
            v.extend([flag(*has3d), flag(*has_uv), curves.len() as f64]);
            for c in curves {
                if *has3d {
                    push_curve(
                        &mut v,
                        c.c3.as_ref().ok_or_else(|| harness("missing 3d curve"))?,
                    );
                }
                if *has_uv {
                    push_curve(
                        &mut v,
                        c.uv.as_ref().ok_or_else(|| harness("missing uv curve"))?,
                    );
                }
                v.push(f64::from(c.orient));
            }
            v.push(points.len() as f64);
            for p in points {
                v.extend(p);
            }
            Op::MakeFaceWithCurves
        }
        Inputs::MakeFace {
            kind,
            side,
            orient,
            region_null,
        } => {
            v.extend([
                f64::from(*kind),
                *side,
                f64::from(*orient),
                flag(*region_null),
            ]);
            Op::MakeFace
        }
        Inputs::Glue {
            brep,
            e1,
            e2,
            orient,
            gap,
            nesting,
        } => {
            v.extend([
                *e1 as f64,
                *e2 as f64,
                f64::from(*orient),
                *gap,
                flag(*nesting),
            ]);
            v.extend(brep_block(brep)?);
            path = shared::path_of(brep)?;
            Op::Glue
        }
        Inputs::Copy { brep, n, merge } => {
            v.extend([*n as f64, flag(*merge)]);
            v.extend(brep_block(brep)?);
            path = shared::path_of(brep)?;
            Op::Copy
        }
        Inputs::MergeSelf {
            brep,
            face,
            tol,
            curves,
        } => {
            v.extend([*face as f64, *tol, curves.len() as f64]);
            for c in curves {
                v.push(flag(c.raw_knots));
                push_curve(&mut v, &c.c3);
                v.push(flag(c.raw_knots));
                push_curve(&mut v, &c.uv1);
                v.push(flag(c.raw_knots));
                push_curve(&mut v, &c.uv2);
            }
            v.extend(brep_block(brep)?);
            path = shared::path_of(brep)?;
            Op::MergeSelf
        }
    };
    Ok((op, path, v))
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let Inputs::Read { file, damage, .. } = i {
        let bytes = read_bytes(file, *damage)?;
        if !guard_accepts(&bytes, is_test_file(file, &bytes)?)? {
            return Ok(Output::err("guard"));
        }
    }
    let (r, _) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    if ub_165(&r)? {
        return Ok(Output::rejected(
            "UB-165: SmBrep::MakeFaceWithCurves dereferences the null UV curve array \
             (curve on surface, no UV curves); not run",
        ));
    }
    let (op, path, input) = encode(&r)?;
    match ffi::eval(op, &path, &input) {
        Ok(out) => Ok(Output::new().values("out", out)),
        // a modified stream's read runs in a fresh child process (the FFI)
        Err(FfiError::Status(solidcore_ffi::SC_ERR_CHILD_CRASHED)) => Ok(Output::rejected(
            "UB-164: SmBrep::ReadFromFile of the modified stream crashed (or ran past \
             READ_SECONDS) in its fresh child process: MakeTopologyFromData uses the \
             stream's cross-reference indices unchecked",
        )),
        Err(e) => Err(harness(format!("c++: {e}"))),
    }
}

/// A `MakeFaceWithCurves` that crashes upstream (`plan/upstream-issues.md` UB-165,
/// AGENTS.md section 4, step 5.6): `pbOptCrvOnSurf` TRUE with 3D curves and no UV
/// curves, past its own input checks (no loops, a degenerate surface:
/// `SmBrep.cpp:15722-15741`, which return first). The same predicate as the shim's `-2`
/// guard, decided from the inputs and the Rust side's degeneracy tests, never from C++
/// output. Where a degeneracy test is unported the Rust side cannot tell: the case is
/// not rejected, the shim's guard keeps it from upstream and the Rust side defers.
fn ub_165(i: &Inputs) -> Result<bool, HarnessError> {
    let Inputs::MakeFaceWithCurves {
        surface,
        crv_on_surf,
        loops,
        has3d,
        has_uv,
        ..
    } = i
    else {
        return Ok(false);
    };
    if *crv_on_surf != 2 || *has_uv || !*has3d {
        return Ok(false);
    }
    match mfwc_input_error(&Kernel::default(), surface, loops) {
        Ok(input_error) => Ok(!input_error),
        Err(Fail::Deferred(_)) => Ok(false),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// What ends a Rust case early: a deferral or a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn hfail(e: impl core::fmt::Display) -> Fail {
    Fail::Harness(harness(e.to_string()))
}

/// The shim's `new SmBrep()`: an empty B-rep that owns its void infinite region, the
/// head of its region list (`SmBrep.cpp:532-536`).
fn native_brep() -> Result<(Brep, RegionId), Fail> {
    let b = Brep::new_with_infinite_region().map_err(hfail)?;
    let infinite = b
        .infinite_region()
        .ok_or_else(|| hfail("new B-rep without infinite region"))?;
    Ok((b, infinite))
}

/// A case the shim answers with `-2` (upstream would dereference null): both sides would
/// only echo that constant, so it is deferred under the guard's `label`, never compared.
fn cpp_null_deref(label: &str, symbol: &str) -> Fail {
    Fail::Deferred(Box::new(Output::deferred(label, symbol)))
}

/// The `SmStatus` of a Rust result (`0` success, `SM_ERR_INVALID_INPUT` for
/// `InvalidInput`); a deferral or another error ends the case.
fn status<T>(r: &Result<T, SolidError>) -> Result<f64, Fail> {
    match r {
        Ok(_) => Ok(0.0),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(*code)),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(SM_ERR_INVALID_INPUT),
        Err(e) => Err(match Output::unported(e) {
            Some(o) => Fail::Deferred(Box::new(o)),
            None => hfail(format!("rust: {e}")),
        }),
    }
}

fn rust_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let cur = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

fn rust_self_curve(c: &CrvIn, raw: bool) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let cur = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    let mut curve = BSplineCurve::from_nurb(cur, c.dim, false);
    if raw {
        curve
            .get_or_create_gw_nurb()
            .map_err(|e| harness(format!("curve: {e}")))?
            .knots_mut()
            .copy_from_slice(&c.u);
    }
    Ok(SmCurve::from(curve))
}

fn rust_surface(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::new(o[0], o[1], o[2]),
            Vec3::new(n[0], n[1], n[2]),
        ))),
        SrfIn::Bspline { net, p, q, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|x| CPoint::new(x[0], x[1], x[2], x[3]))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            s.set_b_spline_surface_form(
                BSplineSurfaceForm::from_code(0).ok_or_else(|| harness("form"))?,
            );
            Ok(Surface::from(s))
        }
    }
}

/// `SmOrientType` codes `SM_OT_UNKNOWN` (0), `SM_OT_SAME` (1), `SM_OT_OPPOSITE` (2).
fn orient_of(code: u8) -> Result<OrientType, Fail> {
    match code {
        0 => Ok(OrientType::Unknown),
        1 => Ok(OrientType::Same),
        2 => Ok(OrientType::Opposite),
        _ => Err(hfail("SmOrientType code")),
    }
}

/// A notification identity for every topology object of the `MakeFace` loop (by kind and
/// index; faces, edges and edge uses as [`key_of`]).
fn loop_key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Face(_) | EntityRef::Edge(_) | EntityRef::Edgeuse(_) => return key_of(e),
        EntityRef::Brep(x) => (16011, 4, x.index()),
        EntityRef::Region(x) => (16012, 5, x.index()),
        EntityRef::Shell(x) => (16013, 6, x.index()),
        EntityRef::Faceuse(x) => (16022, 7, x.index()),
        EntityRef::Loopuse(x) => (16004, 8, x.index()),
        EntityRef::Loop(x) => (16003, 9, x.index()),
        EntityRef::Vertexuse(x) => (16006, 10, x.index()),
        EntityRef::Vertex(x) => (16021, 11, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(kind << 40 | u64::from(i)),
    }
}

fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, key) = match e {
        EntityRef::Face(f) => (
            solidcore::topology::face::p2::TYPE,
            1 + u64::from(f.index()),
        ),
        EntityRef::Edge(x) => (ed::TYPE, 1 << 40 | u64::from(x.index())),
        EntityRef::Edgeuse(x) => (16005, 2 << 40 | u64::from(x.index())),
        _ => (0, 3 << 40),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(key),
    }
}

/// A distinct identity for every topology object `ReadFromFile` builds (geometry copies
/// are keyed from `1 << 50` up).
fn read_key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey((kind + 1) << 40 | u64::from(i)),
    }
}

/// The identity of an edge's curve (a split edge's new curve included), keyed above
/// [`read_key_of`]'s topology identities.
fn edge_curve_key(e: EdgeId, type_code: i64) -> ObjectRef {
    ObjectRef {
        type_code,
        key: ObjectKey(12 << 40 | u64::from(e.index())),
    }
}

fn this_face(f: FaceId) -> ObjectRef {
    key_of(EntityRef::Face(f))
}

/// The record of a run.
type Record = Result<Vec<f64>, Fail>;

/// The region, shell, face, loop, edge and vertex counts of a B-rep as the shim's walk
/// counts them: regions in the B-rep, shells in a region, faces with a face use, loops
/// with a loop use, edges with edge uses and vertices with vertex uses (arena orphans,
/// e.g. a glued-away edge, are not counted).
fn brep_counts(b: &Brep) -> [f64; 6] {
    [
        b.regions().iter().filter(|(_, r)| r.in_brep).count() as f64,
        b.shells()
            .iter()
            .filter(|(_, s)| s.region.is_some())
            .count() as f64,
        b.faces()
            .iter()
            .filter(|(_, f)| f.faceuse.is_some())
            .count() as f64,
        b.loops()
            .iter()
            .filter(|(_, l)| l.loopuse.is_some())
            .count() as f64,
        b.edges()
            .iter()
            .filter(|(_, e)| !e.edgeuses.is_empty())
            .count() as f64,
        b.vertices()
            .iter()
            .filter(|(_, v)| !v.vertexuses.is_empty())
            .count() as f64,
    ]
}

/// The tolerances of the edges and vertices [`brep_counts`] counts, each sorted ascending.
fn brep_tolerances(b: &Brep) -> Vec<f64> {
    let mut te: Vec<f64> = b
        .edges()
        .iter()
        .filter(|(_, e)| !e.edgeuses.is_empty())
        .map(|(_, e)| e.tolerance)
        .collect();
    let mut tv: Vec<f64> = b
        .vertices()
        .iter()
        .filter(|(_, v)| !v.vertexuses.is_empty())
        .map(|(_, v)| v.tolerance)
        .collect();
    te.sort_by(f64::total_cmp);
    tv.sort_by(f64::total_cmp);
    te.extend(tv);
    te
}

/// Whether `MakeFaceWithCurves` fails its input checks (`SmBrep.cpp:15722-15741`) on a
/// face with 3D curves: no loops, or a surface degenerate to a curve or a point (the
/// port's tests, which fail soft as upstream's flags).
fn mfwc_input_error(k: &Kernel, surface: &SrfIn, loops: &[usize]) -> Result<bool, Fail> {
    if loops.is_empty() {
        return Ok(true);
    }
    let srf = rust_surface(surface)?;
    let flag = |r: solidcore::Result<bool>| -> Result<bool, Fail> {
        match r {
            Ok(x) => Ok(x),
            Err(e) => match Output::unported(&e) {
                Some(o) => Err(Fail::Deferred(Box::new(o))),
                None => Ok(false),
            },
        }
    };
    let tol = solidcore::surface::surface::p1::DEGENERATE_TOL_3D;
    Ok(
        flag(srf.is_degenerate_curve(k, solidcore::math::EFF_ZERO_DEG, None, tol))?
            || flag(srf.is_degenerate_point(k, tol, None))?,
    )
}

/// The record of a face maker: `status [face shell region counts]`.
fn new_face_record(b: &Brep, r: &Result<p7::NewFace, SolidError>) -> Record {
    let mut o = vec![status(r)?];
    if let Ok(nf) = r {
        o.extend([
            flag(nf.face.is_some()),
            flag(nf.shell.is_some()),
            flag(nf.region.is_some()),
        ]);
        o.extend(brep_counts(b));
    }
    Ok(o)
}

#[allow(clippy::too_many_lines)] // one record per op
fn rust_record(k: &mut Kernel, i: &Inputs, ex: Option<&[f64]>) -> Record {
    match i {
        Inputs::Read {
            file,
            damage,
            rebuild,
        } => {
            let bytes = read_bytes(file, *damage)?;
            // a B-rep as `new SmBrep(ctx)` makes it (with the infinite region)
            let mut b = Brep::new_with_infinite_region().map_err(hfail)?;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = p10::TopologyEnv {
                all_attributes: &[],
                attributes: &mut attrs,
                key_of: &read_key_of,
                records: &mut records,
                next_key: 1u64 << 50,
            };
            let r = p7::read_from_file(k, &mut b, bytes, FileType::Ascii, *rebuild, &mut env);
            let mut o = vec![status(&r)?];
            if r.is_ok() {
                o.extend(brep_counts(&b));
            }
            Ok(o)
        }
        Inputs::MakeFaceWithCurves {
            surface,
            dom,
            orient,
            region_null,
            crv_on_surf,
            loops,
            has3d,
            has_uv,
            curves,
            points,
        } => {
            // (a curve on surface without UV curves past the input checks crashes upstream:
            // run_cpp rejects it (ub_165); the port returns InvalidInput there)
            // (the shim passes the new B-rep's GetInfiniteRegion() unless null)
            let (mut b, infinite) = native_brep()?;
            let region = (!*region_null).then_some(infinite);
            let mut c3 = Vec::new();
            let mut uv = Vec::new();
            for c in curves {
                if *has3d {
                    let x = c.c3.as_ref().ok_or_else(|| hfail("missing 3d curve"))?;
                    c3.push(Some(rust_curve(x)?));
                }
                if *has_uv {
                    let x = c.uv.as_ref().ok_or_else(|| hfail("missing uv curve"))?;
                    uv.push(rust_curve(x)?);
                }
            }
            let input = p7::FaceCurvesInput {
                loops: loops.clone(),
                curves_3d: has3d.then_some(c3),
                uv_curves: has_uv.then_some(uv),
                orientations: curves
                    .iter()
                    .map(|c| orient_of(c.orient))
                    .collect::<Result<_, _>>()?,
                loop_points: points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect(),
            };
            let srf = rust_surface(surface)?;
            let domain =
                Extent2d::from_points(Point2::new(dom[0], dom[1]), Point2::new(dom[2], dom[3]));
            let cos = match crv_on_surf {
                0 => None,
                1 => Some(false),
                2 => Some(true),
                _ => return Err(hfail("crv_on_surf code")),
            };
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let r = p7::make_face_with_curves(
                k,
                &mut b,
                region,
                input,
                srf,
                &domain,
                orient_of(*orient)?,
                cos,
                &this_face,
                &mut env,
            );
            new_face_record(&b, &r)
        }
        Inputs::MakeFace {
            kind,
            side,
            orient,
            region_null,
        } => {
            if *kind == 1 {
                // the shim's square wire loop: its -3 when a side is too small for a line
                let p = [
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(*side, 0.0, 0.0),
                    Vec3::new(*side, *side, 0.0),
                    Vec3::new(0.0, *side, 0.0),
                ];
                for (a, b) in p.iter().zip(p.iter().cycle().skip(1)) {
                    if Line::create_line_segment(3, *a, *b, None).is_err() {
                        return Ok(vec![NOT_RUN]);
                    }
                }
                // The shim's loop: `new SmBrep()` (its infinite void region), MakeShellVertex
                // there, MakeWireEdgeVertex along three sides and MakeWireEdge closing the
                // fourth; MakeFace gets each edge's SAME edge use.
                let defer = |e: SolidError| match Output::unported(&e) {
                    Some(o) => Fail::Deferred(Box::new(o)),
                    None => hfail(e),
                };
                let mut b = solidcore::topology::brep::p3::new_brep(
                    k,
                    solidcore::geom::tol::USE_DEFAULT,
                    solidcore::geom::tol::USE_DEFAULT,
                )
                .map_err(defer)?;
                let inf = b.infinite_region();
                let mut records = solidcore::topology::topology::NotifyRecords::new();
                let mut attrs = AttributeStore::default();
                let mut env = ed::NotifyEnv {
                    records: &mut records,
                    attributes: &mut attrs,
                    context: None,
                    key_of: &loop_key_of,
                };
                let (_, v0) =
                    solidcore::topology::brep::p9::make_shell_vertex(&mut b, inf, p[0], &mut env)
                        .map_err(defer)?;
                let corner = |i: usize| p.get(i % 4).copied().ok_or_else(|| hfail("corner"));
                let line = |i: usize| {
                    let (a, z) = (corner(i)?, corner(i + 1)?);
                    let c = SmCurve::from(Line::create_line_segment(3, a, z, None).map_err(hfail)?);
                    let ivl = c.natural_interval();
                    Ok::<_, Fail>((c, ivl))
                };
                let mut vs = vec![v0];
                let mut es = Vec::new();
                for i in 0..3 {
                    let (c, ivl) = line(i)?;
                    let start = *vs.last().ok_or_else(|| hfail("no start vertex"))?;
                    match solidcore::topology::brep::p3::make_wire_edge_vertex(
                        k,
                        &mut b,
                        &mut env,
                        inf,
                        start,
                        c,
                        &ivl,
                        OrientType::Same,
                        corner(i + 1)?,
                    ) {
                        Ok((e, v)) => {
                            es.push(e);
                            vs.push(v);
                        }
                        Err(e) if e.is_unported() => return Err(defer(e)),
                        // (the shim cannot build the loop either)
                        Err(_) => return Ok(vec![NOT_RUN]),
                    }
                }
                let (c, ivl) = line(3)?;
                match solidcore::topology::brep::p3::make_wire_edge(
                    k,
                    &mut b,
                    &mut env,
                    inf,
                    *vs.get(3).ok_or_else(|| hfail("no fourth vertex"))?,
                    v0,
                    c,
                    &ivl,
                    OrientType::Same,
                ) {
                    Ok(e) => es.push(e),
                    Err(e) if e.is_unported() => return Err(defer(e)),
                    Err(_) => return Ok(vec![SC_ERR_ARG]),
                }
                let mut eus = Vec::new();
                for &e in &es {
                    let all = ed::edgeuses(&b, e, None, None).map_err(hfail)?;
                    for eu in all {
                        if b.edgeuse(eu).map_err(hfail)?.orientation == OrientType::Same {
                            eus.push(eu);
                            break;
                        }
                    }
                }
                let plane = Plane::from_normal(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 1.0));
                let dom = plane.natural_uv_domain();
                let r = p7::make_face(
                    k,
                    &mut b,
                    if *region_null { None } else { inf },
                    &[4],
                    &eus,
                    &[],
                    Some(Surface::from(plane)),
                    &dom,
                    orient_of(*orient)?,
                    &p7::MakeFaceOptions::default(),
                    &this_face,
                    &mut env,
                );
                return new_face_record(&b, &r);
            }
            // (the shim passes the new B-rep's GetInfiniteRegion() unless null)
            let (mut b, infinite) = native_brep()?;
            let region = (!*region_null).then_some(infinite);
            let surface = (*kind == 1).then(|| {
                Surface::from(Plane::from_normal(
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                ))
            });
            let dom = Extent2d::from_points(Point2::new(0.0, 0.0), Point2::new(1.0, 1.0));
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let r = p7::make_face(
                k,
                &mut b,
                region,
                &[],
                &[],
                &[],
                surface,
                &dom,
                orient_of(*orient)?,
                &p7::MakeFaceOptions::default(),
                &this_face,
                &mut env,
            );
            new_face_record(&b, &r)
        }
        Inputs::Glue {
            e1,
            e2,
            orient,
            gap,
            nesting,
            ..
        } => {
            let ex = ex.ok_or_else(|| hfail("no export"))?;
            let (mut b, _) = shared::import(ex)?;
            let eid = |x: i64| (x >= 0).then(|| EdgeId::from_index(x as u32));
            // the shim's guard: upstream dereferences a null edge curve
            // (CurveMaxDistanceBetween) when gluing two distinct non-wire edges with
            // SM_OT_SAME, the same intervals and exactly one curve (two null curves
            // compare equal)
            if let (Some(a), Some(d)) = (eid(*e1), eid(*e2)) {
                if a != d && !ed::is_wire(&b, d).map_err(hfail)? && *orient == 1 {
                    let (ia, id) = (
                        ed::interval(&b, a).map_err(hfail)?,
                        ed::interval(&b, d).map_err(hfail)?,
                    );
                    let one_null = b.edge(a).map_err(hfail)?.curve.is_none()
                        != b.edge(d).map_err(hfail)?.curve.is_none();
                    if are_same(ia.min(), id.min()) && are_same(ia.max(), id.max()) && one_null {
                        return Err(cpp_null_deref(
                            "cpp-guard:null-curve",
                            "SmBrep::GlueEdgesGeneral",
                        ));
                    }
                }
            }
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let r = p7::glue_edges_general(
                k,
                &mut b,
                eid(*e1),
                orient_of(*orient)?,
                *gap,
                *nesting,
                eid(*e2),
                None,
                &mut env,
            );
            let s = status(&r)?;
            // the B-rep's counts and tolerances after the op (the shim walks the C++ B-rep
            // afterwards)
            let mut o = vec![0.0, s];
            o.extend(brep_counts(&b));
            o.extend(brep_tolerances(&b));
            Ok(o)
        }
        Inputs::Copy { n, merge, .. } => {
            let ex = ex.ok_or_else(|| hfail("no export"))?;
            let (mut b, _) = shared::import(ex)?;
            let faces: Vec<FaceId> = b.faces().ids().take(*n as usize).collect();
            let (mut dest, _) = native_brep()?;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let r = p7::copy_faces(k, &mut b, &faces, &mut dest, *merge, &this_face, &mut env);
            // (a face that could not be copied makes upstream's status SM_ERR; the copies
            // made are kept)
            let s = match &r {
                Ok((_, false)) => f64::from(solidcore::error::SM_ERR),
                _ => status(&r)?,
            };
            let mut o = vec![0.0, s, r.as_ref().map_or(0, |(f, _)| f.len()) as f64];
            o.extend(brep_counts(&dest));
            Ok(o)
        }
        Inputs::MergeSelf {
            brep,
            face,
            tol,
            curves,
        } => {
            let (mut b, f) = if curves.iter().any(|c| c.raw_knots) {
                let BrepIn::Face {
                    surface: shared::SrfIn::Plane { o, n },
                    dom,
                } = brep
                else {
                    return Err(hfail("raw merge requires plane fixture"));
                };
                let mut b = solidcore::topology::brep::p3::new_brep(
                    k,
                    solidcore::geom::tol::USE_DEFAULT,
                    solidcore::geom::tol::USE_DEFAULT,
                )
                .map_err(hfail)?;
                let mut records = solidcore::topology::topology::NotifyRecords::new();
                let mut attrs = AttributeStore::default();
                let mut env = ed::NotifyEnv {
                    records: &mut records,
                    attributes: &mut attrs,
                    context: None,
                    key_of: &read_key_of,
                };
                let mut marks = solidcore::topology::topology_traverser::Marks::default();
                let face_ref = |f| read_key_of(EntityRef::Face(f));
                let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
                    env: &mut env,
                    marks: &mut marks,
                    this_face: &face_ref,
                };
                let domain = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
                let f = solidcore::topology::brep::p8::create_face_from_surface(
                    k,
                    &mut b,
                    Some(Surface::from(Plane::from_normal(
                        Vec3::new(o[0], o[1], o[2]),
                        Vec3::new(n[0], n[1], n[2]),
                    ))),
                    &domain,
                    &mut fenv,
                )
                .map_err(hfail)?;
                (b, f)
            } else {
                let ex = ex.ok_or_else(|| hfail("no export"))?;
                let (b, _) = shared::import(ex)?;
                (b, FaceId::from_index(*face as u32))
            };
            if b.face(f).map_err(hfail)?.surface.is_none() {
                return Err(cpp_null_deref(
                    "cpp-guard:null-surface",
                    "SmBrep::MergeSelfIntersectionCurves",
                ));
            }
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                // (the merges make vertices, vertex uses and loops: every object gets
                // an identity of its own)
                key_of: &read_key_of,
            };
            let curves = curves
                .iter()
                .map(|c| {
                    Ok(p7::SelfIntersectionCurve {
                        curve_3d: rust_self_curve(&c.c3, c.raw_knots)?,
                        uv_curve_1: rust_self_curve(&c.uv1, c.raw_knots)?,
                        uv_curve_2: rust_self_curve(&c.uv2, c.raw_knots)?,
                    })
                })
                .collect::<Result<Vec<_>, HarnessError>>()?;
            let r = p7::merge_self_intersection_curves(
                k,
                &mut b,
                Some(f),
                *tol,
                &curves,
                &mut env,
                &edge_curve_key,
            );
            let s = status(&r)?;
            let n = r.as_ref().map_or(0, Vec::len);
            let mut o = vec![0.0, s, n as f64];
            o.extend(brep_counts(&b));
            if let Ok(edges) = r {
                for id in edges {
                    let edge = b.edge(id).map_err(hfail)?;
                    o.extend([edge.interval.min(), edge.interval.max()]);
                    let (start, end) = ed::start_end_vertices(&b, id).map_err(hfail)?;
                    for vertex in [start, end] {
                        o.push(flag(vertex.is_some()));
                        if let Some(vertex) = vertex {
                            let p = b.vertex(vertex).map_err(hfail)?.point;
                            o.extend([p.x, p.y, p.z]);
                        }
                    }
                    if let Some(curve) = &edge.curve {
                        let natural = curve.natural_interval();
                        o.extend([curve.type_code() as f64, natural.min(), natural.max()]);
                        for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                            let point = curve.evaluate_point(k, edge.interval.evaluate(fraction));
                            o.push(status(&point)?);
                            if let Ok(p) = point {
                                o.extend([p.x, p.y, p.z]);
                            }
                        }
                    } else {
                        o.push(-1.0);
                    }
                    o.push(edge.edgeuses.len() as f64);
                    for &eu in &edge.edgeuses {
                        let (trim, _) = solidcore::topology::edgeuse::p1::uv_trim_curve(&b, eu)
                            .map_err(hfail)?;
                        if let Some(trim) = trim {
                            let natural = trim.natural_interval();
                            o.extend([trim.type_code() as f64, natural.min(), natural.max()]);
                            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                                let point = trim.evaluate_point(k, natural.evaluate(fraction));
                                o.push(status(&point)?);
                                if let Ok(p) = point {
                                    o.extend([p.x, p.y]);
                                }
                            }
                        } else {
                            o.push(-1.0);
                        }
                    }
                }
            }
            Ok(o)
        }
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    if let Inputs::Read { file, damage, .. } = i {
        let bytes = read_bytes(file, *damage)?;
        if !guard_accepts(&bytes, is_test_file(file, &bytes)?)? {
            return Ok(Output::err("guard"));
        }
    }
    let (r, ex) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let mut k = Kernel::default();
    match rust_record(&mut k, &r, ex.as_deref().map(Vec::as_slice)) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- cases ------------------------------------------------------------------------------

/// A degree-1 B-spline line of dimension `dim` from `a` to `b` (`z = NOZ` in 2D).
fn line(dim: u64, a: [f64; 3], b: [f64; 3]) -> CrvIn {
    let (za, zb) = if dim == 2 { (NOZ, NOZ) } else { (a[2], b[2]) };
    CrvIn {
        dim,
        cpts: vec![[a[0], a[1], za, NOW], [b[0], b[1], zb, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A square loop of side `s` at `(x, y)` on `z = 0`, as 3D and UV lines, orientation
/// `orient` for every curve, run counter-clockwise or clockwise.
fn square(x: f64, y: f64, s: f64, ccw: bool) -> Vec<FaceCurveIn> {
    let mut pts = [[x, y], [x + s, y], [x + s, y + s], [x, y + s]];
    if !ccw {
        pts.reverse();
    }
    let at = |i: usize| pts.get(i % 4).copied().unwrap_or([0.0, 0.0]);
    (0..4)
        .map(|i| {
            let a = at(i);
            let b = at(i + 1);
            FaceCurveIn {
                c3: Some(line(3, [a[0], a[1], 0.0], [b[0], b[1], 0.0])),
                uv: Some(line(2, [a[0], a[1], 0.0], [b[0], b[1], 0.0])),
                orient: 1,
            }
        })
        .collect()
}

/// A bilinear B-spline patch over `[0, s]^2` on `z = 0`.
fn patch(s: f64) -> SrfIn {
    SrfIn::Bspline {
        net: vec![
            vec![[0.0, 0.0, 0.0, NOW], [0.0, s, 0.0, NOW]],
            vec![[s, 0.0, 0.0, NOW], [s, s, 0.0, NOW]],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, s, s],
        v: vec![0.0, 0.0, s, s],
    }
}

/// The face-from-curves cases: real faces (outer squares, with a hole, 3D / UV / both,
/// curve on surface), and the input errors.
fn face_cases() -> Vec<Inputs> {
    let plane = SrfIn::Plane {
        o: [0.0, 0.0, 0.0],
        n: [0.0, 0.0, 1.0],
    };
    let mut v = Vec::new();
    for (si, srf) in [plane.clone(), patch(10.0)].into_iter().enumerate() {
        for (has3d, has_uv) in [(true, false), (false, true), (true, true)] {
            for region_null in [false, true] {
                for cos in [0u8, 1, 2] {
                    if cos != 0 && has3d {
                        continue;
                    }
                    let mut curves = square(1.0, 1.0, 8.0, true);
                    let mut loops = vec![4];
                    if si == 0 {
                        curves.extend(square(3.0, 3.0, 2.0, false));
                        loops.push(4);
                    }
                    v.push(Inputs::MakeFaceWithCurves {
                        surface: srf.clone(),
                        dom: [0.0, 0.0, 10.0, 10.0],
                        orient: 1,
                        region_null,
                        crv_on_surf: cos,
                        loops,
                        has3d,
                        has_uv,
                        curves,
                        points: Vec::new(),
                    });
                }
            }
        }
    }
    // input errors: no curves, no loops, degenerate surfaces, all curves degenerate
    let base =
        |loops: Vec<usize>, has3d: bool, has_uv: bool, curves: Vec<FaceCurveIn>, srf: SrfIn| {
            Inputs::MakeFaceWithCurves {
                surface: srf,
                dom: [0.0, 0.0, 10.0, 10.0],
                orient: 1,
                region_null: false,
                crv_on_surf: 0,
                loops,
                has3d,
                has_uv,
                curves,
                points: Vec::new(),
            }
        };
    v.push(base(vec![], false, false, vec![], plane.clone()));
    v.push(base(vec![], true, false, vec![], plane.clone()));
    v.push(base(vec![4], false, false, Vec::new(), plane.clone()));
    let flat = SrfIn::Bspline {
        net: vec![
            vec![[0.0, 0.0, 0.0, NOW], [0.0, 0.0, 0.0, NOW]],
            vec![[5.0, 0.0, 0.0, NOW], [5.0, 0.0, 0.0, NOW]],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    };
    let point = SrfIn::Bspline {
        net: vec![
            vec![[1.0, 1.0, 1.0, NOW], [1.0, 1.0, 1.0, NOW]],
            vec![[1.0, 1.0, 1.0, NOW], [1.0, 1.0, 1.0, NOW]],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    };
    v.push(base(
        vec![4],
        true,
        false,
        square(1.0, 1.0, 8.0, true),
        flat,
    ));
    v.push(base(
        vec![4],
        true,
        false,
        square(1.0, 1.0, 8.0, true),
        point,
    ));
    v
}

/// A self-intersection curve on a face over UV domain `dom`: UV lines `a`-`b` (sheet 1)
/// and `c`-`d` (sheet 2), given as fractions of `dom`, and the 3D line through the
/// first one's UV values at `z = 0`.
fn self_curve(dom: [f64; 4], a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> SelfCurveIn {
    let [u0, v0, u1, v1] = dom;
    let at = |f: [f64; 2]| [u0 + (u1 - u0) * f[0], v0 + (v1 - v0) * f[1], 0.0];
    SelfCurveIn {
        c3: line(3, at(a), at(b)),
        uv1: line(2, at(a), at(b)),
        uv2: line(2, at(c), at(d)),
        raw_knots: false,
    }
}

/// A point for a generated self-intersection UV line, as a fraction of the face domain:
/// a corner, on a side, inside or outside.
fn gen_fraction(rng: &mut Rng) -> [f64; 2] {
    let t = rng.uniform(0.0, 1.0);
    match rng.below(5) {
        0 => [
            f64::from(u8::from(rng.chance(0.5))),
            f64::from(u8::from(rng.chance(0.5))),
        ],
        1 => [t, f64::from(u8::from(rng.chance(0.5)))],
        2 => [f64::from(u8::from(rng.chance(0.5))), t],
        3 => [rng.uniform(0.05, 0.95), rng.uniform(0.05, 0.95)],
        _ => [rng.uniform(-0.3, 1.3), rng.uniform(-0.3, 1.3)],
    }
}

/// A generated `merge_self`: a face B-rep without curves, or a face over a known domain
/// with up to two self-intersection curves (sometimes sharing their sheet-2 line with
/// sheet 1, or running along a side).
fn gen_merge_self(rng: &mut Rng) -> Inputs {
    let tol = rng.uniform(1.0e-6, 1.0e-2);
    if rng.chance(0.3) {
        return Inputs::MergeSelf {
            brep: gen_face_brep(rng),
            face: rng.below(1 << 10) as i64,
            tol,
            curves: Vec::new(),
        };
    }
    let surface = shared::gen_surface(rng);
    let dom = shared::domain(&surface);
    let curves = (0..=rng.below(2))
        .map(|_| {
            let (a, b) = if rng.chance(0.3) {
                // along a side
                let side = rng.below(4);
                let (s, t) = (rng.uniform(0.0, 0.5), rng.uniform(0.5, 1.0));
                let (s, t) = if rng.chance(0.5) { (0.0, 1.0) } else { (s, t) };
                let fixed = f64::from(u8::from(side >= 2));
                if side.is_multiple_of(2) {
                    ([s, fixed], [t, fixed])
                } else {
                    ([fixed, s], [fixed, t])
                }
            } else {
                (gen_fraction(rng), gen_fraction(rng))
            };
            let (c, d) = match rng.below(3) {
                0 => (a, b),
                1 => (b, a),
                _ => (gen_fraction(rng), gen_fraction(rng)),
            };
            self_curve(dom, a, b, c, d)
        })
        .collect();
    Inputs::MergeSelf {
        brep: BrepIn::Face { surface, dom },
        face: 0,
        tol,
        curves,
    }
}

/// The built-in `merge_self` cases with curves, on the square face `[0, 10]^2` of the
/// plane `z = 0`: inside, crossing, along sides, partly along a side, outside, and two
/// curves at once.
fn self_curve_cases() -> Vec<(String, Inputs)> {
    let dom = [0.0, 0.0, 10.0, 10.0];
    let sc = |a, b, c, d| self_curve(dom, a, b, c, d);
    let named: Vec<(&str, Vec<SelfCurveIn>)> = vec![
        (
            "inside",
            vec![sc([0.2, 0.2], [0.8, 0.8], [0.2, 0.8], [0.8, 0.2])],
        ),
        (
            "cross",
            vec![sc([-0.1, 0.5], [1.1, 0.5], [0.5, -0.1], [0.5, 1.1])],
        ),
        (
            "side_same",
            vec![sc([0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [1.0, 0.0])],
        ),
        (
            "side_reversed",
            vec![sc([0.0, 0.0], [1.0, 0.0], [1.0, 0.0], [0.0, 0.0])],
        ),
        (
            "side_opposite",
            vec![sc([0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0])],
        ),
        (
            "side_adjacent",
            vec![sc([0.0, 0.0], [1.0, 0.0], [0.0, 0.0], [0.0, 1.0])],
        ),
        (
            "side_part",
            vec![sc([0.2, 0.0], [0.8, 0.0], [0.2, 1.0], [0.8, 1.0])],
        ),
        (
            "side_then_cross",
            vec![sc([0.0, 0.0], [1.0, 0.0], [-0.1, 0.5], [1.1, 0.5])],
        ),
        (
            "from_corner",
            vec![sc([0.0, 0.0], [0.5, 0.5], [0.5, 0.5], [1.0, 1.0])],
        ),
        (
            "outside",
            vec![sc([1.2, 1.2], [1.5, 1.5], [1.2, 1.5], [1.5, 1.2])],
        ),
        (
            "two",
            vec![
                sc([0.2, 0.2], [0.8, 0.8], [0.2, 0.8], [0.8, 0.2]),
                sc([0.0, 1.0], [1.0, 1.0], [0.0, 1.0], [1.0, 1.0]),
            ],
        ),
    ];
    let mut out = Vec::new();
    for (name, curves) in named {
        for tol in [1.0e-5, 1.0e-2] {
            out.push((
                format!("merge_self_curves_{name}_{tol:e}"),
                Inputs::MergeSelf {
                    brep: BrepIn::Face {
                        surface: shared::SrfIn::Plane {
                            o: [0.0, 0.0, 0.0],
                            n: [0.0, 0.0, 1.0],
                        },
                        dom,
                    },
                    face: 0,
                    tol,
                    curves: curves.clone(),
                },
            ));
        }
    }
    for (name, knots) in [
        ("control", vec![0.0, 0.0, 0.3, 0.49, 0.51, 1.0, 1.0]),
        (
            "close",
            vec![0.0, 0.0, 0.3, 0.5 - 1.0e-12, 0.5 + 0.5e-12, 1.0, 1.0],
        ),
    ] {
        let make = |dim| CrvIn {
            dim,
            p: 1,
            u: knots.clone(),
            cpts: [0.0, 3.0, 4.9, 5.1, 10.0]
                .into_iter()
                .map(|x| [x, 0.0, if dim == 2 { NOZ } else { 0.0 }, NOW])
                .collect(),
        };
        out.push((
            format!("merge_self_raw_{name}"),
            Inputs::MergeSelf {
                brep: BrepIn::Face {
                    surface: shared::SrfIn::Plane {
                        o: [0.0, 0.0, 0.0],
                        n: [0.0, 0.0, 1.0],
                    },
                    dom,
                },
                face: 0,
                tol: 1.0e-5,
                curves: vec![SelfCurveIn {
                    c3: make(3),
                    uv1: make(2),
                    uv2: make(2),
                    raw_knots: true,
                }],
            },
        ));
    }
    out
}

/// A B-rep with faces (a wire or the network would make a case both sides reject).
fn gen_face_brep(rng: &mut Rng) -> BrepIn {
    loop {
        let b = shared::gen_brep(rng);
        if matches!(b, BrepIn::File { .. } | BrepIn::Face { .. }) {
            return b;
        }
    }
}

/// A generated op: a damaged read, a glue, a merge-self, a face from curves, a copy or a
/// `MakeFace`, each under the same guards as the built-in cases.
fn gen_inputs(rng: &mut Rng) -> Inputs {
    let file = (*rng.choose(&FILES).unwrap_or(&"disk.smb")).to_string();
    match rng.below(6) {
        0 => {
            // damaged streams the B-rep data guard accepts (a rejected one would be a
            // case both sides refuse; give up after a few tries)
            let mut damage = Damage::Truncate(0);
            for _ in 0..16 {
                let d = if rng.chance(0.5) {
                    Damage::Truncate(rng.below(4000) as usize)
                } else {
                    Damage::Overwrite {
                        at: rng.below(1 << 16) as usize,
                        byte: *rng.choose(b"0123456789 -.eX\n").unwrap_or(&b'9'),
                    }
                };
                damage = d;
                let ok = read_bytes(&file, Some(d))
                    .and_then(|bytes| guard_accepts(&bytes, false))
                    .unwrap_or(false);
                if ok {
                    break;
                }
            }
            Inputs::Read {
                file,
                damage: Some(damage),
                rebuild: rng.chance(0.5),
            }
        }
        1 => Inputs::Glue {
            brep: shared::gen_brep(rng),
            e1: rng.range_i64(-1, 1 << 10),
            e2: rng.range_i64(-1, 1 << 10),
            orient: rng.below(3) as u8,
            gap: rng.uniform(0.0, 1.0e-3),
            nesting: rng.chance(0.5),
        },
        2 => gen_merge_self(rng),
        3 => {
            // a square outer loop on a plane or a patch over [0, 10]^2, closed or open
            // (2 or 3 sides: a gap target), sometimes with a square hole; 3D, UV or both
            let s = rng.uniform(0.5, 8.0);
            let (x, y) = (rng.uniform(0.0, 10.0 - s), rng.uniform(0.0, 10.0 - s));
            let sides = 2 + rng.below(3) as usize;
            let mut curves = square(x, y, s, true);
            curves.truncate(sides);
            let mut loops = vec![sides];
            if sides == 4 && rng.chance(0.3) {
                curves.extend(square(x + s / 4.0, y + s / 4.0, s / 2.0, false));
                loops.push(4);
            }
            // sometimes every SmOrientType code on the curves
            if rng.chance(0.3) {
                for c in &mut curves {
                    c.orient = rng.below(3) as u8;
                }
            }
            let (has3d, has_uv) = *rng
                .choose(&[(true, false), (false, true), (true, true)])
                .unwrap_or(&(true, true));
            let surface = if rng.chance(0.5) {
                SrfIn::Plane {
                    o: [0.0, 0.0, 0.0],
                    n: [0.0, 0.0, 1.0],
                }
            } else {
                patch(10.0)
            };
            Inputs::MakeFaceWithCurves {
                surface,
                dom: [0.0, 0.0, 10.0, 10.0],
                orient: rng.below(3) as u8,
                region_null: rng.chance(0.5),
                crv_on_surf: rng.below(3) as u8,
                loops,
                has3d,
                has_uv,
                curves,
                points: Vec::new(),
            }
        }
        4 => Inputs::Copy {
            brep: shared::gen_brep(rng),
            n: rng.below(8),
            merge: rng.chance(0.5),
        },
        _ => Inputs::MakeFace {
            kind: rng.below(2) as u8,
            side: rng.uniform(0.1, 10.0),
            orient: rng.below(3) as u8,
            region_null: rng.chance(0.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p7"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        let push = |v: &mut Vec<Case>, name: String, i: &Inputs| -> Result<(), HarnessError> {
            let index = v.len();
            v.push(Case {
                unit: "topology.brep_p7".to_string(),
                name,
                source: crate::CaseSource::Fixture {
                    path: "built-in: topology.brep_p7 cases".to_string(),
                    index,
                    upstream_ref: Some("tests/prog_test (pt_TestFiles B-reps)".to_string()),
                },
                inputs: serde_json::to_value(i).map_err(|e| harness(e.to_string()))?,
            });
            Ok(())
        };
        for file in FILES {
            for rebuild in [false, true] {
                push(
                    &mut v,
                    format!("read_{file}_{}", u8::from(rebuild)),
                    &Inputs::Read {
                        file: file.to_string(),
                        damage: None,
                        rebuild,
                    },
                )?;
            }
            // glue no-ops and errors, merge without curves, copies of small files
            let brep = BrepIn::File {
                file: file.to_string(),
                rebuild: false,
            };
            // ((0, 1), (0, 4) and (2, 5) include every distinct edge pair of the files that
            // shares both vertices: Mouse (0, 4) (2, 5); disk, surf_sphere150, torus, toruswvy (0, 1))
            for (j, (e1, e2)) in [
                (-1, 0),
                (0, -1),
                (-1, -1),
                (0, 0),
                (3, 3),
                (0, 1),
                (1, 5),
                (2, 7),
                (0, 4),
                (2, 5),
            ]
            .into_iter()
            .enumerate()
            {
                push(
                    &mut v,
                    format!("glue_{file}_{j}"),
                    &Inputs::Glue {
                        brep: brep.clone(),
                        e1,
                        e2,
                        orient: 1 + (j % 2) as u8,
                        gap: 1.0e-6,
                        nesting: j % 3 == 0,
                    },
                )?;
            }
            for f in 0..4 {
                push(
                    &mut v,
                    format!("merge_self_{file}_f{f}"),
                    &Inputs::MergeSelf {
                        brep: brep.clone(),
                        face: f,
                        tol: 1.0e-5,
                        curves: Vec::new(),
                    },
                )?;
            }
        }
        for (name, i) in self_curve_cases() {
            push(&mut v, name, &i)?;
        }
        for file in ["plane_square.smb", "disk.smb", "plane_circ.smb"] {
            for merge in [false, true] {
                push(
                    &mut v,
                    format!("copy_{file}_{}", u8::from(merge)),
                    &Inputs::Copy {
                        brep: BrepIn::File {
                            file: file.to_string(),
                            rebuild: false,
                        },
                        n: 2,
                        merge,
                    },
                )?;
            }
        }
        for (j, i) in face_cases().iter().enumerate() {
            push(&mut v, format!("face_curves_{j}"), i)?;
        }
        // pre-review reproducers: crv_on_surf TRUE with 3D but no UV curves (upstream
        // dereferences the null UV array, SmBrep.cpp:16600), a loop of no curves, and
        // more curves than the loops use
        let sq = square(1.0, 1.0, 8.0, true);
        let plane = SrfIn::Plane {
            o: [0.0, 0.0, 0.0],
            n: [0.0, 0.0, 1.0],
        };
        for (j, (loops, curves)) in [
            (vec![4usize], sq.clone()),
            (vec![0], Vec::new()),
            (vec![3], sq.clone()),
        ]
        .into_iter()
        .enumerate()
        {
            for cos in [1u8, 2] {
                push(
                    &mut v,
                    format!("face_curves_cos_no_uv_{j}_{cos}"),
                    &Inputs::MakeFaceWithCurves {
                        surface: plane.clone(),
                        dom: [0.0, 0.0, 10.0, 10.0],
                        orient: 1,
                        region_null: false,
                        crv_on_surf: cos,
                        loops: loops.clone(),
                        has3d: true,
                        has_uv: false,
                        curves: curves.clone(),
                        points: Vec::new(),
                    },
                )?;
            }
        }
        // an open UV-only loop (three sides of the square) with crv_on_surf TRUE: the gap
        // target takes upstream's pOptSurfCrvOnSurf branch of DeformToEndPointTargets
        push(
            &mut v,
            "face_curves_cos_open_loop".to_string(),
            &Inputs::MakeFaceWithCurves {
                surface: patch(10.0),
                dom: [0.0, 0.0, 10.0, 10.0],
                orient: 1,
                region_null: false,
                crv_on_surf: 2,
                loops: vec![3],
                has3d: false,
                has_uv: true,
                curves: sq.iter().take(3).cloned().collect(),
                points: Vec::new(),
            },
        )?;
        // A missing closing side requires UV endpoint deformation, rather than
        // approximating the 3D curve on surface. Exercise both surface kinds,
        // both face orientations and distinct scales around the original case.
        for (surface_kind, scale) in [
            ("plane", 0.1),
            ("plane", 1.0),
            ("bspline", 0.1),
            ("bspline", 1.0),
            ("bspline", 10.0),
        ] {
            for orient in [1, 2] {
                let surface = if surface_kind == "plane" {
                    plane.clone()
                } else {
                    patch(10.0 * scale)
                };
                let curves = square(scale, scale, 8.0 * scale, true)
                    .into_iter()
                    .take(3)
                    .collect();
                push(
                    &mut v,
                    format!("face_curves_cos_gap_{surface_kind}_{scale}_orient{orient}"),
                    &Inputs::MakeFaceWithCurves {
                        surface,
                        dom: [0.0, 0.0, 10.0 * scale, 10.0 * scale],
                        orient,
                        region_null: false,
                        crv_on_surf: 2,
                        loops: vec![3],
                        has3d: false,
                        has_uv: true,
                        curves,
                        points: Vec::new(),
                    },
                )?;
            }
        }
        // a square too small for its lines (CreateLineSegment fails: -3 on both sides) and
        // the smallest side that still makes a face in C++
        for (name, side) in [
            ("make_face_tiny_1e-300", 1.0e-300),
            ("make_face_small_1e-12", 1.0e-12),
        ] {
            push(
                &mut v,
                name.to_string(),
                &Inputs::MakeFace {
                    kind: 1,
                    side,
                    orient: 1,
                    region_null: false,
                },
            )?;
        }
        // damaged streams the B-rep data guard accepts and whose read fails: the compared
        // read errors (a digit of an integer token overwritten; deterministic search)
        let k = Kernel::default();
        for file in ["disk.smb", "plane_square.smb", "cone.smb", "Mouse.smb"] {
            let bytes = read_bytes(file, None)?;
            let mut found = 0;
            for at in (0..bytes.len().min(20_000)).step_by(3) {
                if found >= 3 {
                    break;
                }
                if !bytes.get(at).is_some_and(u8::is_ascii_digit) {
                    continue;
                }
                for byte in *b"90" {
                    let d = Damage::Overwrite { at, byte };
                    let b2 = read_bytes(file, Some(d))?;
                    if b2 == bytes || !guard_accepts(&b2, false)? {
                        continue;
                    }
                    let fails = solidcore::topology::brep_data::BrepData::read_from_file(
                        &k,
                        b2,
                        FileType::Ascii,
                    )
                    .is_err();
                    if fails {
                        push(
                            &mut v,
                            format!("read_{file}_error{found}"),
                            &Inputs::Read {
                                file: file.to_string(),
                                damage: Some(d),
                                rebuild: false,
                            },
                        )?;
                        found += 1;
                        break;
                    }
                }
            }
        }
        // modified streams the B-rep data guard accepts and Rust's B-rep data read reads
        // (chosen from the Rust side only), so that `MakeTopologyFromData` runs on them.
        // The C++ read runs in a fresh child process (UB-164: a crash there is rejected);
        // the reads that survive are compared. First one digit overwritten from a
        // quarter, a half and three quarters into each file (mostly geometry values)...
        for file in FILES {
            let bytes = read_bytes(file, None)?;
            for (j, quarter) in [1usize, 2, 3].into_iter().enumerate() {
                let start = bytes.len() * quarter / 4;
                let mut found = None;
                'scan: for at in start..bytes.len().min(start + 4000) {
                    if !bytes.get(at).is_some_and(u8::is_ascii_digit) {
                        continue;
                    }
                    for byte in *b"19" {
                        let d = Damage::Overwrite { at, byte };
                        let b2 = read_bytes(file, Some(d))?;
                        if b2 == bytes || !guard_accepts(&b2, false)? {
                            continue;
                        }
                        let reads = solidcore::topology::brep_data::BrepData::read_from_file(
                            &k,
                            b2,
                            FileType::Ascii,
                        )
                        .is_ok();
                        if reads {
                            found = Some(d);
                            break 'scan;
                        }
                    }
                }
                if let Some(d) = found {
                    push(
                        &mut v,
                        format!("read_{file}_modified{j}"),
                        &Inputs::Read {
                            file: file.to_string(),
                            damage: Some(d),
                            rebuild: j == 1,
                        },
                    )?;
                }
            }
        }
        // ...then one integer of the topology records (a cross-reference index, flag or
        // count: UB-164's trigger): four tokens spread over each file's topology section,
        // their last digit made `9` (`1` where it is `9`)
        for file in FILES {
            let bytes = read_bytes(file, None)?;
            let tokens = topology_index_digits(&bytes);
            let mut j = 0;
            for q in 1..=4usize {
                let Some(&at) = tokens.get(tokens.len() * q / 5) else {
                    continue;
                };
                let byte = if bytes.get(at) == Some(&b'9') {
                    b'1'
                } else {
                    b'9'
                };
                let d = Damage::Overwrite { at, byte };
                let b2 = read_bytes(file, Some(d))?;
                if b2 == bytes || !guard_accepts(&b2, false)? {
                    continue;
                }
                let reads = solidcore::topology::brep_data::BrepData::read_from_file(
                    &k,
                    b2,
                    FileType::Ascii,
                )
                .is_ok();
                if reads {
                    push(
                        &mut v,
                        format!("read_{file}_index{j}"),
                        &Inputs::Read {
                            file: file.to_string(),
                            damage: Some(d),
                            rebuild: j % 2 == 1,
                        },
                    )?;
                    j += 1;
                }
            }
        }
        for kind in [0u8, 1] {
            for region_null in [false, true] {
                for orient in [1u8, 2] {
                    push(
                        &mut v,
                        format!("make_face_{kind}_{}_{orient}", u8::from(region_null)),
                        &Inputs::MakeFace {
                            kind,
                            side: 4.0,
                            orient,
                            region_null,
                        },
                    )?;
                }
            }
        }
        Ok(v)
    }
}
