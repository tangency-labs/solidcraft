//! Runner for unit `topology.trimming_tools`. C++: `solidcore_ffi::units::topology_trimming_tools`.
//! Rust: `solidcore::topology::trimming_tools`.
//!
//! **Geometry ops** run on curves (`{"bspline": {dim, oob, cpts, p, u}}` with control
//! points as stored `[x, y, z, w]`, `{"line": {p, v, domain, scale, inside_out}}`,
//! `{"ellipse": {c, x, y, domain, radii, inside_out}}` with the domain in degrees, or
//! `{"circle": ..}`, a quadratic B-spline arc made by the Rust `CreateCircleSegment` and
//! handed to both sides as data) and surfaces (`{"bspline": ..}` or `{"plane": {o, n}}`):
//! `order` (`OrderCurvesIntoLoops`, then `FindOuterLoops` on its result when `outer`),
//! `outer` (`FindOuterLoops` on given loops), `mirror`, `iso_at`, `iso`, `iso_seam`,
//! `continuity`, `uv_gap` (`sm_IsLoopUVGapWithinTolerance`, copied verbatim into the
//! shim), `min`, `trim_null` (the null-region error of `TrimSurfaceWithModelSpaceCurves`)
//! and `loops`, a script of `SmCurveLoops` / `SmIsoXSectSegment` / `SmCurveIntersections`
//! operations on a store of curves.
//!
//! **B-rep ops** use the B-reps of unit `topology.vertexuse` (upstream `.smb` files and
//! faces of B-spline surfaces or real `SmPlane`s): the C++ side builds a real `SmBrep`
//! and preserves exactly exported analytic surfaces. Other surfaces and edge curves
//! get the plain B-spline copies the Rust import rebuilds; the Rust side imports the
//! graph that unit's shim exports. Loop,
//! face and edge use selectors are taken modulo their counts. The mutating ops compare
//! every edge use's UV trim curve afterwards.
//!
//! Both sides produce one flat record (`out`), laid out as in the ffi module; `-2` marks a
//! call the shim skips because upstream would dereference null or read out of bounds (the
//! Rust side reports [`SolidError::InvalidInput`] or fails the same guard), `-3` a call
//! the shim skips because the Rust side defers it to an unported unit.
//!
//! `CheckFace`, `FixTrimLoopOrientation` and `FixTrimLoopOrder` run in full on both sides
//! (`SmFace::ComputeLoopOrientation` is ported; it was skipped with `-3` while the Rust
//! side still deferred it), guarded (`-2`) where a loop lacks its loop use, the face its
//! surface or UV trim curves, or `CheckLoop` a loop's UV extremes.
//!
//! Deferred: `SplitAtSeams` (`topology.face_p1` / `topology.face_p4`), `CreateUVTrimCurves`
//! at the face-owned surface's `SmFace::GetTolerance` stub of `GetSingularities`
//! (`topology.face_p2`; it and `ClearUVTrimCurves` are compared, with every edge use's
//! UV trim curve after the call), `SmEdgeuse::GetOrCreateUVTrimCurve`
//! (`topology.edgeuse_p3`), and the vertex-use UV points some B-reps need
//! (`ComputeUVPoint`'s own deferrals). Last run: 3263 cases, 2842 pass, 421 deferred
//! (`topology.edgeuse_p3` 409, `topology.face_p2` 3, `topology.face_p4` 9).
//!
//! The UV curve ops (`mirror`, `iso_at`, `iso`, `iso_seam`) take every curve kind: an
//! `SmLine` or `SmEllipse` is an `SmBSplineCurve` (`rv_ellipse.json`).

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::assert_array::AssertArray;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{ObjectKind, Solution};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::p1::SurfParamType;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::loopuse as lu1;
use solidcore::topology::trimming_tools as tt;
use solidcore::topology::trimming_tools::curve_loops::{
    CurveIntersections, CurveKey, CurveLoops, IsoXSectSegment, LoopObject,
};
use solidcore::topology::{Brep, EdgeuseId, FaceId, LoopId};
use solidcore::SolidError;
use solidcore_ffi::units::topology_trimming_tools::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse as fixture;

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// `.smb` files of the B-rep cases.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A quadratic circular arc, as inline B-spline data for both sides.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircleIn {
    center: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    radius: f64,
    a0: f64,
    a1: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineIn {
    p: [f64; 3],
    v: [f64; 3],
    domain: [f64; 2],
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    inside_out: bool,
}

fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllipseIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radii: [f64; 2],
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Circle(CircleIn),
    Line(LineIn),
    Ellipse(EllipseIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    /// An `SmCurveBoundedSurface` without boundary curves over `basis`.
    Bounded {
        basis: Box<SrfIn>,
        #[serde(default)]
        implicit: bool,
    },
}

/// One `SmCurveLoops` script op.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
enum LoopOp {
    StartIndex {
        i: u64,
    },
    StartEnd {
        i: u64,
    },
    Adjacent {
        key: usize,
        t: f64,
    },
    EnterLeave {
        a: u64,
        b: u64,
    },
    Sample {
        key: usize,
        t: f64,
    },
    SameSide {
        key: usize,
        t: f64,
        dir: u8,
        iso: f64,
        ztol: f64,
        dom: [f64; 4],
    },
    AppendSelf {
        i: u64,
    },
    InsertInto {
        new: usize,
        after: usize,
    },
    InsertIso {
        key: usize,
        ivl: [f64; 2],
        idx: [u64; 4],
    },
    Update {
        ints: Vec<IntsIn>,
        orig: usize,
        new: usize,
        ztol: f64,
    },
    Reset,
    SegIndices {
        idx: [u64; 4],
    },
}

/// An `SmCurveIntersections`: iso curve key (`-1` none), direction, parameter and its
/// solutions' second object (`-1` null, `-2` the surface, else a curve key) and loop
/// curve parameter.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IntsIn {
    iso: i64,
    dir: u8,
    param: f64,
    sols: Vec<(i64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Order {
        curves: Vec<CurveIn>,
        tol: f64,
        outer: bool,
    },
    Outer {
        curves: Vec<CurveIn>,
        counts: Vec<usize>,
        orients: Vec<u8>,
        normal: [f64; 3],
        tol: f64,
    },
    Mirror {
        curve: CurveIn,
        side: u8,
        low: f64,
        high: f64,
        tol: f64,
    },
    IsoAt {
        curve: CurveIn,
        side: u8,
        low: f64,
        tol: f64,
    },
    Iso {
        curve: CurveIn,
    },
    IsoSeam {
        surface: SrfIn,
        curve: CurveIn,
    },
    Continuity {
        surface: SrfIn,
    },
    UvGap {
        surface: SrfIn,
        a: [f64; 2],
        b: [f64; 2],
        tol: f64,
        /// The surface points to use instead of evaluating the surface.
        #[serde(default)]
        pts: Option<[[f64; 3]; 2]>,
    },
    Min {
        a: f64,
        b: f64,
    },
    TrimNull {
        surface: SrfIn,
        curves: Vec<CurveIn>,
    },
    Loops {
        store: Vec<CurveIn>,
        counts: Vec<usize>,
        keys: Vec<usize>,
        orients: Vec<u8>,
        /// 0: 3D curves, 1: UV curves, 2: none (invalid).
        mode: u8,
        surface: Option<SrfIn>,
        ops: Vec<LoopOp>,
    },
    BPresent {
        brep: BrepIn,
    },
    BExtremes {
        brep: BrepIn,
        eu: i64,
    },
    BCheckLoop {
        brep: BrepIn,
        l: i64,
        tol: f64,
        list: bool,
        #[serde(default)]
        mirror: Option<(i64, u8)>,
        /// Remove the selected edge use pair's exact UV trim before checking.
        #[serde(default)]
        project: Option<i64>,
    },
    BCheckFace {
        brep: BrepIn,
        f: i64,
        tol: f64,
        list: bool,
        skip: bool,
    },
    BTiny {
        brep: BrepIn,
        of_face: bool,
        index: i64,
        tol: f64,
        #[serde(default)]
        mirror: Option<(i64, u8)>,
    },
    BEnsure {
        brep: BrepIn,
        l: i64,
        #[serde(default)]
        mirror: Option<(i64, u8)>,
    },
    BBackwards {
        brep: BrepIn,
        f: i64,
        #[serde(default)]
        mirror: Option<(i64, u8)>,
    },
    BOrient {
        brep: BrepIn,
        f: i64,
    },
    BOrder {
        brep: BrepIn,
        f: i64,
    },
    /// `kind`: 0 `SplitAtSeams`, 1 `ClearUVTrimCurves`, 2 `CreateUVTrimCurves`.
    BDeferred {
        brep: BrepIn,
        kind: u8,
        f: i64,
    },
}

// ---- encoding -----------------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

/// `c` with a circle replaced by its inline B-spline.
fn resolve(c: &CurveIn) -> Result<CurveIn, HarnessError> {
    let CurveIn::Circle(ci) = c else {
        return Ok(c.clone());
    };
    let frame = Axis2Placement::new(v3(ci.center), v3(ci.x), v3(ci.y));
    let arc = BSplineCurve::create_circle_segment(
        3,
        &frame,
        ci.radius,
        ci.a0,
        ci.a1,
        solidcore::nurbs::b_spline_curve::NurbCircleParam::Quadratic,
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    let cur = arc.gw_nurb().ok_or_else(|| harness("circle without NURB"))?;
    Ok(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: cur.cpts().iter().map(|p| [p.x, p.y, p.z, p.w]).collect(),
        p: cur.degree(),
        u: cur.knots().to_vec(),
    }))
}

fn push_curve(v: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, flag(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, flag(l.inside_out)]);
        }
        CurveIn::Ellipse(e) => {
            v.extend([3.0, 3.0]);
            v.extend_from_slice(&e.c);
            v.extend_from_slice(&e.x);
            v.extend_from_slice(&e.y);
            v.extend([e.domain[0], e.domain[1], e.radii[0], e.radii[1], flag(e.inside_out)]);
        }
        CurveIn::Circle(_) => return Err(harness("unresolved circle")),
    }
    Ok(())
}

fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Bounded { basis, implicit } => {
            v.push(2.0);
            push_surface(v, basis)?;
            v.push(flag(*implicit));
        }
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline(b) => {
            let n = b.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            v.extend([
                0.0,
                flag(b.oob),
                0.0,
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                b.u.len().saturating_sub(1) as f64,
                b.v.len().saturating_sub(1) as f64,
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

fn push_loop_op(v: &mut Vec<f64>, op: &LoopOp) {
    match op {
        LoopOp::StartIndex { i } => v.extend([1.0, *i as f64]),
        LoopOp::StartEnd { i } => v.extend([2.0, *i as f64]),
        LoopOp::Adjacent { key, t } => v.extend([3.0, *key as f64, *t]),
        LoopOp::EnterLeave { a, b } => v.extend([4.0, *a as f64, *b as f64]),
        LoopOp::Sample { key, t } => v.extend([5.0, *key as f64, *t]),
        LoopOp::SameSide {
            key,
            t,
            dir,
            iso,
            ztol,
            dom,
        } => {
            v.extend([6.0, *key as f64, *t, f64::from(*dir), *iso, *ztol]);
            v.extend(dom);
        }
        LoopOp::AppendSelf { i } => v.extend([7.0, *i as f64]),
        LoopOp::InsertInto { new, after } => v.extend([8.0, *new as f64, *after as f64]),
        LoopOp::InsertIso { key, ivl, idx } => {
            v.extend([9.0, *key as f64, ivl[0], ivl[1]]);
            v.extend(idx.map(|x| x as f64));
        }
        LoopOp::Update {
            ints,
            orig,
            new,
            ztol,
        } => {
            v.extend([10.0, ints.len() as f64]);
            for ci in ints {
                v.extend([ci.iso as f64, f64::from(ci.dir), ci.param, ci.sols.len() as f64]);
                for (o, t) in &ci.sols {
                    v.extend([*o as f64, *t]);
                }
            }
            v.extend([*orig as f64, *new as f64, *ztol]);
        }
        LoopOp::Reset => v.push(11.0),
        LoopOp::SegIndices { idx } => {
            v.push(12.0);
            v.extend(idx.map(|x| x as f64));
        }
    }
}

/// The mirror pre-step `k side` (`-1 0` for none).
fn push_mirror(v: &mut Vec<f64>, m: Option<(i64, u8)>) {
    match m {
        Some((k, side)) if k >= 0 => v.extend([k as f64, f64::from(side)]),
        _ => v.extend([-1.0, 0.0]),
    }
}

/// The ffi op, the `.smb` path and the input of an op whose B-rep selectors are already
/// resolved.
fn encode(op: &OpIn) -> Result<(Op, String, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let (code, path) = match op {
        OpIn::Order { curves, tol, outer } => {
            v.push(curves.len() as f64);
            for c in curves {
                push_curve(&mut v, c)?;
            }
            v.extend([*tol, flag(*outer)]);
            (Op::Order, String::new())
        }
        OpIn::Outer {
            curves,
            counts,
            orients,
            normal,
            tol,
        } => {
            v.push(curves.len() as f64);
            for c in curves {
                push_curve(&mut v, c)?;
            }
            v.push(counts.len() as f64);
            v.extend(counts.iter().map(|&c| c as f64));
            v.extend(orients.iter().map(|&o| f64::from(o)));
            v.extend(normal);
            v.push(*tol);
            (Op::Outer, String::new())
        }
        OpIn::Mirror {
            curve,
            side,
            low,
            high,
            tol,
        } => {
            push_curve(&mut v, curve)?;
            v.extend([f64::from(*side), *low, *high, *tol]);
            (Op::Mirror, String::new())
        }
        OpIn::IsoAt {
            curve,
            side,
            low,
            tol,
        } => {
            push_curve(&mut v, curve)?;
            v.extend([f64::from(*side), *low, *tol]);
            (Op::IsoAt, String::new())
        }
        OpIn::Iso { curve } => {
            push_curve(&mut v, curve)?;
            (Op::Iso, String::new())
        }
        OpIn::IsoSeam { surface, curve } => {
            push_surface(&mut v, surface)?;
            push_curve(&mut v, curve)?;
            (Op::IsoSeam, String::new())
        }
        OpIn::Continuity { surface } => {
            push_surface(&mut v, surface)?;
            (Op::Continuity, String::new())
        }
        OpIn::UvGap {
            surface,
            a,
            b,
            tol,
            pts,
        } => {
            push_surface(&mut v, surface)?;
            v.extend(a);
            v.extend(b);
            v.push(*tol);
            v.push(flag(pts.is_some()));
            let [p0, p1] = pts.unwrap_or([[0.0; 3]; 2]);
            v.extend(p0);
            v.extend(p1);
            (Op::UvGap, String::new())
        }
        OpIn::Min { a, b } => {
            v.extend([*a, *b]);
            (Op::Min, String::new())
        }
        OpIn::TrimNull { surface, curves } => {
            push_surface(&mut v, surface)?;
            v.push(curves.len() as f64);
            for c in curves {
                push_curve(&mut v, c)?;
            }
            (Op::TrimNull, String::new())
        }
        OpIn::Loops {
            store,
            counts,
            keys,
            orients,
            mode,
            surface,
            ops,
        } => {
            v.push(store.len() as f64);
            for c in store {
                push_curve(&mut v, c)?;
            }
            v.push(counts.len() as f64);
            v.extend(counts.iter().map(|&c| c as f64));
            v.push(keys.len() as f64);
            v.extend(keys.iter().map(|&k| k as f64));
            v.extend(orients.iter().map(|&o| f64::from(o)));
            v.push(f64::from(*mode));
            v.push(flag(surface.is_some()));
            if let Some(s) = surface {
                push_surface(&mut v, s)?;
            }
            v.push(ops.len() as f64);
            for o in ops {
                push_loop_op(&mut v, o);
            }
            (Op::Loops, String::new())
        }
        OpIn::BPresent { brep } => {
            v = brep_block(brep)?;
            (Op::BPresent, shared::path_of(brep)?)
        }
        OpIn::BExtremes { brep, eu } => {
            v = brep_block(brep)?;
            v.push(*eu as f64);
            (Op::BExtremes, shared::path_of(brep)?)
        }
        OpIn::BCheckLoop {
            brep,
            l,
            tol,
            list,
            mirror,
            project,
        } => {
            v = brep_block(brep)?;
            v.extend([*l as f64, *tol, flag(*list)]);
            push_mirror(&mut v, *mirror);
            v.push(flag(project.is_some()));
            if let Some(eu) = project {
                v.push(*eu as f64);
            }
            (Op::BCheckLoop, shared::path_of(brep)?)
        }
        OpIn::BCheckFace {
            brep,
            f,
            tol,
            list,
            skip,
        } => {
            v = brep_block(brep)?;
            v.extend([*f as f64, *tol, flag(*list), flag(*skip)]);
            (Op::BCheckFace, shared::path_of(brep)?)
        }
        OpIn::BTiny {
            brep,
            of_face,
            index,
            tol,
            mirror,
        } => {
            v = brep_block(brep)?;
            v.extend([flag(*of_face), *index as f64, *tol]);
            push_mirror(&mut v, *mirror);
            (Op::BTiny, shared::path_of(brep)?)
        }
        OpIn::BEnsure { brep, l, mirror } => {
            v = brep_block(brep)?;
            v.push(*l as f64);
            push_mirror(&mut v, *mirror);
            (Op::BEnsure, shared::path_of(brep)?)
        }
        OpIn::BBackwards { brep, f, mirror } => {
            v = brep_block(brep)?;
            v.push(*f as f64);
            push_mirror(&mut v, *mirror);
            (Op::BBackwards, shared::path_of(brep)?)
        }
        OpIn::BOrient { brep, f } => {
            v = brep_block(brep)?;
            v.push(*f as f64);
            (Op::BOrient, shared::path_of(brep)?)
        }
        OpIn::BOrder { brep, f } => {
            v = brep_block(brep)?;
            v.push(*f as f64);
            (Op::BOrder, shared::path_of(brep)?)
        }
        OpIn::BDeferred { brep, kind, f } => {
            v = brep_block(brep)?;
            v.extend([f64::from(*kind), *f as f64]);
            (Op::BDeferred, shared::path_of(brep)?)
        }
    };
    Ok((code, path, v))
}

fn brep_of(op: &OpIn) -> Option<&BrepIn> {
    match op {
        OpIn::BPresent { brep }
        | OpIn::BExtremes { brep, .. }
        | OpIn::BCheckLoop { brep, .. }
        | OpIn::BCheckFace { brep, .. }
        | OpIn::BTiny { brep, .. }
        | OpIn::BEnsure { brep, .. }
        | OpIn::BBackwards { brep, .. }
        | OpIn::BOrient { brep, .. }
        | OpIn::BOrder { brep, .. }
        | OpIn::BDeferred { brep, .. } => Some(brep),
        _ => None,
    }
}

/// The op with its selectors resolved against the B-rep's counts (regions, shells, face
/// uses, faces, loop uses, loops, edge uses, edges, vertex uses, vertices); `None` when
/// the selected kind is empty.
fn resolve_op(op: &OpIn, c: [usize; 10]) -> Option<OpIn> {
    let [_, _, _, n_f, _, n_l, n_eu, _, _, _] = c;
    let req = |s: i64, n: usize| -> Option<i64> {
        let r = shared::sel(s.max(0), n);
        (r >= 0).then_some(r)
    };
    let mut op = op.clone();
    match &mut op {
        OpIn::BExtremes { eu, .. } => *eu = req(*eu, n_eu)?,
        OpIn::BCheckLoop { l, .. } | OpIn::BEnsure { l, .. } => *l = req(*l, n_l)?,
        OpIn::BCheckFace { f, .. }
        | OpIn::BBackwards { f, .. }
        | OpIn::BOrient { f, .. }
        | OpIn::BOrder { f, .. }
        | OpIn::BDeferred { f, .. } => *f = req(*f, n_f)?,
        OpIn::BTiny { of_face, index, .. } => {
            *index = req(*index, if *of_face { n_f } else { n_l })?;
        }
        _ => {}
    }
    Some(op)
}

/// A cached export with its key (the `.smb` path and the block's bits).
type CachedExport = (String, Vec<u64>, Rc<Vec<f64>>);

thread_local! {
    /// The last few exports (both sides of a case export the same B-rep).
    static EXPORTS: RefCell<Vec<CachedExport>> = const { RefCell::new(Vec::new()) };
}

/// The export of a B-rep op's B-rep.
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

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &OpIn) -> Result<Output, HarnessError> {
    let op = match brep_of(i) {
        Some(b) => {
            let ex = export_of(b)?;
            let Some(c) = fixture::counts(&ex) else {
                return Ok(Output::new().values("out", ex.to_vec()));
            };
            match resolve_op(i, c) {
                Some(op) => op,
                None => return Ok(Output::err("no_entity_of_that_kind")),
            }
        }
        None => i.clone(),
    };
    let (code, path, input) = encode(&op)?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
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

fn fail(e: &SolidError) -> Fail {
    match Output::unported(e) {
        Some(o) => Fail::Deferred(Box::new(o)),
        None => Fail::Harness(harness(format!("rust: {e}"))),
    }
}

/// The `SmStatus` of an error (`0` success); deferral and other errors end the case.
fn status<T>(r: &Result<T, SolidError>) -> Result<f64, Fail> {
    match r {
        Ok(_) => Ok(0.0),
        Err(SolidError::Upstream { code, .. }) => Ok(f64::from(*code)),
        Err(e) => Err(fail(e)),
    }
}

fn rust_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match &resolve(c)? {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m = bs.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Line(l) => {
            let opts = BoundedLineOptions {
                scale: l.scale,
                inside_out: l.inside_out,
                ..BoundedLineOptions::default()
            };
            Ok(Curve::from(Line::bounded(
                v3(l.p),
                v3(l.v),
                &Extent1d::from_min_max(l.domain[0], l.domain[1]),
                &opts,
            )))
        }
        CurveIn::Ellipse(e) => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: e.inside_out,
            };
            Ok(Curve::from(
                Ellipse::new(
                    &Kernel::default(),
                    v3(e.c),
                    v3(e.x),
                    v3(e.y),
                    Extent1d::from_min_max(e.domain[0], e.domain[1]),
                    e.radii[0],
                    e.radii[1],
                    &opts,
                )
                .map_err(|e| harness(format!("ellipse: {e}")))?,
            ))
        }
        CurveIn::Circle(_) => Err(harness("unresolved circle")),
    }
}

fn rust_surface(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Bounded { basis, implicit } => Ok(Surface::from(CurveBoundedSurface::new(
            rust_surface(basis)?,
            Vec::new(),
            Vec::new(),
            *implicit,
        )
        .map_err(|e| harness(e.to_string()))?)),
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bspline(b) => {
            let net = b
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = b.oob;
            s.set_b_spline_surface_form(
                BSplineSurfaceForm::from_code(0).ok_or_else(|| harness("form"))?,
            );
            Ok(Surface::from(s))
        }
    }
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn orient_of(code: u8) -> OrientType {
    match code {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}

fn side_of(code: u8) -> SurfParamType {
    if code == 0 {
        SurfParamType::U
    } else {
        SurfParamType::V
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

/// The shim's `tt_put_nurb` of a curve.
fn put_nurb(o: &mut Vec<f64>, c: &Curve) {
    let nurb = match c.kind() {
        CurveKind::BSpline(b) => b.gw_nurb(),
        CurveKind::Line(l) => l.base().gw_nurb(),
        CurveKind::Ellipse(e) => e.base().gw_nurb(),
        _ => None,
    };
    let Some(cur) = nurb else {
        o.push(-1.0);
        return;
    };
    o.extend([
        cur.n() as f64,
        cur.degree() as f64,
        cur.knots().len().saturating_sub(1) as f64,
    ]);
    for p in cur.cpts() {
        o.extend([p.x, p.y, p.z, p.w]);
    }
    o.extend(cur.knots());
}

/// The shim's `tt_put_samples`.
fn put_samples(k: &Kernel, o: &mut Vec<f64>, c: &Curve) {
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=4 {
        match c.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => {
                o.push(0.0);
                put_vec(o, p);
            }
            Err(e) => {
                o.push(match e {
                    SolidError::Upstream { code, .. } => f64::from(code),
                    _ => -1.0,
                });
                o.extend([0.0, 0.0, 0.0]);
            }
        }
    }
}

fn rust_curves(cs: &[CurveIn]) -> Result<Vec<Curve>, HarnessError> {
    cs.iter().map(rust_curve).collect()
}

/// The shim's `tt_put_outer` after a successful `FindOuterLoops`.
fn put_outer(
    o: &mut Vec<f64>,
    ordered: &[usize],
    counts: &[usize],
    orients: &[OrientType],
    normal: Vec3,
    outer: &[usize],
) {
    o.push(ordered.len() as f64);
    o.extend(ordered.iter().map(|&i| i as f64));
    o.push(counts.len() as f64);
    o.extend(counts.iter().map(|&c| c as f64));
    o.extend(orients.iter().map(|&x| orient_code(x)));
    put_vec(o, normal);
    o.push(outer.len() as f64);
    o.extend(outer.iter().map(|&c| c as f64));
}

/// The shim's `tt_find_outer`.
fn find_outer(
    k: &Kernel,
    o: &mut Vec<f64>,
    curves: &[Curve],
    mut ordered: Vec<usize>,
    mut counts: Vec<usize>,
    mut orients: Vec<OrientType>,
    mut normal: Vec3,
    tol: f64,
) -> Result<(), Fail> {
    let r = tt::find_outer_loops(
        k,
        curves,
        &mut ordered,
        &mut counts,
        &mut orients,
        &mut normal,
        tol,
    );
    match r {
        Ok(outer) => {
            o.push(0.0);
            put_outer(o, &ordered, &counts, &orients, normal, &outer);
        }
        Err(SolidError::InvalidInput { .. }) => o.extend([0.0, NULL_DEREF]),
        Err(e) => {
            o.push(status::<()>(&Err(e))?);
        }
    }
    Ok(())
}

fn run_rust_geom(k: &mut Kernel, op: &OpIn) -> Result<Vec<f64>, Fail> {
    let mut o = Vec::new();
    match op {
        OpIn::Order { curves, tol, outer } => {
            let cs = rust_curves(curves)?;
            match tt::order_curves_into_loops(k, &cs, *tol) {
                Ok(r) => {
                    let st = status(&r.status())?;
                    o.push(st);
                    o.push(r.ordered.len() as f64);
                    o.extend(r.ordered.iter().map(|&i| i as f64));
                    o.push(r.loop_counts.len() as f64);
                    o.extend(r.loop_counts.iter().map(|&c| c as f64));
                    o.extend(r.orients.iter().map(|&x| orient_code(x)));
                    put_vec(&mut o, r.plane_point);
                    put_vec(&mut o, r.plane_normal);
                    put_vec(&mut o, r.curves_box.min());
                    put_vec(&mut o, r.curves_box.max());
                    if *outer && st == 0.0 {
                        find_outer(
                            k,
                            &mut o,
                            &cs,
                            r.ordered,
                            r.loop_counts,
                            r.orients,
                            r.plane_normal,
                            *tol,
                        )?;
                    }
                }
                Err(e) => o.push(status::<()>(&Err(e))?),
            }
        }
        OpIn::Outer {
            curves,
            counts,
            orients,
            normal,
            tol,
        } => {
            let cs = rust_curves(curves)?;
            find_outer(
                k,
                &mut o,
                &cs,
                (0..cs.len()).collect(),
                counts.clone(),
                orients.iter().map(|&x| orient_of(x)).collect(),
                v3(*normal),
                *tol,
            )?;
        }
        OpIn::Mirror {
            curve,
            side,
            low,
            high,
            tol,
        } => {
            let mut c = rust_curve(curve)?;
            let r = tt::mirror_curve(k, &mut c, side_of(*side), *low, *high, *tol);
            o.push(status(&r)?);
            o.push(c.dim() as f64);
            put_nurb(&mut o, &c);
        }
        OpIn::IsoAt {
            curve,
            side,
            low,
            tol,
        } => {
            let c = rust_curve(curve)?;
            let r = tt::is_iso_curve_at(&c, side_of(*side), *low, *tol).map_err(|e| fail(&e))?;
            o.push(flag(r));
        }
        OpIn::Iso { curve } => {
            let c = rust_curve(curve)?;
            match tt::is_iso_curve(k, &c).map_err(|e| fail(&e))? {
                Some((side, param)) => o.extend([1.0, f64::from(side as u8), param]),
                None => o.push(0.0),
            }
        }
        OpIn::IsoSeam { surface, curve } => {
            let s = rust_surface(surface)?;
            let c = rust_curve(curve)?;
            o.push(flag(
                tt::is_iso_curve_on_seam(k, &s, &c).map_err(|e| fail(&e))?,
            ));
        }
        OpIn::Continuity { surface } => {
            let s = rust_surface(surface)?;
            let r = tt::surface_continuity_check(k, &s);
            o.push(status(&r)?);
            if let Ok(g1) = r {
                o.push(flag(g1));
            }
        }
        OpIn::UvGap {
            surface,
            a,
            b,
            tol,
            pts,
        } => {
            let s = rust_surface(surface)?;
            let (ua, ub) = (Point2::new(a[0], a[1]), Point2::new(b[0], b[1]));
            let (pa, pb) = match pts {
                Some([p0, p1]) => (Ok(v3(*p0)), Ok(v3(*p1))),
                None => (s.evaluate_point(k, ua), s.evaluate_point(k, ub)),
            };
            o.push(status(&pa)?);
            o.push(status(&pb)?);
            if let (Ok(pa), Ok(pb)) = (pa, pb) {
                put_vec(&mut o, pa);
                put_vec(&mut o, pb);
                let r = tt::is_loop_uv_gap_within_tolerance(
                    k,
                    &s,
                    ua,
                    ub,
                    pa,
                    pb,
                    solidcore::geom::tol::xsect_tol3d(k, *tol),
                )
                .map_err(|e| fail(&e))?;
                o.push(flag(r));
            }
        }
        OpIn::Min { a, b } => o.push(tt::min(*a, *b)),
        OpIn::TrimNull { surface, curves } => {
            let s = rust_surface(surface)?;
            let cs = rust_curves(curves)?;
            let n = cs.len();
            let dom = s.natural_uv_domain();
            let input = tt::TrimInput {
                loop_counts: vec![n],
                curves: cs,
                orientations: vec![OrientType::Same; n],
                loop_points: Vec::new(),
                surface: s,
                uv_domain: dom,
                surface_orientation: OrientType::Same,
            };
            let mut b = Brep::new();
            let mut records = std::collections::BTreeMap::new();
            let mut attributes = solidcore::geom::attribute::AttributeStore::default();
            let key_of = |_| solidcore::geom::object::ObjectRef {
                type_code: 0,
                key: solidcore::geom::attribute::ObjectKey(0),
            };
            let mut env = solidcore::topology::edge::NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            // Without a region the call returns before any notification.
            let r = tt::trim_surface_with_model_space_curves(
                k, &mut b, None, 1e-5, input, true, true, false, &mut env,
            );
            o.push(status(&r)?);
        }
        OpIn::Loops {
            store,
            counts,
            keys,
            orients,
            mode,
            surface,
            ops,
        } => run_rust_loops(
            k,
            store,
            counts,
            keys,
            orients,
            *mode,
            surface.as_ref(),
            ops,
            &mut o,
        )?,
        _ => return Err(Fail::Harness(harness("not a geometry op"))),
    }
    Ok(o)
}

/// The shim's `tt_sample_safe`.
fn sample_safe(l: &CurveLoops, store: &[Curve], key: CurveKey, t: f64) -> Result<bool, Fail> {
    let c = store.get(key.0).ok_or_else(|| harness("key"))?;
    let d = c.natural_interval();
    let tol = d.length() / 1000.0;
    if d.contains_value(t, -tol) {
        return Ok(true);
    }
    let adj = match l.find_adjacent_loop_curve(store, key, t) {
        Ok(a) => a,
        Err(e) if e.is_unported() => return Err(fail(&e)),
        Err(_) => 0,
    };
    Ok(adj < l.curves.len() && adj < l.orients.len())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn run_rust_loops(
    k: &Kernel,
    store_in: &[CurveIn],
    counts: &[usize],
    keys: &[usize],
    orients: &[u8],
    mode: u8,
    surface: Option<&SrfIn>,
    ops: &[LoopOp],
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let mut store = rust_curves(store_in)?;
    let ns = store.len();
    let srf = surface.map(rust_surface).transpose()?;
    let ks: Vec<CurveKey> = keys.iter().map(|&x| CurveKey(x)).collect();
    let os: Vec<OrientType> = orients.iter().map(|&x| orient_of(x)).collect();
    let mut l = CurveLoops::from_loops(
        counts,
        (mode == 1).then_some(ks.as_slice()),
        (mode == 0).then_some(ks.as_slice()),
        &os,
    );
    let u = |x: u64| usize::try_from(x).unwrap_or(usize::MAX);
    for op in ops {
        match op {
            LoopOp::StartIndex { i } => {
                o.push(1.0);
                let r = l.loop_start_index(u(*i));
                o.push(status(&r)?);
                o.push(r.map_or(0.0, |s| s as f64));
            }
            LoopOp::StartEnd { i } => {
                o.push(2.0);
                let r = l.loop_start_end(u(*i));
                o.push(status(&r)?);
                if let Ok((s, e)) = r {
                    o.extend([s as f64, e as f64]);
                }
            }
            LoopOp::Adjacent { key, t } => {
                o.push(3.0);
                let r = l.find_adjacent_loop_curve(&store, CurveKey(*key), *t);
                o.push(status(&r)?);
                if let Ok(a) = r {
                    o.push(a as f64);
                }
            }
            LoopOp::EnterLeave { a, b } => {
                o.push(4.0);
                let r = l.find_entering_and_leaving(u(*a), u(*b));
                o.push(status(&r)?);
                if let Ok((e, lv)) = r {
                    o.extend([e as f64, lv as f64]);
                }
            }
            LoopOp::Sample { key, t } => {
                o.push(5.0);
                if !sample_safe(&l, &store, CurveKey(*key), *t)? {
                    o.push(NULL_DEREF);
                    continue;
                }
                let (p1, p2) = l
                    .sample_nearby_points(k, &store, CurveKey(*key), *t)
                    .map_err(|e| fail(&e))?;
                o.push(0.0);
                put_vec(o, p1);
                put_vec(o, p2);
            }
            LoopOp::SameSide {
                key,
                t,
                dir,
                iso,
                ztol,
                dom,
            } => {
                o.push(6.0);
                let Some(s) = srf.as_ref() else {
                    o.push(NULL_DEREF);
                    continue;
                };
                if !sample_safe(&l, &store, CurveKey(*key), *t)? {
                    o.push(NULL_DEREF);
                    continue;
                }
                let ci = CurveIntersections::new(None, side_of(*dir), *iso);
                let d = Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]);
                let r = l
                    .is_same_side_intersection(
                        k,
                        &store,
                        CurveKey(*key),
                        *t,
                        &ci,
                        s,
                        &d,
                        solidcore::geom::tol::zone_tol3d(k, *ztol),
                    )
                    .map_err(|e| fail(&e))?;
                o.push(flag(r));
            }
            LoopOp::AppendSelf { i } => {
                o.push(7.0);
                let other = l.clone();
                let r = l.append_loop(&other, u(*i));
                o.push(status(&r)?);
            }
            LoopOp::InsertInto { new, after } => {
                o.push(8.0);
                let r = l.insert_into_loops(CurveKey(*new), CurveKey(*after));
                o.push(status(&r)?);
            }
            LoopOp::InsertIso { key, ivl, idx } => {
                o.push(9.0);
                let mut seg = IsoXSectSegment::new(CurveKey(*key));
                seg.interval = Extent1d::from_raw(ivl[0], ivl[1]);
                seg.start_enter = u(idx[0]);
                seg.start_leave = u(idx[1]);
                seg.end_enter = u(idx[2]);
                seg.end_leave = u(idx[3]);
                let r = l.insert_iso_xsect_segment(k, &mut store, &mut seg);
                o.push(status(&r)?);
                o.extend([seg.interval.min(), seg.interval.max()]);
            }
            LoopOp::Update {
                ints,
                orig,
                new,
                ztol,
            } => {
                o.push(10.0);
                let mut cis: Vec<CurveIntersections> = Vec::new();
                for ci in ints {
                    let iso = usize::try_from(ci.iso).ok().map(CurveKey);
                    let mut c = CurveIntersections::new(iso, side_of(ci.dir), ci.param);
                    for &(obj, t) in &ci.sols {
                        let mut s = Solution::<LoopObject>::new();
                        s.num_objects = 2;
                        s.num_variables = 2;
                        s.objects[0] = iso.map(LoopObject::Curve);
                        s.objects[1] = match obj {
                            -1 => None,
                            -2 => Some(LoopObject::Other(ObjectKind::Surface)),
                            x => Some(LoopObject::Curve(CurveKey(
                                usize::try_from(x).unwrap_or(usize::MAX),
                            ))),
                        };
                        s.start.parameters[0] = 0.0;
                        s.start.parameters[1] = t;
                        c.solutions.add(&s);
                    }
                    cis.push(c);
                }
                let r = l.update_solution_objects(
                    k,
                    &store,
                    &mut cis,
                    CurveKey(*orig),
                    CurveKey(*new),
                    solidcore::geom::tol::zone_tol3d(k, *ztol),
                );
                o.push(status(&r)?);
                for c in &cis {
                    o.push(c.solutions.len() as f64);
                    for s in c.solutions.as_slice() {
                        o.push(match s.objects[1] {
                            None => -1.0,
                            Some(LoopObject::Other(_)) => -2.0,
                            Some(LoopObject::Curve(key)) => key.0 as f64,
                        });
                    }
                }
            }
            LoopOp::Reset => {
                o.push(11.0);
                l.reset();
                o.push(0.0);
            }
            LoopOp::SegIndices { idx } => {
                o.push(12.0);
                let mut seg = IsoXSectSegment::new(CurveKey(0));
                seg.start_enter = u(idx[0]);
                seg.start_leave = u(idx[1]);
                seg.end_enter = u(idx[2]);
                seg.end_leave = u(idx[3]);
                o.extend([seg.min_index() as f64, seg.max_index() as f64]);
            }
        }
    }
    // final state
    o.push(flag(l.is_valid()));
    o.push(l.num_loops() as f64);
    o.extend(l.loop_counts.iter().map(|&c| c as f64));
    o.push(l.curves.len() as f64);
    o.extend(l.curves.iter().map(|c| c.0 as f64));
    o.push(l.orients.len() as f64);
    o.extend(l.orients.iter().map(|&x| orient_code(x)));
    o.push(store.len().saturating_sub(ns) as f64);
    for c in store.iter().skip(ns) {
        put_samples(k, o, c);
    }
    Ok(())
}

// ---- Rust B-rep ops ---------------------------------------------------------------------

fn lid(i: i64) -> LoopId {
    LoopId::from_index(u32::try_from(i).unwrap_or(u32::MAX))
}

fn fid(i: i64) -> FaceId {
    FaceId::from_index(u32::try_from(i).unwrap_or(u32::MAX))
}

fn eid(i: i64) -> EdgeuseId {
    EdgeuseId::from_index(u32::try_from(i).unwrap_or(u32::MAX))
}

/// The shim's `tt_ok_loop`: the loop has its loop use.
fn ok_loop(b: &Brep, l: LoopId) -> Result<bool, Fail> {
    Ok(b.loop_(l).map_err(|e| fail(&e))?.loopuse.is_some())
}

/// The shim's `tt_ok_extremes`.
fn ok_extremes(b: &Brep, eu: EdgeuseId) -> Result<bool, Fail> {
    let f = |e: SolidError| fail(&e);
    if eu1::uv_trim_curve(b, eu).map_err(f)?.0.is_some() {
        return Ok(true);
    }
    let m = eu1::mate(b, eu).map_err(f)?;
    Ok(eu1::vertexuse(b, eu).map_err(f)?.is_some()
        && match m {
            Some(m) => eu1::vertexuse(b, m).map_err(f)?.is_some(),
            None => false,
        })
}

fn loop_eus(b: &Brep, l: LoopId) -> Result<Vec<EdgeuseId>, Fail> {
    let (lu, _) = lu1::loop_loopuses(b, l).map_err(|e| fail(&e))?;
    lu1::edgeuses(b, lu, None, None).map_err(|e| fail(&e))
}

fn ok_loop_extremes(b: &Brep, l: LoopId) -> Result<bool, Fail> {
    for eu in loop_eus(b, l)? {
        if !ok_extremes(b, eu)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The shim's `tt_ok_face_loops`: every loop has its loop use, and a face with loops
/// has a surface and UV trim curves everywhere.
fn ok_face_loops(b: &Brep, f: FaceId) -> Result<bool, Fail> {
    let loops = face_loops(b, f)?;
    for &l in &loops {
        if !ok_loop(b, l)? {
            return Ok(false);
        }
    }
    Ok(loops.is_empty()
        || (b.face(f).map_err(|e| fail(&e))?.surface.is_some()
            && tt::face_uv_curves_present(b, f).map_err(|e| fail(&e))?))
}

/// The surface of a loop's face (a copy), `None` without face or surface.
fn loop_surface(b: &Brep, l: LoopId) -> Result<Option<Surface>, Fail> {
    let f = lu1::loop_face(b, l).map_err(|e| fail(&e))?;
    Ok(match f {
        Some(f) => b.face(f).map_err(|e| fail(&e))?.surface.clone(),
        None => None,
    })
}

fn face_loops(b: &Brep, f: FaceId) -> Result<Vec<LoopId>, Fail> {
    fp2::loops(b, f, false, None).map_err(|e| fail(&e))
}

/// The shim's `tt_mirror_pre`.
fn mirror_pre(
    k: &Kernel,
    b: &mut Brep,
    l: Option<LoopId>,
    m: Option<(i64, u8)>,
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    let f = |e: SolidError| fail(&e);
    let (Some((sel, side)), Some(l)) = (m.filter(|x| x.0 >= 0), l) else {
        o.push(-1.0);
        return Ok(());
    };
    if !ok_loop(b, l)? {
        o.push(-1.0);
        return Ok(());
    }
    let eus = loop_eus(b, l)?;
    let srf = loop_surface(b, l)?;
    let (Some(srf), false) = (srf, eus.is_empty()) else {
        o.push(-1.0);
        return Ok(());
    };
    let pos = usize::try_from(sel).unwrap_or(0) % eus.len();
    let eu = *eus.get(pos).ok_or_else(|| harness("eu"))?;
    let (c, slot) = eu1::uv_trim_curve(b, eu).map_err(f)?;
    let (Some(c), Some(slot)) = (c, slot) else {
        o.push(-1.0);
        return Ok(());
    };
    if !matches!(c.kind(), CurveKind::BSpline(_) | CurveKind::Line(_)) {
        o.push(-1.0);
        return Ok(());
    }
    let d = srf.natural_uv_domain();
    let side = side_of(side);
    let (low, high) = if side == SurfParamType::U {
        (d.min().x, d.max().x)
    } else {
        (d.min().y, d.max().y)
    };
    let c = b
        .edgeuse_mut(slot)
        .map_err(f)?
        .uv_trim_curve
        .as_mut()
        .ok_or_else(|| harness("curve"))?;
    let r = tt::mirror_curve(k, c, side, low, high, 0.0);
    o.push(status(&r)?);
    Ok(())
}

/// The shim's `tt_put_reports`.
fn put_reports(o: &mut Vec<f64>, a: &AssertArray) {
    o.push(a.len() as f64);
    for r in a.reports() {
        let owner = r.owner.map_or(-1.0, |k| (k.0 & 0xffff_ffff) as f64);
        o.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            r.owner_type as f64,
            owner,
            r.tol3d,
            r.value,
        ]);
    }
    o.push(flag(a.last_add()));
}

/// The shim's `tt_put_uv_curves`.
fn put_uv_curves(o: &mut Vec<f64>, b: &Brep) {
    o.push(b.edgeuses().len() as f64);
    for (_, r) in b.edgeuses().iter() {
        match r.uv_trim_curve.as_ref() {
            None => o.push(-1.0),
            Some(c) => {
                o.push(c.dim() as f64);
                put_nurb(o, c);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn run_rust_brep(k: &mut Kernel, b: &mut Brep, op: &OpIn) -> Result<Vec<f64>, Fail> {
    let mut o = vec![0.0];
    let f = |e: SolidError| fail(&e);
    match op {
        OpIn::BPresent { .. } => {
            let n_l = b.loops().len();
            for l in 0..n_l {
                let l = lid(l as i64);
                o.push(if ok_loop(b, l)? {
                    flag(tt::loop_uv_curves_present(b, l).map_err(f)?)
                } else {
                    NULL_DEREF
                });
            }
            for fi in 0..b.faces().len() {
                let fc = fid(fi as i64);
                let mut ok = true;
                for l in face_loops(b, fc)? {
                    ok = ok && ok_loop(b, l)?;
                }
                o.push(if ok {
                    flag(tt::face_uv_curves_present(b, fc).map_err(f)?)
                } else {
                    NULL_DEREF
                });
            }
        }
        OpIn::BExtremes { eu, .. } => {
            let eu = eid(*eu);
            if !ok_extremes(b, eu)? {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = tt::edge_uv_extremes(k, b, eu);
            o.push(status(&r)?);
            if let Ok(x) = r {
                o.extend([x.start.x, x.start.y, x.end.x, x.end.y]);
                match (x.start_weight, x.end_weight) {
                    (Some(sw), Some(ew)) => o.extend([1.0, sw, ew]),
                    _ => o.push(0.0),
                }
            }
        }
        OpIn::BCheckLoop {
            l,
            tol,
            list,
            mirror,
            project,
            ..
        } => {
            let l = lid(*l);
            mirror_pre(k, b, Some(l), *mirror, &mut o)?;
            if let Some(selector) = project.filter(|_| ok_loop(b, l).unwrap_or(false)) {
                let eus = loop_eus(b, l)?;
                if let Some(&eu) = eus.get(selector as usize % eus.len().max(1)) {
                    let mate = eu1::mate(b, eu).map_err(f)?;
                    b.edgeuse_mut(eu).map_err(f)?.uv_trim_curve = None;
                    if let Some(mate) = mate {
                        b.edgeuse_mut(mate).map_err(f)?.uv_trim_curve = None;
                    }
                }
            }
            let srf = if ok_loop(b, l)? {
                loop_surface(b, l)?
            } else {
                None
            };
            let Some(srf) = srf.filter(|_| ok_loop_extremes(b, l).unwrap_or(false)) else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            let mut a = AssertArray::new();
            let r = tt::check_loop(k, b, &srf, l, *tol, list.then_some(&mut a));
            o.push(status(&r)?);
            if let Ok(c) = r {
                o.push(f64::from(c));
            }
            put_reports(&mut o, &a);
        }
        OpIn::BCheckFace {
            f: face,
            tol,
            list,
            skip,
            ..
        } => {
            let fc = fid(*face);
            for l in face_loops(b, fc)? {
                if !ok_loop(b, l)? {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
            }
            // the shim's guards of the CheckLoop / ComputeLoopOrientation walk
            let loops = face_loops(b, fc)?;
            if !loops.is_empty() && tt::face_uv_curves_present(b, fc).map_err(|e| fail(&e))? {
                let mut ok = b.face(fc).map_err(|e| fail(&e))?.surface.is_some();
                for &l in &loops {
                    if !ok || *skip {
                        break;
                    }
                    ok = ok_loop_extremes(b, l)?;
                }
                if !ok {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
            }
            let mut a = AssertArray::new();
            let r = tt::check_face(k, b, fc, *tol, list.then_some(&mut a), *skip);
            o.push(status(&r)?);
            put_reports(&mut o, &a);
        }
        OpIn::BTiny {
            of_face,
            index,
            tol,
            mirror,
            ..
        } => {
            let loops = if *of_face {
                face_loops(b, fid(*index))?
            } else {
                vec![lid(*index)]
            };
            mirror_pre(k, b, loops.first().copied(), *mirror, &mut o)?;
            // the shim's dry run of the gaps at edge uses without UV trim curve
            for &l in &loops {
                if !ok_loop(b, l)? || !ok_loop_extremes(b, l)? {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
                let eus = loop_eus(b, l)?;
                let n = eus.len();
                for kk in 0..n {
                    let cur = *eus.get(kk).ok_or_else(|| harness("eu"))?;
                    let prev = *eus
                        .get(if kk == 0 { n - 1 } else { kk - 1 })
                        .ok_or_else(|| harness("eu"))?;
                    if eu1::uv_trim_curve(b, cur).map_err(f)?.0.is_some()
                        && eu1::uv_trim_curve(b, prev).map_err(f)?.0.is_some()
                    {
                        continue;
                    }
                    let pe = match tt::edge_uv_ends(k, b, prev) {
                        Ok(x) => x.1,
                        Err(e) if e.is_unported() => return Err(fail(&e)),
                        Err(_) => continue,
                    };
                    let cs = match tt::edge_uv_ends(k, b, cur) {
                        Ok(x) => x.0,
                        Err(e) if e.is_unported() => return Err(fail(&e)),
                        Err(_) => continue,
                    };
                    if pe.distance(cs) < 2.0 * tol {
                        o.push(NULL_DEREF);
                        return Ok(o);
                    }
                }
            }
            let r = if *of_face {
                tt::fix_face_tiny_parameter_space_gaps(k, b, fid(*index), *tol)
            } else {
                tt::fix_loop_tiny_parameter_space_gaps(k, b, lid(*index), *tol)
            };
            o.push(status(&r)?);
            if let Ok(m) = r {
                o.push(flag(m));
            }
            put_uv_curves(&mut o, b);
        }
        OpIn::BEnsure { l, mirror, .. } => {
            let l = lid(*l);
            mirror_pre(k, b, Some(l), *mirror, &mut o)?;
            let srf = if ok_loop(b, l)? {
                loop_surface(b, l)?
            } else {
                None
            };
            let Some(srf) = srf else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            let mut eus = loop_eus(b, l)?;
            for &eu in &eus {
                if eu1::uv_trim_curve(b, eu).map_err(f)?.0.is_none() {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
            }
            let r = tt::ensure_last_edgeuse_uv_correct(k, b, &srf, &mut eus);
            o.push(status(&r)?);
            o.push(flag(r.unwrap_or(false)));
            o.push(eus.len() as f64);
            o.extend(eus.iter().map(|e| f64::from(e.index())));
        }
        OpIn::BBackwards {
            f: face, mirror, ..
        } => {
            let fc = fid(*face);
            let first = face_loops(b, fc)?.first().copied();
            mirror_pre(k, b, first, *mirror, &mut o)?;
            let Some(srf) = b.face(fc).map_err(f)?.surface.clone() else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            let bounds = srf.natural_uv_domain();
            let closed = srf
                .is_closed(k, &bounds, SurfParamType::U, None, false)
                .map_err(f)?
                .0
                || srf
                    .is_closed(k, &bounds, SurfParamType::V, None, false)
                    .map_err(f)?
                    .0;
            if closed {
                let loops = face_loops(b, fc)?;
                let Some(&l0) = loops.first() else {
                    o.push(NULL_DEREF);
                    return Ok(o);
                };
                if !ok_loop(b, l0)? || !ok_loop_extremes(b, l0)? {
                    o.push(NULL_DEREF);
                    return Ok(o);
                }
                for eu in loop_eus(b, l0)? {
                    if eu1::uv_trim_curve(b, eu).map_err(f)?.0.is_none() {
                        o.push(NULL_DEREF);
                        return Ok(o);
                    }
                }
            }
            let r = tt::fix_backwards_outer_loop_on_seam(k, b, fc);
            o.push(status(&r)?);
            if let Ok(m) = r {
                o.push(flag(m));
            }
            put_uv_curves(&mut o, b);
        }
        OpIn::BOrient { f: face, .. } => {
            if !ok_face_loops(b, fid(*face))? {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = tt::fix_face_loop_orientations(k, b, fid(*face));
            o.push(status(&r)?);
            o.push(r.map_or(0.0, |n| n as f64));
        }
        OpIn::BOrder { f: face, .. } => {
            if !ok_face_loops(b, fid(*face))? {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let r = tt::fix_trim_loop_order(k, b, fid(*face));
            o.push(status(&r)?);
        }
        OpIn::BDeferred { kind, f: face, .. } => {
            let fc = fid(*face);
            let r = match kind {
                0 => tt::split_at_seams(k, b, fc).map(|_| ()),
                1 => tt::clear_uv_trim_curves(b, fc),
                _ => tt::create_uv_trim_curves(k, b, fc),
            };
            o.push(status(&r)?);
            // ClearUVTrimCurves, CreateUVTrimCurves: every edge use's UV trim curve after
            // the call
            put_uv_curves(&mut o, b);
        }
        _ => return Err(Fail::Harness(harness("not a B-rep op"))),
    }
    Ok(o)
}

fn run_rust(i: &OpIn) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let r = match brep_of(i) {
        Some(brep) => {
            let ex = export_of(brep)?;
            let Some(c) = fixture::counts(&ex) else {
                return Ok(Output::new().values("out", ex.to_vec()));
            };
            let Some(op) = resolve_op(i, c) else {
                return Ok(Output::err("no_entity_of_that_kind"));
            };
            let (mut b, _) = shared::import(&ex)?;
            run_rust_brep(&mut k, &mut b, &op)
        }
        None => run_rust_geom(&mut k, i),
    };
    match r {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -------------------------------------------------------------------------

fn clamped_knots(rng: &mut Rng, p: usize, n: usize, a: f64, b: f64) -> Vec<f64> {
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

/// A random 2D UV B-spline in `[0, 1]^2`: free, or an iso curve at `u = c` / `v = c`
/// (sometimes off by less or more than a tolerance).
fn gen_uv_curve(rng: &mut Rng, iso: Option<(u8, f64)>) -> CurveIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let rational = rng.chance(0.2);
    let wobble = *rng.choose(&[0.0, 0.0, 1e-7, 1e-5, 1e-3]).unwrap_or(&0.0);
    let cpts = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let (mut x, mut y) = (rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0));
            if let Some((side, c)) = iso {
                if side == 0 {
                    x = c + rng.uniform(-wobble, wobble);
                    y = t;
                } else {
                    y = c + rng.uniform(-wobble, wobble);
                    x = t;
                }
            }
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            if rational {
                [x * w, y * w, NOZ, w]
            } else {
                [x, y, NOZ, w]
            }
        })
        .collect();
    let (a, b) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-3.0, 3.0);
        (a, a + rng.uniform(0.5, 5.0))
    };
    CurveIn::Bspline(BSplineIn {
        dim: 2,
        oob: false,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    })
}

/// A random surface: a plane, a B-spline (sometimes closed in u or v, pinched at a row,
/// with a C0 kink or rational), scaled; sometimes bounded by no curves
/// (`SmCurveBoundedSurface`, not an `SmBSplineSurface`).
fn gen_surface(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.08) {
        let basis = gen_surface_plain(rng);
        return SrfIn::Bounded {
            basis: Box::new(basis),
            implicit: rng.chance(0.5),
        };
    }
    gen_surface_plain(rng)
}

/// [`gen_surface`] without bounded surfaces.
fn gen_surface_plain(rng: &mut Rng) -> SrfIn {
    let closed = if rng.chance(0.35) {
        Some(u8::from(rng.chance(0.5)))
    } else {
        None
    };
    gen_surface_closed(rng, closed)
}

/// [`gen_surface`] with the closure direction chosen (`Some(0)` closed in u, `Some(1)`
/// in v; never a plane then).
fn gen_surface_closed(rng: &mut Rng, closed: Option<u8>) -> SrfIn {
    let s = gen_surface_net(rng, closed.is_some());
    match (closed, s) {
        (Some(1), SrfIn::Bspline(b)) => {
            let m = b.net.first().map_or(0, Vec::len);
            let net = (0..m)
                .map(|j| b.net.iter().filter_map(|r| r.get(j).copied()).collect())
                .collect();
            SrfIn::Bspline(BssIn {
                oob: b.oob,
                net,
                p: b.q,
                q: b.p,
                u: b.v,
                v: b.u,
            })
        }
        (_, s) => s,
    }
}

/// A plane or a B-spline net, closed in u (a tube around z) when `closed_u`.
fn gen_surface_net(rng: &mut Rng, closed_u: bool) -> SrfIn {
    let scale = *rng
        .choose(&[1e-3, 1.0, 1.0, 1.0, 10.0, 1e3])
        .unwrap_or(&1.0);
    if !closed_u && rng.chance(0.12) {
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + 1 + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let pinch = rng.chance(0.2);
    let rational = rng.chance(0.15);
    let mut net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (x, y, z) = if closed_u {
                        // a tube around z: rows go round, the last equals the first
                        let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
                        let r = 1.0 + 0.3 * j as f64;
                        (r * a.cos(), r * a.sin(), j as f64)
                    } else {
                        (i as f64, j as f64, rng.uniform(-0.5, 0.5))
                    };
                    let (x, y) = if pinch && j == 0 { (0.0, 0.0) } else { (x, y) };
                    [x * scale, y * scale, z * scale, NOW]
                })
                .collect()
        })
        .collect();
    if closed_u {
        if let (Some(first), Some(last)) = (net.first().cloned(), net.last_mut()) {
            *last = first;
        }
    }
    if rational {
        for row in &mut net {
            for c in row.iter_mut() {
                let w = 1.0 + 0.1 * (c[0] + c[1]).abs().min(5.0) / scale.clamp(1e-9, 1.0);
                *c = [c[0] * w, c[1] * w, c[2] * w, w];
            }
        }
    }
    let mut u = clamped_knots(rng, p, n, 0.0, 1.0);
    if p >= 2 && n > p + 1 && rng.chance(0.25) {
        // a C0 kink: an interior knot of multiplicity p
        let mid = 0.5;
        u = vec![0.0; p + 1];
        let extra = n - p;
        for i in 0..extra {
            u.push(if i < p { mid } else { 0.75 });
        }
        u.extend(vec![1.0; p + 1]);
        if u.len() != n + p + 2 {
            u = clamped_knots(rng, p, n, 0.0, 1.0);
        }
    }
    SrfIn::Bspline(BssIn {
        oob: rng.chance(0.1),
        net,
        p,
        q,
        u,
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

/// An orthonormal-ish plane frame (origin, two axes) at a random scale.
fn gen_frame(rng: &mut Rng) -> ([f64; 3], [f64; 3], [f64; 3], f64) {
    let scale = *rng
        .choose(&[1e-3, 1.0, 1.0, 1.0, 50.0, 1e4])
        .unwrap_or(&1.0);
    let o = rng.point3(-5.0, 5.0).map(|c| c * scale);
    let (e1, e2) = match rng.below(4) {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        _ => {
            let a = rng.uniform(0.0, std::f64::consts::TAU);
            let b = rng.uniform(-1.2, 1.2);
            let e1 = [a.cos(), a.sin(), 0.0];
            let e2 = [-a.sin() * b.cos(), a.cos() * b.cos(), b.sin()];
            (e1, e2)
        }
    };
    (o, e1, e2, scale)
}

fn at(o: [f64; 3], e1: [f64; 3], e2: [f64; 3], s: f64, x: f64, y: f64) -> [f64; 3] {
    [
        o[0] + s * (x * e1[0] + y * e2[0]),
        o[1] + s * (x * e1[1] + y * e2[1]),
        o[2] + s * (x * e1[2] + y * e2[2]),
    ]
}

/// A segment from `a` to `b`: a line or a degree-1/2 B-spline, either direction.
fn segment(rng: &mut Rng, a: [f64; 3], b: [f64; 3]) -> CurveIn {
    let (a, b) = if rng.chance(0.4) { (b, a) } else { (a, b) };
    let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if rng.chance(0.35) && len > 0.0 {
        let d0 = *rng.choose(&[0.0, 0.0, 2.5]).unwrap_or(&0.0);
        return CurveIn::Line(LineIn {
            p: a,
            v,
            domain: [d0, d0 + len],
            scale: 1.0,
            inside_out: false,
        });
    }
    let mid = [
        (a[0] + b[0]) / 2.0,
        (a[1] + b[1]) / 2.0,
        (a[2] + b[2]) / 2.0,
    ];
    let quad = rng.chance(0.4);
    let (cpts, p, u) = if quad {
        (
            vec![
                [a[0], a[1], a[2], NOW],
                [mid[0], mid[1], mid[2], NOW],
                [b[0], b[1], b[2], NOW],
            ],
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        )
    } else {
        (
            vec![[a[0], a[1], a[2], NOW], [b[0], b[1], b[2], NOW]],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
    };
    CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts,
        p,
        u,
    })
}

/// The curves of a polygon loop (or a full ellipse) centred at `(cx, cy)` with radius
/// `r` on the frame.
fn gen_loop(
    rng: &mut Rng,
    f: ([f64; 3], [f64; 3], [f64; 3], f64),
    cx: f64,
    cy: f64,
    r: f64,
) -> Vec<CurveIn> {
    let (o, e1, e2, s) = f;
    if rng.chance(0.2) {
        let c = at(o, e1, e2, s, cx, cy);
        let d0 = *rng.choose(&[0.0, 0.0, 90.0, -45.0]).unwrap_or(&0.0);
        return vec![CurveIn::Ellipse(EllipseIn {
            c,
            x: e1,
            y: e2,
            domain: [d0, d0 + 360.0],
            radii: [r * s, r * s * rng.uniform(0.4, 1.0)],
            inside_out: rng.chance(0.2),
        })];
    }
    let n = 2 + rng.below(5) as usize;
    let a0 = rng.uniform(0.0, 1.0);
    let pts: Vec<[f64; 3]> = (0..n)
        .map(|i| {
            let a = a0 + std::f64::consts::TAU * i as f64 / n as f64;
            let rr = r * rng.uniform(0.7, 1.0);
            at(o, e1, e2, s, cx + rr * a.cos(), cy + rr * a.sin())
        })
        .collect();
    let mut cs = Vec::new();
    for (i, (&a, &b)) in pts.iter().zip(pts.iter().cycle().skip(1)).enumerate() {
        if n == 2 {
            // two arcs: bow the segments apart
            let mid = [
                (a[0] + b[0]) / 2.0,
                (a[1] + b[1]) / 2.0,
                (a[2] + b[2]) / 2.0,
            ];
            let off = if i == 0 { 0.4 } else { -0.4 };
            let bow = [
                mid[0] + off * s * r * e2[0],
                mid[1] + off * s * r * e2[1],
                mid[2] + off * s * r * e2[2],
            ];
            cs.push(CurveIn::Bspline(BSplineIn {
                dim: 3,
                oob: false,
                cpts: vec![
                    [a[0], a[1], a[2], NOW],
                    [bow[0], bow[1], bow[2], NOW],
                    [b[0], b[1], b[2], NOW],
                ],
                p: 2,
                u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            }));
        } else {
            cs.push(segment(rng, a, b));
        }
    }
    cs
}

/// Planar loops (an outer one with holes, or side by side), shuffled; sometimes broken
/// (a curve removed, one lifted off the plane).
fn gen_order(rng: &mut Rng) -> OpIn {
    let f = gen_frame(rng);
    let mut curves = gen_loop(rng, f, 0.0, 0.0, 10.0);
    match rng.below(4) {
        0 => {}
        1 => {
            let holes = 1 + rng.below(3);
            for h in 0..holes {
                let cx = -5.0 + 5.0 * h as f64;
                let cy = rng.uniform(-1.0, 1.0);
                curves.extend(gen_loop(rng, f, cx, cy, 1.5));
            }
        }
        2 => curves.extend(gen_loop(rng, f, 30.0, 0.0, 5.0)),
        _ => {
            curves.extend(gen_loop(rng, f, 30.0, 0.0, 5.0));
            curves.extend(gen_loop(rng, f, 30.0, 0.0, 2.0));
        }
    }
    // shuffle
    for i in (1..curves.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        curves.swap(i, j);
    }
    if rng.chance(0.08) && curves.len() > 1 {
        curves.pop();
    }
    if rng.chance(0.05) {
        if let Some(CurveIn::Bspline(b)) = curves.first_mut() {
            if let Some(c) = b.cpts.last_mut() {
                c[2] += f.3 * 3.0;
            }
        }
    }
    let tol = f.3 * *rng.choose(&[1e-6, 1e-4, 1e-3, 0.0]).unwrap_or(&1e-6);
    OpIn::Order {
        curves,
        tol,
        outer: rng.chance(0.85),
    }
}

/// Loops given directly to `FindOuterLoops` with random orientations.
fn gen_outer(rng: &mut Rng) -> OpIn {
    let f = gen_frame(rng);
    let mut curves = Vec::new();
    let mut counts = Vec::new();
    let loops = 1 + rng.below(4);
    for li in 0..loops {
        let (cx, r) = match li {
            0 => (0.0, 10.0),
            1 => (-4.0, 2.0),
            2 => (30.0, 4.0),
            _ => (4.0, 2.0),
        };
        let l = gen_loop(rng, f, cx, 0.0, r);
        counts.push(l.len());
        curves.extend(l);
    }
    // per-curve orientation along its loop is unknown here: random
    let orients = curves
        .iter()
        .map(|_| *rng.choose(&[1u8, 1, 2, 0]).unwrap_or(&1))
        .collect();
    let mut n = [
        f.1[1] * f.2[2] - f.1[2] * f.2[1],
        f.1[2] * f.2[0] - f.1[0] * f.2[2],
        f.1[0] * f.2[1] - f.1[1] * f.2[0],
    ];
    if rng.chance(0.3) {
        n = n.map(|c| c * rng.uniform(0.2, 3.0));
    }
    if rng.chance(0.2) {
        n = n.map(|c| -c);
    }
    OpIn::Outer {
        curves,
        counts,
        orients,
        normal: n,
        tol: f.3 * 1e-6,
    }
}

fn gen_iso_curve(rng: &mut Rng) -> (CurveIn, u8, f64) {
    let side = u8::from(rng.chance(0.5));
    let c = *rng.choose(&[0.0, 1.0, 0.5, 0.25, 0.0, 1.0]).unwrap_or(&0.0);
    let curve = if rng.chance(0.75) {
        gen_uv_curve(rng, Some((side, c)))
    } else {
        gen_uv_curve(rng, None)
    };
    (curve, side, c)
}

fn gen_loops_op(rng: &mut Rng) -> OpIn {
    let f = gen_frame(rng);
    let mut store = gen_loop(rng, f, 0.0, 0.0, 10.0);
    let first = store.len();
    if rng.chance(0.6) {
        store.extend(gen_loop(rng, f, 30.0, 0.0, 3.0));
    }
    let second = store.len() - first;
    // an iso line across the first loop
    let a = at(f.0, f.1, f.2, f.3, -12.0, 0.5);
    let b = at(f.0, f.1, f.2, f.3, 12.0, 0.5);
    store.push(CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: vec![[a[0], a[1], a[2], NOW], [b[0], b[1], b[2], NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }));
    let ns = store.len();
    let mut counts = vec![first];
    if second > 0 {
        counts.push(second);
    }
    let nk = first + second;
    let keys: Vec<usize> = (0..nk).collect();
    let orients = (0..nk)
        .map(|_| *rng.choose(&[1u8, 1, 2]).unwrap_or(&1))
        .collect();
    let mode = *rng.choose(&[0u8, 0, 0, 1, 2]).unwrap_or(&0);
    let (counts, keys, orients) = if mode == 2 {
        (Vec::new(), Vec::new(), Vec::new())
    } else {
        (counts, keys, orients)
    };
    let surface = rng.chance(0.4).then(|| SrfIn::Plane {
        o: f.0,
        n: [
            f.1[1] * f.2[2] - f.1[2] * f.2[1],
            f.1[2] * f.2[0] - f.1[0] * f.2[2],
            f.1[0] * f.2[1] - f.1[1] * f.2[0],
        ],
    });
    let nops = 1 + rng.below(8);
    let mut ops = Vec::new();
    let key = |rng: &mut Rng| rng.below(ns as u64) as usize;
    let small = |rng: &mut Rng| rng.below(nk as u64 + 3);
    for _ in 0..nops {
        let t = *rng.choose(&[0.0, 1.0, 0.5, 0.0005, 0.9999]).unwrap_or(&0.5);
        let op = match rng.below(12) {
            0 => LoopOp::StartIndex { i: rng.below(4) },
            1 => LoopOp::StartEnd { i: small(rng) },
            2 => LoopOp::Adjacent { key: key(rng), t },
            3 => LoopOp::EnterLeave {
                a: small(rng),
                b: small(rng),
            },
            4 => LoopOp::Sample { key: key(rng), t },
            5 => LoopOp::SameSide {
                key: key(rng),
                t,
                dir: u8::from(rng.chance(0.5)),
                iso: rng.uniform(-1.0, 1.0),
                ztol: 1e-5,
                dom: [-1e6, -1e6, 1e6, 1e6],
            },
            6 => LoopOp::AppendSelf { i: rng.below(3) },
            7 => LoopOp::InsertInto {
                new: ns - 1,
                after: key(rng),
            },
            8 => {
                let se = rng.below(nk.max(1) as u64);
                let ee = rng.below(nk.max(1) as u64);
                let jitter = |rng: &mut Rng, x: u64| {
                    if rng.chance(0.8) {
                        x + 1
                    } else {
                        rng.below(nk as u64 + 4)
                    }
                };
                let sl = jitter(rng, se);
                let el = jitter(rng, ee);
                LoopOp::InsertIso {
                    key: ns - 1,
                    ivl: [rng.uniform(0.0, 0.4), rng.uniform(0.6, 1.0)],
                    idx: [se, sl, ee, el],
                }
            }
            9 => {
                let nint = 1 + rng.below(2) as usize;
                let ints = (0..nint)
                    .map(|_| IntsIn {
                        iso: if rng.chance(0.8) { (ns - 1) as i64 } else { -1 },
                        dir: u8::from(rng.chance(0.5)),
                        param: rng.uniform(-1.0, 1.0),
                        sols: (0..=rng.below(3))
                            .map(|_| {
                                let o = match rng.below(5) {
                                    0 => -1,
                                    1 if surface.is_some() => -2,
                                    _ => key(rng) as i64,
                                };
                                (o, rng.uniform(-0.2, 1.2) * 12.0)
                            })
                            .collect(),
                    })
                    .collect();
                LoopOp::Update {
                    ints,
                    orig: key(rng),
                    new: key(rng),
                    ztol: *rng.choose(&[1e-5, 1e-2, 0.0]).unwrap_or(&1e-5),
                }
            }
            10 => LoopOp::Reset,
            _ => LoopOp::SegIndices {
                idx: [
                    rng.below(10),
                    rng.below(10),
                    if rng.chance(0.3) { 9999 } else { rng.below(10) },
                    rng.below(10),
                ],
            },
        };
        ops.push(op);
    }
    OpIn::Loops {
        store,
        counts,
        keys,
        orients,
        mode,
        surface,
        ops,
    }
}

fn gen_brep(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.7) {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"cone.smb")).to_string(),
            rebuild: rng.chance(0.2),
        }
    } else {
        shared::gen_brep(rng)
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_mirror(rng: &mut Rng) -> Option<(i64, u8)> {
    rng.chance(0.5)
        .then(|| (rng.below(16) as i64, u8::from(rng.chance(0.5))))
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-5, 1e-3, 1e-2, 0.1, 0.0, 1e-8]).unwrap_or(&1e-5)
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(40) {
        0..=7 => gen_order(rng),
        8..=10 => gen_outer(rng),
        11 | 12 => {
            let (curve, side, c) = gen_iso_curve(rng);
            OpIn::Mirror {
                curve,
                side,
                low: c,
                high: *rng.choose(&[1.0, 0.75, 2.0]).unwrap_or(&1.0),
                tol: 1e-3,
            }
        }
        13 | 14 => {
            let (curve, side, c) = gen_iso_curve(rng);
            OpIn::IsoAt {
                curve,
                side: if rng.chance(0.8) { side } else { 1 - side },
                low: c,
                tol: *rng.choose(&[1e-6, 1e-4, 1e-2]).unwrap_or(&1e-6),
            }
        }
        15 | 16 => OpIn::Iso {
            curve: gen_iso_curve(rng).0,
        },
        17 | 18 => {
            let (curve, side, _) = gen_iso_curve(rng);
            let surface = if rng.chance(0.7) {
                let dir = if rng.chance(0.8) { side } else { 1 - side };
                gen_surface_closed(rng, Some(dir))
            } else {
                gen_surface(rng)
            };
            OpIn::IsoSeam { surface, curve }
        }
        19 | 20 => OpIn::Continuity {
            surface: gen_surface(rng),
        },
        21..=23 => {
            let surface = gen_surface(rng);
            let a = [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)];
            let b = match rng.below(4) {
                0 => [a[0], rng.uniform(0.0, 1.0)],
                1 => [rng.uniform(0.0, 1.0), a[1]],
                2 => [a[0] + rng.uniform(-1e-3, 1e-3), 0.0],
                _ => [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
            };
            let b = [b[0].clamp(0.0, 1.0), b[1].clamp(0.0, 1.0)];
            let a = if rng.chance(0.3) { [a[0], 0.0] } else { a };
            let pts = matches!(surface, SrfIn::Bounded { .. }).then(|| {
                let p0 = rng.point3(-1.0, 1.0);
                let p1 = rng.point3(-1.0, 1.0);
                [p0, p1]
            });
            OpIn::UvGap {
                surface,
                a,
                b,
                tol: *rng.choose(&[1e-5, 1e-2, 0.5, 2.0]).unwrap_or(&1e-5),
                pts,
            }
        }
        24 => {
            let a = rng.uniform(-10.0, 10.0);
            let r = rng.uniform(-10.0, 10.0);
            OpIn::Min {
                a,
                b: *rng.choose(&[r, 0.0, -0.0, a]).unwrap_or(&0.0),
            }
        }
        25 => {
            let surface = gen_surface(rng);
            let f = gen_frame(rng);
            OpIn::TrimNull {
                surface,
                curves: gen_loop(rng, f, 0.0, 0.0, 1.0),
            }
        }
        26..=28 => gen_loops_op(rng),
        29 => OpIn::BPresent { brep: gen_brep(rng) },
        30 => OpIn::BExtremes {
            brep: gen_brep(rng),
            eu: gen_sel(rng),
        },
        31 | 32 => OpIn::BCheckLoop {
            brep: gen_brep(rng),
            l: gen_sel(rng),
            tol: gen_tol(rng),
            list: rng.chance(0.7),
            project: None,
            mirror: gen_mirror(rng),
        },
        33 => OpIn::BCheckFace {
            brep: gen_brep(rng),
            f: gen_sel(rng),
            tol: gen_tol(rng),
            list: rng.chance(0.7),
            skip: rng.chance(0.2),
        },
        34 | 35 => OpIn::BTiny {
            brep: gen_brep(rng),
            of_face: rng.chance(0.5),
            index: gen_sel(rng),
            tol: *rng.choose(&[1e-6, 1e-4, 1e-2, 0.5]).unwrap_or(&1e-6),
            mirror: gen_mirror(rng),
        },
        36 => OpIn::BEnsure {
            brep: gen_brep(rng),
            l: gen_sel(rng),
            mirror: gen_mirror(rng),
        },
        37 => OpIn::BBackwards {
            brep: gen_brep(rng),
            f: gen_sel(rng),
            mirror: gen_mirror(rng),
        },
        38 => OpIn::BOrient {
            brep: gen_brep(rng),
            f: gen_sel(rng),
        },
        _ => {
            if rng.chance(0.5) {
                OpIn::BOrder {
                    brep: gen_brep(rng),
                    f: gen_sel(rng),
                }
            } else {
                OpIn::BDeferred {
                    brep: gen_brep(rng),
                    kind: rng.below(3) as u8,
                    f: gen_sel(rng),
                }
            }
        }
    }
}

/// Whether the C++ wrapper accepts the op (generated cases that it rejects are drawn
/// again).
fn accepted(op: &OpIn) -> bool {
    if brep_of(op).is_some() {
        return true;
    }
    match encode(op) {
        Ok((code, path, input)) => ffi::eval(code, &path, &input).is_ok(),
        Err(_) => false,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.trimming_tools"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..20 {
            let op = gen_op(rng);
            if accepted(&op) {
                return serde_json::to_value(op).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}
