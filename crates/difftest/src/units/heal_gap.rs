//! Runner for unit `heal.gap`. C++: `solidcore_ffi::units::heal_gap`. Rust:
//! `solidcore::heal::gap`.
//!
//! Ops (see the FFI module for the layouts):
//!
//! * `gf`: a gap function (curve or surface to point, curve or surface) over real
//!   `SmBSplineCurve`, `SmHermiteCurve`, `SmLine`, `SmEllipse`, `SmCircle` and
//!   `SmCrvOnSurf` curves and `SmBSplineSurface`, `SmPlane` and `SmCurveBoundedSurface`
//!   surfaces, optionally re-counted (`SetSampleCnt`) and re-toleranced, then
//!   `GetMaxGapSample` or `GetSampleSet` (to the first big gap with `stop`); compares
//!   the status, counts, sample state, extreme indices, the all-normal flag, the other
//!   parameter ranges and every sample.
//! * `eval`: one `Evaluate` with optional drop guesses.
//! * `add_unique`: `sm_AddUniqueGetOrderedIndex`.
//! * `sample`: `SmGapSample::SetCanonical` (tolerance point, in-tolerance flag), with
//!   undefined and 2D positions.
//! * `gap_array`: `SmGapArray::Add` / `Append` / `ReSet` and the gap comparisons.
//! * `topo`: a gap record (`SmVertexEdgeGap` ... `SmEdgeFaceGap`) set on a real
//!   `SmBrep` built as the `topology.vertexuse` runner builds it (an upstream `.smb`
//!   file, a wire edge, a face from a surface, the `my_test_topology` network), whose
//!   export the Rust side imports with that runner's importer; compares the gap, its
//!   accessors, `GetGapEndPoints` and `CacheGap`. Parameters are given as fractions of
//!   the selected edge's interval or face's surface domain (or `SM_UNDEF_DOUBLE`), so
//!   both sides read them from the same graph.
//!
//! Deferred cases: the surface drops of the curve/surface and surface/surface gap
//! functions (`SmSurface::DropPoint`, unit `surface.surface_p5`) and the creation of a
//! missing UV trim curve (`SmEdgeuse::GetOrCreateUVTrimCurve`, unit
//! `topology.edgeuse_p3`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::xsect_tol3d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::heal::gap::{
    add_unique_get_ordered_index, EdgeEdgeGap, EdgeFaceGap, EdgeFaceTrimCurveGap, Gap, GapArray,
    GapDropType, GapFunction, GapSample, SamplesState, VertexEdgeGap, VertexFaceGap,
    VertexFaceTrimCurveGap,
};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::vertexuse as vu1;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, VertexId, VertexuseId};
use solidcore::SolidError;
use solidcore_ffi::units::heal_gap::{self as ffi, Op, UNDEF};
use solidcore_ffi::units::topology_vertexuse::counts;

use super::topology_vertexuse::{gen_brep, import, path_of, push_curve, push_surface, sel, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// "Upstream would dereference null".
const NULL_DEREF: f64 = -2.0;

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn one() -> f64 {
    1.0
}

// ---- inputs -------------------------------------------------------------------------

/// A B-spline curve's NURB: control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NurbIn {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default)]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline {
        dim: u64,
        #[serde(default)]
        oob: bool,
        nurb: NurbIn,
    },
    Hermite {
        dim: u64,
        p1: [f64; 3],
        d1: [f64; 3],
        p2: [f64; 3],
        d2: [f64; 3],
    },
    Line {
        p: [f64; 3],
        v: [f64; 3],
        domain: [f64; 2],
        #[serde(default = "one")]
        scale: f64,
        #[serde(default)]
        inside_out: bool,
    },
    Ellipse {
        dim: u64,
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        domain: [f64; 2],
        radii: [f64; 2],
        #[serde(default)]
        inside_out: bool,
    },
    Circle {
        dim: u64,
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        domain: [f64; 2],
        radius: f64,
        #[serde(default)]
        inside_out: bool,
    },
    CrvOnSurf {
        uv: NurbIn,
        surface: BssIn,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SurfIn {
    Bspline(BssIn),
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default = "unit_scale")]
        scale: [f64; 2],
        dom: [f64; 4],
    },
    CurveBounded(BssIn),
}

fn unit_scale() -> [f64; 2] {
    [1.0, 1.0]
}

/// What a gap function samples from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ThisIn {
    Curve { curve: CurveIn, ivl: [f64; 2] },
    Surface { surface: SurfIn, dom: [f64; 4] },
}

/// What a gap function samples to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OtherIn {
    Point([f64; 3]),
    Curve(CurveIn),
    Surface(SurfIn),
}

/// A gap function (its kind follows from the shapes).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GfIn {
    /// The raw `SmXSectTol3d` value.
    tol: f64,
    this: ThisIn,
    other: OtherIn,
    #[serde(default = "twenty")]
    cnt_u: usize,
    #[serde(default = "twenty")]
    cnt_v: usize,
}

fn twenty() -> usize {
    20
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Gf {
        gf: GfIn,
        #[serde(default)]
        set_cnt: Option<[usize; 2]>,
        #[serde(default)]
        set_tol: Option<f64>,
        #[serde(default)]
        stop: bool,
        /// 0 `GetMaxGapSample`, 1 `GetSampleSet`.
        #[serde(default)]
        mode: u8,
    },
    Eval {
        gf: GfIn,
        u: f64,
        #[serde(default)]
        v: f64,
        #[serde(default)]
        gu: Option<f64>,
        #[serde(default)]
        gv: Option<f64>,
    },
    AddUnique {
        a: Vec<f64>,
        value: f64,
    },
    Sample {
        this: [f64; 3],
        tu: f64,
        tv: f64,
        other: [f64; 3],
        ou: f64,
        ov: f64,
        drop: u8,
        tol: f64,
    },
    GapArray {
        a: Vec<f64>,
        b: Vec<f64>,
    },
    Topo {
        brep: BrepIn,
        /// 0 vertex/edge, 1 vertex/face trim curve, 2 vertex/face, 3 edge/edge, 4
        /// edge/face trim curve, 5 edge/face.
        kind: u8,
        /// Vertex use (kinds 0-2) or edge use selector, modulo the count; `-1` null.
        sel0: i64,
        /// The second edge use of an edge/edge gap.
        #[serde(default)]
        sel1: i64,
        /// Fractions of the selected edge use's edge interval (or of the second edge's
        /// for `f1` of an edge/edge gap); `None` = `SM_UNDEF_DOUBLE`.
        f0: Option<f64>,
        #[serde(default)]
        f1: Option<f64>,
        /// Fractions of the face's surface domain; `None` = no UV point.
        #[serde(default)]
        uv: Option<[f64; 2]>,
    },
}

// ---- C++ encoding ---------------------------------------------------------------------

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn push_nurb(o: &mut Vec<f64>, n: &NurbIn) -> Result<(), HarnessError> {
    let k = n.cpts.len().checked_sub(1).ok_or_else(|| harness("no points"))?;
    let m = n.u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
    o.extend([k as f64, n.p as f64, m as f64]);
    for p in &n.cpts {
        o.extend(p);
    }
    o.extend(&n.u);
    Ok(())
}

fn push_bss(o: &mut Vec<f64>, b: &BssIn) -> Result<(), HarnessError> {
    let n = b.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let m = b
        .net
        .first()
        .and_then(|r| r.len().checked_sub(1))
        .ok_or_else(|| harness("empty net row"))?;
    if b.net.iter().any(|r| r.len() != m + 1) {
        return Err(harness("ragged net"));
    }
    let r = b.u.len().checked_sub(1).ok_or_else(|| harness("no u knots"))?;
    let s = b.v.len().checked_sub(1).ok_or_else(|| harness("no v knots"))?;
    o.extend([
        flag(b.oob),
        f64::from(b.form),
        n as f64,
        m as f64,
        b.p as f64,
        b.q as f64,
        r as f64,
        s as f64,
    ]);
    for row in &b.net {
        for p in row {
            o.extend(p);
        }
    }
    o.extend(&b.u);
    o.extend(&b.v);
    Ok(())
}

fn push_curve_in(o: &mut Vec<f64>, c: &CurveIn) -> Result<(), HarnessError> {
    match c {
        CurveIn::Bspline { dim, oob, nurb } => {
            o.extend([0.0, *dim as f64, flag(*oob)]);
            push_nurb(o, nurb)?;
        }
        CurveIn::Hermite { dim, p1, d1, p2, d2 } => {
            o.extend([1.0, *dim as f64]);
            for v in [p1, d1, p2, d2] {
                o.extend(v);
            }
        }
        CurveIn::Line {
            p,
            v,
            domain,
            scale,
            inside_out,
        } => {
            o.extend([2.0, 3.0]);
            o.extend(p);
            o.extend(v);
            o.extend(domain);
            o.extend([*scale, flag(*inside_out)]);
        }
        CurveIn::Ellipse {
            dim,
            c,
            x,
            y,
            domain,
            radii,
            inside_out,
        } => {
            o.extend([3.0, *dim as f64]);
            o.extend(c);
            o.extend(x);
            o.extend(y);
            o.extend(domain);
            o.extend(radii);
            o.push(flag(*inside_out));
        }
        CurveIn::Circle {
            dim,
            c,
            x,
            y,
            domain,
            radius,
            inside_out,
        } => {
            o.extend([4.0, *dim as f64]);
            o.extend(c);
            o.extend(x);
            o.extend(y);
            o.extend(domain);
            o.extend([*radius, flag(*inside_out)]);
        }
        CurveIn::CrvOnSurf { uv, surface } => {
            o.extend([5.0, 2.0]);
            push_nurb(o, uv)?;
            push_bss(o, surface)?;
        }
    }
    Ok(())
}

fn push_surf_in(o: &mut Vec<f64>, s: &SurfIn) -> Result<(), HarnessError> {
    match s {
        SurfIn::Bspline(b) => {
            o.push(0.0);
            push_bss(o, b)
        }
        SurfIn::Plane { o: org, x, y, scale, dom } => {
            o.push(1.0);
            o.extend(org);
            o.extend(x);
            o.extend(y);
            o.extend(scale);
            o.extend(dom);
            Ok(())
        }
        SurfIn::CurveBounded(b) => {
            o.push(2.0);
            push_bss(o, b)
        }
    }
}

fn kind_of(g: &GfIn) -> u8 {
    let base = match g.this {
        ThisIn::Curve { .. } => 0,
        ThisIn::Surface { .. } => 3,
    };
    base + match g.other {
        OtherIn::Point(_) => 0,
        OtherIn::Curve(_) => 1,
        OtherIn::Surface(_) => 2,
    }
}

fn push_gf(o: &mut Vec<f64>, g: &GfIn) -> Result<(), HarnessError> {
    o.extend([f64::from(kind_of(g)), g.tol]);
    match &g.this {
        ThisIn::Curve { curve, ivl } => {
            push_curve_in(o, curve)?;
            o.extend(ivl);
        }
        ThisIn::Surface { surface, dom } => {
            push_surf_in(o, surface)?;
            o.extend(dom);
        }
    }
    match &g.other {
        OtherIn::Point(p) => o.extend(p),
        OtherIn::Curve(c) => push_curve_in(o, c)?,
        OtherIn::Surface(s) => push_surf_in(o, s)?,
    }
    o.extend([g.cnt_u as f64, g.cnt_v as f64]);
    Ok(())
}

// ---- Rust geometry ---------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn nl_curve(n: &NurbIn) -> Result<NlCurve, HarnessError> {
    let pw = n.cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
    NlCurve::new(pw, n.p, n.u.clone()).map_err(|e| harness(format!("curve input: {e}")))
}

fn bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let net = b
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    let mut s = BSplineSurface::from_nurb(nl, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

fn curve_of(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    Ok(match c {
        CurveIn::Bspline { dim, oob, nurb } => {
            let mut b = BSplineCurve::from_nurb(nl_curve(nurb)?, *dim, false);
            b.set_out_of_bounds_enabled(*oob);
            Curve::from(b)
        }
        CurveIn::Hermite { dim, p1, d1, p2, d2 } => Curve::from(HermiteCurve::new(
            v3(*p1),
            v3(*d1),
            v3(*p2),
            v3(*d2),
            *dim,
        )),
        CurveIn::Line {
            p,
            v,
            domain,
            scale,
            inside_out,
        } => {
            let opts = BoundedLineOptions {
                scale: *scale,
                inside_out: *inside_out,
                ..BoundedLineOptions::default()
            };
            Curve::from(Line::bounded(
                v3(*p),
                v3(*v),
                &Extent1d::from_min_max(domain[0], domain[1]),
                &opts,
            ))
        }
        CurveIn::Ellipse {
            dim,
            c,
            x,
            y,
            domain,
            radii,
            inside_out,
        } => {
            let opts = EllipseOptions {
                dim: *dim,
                nurb: None,
                inside_out: *inside_out,
            };
            Curve::from(
                Ellipse::new(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(domain[0], domain[1]),
                    radii[0],
                    radii[1],
                    &opts,
                )
                .map_err(|e| harness(format!("ellipse: {e}")))?,
            )
        }
        CurveIn::Circle {
            dim,
            c,
            x,
            y,
            domain,
            radius,
            inside_out,
        } => {
            let opts = EllipseOptions {
                dim: *dim,
                nurb: None,
                inside_out: *inside_out,
            };
            Curve::from(
                Ellipse::new_circle(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(domain[0], domain[1]),
                    *radius,
                    &opts,
                )
                .map_err(|e| harness(format!("circle: {e}")))?,
            )
        }
        CurveIn::CrvOnSurf { uv, surface } => {
            let c = Curve::from(BSplineCurve::from_nurb(nl_curve(uv)?, 2, false));
            Curve::from(
                CrvOnSurf::new(k, c, bss_of(surface)?, None, 3)
                    .map_err(|e| harness(format!("crv on surf: {e}")))?,
            )
        }
    })
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn surface_of(s: &SurfIn) -> Result<Surface, HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b) => Surface::from(bss_of(b)?),
        SurfIn::Plane { o, x, y, scale, dom } => Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &dom_of(*dom),
        )),
        SurfIn::CurveBounded(b) => Surface::from(CurveBoundedSurface::new(
            Surface::from(bss_of(b)?),
            Vec::new(),
            Vec::new(),
            true,
        )),
    })
}

/// The shapes of a gap function, owned.
enum Shape {
    Curve(Curve),
    Surface(Surface),
    Point(Vec3),
}

struct Shapes {
    this: Shape,
    other: Shape,
}

fn shapes(k: &Kernel, g: &GfIn) -> Result<Shapes, HarnessError> {
    let this = match &g.this {
        ThisIn::Curve { curve, .. } => Shape::Curve(curve_of(k, curve)?),
        ThisIn::Surface { surface, .. } => Shape::Surface(surface_of(surface)?),
    };
    let other = match &g.other {
        OtherIn::Point(p) => Shape::Point(v3(*p)),
        OtherIn::Curve(c) => Shape::Curve(curve_of(k, c)?),
        OtherIn::Surface(s) => Shape::Surface(surface_of(s)?),
    };
    Ok(Shapes { this, other })
}

fn function<'a>(k: &Kernel, g: &GfIn, s: &'a Shapes) -> Result<GapFunction<'a>, HarnessError> {
    let tol = xsect_tol3d(k, g.tol);
    let ivl = match &g.this {
        ThisIn::Curve { ivl, .. } => Extent1d::from_min_max(ivl[0], ivl[1]),
        ThisIn::Surface { .. } => Extent1d::new(),
    };
    let dom = match &g.this {
        ThisIn::Surface { dom, .. } => dom_of(*dom),
        ThisIn::Curve { .. } => dom_of([0.0, 0.0, 1.0, 1.0]),
    };
    Ok(match (&s.this, &s.other) {
        (Shape::Curve(c), Shape::Point(p)) => GapFunction::crv_pt(tol, c, &ivl, *p, g.cnt_u),
        (Shape::Curve(c), Shape::Curve(o)) => GapFunction::crv_crv(tol, c, &ivl, o, g.cnt_u),
        (Shape::Curve(c), Shape::Surface(o)) => GapFunction::crv_srf(tol, c, &ivl, o, g.cnt_u),
        (Shape::Surface(c), Shape::Point(p)) => GapFunction::srf_pt(tol, c, &dom, *p, g.cnt_u),
        (Shape::Surface(c), Shape::Curve(o)) => GapFunction::srf_crv(tol, c, &dom, o, g.cnt_u),
        (Shape::Surface(c), Shape::Surface(o)) => {
            GapFunction::srf_srf(tol, c, &dom, o, g.cnt_u, g.cnt_v)
        }
        (Shape::Point(_), _) => return Err(harness("a gap function from a point")),
    })
}

// ---- outputs ---------------------------------------------------------------------------

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_sample(o: &mut Vec<f64>, s: &GapSample) {
    o.push(f64::from(s.drop_type().code()));
    push3(o, s.this_pos());
    push3(o, s.other_pos());
    push3(o, s.tol_point());
    o.extend([s.this_param().x, s.this_param().y]);
    o.extend([s.other_param().x, s.other_param().y]);
    o.push(s.xsect_tol_3d());
    o.push(flag(s.is_in_tol()));
}

fn put_gf(o: &mut Vec<f64>, gf: &GapFunction<'_>) {
    o.push(gf.sample_cnt_u() as f64);
    o.push(gf.sample_cnt_v() as f64);
    o.push(match gf.samples_state() {
        SamplesState::NotSet => 0.0,
        SamplesState::Set => 1.0,
        SamplesState::UntilBigGap => 2.0,
    });
    o.push(gf.max_sample_index() as f64);
    o.push(gf.min_sample_index() as f64);
    o.push(match gf.all_normal_gaps() {
        Some(true) => 1.0,
        Some(false) => 0.0,
        None => 2.0,
    });
    o.extend([gf.other_ivl_u().min(), gf.other_ivl_u().max()]);
    o.extend([gf.other_ivl_v().min(), gf.other_ivl_v().max()]);
    o.push(gf.samples().len() as f64);
    for s in gf.samples() {
        put_sample(o, s);
    }
}

fn state(v: Vec<f64>) -> Output {
    Output::new().values("state", v)
}

/// A Rust failure: deferred for `Unported`, else the harness fails.
fn rust_fail(e: &SolidError) -> Result<Output, HarnessError> {
    Output::unported(e).ok_or_else(|| harness(format!("rust: {e}")))
}

// ---- topology --------------------------------------------------------------------------

/// The topology op's resolved arguments `kind sel0 sel1 t0 t1 u v has_uv`, read from the
/// imported graph (the same on both sides).
fn topo_args(
    b: &Brep,
    c: [usize; 10],
    kind: u8,
    sel0: i64,
    sel1: i64,
    f0: Option<f64>,
    f1: Option<f64>,
    uv: Option<[f64; 2]>,
) -> Result<Vec<f64>, HarnessError> {
    let n = if kind <= 2 { c[8] } else { c[6] };
    let s0 = sel(sel0, n);
    let s1 = if kind == 3 { sel(sel1, c[6]) } else { -1 };
    let err = |e: SolidError| harness(format!("topo args: {e}"));
    // the edge use whose edge the parameters refer to
    let eu_of = |s: i64| -> Result<Option<EdgeuseId>, HarnessError> {
        if s < 0 {
            return Ok(None);
        }
        Ok(if kind <= 2 {
            vu1::edgeuse(b, VertexuseId::from_index(s as u32)).map_err(err)?
        } else {
            Some(EdgeuseId::from_index(s as u32))
        })
    };
    let ivl_of = |eu: Option<EdgeuseId>| -> Result<Extent1d, HarnessError> {
        let e = match eu {
            Some(eu) => eu1::edge(b, eu).map_err(err)?,
            None => None,
        };
        Ok(match e {
            Some(e) => b.edge(e).map_err(err)?.interval,
            None => Extent1d::from_min_max(0.0, 1.0),
        })
    };
    let at = |f: Option<f64>, ivl: Extent1d| f.map_or(UNDEF, |f| ivl.evaluate(f));
    let eu0 = eu_of(s0)?;
    let t0 = at(f0, ivl_of(eu0)?);
    let t1 = match kind {
        3 => at(f1, ivl_of(eu_of(s1)?)?),
        4 => at(f1, ivl_of(eu0)?),
        _ => UNDEF,
    };
    // the face's surface domain
    let face = if s0 < 0 {
        None
    } else if kind <= 2 {
        match vu1::faceuse(b, VertexuseId::from_index(s0 as u32)) {
            Ok(Some(fu)) => b.faceuse(fu).map_err(err)?.face,
            _ => None,
        }
    } else {
        eu1::face(b, EdgeuseId::from_index(s0 as u32)).ok().flatten()
    };
    let dom = match face {
        Some(f) => match &b.face(f).map_err(err)?.surface {
            Some(s) => s.natural_uv_domain(),
            None => dom_of([0.0, 0.0, 1.0, 1.0]),
        },
        None => dom_of([0.0, 0.0, 1.0, 1.0]),
    };
    let (u, v, has_uv) = match uv {
        Some([fu, fv]) => (
            dom.u_interval().evaluate(fu),
            dom.v_interval().evaluate(fv),
            true,
        ),
        None => (UNDEF, UNDEF, false),
    };
    Ok(vec![
        f64::from(kind),
        s0 as f64,
        s1 as f64,
        t0,
        t1,
        u,
        v,
        flag(has_uv),
    ])
}

fn brep_block(brep: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match brep {
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
        BrepIn::Network => v.push(3.0),
    }
    // validate_only, init_gaps
    v.extend([0.0, 0.0]);
    Ok(v)
}

/// The graph of a topology case: its path, block, export, counts and import.
fn topo_graph(brep: &BrepIn) -> Result<(String, Vec<f64>, Vec<f64>), HarnessError> {
    let path = path_of(brep)?;
    let block = brep_block(brep)?;
    let ex = ffi::export(&path, &block).map_err(|e| harness(format!("c++ export: {e}")))?;
    Ok((path, block, ex))
}

fn idx(i: Option<u32>) -> f64 {
    i.map_or(-1.0, f64::from)
}

/// The Rust topology op; `Err(SolidError)` for the gap's own failures.
fn topo_rust(k: &Kernel, b: &mut Brep, args: &[f64]) -> Result<Vec<f64>, SolidError> {
    let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
    let kind = a(0) as u8;
    let s0 = a(1);
    let s1 = a(2);
    let (t0, t1) = (a(3), a(4));
    let uv = (a(7) != 0.0).then(|| Point2::new(a(5), a(6)));
    let vu = (s0 >= 0.0 && kind <= 2).then(|| VertexuseId::from_index(s0 as u32));
    let eu0 = (s0 >= 0.0 && kind >= 3).then(|| EdgeuseId::from_index(s0 as u32));
    let eu1 = (s1 >= 0.0).then(|| EdgeuseId::from_index(s1 as u32));
    let gap: Gap = match kind {
        0 => VertexEdgeGap::with(k, b, vu, t0)?.into(),
        1 => VertexFaceTrimCurveGap::with(k, b, vu, t0)?.into(),
        2 => VertexFaceGap::with(k, b, vu, uv)?.into(),
        3 => EdgeEdgeGap::with(k, b, eu0, eu1, t0, t1)?.into(),
        4 => EdgeFaceTrimCurveGap::with(k, b, eu0, t0, t1)?.into(),
        _ => EdgeFaceGap::with(k, b, eu0, t0, uv)?.into(),
    };
    let mut o = vec![
        0.0,
        gap.length(),
        flag(gap.is_init()),
        gap.edge_t(0),
        gap.edge_t(1),
        gap.uv_trim_curve_t(),
    ];
    let fuv = gap.face_uv(k, b)?;
    o.extend([fuv.x, fuv.y]);
    o.push(idx(gap.vertex(b)?.map(VertexId::index)));
    o.push(idx(gap.vertexuse().map(VertexuseId::index)));
    o.push(idx(gap.edge(b, 0)?.map(EdgeId::index)));
    o.push(idx(gap.edge(b, 1)?.map(EdgeId::index)));
    o.push(idx(gap.edgeuse(b, 0)?.map(EdgeuseId::index)));
    o.push(idx(gap.edgeuse(b, 1)?.map(EdgeuseId::index)));
    o.push(idx(gap.face(b)?.map(FaceId::index)));
    o.push(gap.type_code() as f64);
    let e = gap.end_points(k, b)?;
    o.push(if e.count == 2 { 0.0 } else { 1001.0 });
    push3(&mut o, e.point1);
    push3(&mut o, e.point2);
    // CacheGap on a copy (its length is overwritten)
    let mut c = gap;
    c.cache_gap(k, b)?;
    o.push(c.length());
    Ok(o)
}

// ---- run ---------------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let call = |op: Op, input: &[f64]| {
        ffi::eval(op, input).map_err(|e| harness(format!("c++: {e}")))
    };
    let mut o = Vec::new();
    match i {
        Inputs::Gf {
            gf,
            set_cnt,
            set_tol,
            stop,
            mode,
        } => {
            push_gf(&mut o, gf)?;
            let [su, sv] = set_cnt.unwrap_or([1, 1]);
            o.extend([flag(set_cnt.is_some()), su as f64, sv as f64]);
            o.extend([flag(set_tol.is_some()), set_tol.unwrap_or(0.0)]);
            o.extend([flag(*stop), f64::from(*mode)]);
            Ok(state(call(Op::Gf, &o)?))
        }
        Inputs::Eval { gf, u, v, gu, gv } => {
            push_gf(&mut o, gf)?;
            o.extend([*u, *v]);
            o.extend([flag(gu.is_some()), gu.unwrap_or(0.0)]);
            o.extend([flag(gv.is_some()), gv.unwrap_or(0.0)]);
            Ok(state(call(Op::Eval, &o)?))
        }
        Inputs::AddUnique { a, value } => {
            o.push(a.len() as f64);
            o.extend(a);
            o.push(*value);
            Ok(state(call(Op::AddUnique, &o)?))
        }
        Inputs::Sample {
            this,
            tu,
            tv,
            other,
            ou,
            ov,
            drop,
            tol,
        } => {
            o.extend(this);
            o.extend([*tu, *tv]);
            o.extend(other);
            o.extend([*ou, *ov, f64::from(*drop), *tol]);
            Ok(state(call(Op::Sample, &o)?))
        }
        Inputs::GapArray { a, b } => {
            o.push(a.len() as f64);
            o.extend(a);
            o.push(b.len() as f64);
            o.extend(b);
            Ok(state(call(Op::GapArray, &o)?))
        }
        Inputs::Topo {
            brep,
            kind,
            sel0,
            sel1,
            f0,
            f1,
            uv,
        } => {
            let (path, block, ex) = topo_graph(brep)?;
            let Some(c) = counts(&ex) else {
                return Ok(state(vec![-1.0, ex.get(1).copied().unwrap_or(0.0)]));
            };
            let (b, _) = import(&ex)?;
            let args = topo_args(&b, c, *kind, *sel0, *sel1, *f0, *f1, *uv)?;
            let out = ffi::topo(&path, &block, &args).map_err(|e| harness(format!("c++: {e}")))?;
            Ok(state(out))
        }
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    match i {
        Inputs::Gf {
            gf,
            set_cnt,
            set_tol,
            stop,
            mode,
        } => {
            let s = shapes(&k, gf)?;
            let mut f = function(&k, gf, &s)?;
            if let Some([su, sv]) = set_cnt {
                f.set_sample_cnt(*su, *sv);
            }
            if let Some(t) = set_tol {
                f.set_xsect_tol_3d(xsect_tol3d(&k, *t));
            }
            let r = if *mode == 0 {
                f.max_gap_sample(&k, *stop).map(|_| ())
            } else {
                match f.build_sample_set(&k, *stop) {
                    Err(e) if e.is_unported() => Err(e),
                    _ => Ok(()),
                }
            };
            match r {
                Ok(()) => o.push(0.0),
                Err(SolidError::Upstream { code, .. }) => o.push(f64::from(code)),
                Err(e) => return rust_fail(&e),
            }
            put_gf(&mut o, &f);
        }
        Inputs::Eval { gf, u, v, gu, gv } => {
            let s = shapes(&k, gf)?;
            let f = function(&k, gf, &s)?;
            let mut sample = GapSample::default();
            if let Err(e) = f.evaluate(&k, *u, *v, &mut sample, *gu, *gv) {
                return rust_fail(&e);
            }
            o.push(0.0);
            put_sample(&mut o, &sample);
        }
        Inputs::AddUnique { a, value } => {
            let mut a = a.clone();
            let i = add_unique_get_ordered_index(&mut a, *value);
            o.push(i as f64);
            o.push(a.len() as f64);
            o.extend(a);
        }
        Inputs::Sample {
            this,
            tu,
            tv,
            other,
            ou,
            ov,
            drop,
            tol,
        } => {
            let drop = match drop {
                0 => GapDropType::Unknown,
                1 => GapDropType::Normal,
                2 => GapDropType::Boundary,
                _ => GapDropType::NoDrop,
            };
            let mut s = GapSample::default();
            s.set_canonical(
                v3(*this),
                *tu,
                *tv,
                v3(*other),
                *ou,
                *ov,
                drop,
                xsect_tol3d(&k, *tol),
            );
            put_sample(&mut o, &s);
            o.push(s.length());
            o.extend([s.this_param_at(0), s.this_param_at(1)]);
            o.extend([s.other_param_at(0), s.other_param_at(1)]);
        }
        Inputs::GapArray { a, b } => {
            let gap = |l: f64| {
                Gap::from(VertexEdgeGap {
                    gap_3d: l.abs(),
                    ..VertexEdgeGap::new()
                })
            };
            let mut ga = GapArray::new();
            let mut gb = GapArray::new();
            for &l in a {
                ga.add(gap(l));
            }
            for &l in b {
                gb.add(gap(l));
            }
            gb.append(&ga);
            for x in [&ga, &gb] {
                o.push(x.len() as f64);
                o.push(x.max_gap_index().map_or(-1.0, |i| i as f64));
                o.push(x.max_gap_length());
            }
            for i in 0..gb.len() {
                let g = gb.get(i).ok_or_else(|| harness("gap"))?;
                o.push(g.length());
                if let Some(n) = gb.get(i + 1) {
                    o.extend([flag(g.is_greater(n)), flag(g.is_less(n))]);
                }
            }
            ga.reset();
            gb.reset();
            o.push(gb.len() as f64);
        }
        Inputs::Topo {
            brep,
            kind,
            sel0,
            sel1,
            f0,
            f1,
            uv,
        } => {
            let (_, _, ex) = topo_graph(brep)?;
            let Some(c) = counts(&ex) else {
                return Ok(state(vec![-1.0, ex.get(1).copied().unwrap_or(0.0)]));
            };
            let (mut b, _) = import(&ex)?;
            let args = topo_args(&b, c, *kind, *sel0, *sel1, *f0, *f1, *uv)?;
            match topo_rust(&k, &mut b, &args) {
                Ok(v) => o = v,
                Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
                Err(e) => return rust_fail(&e),
            }
        }
    }
    Ok(state(o))
}

// ---- generation --------------------------------------------------------------------------

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

fn gen_nurb(rng: &mut Rng, dim: u64, scale: f64, shift: [f64; 3]) -> NurbIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            let k = if rational { w } else { 1.0 };
            let x = (i as f64 / n as f64 + rng.uniform(-0.1, 0.1)) * scale + shift[0];
            let y = rng.uniform(-0.3, 0.3) * scale + shift[1];
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-0.3, 0.3) * scale + shift[2]
            };
            [x * k, y * k, if dim == 2 { z } else { z * k }, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.6) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-3.0, 0.0), rng.uniform(0.5, 4.0))
    };
    NurbIn {
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

fn gen_bss(rng: &mut Rng, scale: f64) -> BssIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 / n as f64 * scale,
                        j as f64 / m as f64 * scale,
                        rng.uniform(-0.2, 0.2) * scale,
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    let (a, b) = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0))
    };
    BssIn {
        oob: rng.chance(0.2),
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, a, b),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    }
}

fn unit3(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = v.iter().map(|c| c * c).sum::<f64>().sqrt();
        if l > 0.2 {
            return v.map(|c| c / l);
        }
    }
}

/// A unit vector perpendicular to `x`.
fn perp(rng: &mut Rng, x: [f64; 3]) -> [f64; 3] {
    loop {
        let r = unit3(rng);
        let c = [
            x[1] * r[2] - x[2] * r[1],
            x[2] * r[0] - x[0] * r[2],
            x[0] * r[1] - x[1] * r[0],
        ];
        let l = c.iter().map(|a| a * a).sum::<f64>().sqrt();
        if l > 0.2 {
            return c.map(|a| a / l);
        }
    }
}

fn gen_curve(rng: &mut Rng, scale: f64) -> CurveIn {
    match rng.below(9) {
        0..=2 => CurveIn::Bspline {
            dim: 3,
            oob: rng.chance(0.2),
            nurb: gen_nurb(rng, 3, scale, [0.0; 3]),
        },
        3 => CurveIn::Hermite {
            dim: 3,
            p1: rng.point3(-1.0, 1.0).map(|c| c * scale),
            d1: rng.point3(-1.0, 1.0).map(|c| c * scale),
            p2: rng.point3(-1.0, 1.0).map(|c| c * scale),
            d2: rng.point3(-1.0, 1.0).map(|c| c * scale),
        },
        4 => {
            let a = rng.uniform(-2.0, 0.5);
            CurveIn::Line {
                p: rng.point3(-0.5, 0.5).map(|c| c * scale),
                v: unit3(rng).map(|c| c * scale),
                domain: [a, a + rng.uniform(0.2, 3.0)],
                scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
                inside_out: rng.chance(0.2),
            }
        }
        5 => {
            let x = unit3(rng);
            let a = rng.uniform(-1.0, 1.0);
            CurveIn::Ellipse {
                dim: 3,
                c: rng.point3(-0.5, 0.5).map(|c| c * scale),
                x,
                y: perp(rng, x),
                domain: [a, a + rng.uniform(0.3, 6.0)],
                radii: [rng.uniform(0.3, 1.5) * scale, rng.uniform(0.3, 1.5) * scale],
                inside_out: rng.chance(0.2),
            }
        }
        6 => {
            let x = unit3(rng);
            let a = rng.uniform(-1.0, 1.0);
            CurveIn::Circle {
                dim: 3,
                c: rng.point3(-0.5, 0.5).map(|c| c * scale),
                x,
                y: perp(rng, x),
                domain: [a, a + rng.uniform(0.3, 6.0)],
                radius: rng.uniform(0.3, 1.5) * scale,
                inside_out: rng.chance(0.2),
            }
        }
        _ => {
            let surface = gen_bss(rng, scale);
            let (u0, u1) = (
                surface.u.first().copied().unwrap_or(0.0),
                surface.u.last().copied().unwrap_or(1.0),
            );
            let mut uv = gen_nurb(rng, 2, 1.0, [0.0; 3]);
            for c in &mut uv.cpts {
                let k = if c[3] == NOW { 1.0 } else { c[3] };
                c[0] = (u0 + (u1 - u0) * (c[0] / k).clamp(0.0, 1.0)) * k;
                c[1] = (0.5 + (c[1] / k).clamp(-0.45, 0.45)) * k;
            }
            CurveIn::CrvOnSurf { uv, surface }
        }
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    match rng.below(6) {
        0..=2 => SurfIn::Bspline(gen_bss(rng, scale)),
        3 | 4 => {
            let x = unit3(rng);
            SurfIn::Plane {
                o: rng.point3(-0.5, 0.5).map(|c| c * scale),
                x,
                y: perp(rng, x),
                scale: if rng.chance(0.7) {
                    [1.0, 1.0]
                } else {
                    [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]
                },
                dom: [-scale, -scale, scale, scale],
            }
        }
        _ => SurfIn::CurveBounded(gen_bss(rng, scale)),
    }
}

/// The curve's natural interval, as both sides compute it (from the Rust curve).
fn natural_ivl(c: &CurveIn) -> Option<[f64; 2]> {
    let k = Kernel::default();
    let c = curve_of(&k, c).ok()?;
    let i = c.natural_interval();
    Some([i.min(), i.max()])
}

fn surface_dom(s: &SurfIn) -> Option<[f64; 4]> {
    let s = surface_of(s).ok()?;
    let d = s.natural_uv_domain();
    let (u, v) = (d.u_interval(), d.v_interval());
    Some([u.min(), v.min(), u.max(), v.max()])
}

/// A sub-interval of `[a, b]` (whole, inner, or a degenerate or outer one rarely).
fn sub_ivl(rng: &mut Rng, a: f64, b: f64) -> [f64; 2] {
    let d = b - a;
    match rng.below(10) {
        0..=4 => [a, b],
        5..=7 => {
            let s = a + d * rng.uniform(0.0, 0.4);
            [s, s + d * rng.uniform(0.2, 0.6)]
        }
        8 => [a + d * 0.5, a + d * 0.5],
        _ => [a - d * 0.1, b + d * 0.1],
    }
}

fn gen_tol(rng: &mut Rng, scale: f64) -> f64 {
    match rng.below(8) {
        0 => 9876543.21, // SM_USE_DEFAULT
        1 => 0.0,
        2 => -1e-3 * scale,
        3 => 10.0 * scale,
        _ => *rng.choose(&[1e-6, 1e-5, 2e-5, 1e-3, 1e-1]).unwrap_or(&2e-5) * scale,
    }
}

fn gen_gf(rng: &mut Rng) -> GfIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let this = if rng.chance(0.6) {
        let curve = gen_curve(rng, scale);
        let [a, b] = natural_ivl(&curve).unwrap_or([0.0, 1.0]);
        let ivl = sub_ivl(rng, a, b);
        ThisIn::Curve { curve, ivl }
    } else {
        let surface = gen_surface(rng, scale);
        let [u0, v0, u1, v1] = surface_dom(&surface).unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let [a, b] = sub_ivl(rng, u0, u1);
        let [c, d] = sub_ivl(rng, v0, v1);
        ThisIn::Surface {
            surface,
            dom: [a, c, b, d],
        }
    };
    let other = match rng.below(10) {
        0..=3 => OtherIn::Point(rng.point3(-1.0, 1.0).map(|c| c * scale)),
        4..=7 => OtherIn::Curve(gen_curve(rng, scale)),
        _ => OtherIn::Surface(gen_surface(rng, scale)),
    };
    GfIn {
        tol: gen_tol(rng, scale),
        this,
        other,
        cnt_u: *rng.choose(&[1, 2, 3, 5, 8, 20, 20]).unwrap_or(&20),
        cnt_v: *rng.choose(&[1, 2, 4, 6]).unwrap_or(&2),
    }
}

fn gen_point(rng: &mut Rng) -> [f64; 3] {
    let mut p = rng.point3(-2.0, 2.0);
    match rng.below(10) {
        0 => p[0] = UNDEF,
        1 => p[2] = NOZ,
        _ => {}
    }
    p
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    match rng.below(20) {
        0..=8 => Inputs::Gf {
            gf: gen_gf(rng),
            set_cnt: rng
                .chance(0.2)
                .then(|| [1 + rng.below(12) as usize, 1 + rng.below(6) as usize]),
            set_tol: rng.chance(0.15).then(|| rng.uniform(1e-6, 1e-1)),
            stop: rng.chance(0.3),
            mode: u8::from(rng.chance(0.3)),
        },
        9 | 10 => {
            let gf = gen_gf(rng);
            let (u, v) = match &gf.this {
                ThisIn::Curve { ivl, .. } => (ivl[0] + (ivl[1] - ivl[0]) * rng.unit(), 0.0),
                ThisIn::Surface { dom, .. } => (
                    dom[0] + (dom[2] - dom[0]) * rng.unit(),
                    dom[1] + (dom[3] - dom[1]) * rng.unit(),
                ),
            };
            Inputs::Eval {
                gf,
                u,
                v,
                gu: rng.chance(0.5).then(|| rng.uniform(-0.5, 1.5)),
                gv: rng.chance(0.5).then(|| rng.uniform(-0.5, 1.5)),
            }
        }
        11 => {
            let n = rng.below(8) as usize;
            let mut a: Vec<f64> = (0..n).map(|_| rng.uniform(-2.0, 2.0)).collect();
            a.sort_by(f64::total_cmp);
            let value = match rng.below(5) {
                0 if !a.is_empty() => {
                    let i = rng.below(a.len() as u64) as usize;
                    a.get(i).copied().unwrap_or(0.0) + rng.uniform(-2e-8, 2e-8)
                }
                _ => rng.uniform(-2.5, 2.5),
            };
            Inputs::AddUnique { a, value }
        }
        12 => Inputs::Sample {
            this: gen_point(rng),
            tu: rng.uniform(-1.0, 1.0),
            tv: rng.uniform(-1.0, 1.0),
            other: gen_point(rng),
            ou: rng.uniform(-1.0, 1.0),
            ov: rng.uniform(-1.0, 1.0),
            drop: rng.below(4) as u8,
            tol: gen_tol(rng, 1.0),
        },
        13 => Inputs::GapArray {
            a: (0..rng.below(6)).map(|_| rng.uniform(-1.0, 1.0)).collect(),
            b: (0..rng.below(6)).map(|_| rng.uniform(-1.0, 1.0)).collect(),
        },
        _ => {
            let kind = rng.below(6) as u8;
            let frac = |rng: &mut Rng| -> Option<f64> {
                match rng.below(6) {
                    0 => None,
                    1 => Some(0.0),
                    2 => Some(1.0),
                    _ => Some(rng.unit()),
                }
            };
            Inputs::Topo {
                brep: gen_brep(rng),
                kind,
                sel0: if rng.chance(0.05) {
                    -1
                } else {
                    rng.below(1 << 20) as i64
                },
                sel1: rng.below(1 << 20) as i64,
                f0: frac(rng),
                f1: frac(rng),
                uv: rng.chance(0.8).then(|| [rng.unit(), rng.unit()]),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "heal.gap"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
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
}
