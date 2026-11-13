//! Runner for unit `topology.edgeuse_p1`. Ported from upstream `SmEdgeuse.cpp`; checked
//! against its golden. Rust: `solidcore::topology::edgeuse::p1`.
//!
//! **B-rep ops.** The case's B-rep is one of the reference shim's sources: read from an
//! upstream `.smb` test file (optionally rebuilding its UV trim curves), a wire edge from a
//! B-spline curve (`CreateWireEdgeFromCurve`), a face from a B-spline surface or a plane
//! (`CreateFaceFromSurface`), or the wire network of `prog_test` `my_test_topology`. The
//! runner builds every non-file source with the Rust kernel in the shim's call order
//! ([`crate::own_builder`], renumbered into the shim's traversal order), and reads an
//! `.smb` file with the Rust reader, renumbered the same way
//! ([`crate::own_reader::brep_in_order`]). Results are recorded as traversal indices, and
//! the gaps the gap ops compute by value. A case evaluating geometry a B-rep holds only as
//! its B-spline part is deferred ([`inexact_geometry`]), which no Rust-built or Rust-read
//! source triggers.
//!
//! Index arguments are selectors taken modulo the entity count (so generated cases
//! need no counts); `-1` selects null.
//!
//! **Geometry ops** (`SmFindStepOffENFO::Evaluate`, `sm_CalcDistAndDev`,
//! `sm_AreSeamCurves`) run on B-spline surfaces, planes and B-spline curves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::solution_array::{ObjectKind, Solution};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{
    Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, ObjectHandle, RegionId,
    ShellId, VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};

use crate::util::trim_curve::put_trim_curve;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The unit runner.
pub struct Runner;

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [
    16005, 16022, 16001, 16010, 12060, 11001, 16013, 16004, 16999,
];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: i64 = -2;

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve: `dim`, control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrvIn {
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BssIn {
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default)]
    pub(crate) form: u8,
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BrepIn {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`, or a
    /// repository fixture file (a path starting `fixtures/`, relative to the repository
    /// root).
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    Wire {
        curve: CrvIn,
        t0: f64,
        t1: f64,
    },
    Face {
        surface: SrfIn,
        dom: [f64; 4],
    },
    /// A face as [`BrepIn::Face`] whose edges are `SmCrvOnSurf`s of the face's (B-spline)
    /// surface over their UV trim curves: the `CrvOnSurf` path of `CreateUVTrimCurve`.
    CrvOnSurfFace {
        surface: SrfIn,
        dom: [f64; 4],
    },
    /// Native planar face with exact rational UV trims, including a closing chord for arcs.
    EllipseFace {
        circle: bool,
        origin: [f64; 3],
        axis: u8,
        radii: [f64; 2],
        angles: [f64; 2],
        natural: [f64; 2],
        inside_out: bool,
        oob: bool,
    },
    /// Native sheet face on the cubic Hermite shape `S(u, v) = C(u) + v * (0, 0, scale)`
    /// over the unit domain: its four edges are cubic Hermite curves of the affine UV
    /// segments of that sheet, with the edge tolerance `edge_tol` (kind 15; `scale` is
    /// the sheet's physical size).
    HermiteSheetFace {
        scale: f64,
        edge_tol: f64,
    },
    Network,
    Vertices {
        points: Vec<[f64; 3]>,
    },
    /// An optional face and `networks` copies of the wire network in one B-rep, so
    /// face-use shells and wire shells coexist (native constructors only).
    Mixed {
        #[serde(default)]
        face: Option<FaceIn>,
        networks: usize,
    },
    /// The closed box solid of `SmApiCreateBox` (planes and lines; fixtures only).
    Box { origin: [f64; 3], size: [f64; 3] },
    /// Isolated vertices `[x, y, z, tol]` and wire line segments `[x0, y0, z0, x1, y1,
    /// z1, tol]`, each set to its tolerance (fixtures and other units' generators).
    Points {
        vertices: Vec<[f64; 4]>,
        lines: Vec<[f64; 7]>,
    },
}

/// Whether a B-rep source keeps the reference wrapper's size bounds: at most 64 vertices,
/// 3 wire networks, 600 isolated points and 150 line segments. The builders take time in
/// these sizes (fuzz case `topology.shell/fuzz00180`, 2^32 networks, wired for minutes).
pub(crate) fn source_sizes_ok(b: &BrepIn) -> bool {
    match b {
        BrepIn::Vertices { points } => points.len() <= 64,
        BrepIn::Mixed { networks, .. } => *networks <= 3,
        BrepIn::Points { vertices, lines } => vertices.len() <= 600 && lines.len() <= 150,
        _ => true,
    }
}

/// The face of a [`BrepIn::Mixed`] B-rep (as [`BrepIn::Face`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FaceIn {
    pub(crate) surface: SrfIn,
    pub(crate) dom: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    EuQuery {
        eu: i64,
        vertex: i64,
    },
    Connected {
        eu: i64,
        ty: i64,
        index: i64,
    },
    FuQuery {
        fu: i64,
        tgt_only: bool,
        has_loops: bool,
        loops: Vec<i64>,
    },
    Collect {
        fu: i64,
        threshold: Option<f64>,
    },
    UvLoopGap {
        eu: i64,
    },
    /// Evaluates the native 3D edge and retained UV trim at the same NURB parameter.
    TrimSample { eu: i64, fraction: f64 },
    /// `kind`: 0 NULL, 1 the attached curve, 2 a new curve, 3 `DeleteUVTrimCurve`, 4
    /// `SetMemberUVTrimCurve` with a new curve.
    SetUv {
        eu: i64,
        kind: u8,
        curve: Option<CrvIn>,
        dmax: f64,
        delete: bool,
        fail: Option<i64>,
    },
    /// `which`: 0 `GetMaxEdgeFaceGap`, 1 `GetEdgeCCWEdgeGap`, 2 `CalcMaxEdgeFaceGap`, 3
    /// `CalcEdgeCCWEdgeGap`.
    Gaps {
        eu: i64,
        which: u8,
        flag: bool,
        want_out: bool,
    },
    SetOrient {
        eu: i64,
        orient: u8,
    },
    StepOff {
        surface: SrfIn,
        dist: f64,
        p: [f64; 3],
        n: [f64; 3],
        x: [f64; 2],
        jac: bool,
    },
    DistDev {
        surface: SrfIn,
        p: [f64; 3],
        uv: [f64; 2],
        value: f64,
        obj: u8,
        want_dev: bool,
    },
    Seam {
        curves: Vec<CrvIn>,
        dom: [f64; 2],
        called: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

// ---- encoding ---------------------------------------------------------------------------

pub(crate) fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("curve without points"))?;
    let m =
        c.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("curve without knots"))?;
    v.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        v.extend(p);
    }
    v.extend(&c.u);
    Ok(())
}

pub(crate) fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline(b) => {
            let n = b
                .net
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            if b.u.is_empty() || b.v.is_empty() {
                return Err(harness("empty knot vector"));
            }
            v.extend([
                0.0,
                f64::from(u8::from(b.oob)),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("empty u knots"))? as f64,
                b.v.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("empty v knots"))? as f64,
            ]);
            for row in &b.net {
                for p in row {
                    v.extend(p);
                }
            }
            v.extend(&b.u);
            v.extend(&b.v);
        }
    }
    Ok(())
}

pub(crate) fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
pub(crate) fn path_of(b: Option<&BrepIn>) -> Result<String, HarnessError> {
    match b {
        Some(BrepIn::File { file, .. }) => {
            let root = crate::case::repo_root();
            let p = if file.starts_with("fixtures/") {
                crate::fixtures::repo_fixture_file(&root, file)?
            } else {
                crate::fixtures::upstream_test_file(&root, file)?
            };
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

/// The B-rep block.
fn brep_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    brep_block_of(i.brep.as_ref(), i.validate_only, i.init_gaps)
}

/// The B-rep block of `brep` (shared with unit `topology.edgeuse_p2`).
pub(crate) fn brep_block_of(
    brep: Option<&BrepIn>,
    validate_only: bool,
    init_gaps: bool,
) -> Result<Vec<f64>, HarnessError> {
    let b = brep.ok_or_else(|| harness("B-rep op without brep"))?;
    let mut v = Vec::new();
    match b {
        BrepIn::File { rebuild, .. } => {
            v.push(0.0);
            v.push(flag(*rebuild));
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::CrvOnSurfFace { surface, dom } => {
            v.push(6.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::EllipseFace { circle, origin, axis, radii, angles, natural, inside_out, oob } => {
            v.extend([9.0, flag(*circle)]);
            v.extend(origin);
            v.push(f64::from(*axis));
            v.extend(radii);
            v.extend(angles);
            v.extend(natural);
            v.extend([flag(*inside_out), flag(*oob)]);
        }
        BrepIn::HermiteSheetFace { scale, edge_tol } => {
            v.extend([15.0, *scale, *edge_tol]);
        }
        BrepIn::Network => v.push(3.0),
        BrepIn::Vertices { points } => {
            v.extend([4.0, points.len() as f64]);
            for point in points {
                v.extend(point);
            }
        }
        BrepIn::Mixed { face, networks } => {
            v.extend([5.0, flag(face.is_some())]);
            if let Some(face) = face {
                push_surface(&mut v, &face.surface)?;
                v.extend(face.dom);
            }
            v.push(*networks as f64);
        }
        BrepIn::Box { origin, size } => {
            v.push(7.0);
            v.extend(origin);
            v.extend(size);
        }
        BrepIn::Points { vertices, lines } => {
            v.extend([8.0, vertices.len() as f64]);
            for p in vertices {
                v.extend(p);
            }
            v.push(lines.len() as f64);
            for l in lines {
                v.extend(l);
            }
        }
    }
    v.push(flag(validate_only));
    v.push(flag(init_gaps));
    Ok(v)
}

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
pub(crate) fn sel(s: i64, n: usize) -> i64 {
    if s < 0 || n == 0 {
        -1
    } else {
        (s as u64 % n as u64) as i64
    }
}

/// The selectors of an op resolved against the B-rep's counts; `None` when the op
/// needs an edge use or face use and the B-rep has none.
fn resolve(op: &OpIn, c: [usize; 10]) -> Option<OpIn> {
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let needs_fu = matches!(op, OpIn::FuQuery { .. } | OpIn::Collect { .. });
    if (needs_fu && n_fu == 0) || (!needs_fu && is_brep_op(op) && n_eu == 0) {
        return None;
    }
    let eu = |s: i64| sel(s, n_eu).max(0);
    Some(match op.clone() {
        OpIn::EuQuery { eu: e, vertex } => OpIn::EuQuery {
            eu: eu(e),
            vertex: sel(vertex, n_v),
        },
        OpIn::Connected { eu: e, ty, index } => {
            let n = match ty {
                16012 => n_r,
                16013 => n_s,
                16022 => n_fu,
                16002 => n_f,
                16004 => n_lu,
                16003 => n_l,
                16005 => n_eu,
                16016 => n_e,
                16006 => n_vu,
                16021 => n_v,
                _ => 0,
            };
            OpIn::Connected {
                eu: eu(e),
                ty,
                index: if n == 0 { index.min(-1) } else { sel(index, n) },
            }
        }
        OpIn::FuQuery {
            fu,
            tgt_only,
            has_loops,
            loops,
        } => OpIn::FuQuery {
            fu: sel(fu, n_fu).max(0),
            tgt_only,
            has_loops,
            loops: if n_l == 0 {
                Vec::new()
            } else {
                loops.iter().map(|&l| sel(l, n_l).max(0)).collect()
            },
        },
        OpIn::Collect { fu, threshold } => OpIn::Collect {
            fu: sel(fu, n_fu).max(0),
            threshold,
        },
        OpIn::TrimSample { eu: e, fraction } => OpIn::TrimSample { eu: eu(e), fraction },
        OpIn::UvLoopGap { eu: e } => OpIn::UvLoopGap { eu: eu(e) },
        OpIn::SetUv {
            eu: e,
            kind,
            curve,
            dmax,
            delete,
            fail,
        } => OpIn::SetUv {
            eu: eu(e),
            kind,
            curve,
            dmax,
            delete,
            fail,
        },
        OpIn::Gaps {
            eu: e,
            which,
            flag,
            want_out,
        } => OpIn::Gaps {
            eu: eu(e),
            which,
            flag,
            want_out,
        },
        OpIn::SetOrient { eu: e, orient } => OpIn::SetOrient { eu: eu(e), orient },
        other => other,
    })
}

/// The output when the B-rep has no entity of the kind an op needs.
fn no_entity() -> Output {
    Output::err("no_entity_of_that_kind")
}

fn is_brep_op(op: &OpIn) -> bool {
    !matches!(
        op,
        OpIn::StepOff { .. } | OpIn::DistDev { .. } | OpIn::Seam { .. }
    )
}

/// Reuse the native B-rep fixture encoding without duplicating its geometry
/// constructors or input validation. Shell cases supply a B-rep description.
pub(crate) fn fixture_input(value: Value) -> Result<(String, Vec<f64>), HarnessError> {
    let brep: BrepIn =
        serde_json::from_value(value).map_err(|e| harness(format!("topology fixture: {e}")))?;
    let inputs = Inputs {
        brep: Some(brep),
        validate_only: false,
        init_gaps: false,
        op: OpIn::SetOrient { eu: 0, orient: 0 },
    };
    Ok((path_of(inputs.brep.as_ref())?, brep_block(&inputs)?))
}

fn trim_sample_output(body: &[f64]) -> Output {
    match body.split_first() {
        Some((&0.0, values)) => Output::new().values("sample", values.to_vec()),
        Some((&status, _)) => Output::err(format!("trim_sample_status_{status}")),
        None => Output::err("trim_sample_empty"),
    }
}

// ---- import -----------------------------------------------------------------------------

fn orient_of(code: i64) -> Result<OrientType, HarnessError> {
    match code {
        0 => Ok(OrientType::Unknown),
        1 => Ok(OrientType::Same),
        2 => Ok(OrientType::Opposite),
        x => Err(harness(format!("bad orientation {x}"))),
    }
}

fn orient_code(o: OrientType) -> i64 {
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

/// The upstream type codes of the face surfaces and edge curves (`-1` none), by index, with
/// whether each is held exactly. Empty for a B-rep the Rust kernel built or read, whose
/// geometry is exact.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImportTypes {
    /// `GetSurface()->GetType()` of each face.
    pub(crate) face_surface_types: Vec<i64>,
    /// Whether each face's Rust surface is the upstream kind (or the face has none).
    pub(crate) face_exact: Vec<bool>,
    /// `GetCurve()->GetType()` of each edge.
    pub(crate) edge_curve_types: Vec<i64>,
    /// Whether each edge curve was imported exactly (or the edge has none).
    pub(crate) edge_exact: Vec<bool>,
}

impl ImportTypes {
    /// Native type only when the edge's curve could not be imported exactly.
    pub(crate) fn inexact_curve(&self, e: usize) -> Option<i64> {
        let t = *self.edge_curve_types.get(e)?;
        (t >= 0 && !self.edge_exact.get(e).copied().unwrap_or(false)).then_some(t)
    }

    /// The upstream surface type code of face `f` when the import holds that surface
    /// only as its B-spline part (`None`: imported exactly, no surface, or no face `f`).
    pub(crate) fn inexact_surface(&self, f: usize) -> Option<i64> {
        let t = *self.face_surface_types.get(f)?;
        (t >= 0 && !self.face_exact.get(f).copied().unwrap_or(false)).then_some(t)
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// The value of a query slot: an id index, `-1` for none, [`NULL_DEREF`] where
/// upstream dereferences null; deferral and missing ids propagate.
fn slot<T>(r: Result<T, SolidError>, f: impl FnOnce(T) -> i64) -> Result<i64, SolidError> {
    match r {
        Ok(v) => Ok(f(v)),
        Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => Ok(NULL_DEREF),
        Err(e) => Err(e),
    }
}

fn id<I: Copy>(o: Option<I>, f: impl Fn(I) -> u32) -> i64 {
    o.map_or(-1, |x| i64::from(f(x)))
}

fn rust_err(e: &SolidError) -> Result<Output, HarnessError> {
    match Output::unported(e) {
        Some(o) => Ok(o),
        None => Err(harness(format!("rust: {e}"))),
    }
}

/// The Rust B-rep of a B-rep input, its geometry types and its entity counts, or the
/// output of a failed build. Shim-built sources are built by the Rust kernel
/// ([`crate::own_builder`]); `.smb` file sources are read by the Rust reader and
/// renumbered into the shim's traversal order ([`crate::own_reader::brep_in_order`]),
/// so every field the Rust side reports (geometry, owners, drop records, cached gap
/// states) is Rust's own; only the C++ side imports the export.
pub(crate) fn rust_brep(
    k: &mut Kernel,
    brep: Option<&BrepIn>,
    validate_only: bool,
    init_gaps: bool,
) -> Result<crate::own_builder::RustBrep<(Brep, ImportTypes, [usize; 10])>, HarnessError> {
    use crate::own_builder::{build, build_status, counts, Built};
    let src = brep.ok_or_else(|| harness("B-rep op without brep"))?;
    match build(k, src, validate_only, init_gaps)? {
        Some(Built::Brep(b)) => {
            let b = *b;
            let c = counts(&b);
            Ok(Ok((b, ImportTypes::default(), c)))
        }
        Some(Built::Failed(e)) => match Output::unported(&e) {
            Some(o) => Ok(Err(o)),
            None => Ok(Err(Output::err(format!(
                "brep_build_status_{}",
                build_status(&e)
            )))),
        },
        None => {
            let conv = &crate::own_builder::Conventions::EDGEUSE_GRAPH;
            match crate::own_reader::brep_in_order(k, src, validate_only, init_gaps, conv)? {
                Built::Brep(b) => {
                    let b = *b;
                    let c = counts(&b);
                    Ok(Ok((b, ImportTypes::default(), c)))
                }
                Built::Failed(e) => match Output::unported(&e) {
                    Some(o) => Ok(Err(o)),
                    None => Ok(Err(Output::err(format!(
                        "brep_build_status_{}",
                        build_status(&e)
                    )))),
                },
            }
        }
    }
}

fn run_rust(k: &mut Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    if !is_brep_op(&i.op) {
        return match run_rust_geom(k, &i.op) {
            Ok(o) => Ok(o),
            Err(Fail::Solid(e)) => rust_err(&e),
            Err(Fail::Harness(h)) => Err(h),
        };
    }
    let (mut b, types, c) = match rust_brep(k, i.brep.as_ref(), i.validate_only, i.init_gaps)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(no_entity());
    };
    if let Some(o) = inexact_geometry(&b, &types, &op) {
        return Ok(o);
    }
    match run_rust_brep(k, &mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral label of cases whose geometry the shared B-rep export holds only as a
/// B-spline part (an analytic face surface or edge curve the import cannot rebuild
/// exactly): a harness limitation, not an unported unit (`topology.brep_p1`'s
/// convention).
const HARNESS_ANALYTIC_EXPORT: &str = "harness:analytic-geometry-export";

/// The deferral of a case that evaluates a face surface or edge curve the import holds
/// only as its B-spline part (an analytic `SmBSplineSurface` / `SmBSplineCurve`
/// subclass): the gap calculations and the 3D-drop branch of `GetStartPointUVLoopGap`.
fn inexact_geometry(b: &Brep, types: &ImportTypes, op: &OpIn) -> Option<Output> {
    let surface_of = |e: EdgeuseId| {
        eu1::face(b, e)
            .ok()
            .flatten()
            .and_then(|f| types.inexact_surface(f.index() as usize))
            .unwrap_or(-1)
    };
    let curve_of = |e: Option<EdgeuseId>| {
        e.and_then(|e| eu1::edge(b, e).ok().flatten())
            .and_then(|x| types.inexact_curve(x.index() as usize))
            .unwrap_or(-1)
    };
    let (e, surface, other) = match op {
        OpIn::TrimSample { eu, .. } => (eid(*eu), true, None),
        OpIn::Gaps { eu, which, .. } => {
            let e = eid(*eu);
            if !eu1::is_loop_edgeuse(b, e).unwrap_or(false) {
                return None;
            }
            if *which == 0 || *which == 2 {
                (e, true, None)
            } else {
                (e, false, eu1::ccw_edgeuse(b, e).ok().flatten())
            }
        }
        OpIn::UvLoopGap { eu } => {
            let e = eid(*eu);
            let r = b.edgeuse(e).ok()?;
            let cw = r.cw?;
            let both = eu1::uv_trim_curve(b, e).ok().and_then(|x| x.0).is_some()
                && eu1::uv_trim_curve(b, cw).ok().and_then(|x| x.0).is_some();
            if both {
                return None;
            }
            (e, true, Some(cw))
        }
        _ => return None,
    };
    let st = if surface { surface_of(e) } else { -1 };
    if st >= 0 && !EXACT_SURFACES.contains(&st) {
        let (unit, symbol) = match st {
            // the DISPATCH GAP of `Surface::read_from_db` (issue #76) belongs to
            // `surface.surface_p2`, not to the analytic surface units (all green)
            15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
            15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCylinder dispatch)"),
            15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere dispatch)"),
            15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus dispatch)"),
            15030 => (
                HARNESS_ANALYTIC_EXPORT,
                "SmSurfOfExtrusion face surface not exported exactly",
            ),
            _ => (
                HARNESS_ANALYTIC_EXPORT,
                "face surface of an analytic kind exported as its B-spline part",
            ),
        };
        return Some(Output::deferred(unit, symbol));
    }
    for t in [curve_of(Some(e)), curve_of(other)] {
        if t >= 0 && !EXACT_CURVES.contains(&t) {
            return Some(Output::deferred(
                HARNESS_ANALYTIC_EXPORT,
                "edge curve of an analytic kind exported as its B-spline part",
            ));
        }
    }
    None
}

enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn eid(i: i64) -> EdgeuseId {
    EdgeuseId::from_index(i as u32)
}

/// The state of an edge use after `SetUVTrimCurve`, as the shim's `put_eu_state`.
fn eu_state(k: &Kernel, b: &Brep, eu: Option<EdgeuseId>, out: &mut Vec<f64>) -> Result<(), SolidError> {
    let Some(eu) = eu else {
        out.push(-1.0);
        return Ok(());
    };
    out.push(1.0);
    let r = b.edgeuse(eu)?;
    put_trim_curve(k, out, r.uv_trim_curve.as_ref())?;
    // the curve's owner, as the shim's `g.find(6, uv->GetOwner())`
    out.push(crate::own_builder::uv_owner_index(b, eu, None));
    match r.drop_curve_fail.as_deref() {
        Some(f) => out.extend([1.0, f64::from(f.fail_status), flag(f.brep.is_some())]),
        None => out.extend([0.0, 0.0, 0.0]),
    }
    match r.vertexuse {
        Some(vu) => out.push(flag(b.vertexuse(vu)?.vertex_face_gap.is_init())),
        None => out.push(-1.0),
    }
    Ok(())
}

fn status_of(e: &SolidError) -> f64 {
    match e {
        up_err if solidcore::error::upstream_status(up_err).is_some() => f64::from(solidcore::error::upstream_status(up_err).unwrap_or(-1)),
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => NULL_DEREF as f64,
        _ => -9.0,
    }
}

/// The Rust side of B-rep op `op` on `b`.
fn run_rust_brep(k: &Kernel, b: &mut Brep, op: &OpIn) -> Result<Output, Fail> {
    match op {
        OpIn::EuQuery { eu, vertex } => {
            let e = eid(*eu);
            let v = (*vertex >= 0).then(|| VertexId::from_index(*vertex as u32));
            let mut q = vec![
                id(eu1::edge(b, e)?, EdgeId::index),
                orient_code(eu1::orientation(b, e)?),
            ];
            q.push(id(eu1::cw_edgeuse(b, e)?, EdgeuseId::index));
            q.push(id(eu1::ccw_edgeuse(b, e)?, EdgeuseId::index));
            q.push(i64::from(eu1::has_edge_face_gap(b, e)?));
            q.push(i64::from(eu1::has_edge_ccw_edge_gap(b, e)?));
            q.push(i64::from(eu1::is_max_edge_face_gap_init(b, e)?));
            q.push(i64::from(eu1::is_loop_edgeuse(b, e)?));
            q.push(i64::from(eu1::is_shell_edgeuse(b, e)?));
            q.push(id(eu1::vertexuse(b, e)?, VertexuseId::index));
            q.push(id(eu1::loopuse(b, e)?, LoopuseId::index));
            q.push(id(eu1::faceuse(b, e)?, FaceuseId::index));
            q.push(slot(eu1::face(b, e), |x| id(x, FaceId::index))?);
            q.push(id(eu1::shell(b, e)?, ShellId::index));
            q.push(slot(eu1::brep(b, e), |x| i64::from(x.is_some()))?);
            let tol = eu1::tolerance(b, e)?;
            q.push(slot(eu1::mate(b, e), |x| id(x, EdgeuseId::index))?);
            q.push(slot(eu1::radial(b, e), |x| id(x, EdgeuseId::index))?);
            q.push(slot(eu1::is_strut(b, e), i64::from)?);
            let uv = eu1::uv_trim_curve(b, e).map(|(c, o)| (c.is_some(), o));
            q.push(slot(uv.clone(), |(c, _)| i64::from(c))?);
            q.push(slot(uv, |(_, o)| id(o, EdgeuseId::index))?);
            let fail = eu1::drop_curve_fail(b, e).map(|(f, o)| (f.is_some(), o));
            q.push(slot(fail.clone(), |(f, _)| i64::from(f))?);
            q.push(slot(fail, |(_, o)| id(o, EdgeuseId::index))?);
            q.push(i64::from(eu1::uv_trim_curve_pointer(b, e)?.is_some()));
            q.push(slot(eu1::corner_mate_edgeuse(b, e, v), |x| {
                id(x, EdgeuseId::index)
            })?);
            q.push(eu1::TYPE);
            q.extend(KIND_TYPES.iter().map(|&t| i64::from(eu1::is_kind_of(t))));
            Ok(Output::new().ints("q", q).values("tol", vec![tol]))
        }
        OpIn::Connected { eu, ty, index } => {
            let t = target(*ty, *index);
            Ok(Output::new().ints("q", vec![i64::from(eu1::is_connected_to(b, eid(*eu), t)?)]))
        }
        OpIn::FuQuery {
            fu,
            tgt_only,
            has_loops,
            loops,
        } => {
            let f = FaceuseId::from_index(*fu as u32);
            let mut q = vec![
                id(eu1::faceuse_shell(b, f)?, ShellId::index),
                id(eu1::faceuse_region(b, f)?, RegionId::index),
                id(eu1::faceuse_face(b, f)?, FaceId::index),
                id(eu1::faceuse_mate(b, f)?, FaceuseId::index),
                orient_code(eu1::faceuse_orientation(b, f)?),
                eu1::FACEUSE_TYPE,
            ];
            q.extend(
                KIND_TYPES
                    .iter()
                    .map(|&t| i64::from(eu1::faceuse_is_kind_of(t))),
            );
            let ls: Vec<LoopId> = loops
                .iter()
                .map(|&l| LoopId::from_index(l as u32))
                .collect();
            let tl = has_loops.then_some(ls.as_slice());
            let lists = vec![
                eu1::faceuse_loopuses(b, f, *tgt_only, tl)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_edgeuses(b, f, *tgt_only, tl)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_neighbors(b, f, false)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_neighbors(b, f, true)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
            ];
            Ok(Output::new()
                .ints("q", q)
                .topology(crate::output::Topology::new().adjacency("lists", lists)))
        }
        OpIn::Collect { fu, threshold } => {
            let mut list = Vec::new();
            eu1::faceuse_collect_adjacent(
                &Kernel::default(),
                b,
                FaceuseId::from_index(*fu as u32),
                &mut list,
                *threshold,
            )?;
            Ok(Output::new().int("status", 0).ints(
                "list",
                list.into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect::<Vec<_>>(),
            ))
        }
        OpIn::TrimSample { eu, fraction } => {
            let eu = eid(*eu);
            let curve = eu1::edge(b, eu)?.map(|e| b.edge(e)).transpose()?;
            let face = eu1::face(b, eu)?.map(|f| b.face(f)).transpose()?;
            let trim = eu1::uv_trim_curve(b, eu)?.0;
            let (Some(edge), Some(surface), Some(trim)) = (curve, face.and_then(|f| f.surface.as_ref()), trim) else {
                return Ok(trim_sample_output(&[-2.0]));
            };
            let Some(curve) = edge.curve.as_ref() else { return Ok(trim_sample_output(&[-2.0])); };
            let t = edge.interval.evaluate(*fraction);
            let evaluated = (|| -> Result<_, SolidError> {
                let xyz = curve.evaluate_point(k, t)?;
                let uv = trim.evaluate_point(k, t)?;
                let lifted = surface.evaluate_point(k, solidcore::math::Point2::new(uv.x, uv.y))?;
                Ok((xyz, uv, lifted))
            })();
            match evaluated {
                Ok((xyz, uv, lifted)) => {
                    let ivl = trim.natural_interval();
                    Ok(Output::new().values("sample", vec![
                        curve.type_code() as f64, edge.interval.min(), edge.interval.max(),
                        ivl.min(), ivl.max(), t, xyz.x, xyz.y, xyz.z, uv.x, uv.y,
                        lifted.x, lifted.y, lifted.z,
                    ]))
                }
                Err(e) if e.is_unported() => Err(e.into()),
                Err(e) => Ok(trim_sample_output(&[status_of(&e)])),
            }
        }
        OpIn::UvLoopGap { eu } => Ok(Output::new().values(
            "gap",
            vec![eu1::start_point_uv_loop_gap(
                &Kernel::default(),
                b,
                eid(*eu),
            )?],
        )),
        OpIn::SetUv {
            eu,
            kind,
            curve,
            dmax,
            delete,
            fail,
        } => {
            let e = eid(*eu);
            let mate = match eu1::mate(b, e) {
                Ok(m) => m,
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF as f64]))
                }
                Err(x) => return Err(x.into()),
            };
            let mut arg_fail = fail.map(|s| DropCurveFail::<ObjectHandle> {
                fail_status: s as i32,
                ..DropCurveFail::default()
            });
            let new_curve = match curve {
                Some(c) => Some(SmCurve::from(bspline_of(c)?)),
                None => None,
            };
            let r = match kind {
                3 => eu1::delete_uv_trim_curve(b, e),
                4 => {
                    let c = new_curve.ok_or_else(|| harness("set_uv: no curve"))?;
                    eu1::set_member_uv_trim_curve(b, e, c, arg_fail.clone()).map(|_| ())
                }
                _ => {
                    let arg = match kind {
                        0 => eu1::UvTrimCurveArg::Null,
                        1 => eu1::UvTrimCurveArg::Current,
                        _ => eu1::UvTrimCurveArg::New(Box::new(
                            new_curve.ok_or_else(|| harness("set_uv: no curve"))?,
                        )),
                    };
                    eu1::set_uv_trim_curve(b, e, arg, *dmax, *delete, arg_fail.as_mut()).map(|_| ())
                }
            };
            match r {
                Ok(()) => {}
                Err(SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF as f64]))
                }
                Err(x) => return Err(x.into()),
            }
            let mut out = vec![0.0];
            eu_state(k, b, Some(e), &mut out)?;
            eu_state(k, b, mate, &mut out)?;
            out.push(match eu1::edge(b, e)? {
                Some(x) => b.edge(x)?.tolerance,
                None => -1.0,
            });
            out.push(flag(
                *kind != 4 && arg_fail.as_ref().is_some_and(|f| f.brep.is_some()),
            ));
            Ok(Output::new().values("state", out))
        }
        OpIn::Gaps {
            eu,
            which,
            flag: f,
            want_out,
        } => {
            let e = eid(*eu);
            let mut out = Vec::new();
            let fg0 = b.edgeuse(e)?.max_edge_face_gap.is_init();
            let cg0 = b.edgeuse(e)?.edge_ccw_edge_gap.is_init();
            let mut vals: Vec<f64> = Vec::new();
            match which {
                0 => out.push(flag(
                    eu1::max_edge_face_gap(&Kernel::default(), b, e, *f)?.is_some(),
                )),
                1 => out.push(flag(
                    eu1::edge_ccw_edge_gap(&Kernel::default(), b, e, *f)?.is_some(),
                )),
                2 => match eu1::calc_max_edge_face_gap(&Kernel::default(), b, e, *f) {
                    Ok(g) => {
                        out.extend([0.0, flag(*want_out && g.is_init())]);
                        if *want_out && g.is_init() {
                            vals.extend([g.gap_3d, g.edge_t, g.face_uv.x, g.face_uv.y]);
                        }
                    }
                    Err(x) if x.is_unported() => return Err(x.into()),
                    Err(x) => out.extend([status_of(&x), 0.0]),
                },
                _ => match eu1::calc_edge_ccw_edge_gap(&Kernel::default(), b, e, *want_out, *f) {
                    Ok(g) => {
                        out.extend([0.0, flag(g.is_some_and(|g| g.is_init()))]);
                        if let Some(g) = g.filter(solidcore::heal::gap::EdgeEdgeGap::is_init) {
                            vals.extend([g.gap_3d, g.edge_t0, g.edge_t1]);
                        }
                    }
                    Err(x) if x.is_unported() => return Err(x.into()),
                    Err(x) => out.extend([status_of(&x), 0.0]),
                },
            }
            let is_loop = eu1::is_loop_edgeuse(b, e)?;
            let fcomp = is_loop && ((*which == 0 && (*f || !fg0)) || (*which == 2 && *f));
            let ccomp = is_loop && ((*which == 1 && (*f || !cg0)) || (*which == 3 && *f));
            {
                let r = b.edgeuse(e)?;
                let g = r.max_edge_face_gap;
                if fcomp && g.is_init() {
                    vals.extend([g.gap_3d, g.edge_t, g.face_uv.x, g.face_uv.y]);
                }
                let g = r.edge_ccw_edge_gap;
                if ccomp && g.is_init() {
                    vals.extend([g.gap_3d, g.edge_t0, g.edge_t1]);
                }
            }
            let r = b.edgeuse(e)?;
            out.push(flag(r.max_edge_face_gap.is_init()));
            out.push(flag(r.edge_ccw_edge_gap.is_init()));
            let mate = eu1::mate(b, e).ok().flatten();
            out.push(match mate {
                Some(m) => flag(b.edgeuse(m)?.max_edge_face_gap.is_init()),
                None => -1.0,
            });
            out.extend(vals);
            Ok(Output::new().values("state", out))
        }
        OpIn::SetOrient { eu, orient } => {
            let e = eid(*eu);
            eu1::set_orientation(b, e, orient_of(i64::from(*orient))?)?;
            let q = vec![
                orient_code(eu1::orientation(b, e)?),
                slot(eu1::mate(b, e), |x| id(x, EdgeuseId::index))?,
                slot(eu1::radial(b, e), |x| id(x, EdgeuseId::index))?,
            ];
            Ok(Output::new().ints("q", q))
        }
        _ => Err(harness("not a B-rep op").into()),
    }
}

/// The `IsConnectedTo` target of a type code and index (`None` for null and unknown
/// codes, which upstream also answers `TRUE` for... via the null check).
fn target(ty: i64, index: i64) -> Option<EntityRef> {
    let i = u32::try_from(index).ok()?;
    Some(match ty {
        16012 => EntityRef::Region(RegionId::from_index(i)),
        16013 => EntityRef::Shell(ShellId::from_index(i)),
        16022 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        16002 => EntityRef::Face(FaceId::from_index(i)),
        16004 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        16003 => EntityRef::Loop(LoopId::from_index(i)),
        16005 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        16016 => EntityRef::Edge(EdgeId::from_index(i)),
        16006 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        16021 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

pub(crate) fn bspline_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, c.dim, false))
}

pub(crate) fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::from_array(*o),
            Vec3::from_array(*n),
        ))),
        SrfIn::Bspline(bs) => {
            let net = bs
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, bs.p, bs.q, bs.u.clone(), bs.v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = bs.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(bs.form))
                .ok_or_else(|| harness(format!("bad form {}", bs.form)))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

/// A solution object handle of `sm_CalcDistAndDev` cases.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(ObjectKind);

impl solidcore::geom::solution_array::SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        self.0
    }
}

fn run_rust_geom(k: &Kernel, op: &OpIn) -> Result<Output, Fail> {
    match op {
        OpIn::StepOff {
            surface,
            dist,
            p,
            n,
            x,
            jac,
        } => {
            let s = surface_of(surface)?;
            let mut e = eu1::FindStepOff::new(k, *dist, Vec3::from_array(*p), Vec3::from_array(*n), &s);
            let mut f = [UNDEF_DOUBLE; 2];
            let mut m = Matrix::new(2, 2);
            for r in 0..2 {
                for c in 0..2 {
                    m.set_at(r, c, UNDEF_DOUBLE)?;
                }
            }
            let res = e.evaluate(x, &mut f, if *jac { Some(&mut m) } else { None });
            let failed = match res {
                Ok(_) => 0,
                Err(x) if x.is_unported() => return Err(x.into()),
                Err(_) => 1,
            };
            let mut fj = f.to_vec();
            for r in 0..2 {
                for c in 0..2 {
                    fj.push(m.at(r, c)?);
                }
            }
            Ok(Output::new()
                .int("failed", failed)
                .int("found", 0)
                .values("fj", fj))
        }
        OpIn::DistDev {
            surface,
            p,
            uv,
            value,
            obj,
            want_dev,
        } => {
            let s = surface_of(surface)?;
            let mut sol = Solution::<Obj>::new();
            sol.start.parameters[0] = uv[0];
            sol.start.parameters[1] = uv[1];
            sol.start.solution_value = *value;
            sol.objects[0] = match obj {
                1 => Some(Obj(ObjectKind::Surface)),
                2 => Some(Obj(ObjectKind::Curve)),
                _ => None,
            };
            let (dist, dev) =
                eu1::calc_dist_and_dev(k, Vec3::from_array(*p), &sol, |_| Some(&s), *want_dev)?;
            Ok(Output::new().values("dist_dev", vec![dist, dev.unwrap_or(UNDEF_DOUBLE)]))
        }
        OpIn::Seam {
            curves,
            dom,
            called,
        } => {
            let cs = curves
                .iter()
                .map(|c| bspline_of(c).map(SmCurve::from))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<&SmCurve> = cs.iter().collect();
            let d = Extent1d::from_min_max(dom[0], dom[1]);
            Ok(Output::new().ints(
                "seam",
                vec![i64::from(eu1::are_seam_curves(&refs, &d, *called))],
            ))
        }
        _ => Err(harness("not a geometry op").into()),
    }
}

// ---- generation -------------------------------------------------------------------------

/// The upstream `.smb` files used (all B-spline geometry).
/// (`SmallBox.smp` and `Platform.smp` crash upstream's `SmBrep::ReadFromFile`; see
/// `plan/findings.md`.)
pub(crate) const FILES: [&str; 15] = [
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

pub(crate) fn clamped_knots(rng: &mut Rng, p: usize, n: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(ks);
    u.extend(vec![b; p + 1]);
    u
}

pub(crate) fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            let k = if rational { w } else { 1.0 };
            let x = (i as f64 + rng.uniform(-0.3, 0.3)) * scale;
            let y = rng.uniform(-1.0, 1.0) * scale;
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-1.0, 1.0) * scale * k
            };
            [x * k, y * k, z, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0))
    };
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

pub(crate) fn gen_surface(rng: &mut Rng) -> SrfIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    if rng.chance(0.25) {
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let pinch = rng.chance(0.15);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (x, y) = if pinch && i == 0 {
                        (0.0, 0.0)
                    } else {
                        (i as f64, j as f64)
                    };
                    [x * scale, y * scale, rng.uniform(-0.5, 0.5) * scale, NOW]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: rng.chance(0.2),
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

pub(crate) fn domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Bspline(b) => [
            b.u.first().copied().unwrap_or(0.0),
            b.v.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
            b.v.last().copied().unwrap_or(1.0),
        ],
    }
}

pub(crate) fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(10) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smp")).to_string(),
            rebuild: rng.chance(0.25),
        },
        6 => {
            let c = gen_curve(rng, 3, 1.0);
            let (a, b) = (
                c.u.first().copied().unwrap_or(0.0),
                c.u.last().copied().unwrap_or(1.0),
            );
            let (t0, t1) = if rng.chance(0.5) {
                (a, b)
            } else {
                (
                    a + (b - a) * rng.uniform(0.0, 0.3),
                    b - (b - a) * rng.uniform(0.0, 0.3),
                )
            };
            BrepIn::Wire { curve: c, t0, t1 }
        }
        7 | 8 => {
            let s = gen_surface(rng);
            let [u0, v0, u1, v1] = domain(&s);
            let dom = if rng.chance(0.5) {
                [u0, v0, u1, v1]
            } else {
                let (du, dv) = (u1 - u0, v1 - v0);
                [
                    u0 + du * rng.uniform(0.0, 0.3),
                    v0 + dv * rng.uniform(0.0, 0.3),
                    u1 - du * rng.uniform(0.0, 0.3),
                    v1 - dv * rng.uniform(0.0, 0.3),
                ]
            };
            BrepIn::Face { surface: s, dom }
        }
        _ => BrepIn::Network,
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = domain(s);
    let pick = |rng: &mut Rng, a: f64, b: f64| match rng.below(8) {
        0 => a,
        1 => b,
        2 => 0.5 * (a + b),
        3 => a - (b - a) * rng.uniform(0.0, 0.5),
        _ => a + (b - a) * rng.unit(),
    };
    [pick(rng, u0, u1), pick(rng, v0, v1)]
}

fn gen_op(rng: &mut Rng) -> OpIn {
    let types = [
        16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021, 16011, 7,
    ];
    match rng.below(24) {
        0..=4 => OpIn::EuQuery {
            eu: gen_sel(rng),
            vertex: if rng.chance(0.15) { -1 } else { gen_sel(rng) },
        },
        5..=7 => OpIn::Connected {
            eu: gen_sel(rng),
            ty: *rng.choose(&types).unwrap_or(&16005),
            index: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        8..=10 => OpIn::FuQuery {
            fu: gen_sel(rng),
            tgt_only: rng.chance(0.5),
            has_loops: rng.chance(0.5),
            loops: (0..rng.below(4)).map(|_| gen_sel(rng)).collect(),
        },
        11 => OpIn::Collect {
            fu: gen_sel(rng),
            threshold: rng.chance(0.2).then(|| rng.uniform(0.0, 1.0)),
        },
        12..=13 => OpIn::UvLoopGap { eu: gen_sel(rng) },
        14..=16 => {
            let kind = rng.below(5) as u8;
            OpIn::SetUv {
                eu: gen_sel(rng),
                kind,
                curve: (kind == 2 || kind == 4).then(|| gen_curve(rng, 2, 1.0)),
                dmax: *rng
                    .choose(&[0.0, 0.0, 1e-9, 1e-6, 1e-3, 1.0])
                    .unwrap_or(&0.0),
                delete: rng.chance(0.5),
                fail: rng.chance(0.4).then(|| rng.range_i64(0, 1100)),
            }
        }
        17 => OpIn::Gaps {
            eu: gen_sel(rng),
            which: rng.below(4) as u8,
            flag: rng.chance(0.5),
            want_out: rng.chance(0.5),
        },
        18 => OpIn::SetOrient {
            eu: gen_sel(rng),
            orient: rng.below(3) as u8,
        },
        19..=20 => {
            let s = gen_surface(rng);
            let x = gen_uv(rng, &s);
            OpIn::StepOff {
                dist: *rng.choose(&[0.0, 1e-6, 0.1, 1.0, 10.0]).unwrap_or(&0.1),
                p: rng.point3(-3.0, 3.0),
                n: if rng.chance(0.1) {
                    [0.0; 3]
                } else {
                    rng.point3(-1.0, 1.0)
                },
                x,
                jac: rng.chance(0.7),
                surface: s,
            }
        }
        21..=22 => {
            let s = gen_surface(rng);
            let uv = gen_uv(rng, &s);
            OpIn::DistDev {
                p: rng.point3(-3.0, 3.0),
                uv,
                value: rng.uniform(0.0, 2.0),
                obj: rng.below(3) as u8,
                want_dev: rng.chance(0.8),
                surface: s,
            }
        }
        _ => {
            let n = *rng.choose(&[0usize, 1, 2, 2, 2, 3]).unwrap_or(&2);
            let mut curves: Vec<CrvIn> = (0..n).map(|_| gen_curve(rng, 2, 1.0)).collect();
            if n == 2 && rng.chance(0.5) {
                // Copy curve 0's knots only onto a curve of the same degree, or the
                // result has an end multiplicity above degree + 1.
                let (first, first_p) = curves
                    .first()
                    .map(|c| (c.u.clone(), c.p))
                    .unwrap_or_default();
                if let Some(c) = curves.get_mut(1) {
                    if c.u.len() == first.len() && c.p == first_p {
                        c.u = first;
                    }
                }
            }
            let (a, b) = curves.first().map_or((0.0, 1.0), |c| {
                (
                    c.u.first().copied().unwrap_or(0.0),
                    c.u.last().copied().unwrap_or(1.0),
                )
            });
            let slop = rng.uniform(0.0, 0.02) * (b - a);
            OpIn::Seam {
                curves,
                dom: if rng.chance(0.6) {
                    [a - slop, b + slop]
                } else {
                    [a, a + 0.5 * (b - a)]
                },
                called: rng.chance(0.3),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.edgeuse_p1"
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &mut Kernel::default();
        run_rust(k, &case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = gen_op(rng);
        let brep = is_brep_op(&op).then(|| gen_brep(rng));
        let i = Inputs {
            brep,
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.2),
            op,
        };
        serde_json::to_value(i).ok()
    }
}

