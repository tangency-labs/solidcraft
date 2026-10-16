//! Runner for unit `query.edge`. C++: `solidcore_ffi::units::query_edge`. Rust:
//! `solidcore::query::edge`.
//!
//! The C++ side builds a real `SmBrep` per call: read from an upstream `.smb` test file
//! (all B-spline geometry, as unit `topology.edgeuse_p1`), a face from a B-spline surface
//! (`CreateFaceFromSurface`) or a wire edge from a curve of any kind the edge code
//! branches on (`CreateWireEdgeFromCurve` over a real `SmBSplineCurve`, `SmHermiteCurve`,
//! `SmLine`, `SmEllipse` or `SmCircle`), then optionally edits one edge (tolerance,
//! start / end vertex point, curve removed). There is no Rust B-rep reader or builder
//! yet, so the Rust side imports the edited graph the shim exports (with unit
//! `topology.edgeuse_p1`'s importer: list owners, list order, orientations, tolerances,
//! vertex points, B-spline and line curves); a wire edge's curve is the case's curve,
//! built the same way on both sides. The ported function then runs on the imported
//! B-rep.
//!
//! Selectors are taken modulo the entity count (the radial ops' edge uses modulo the
//! loop edge uses), so generated cases need no counts. The closest point is the edge
//! curve's point at the normalised edge parameter `nt` plus `offset` (the offset alone
//! without curve); the curve classified by `radial_curve` is the line from the edge's
//! point at `nt` along `dir` over `[0, len]`. Both sides compute these inputs from the
//! imported B-rep, identically.
//!
//! Outputs: `failed` (`0` success, `1` a failing status, `-2` where upstream would
//! dereference null; the shim does not call it, the port returns `InvalidInput`), then
//! on success the op's values. The radial classifications are deferred
//! (`SmEdgeuse::EvaluateBinormalStepOff`, unit `topology.edgeuse_p2`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{Curve, BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BoundingBoxRequest, BoundingBoxes};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::query::edge as qe;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{Brep, EdgeId, EdgeuseId};
use solidcore::SolidError;
use solidcore_ffi::units::query_edge::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `query.edge` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" (output of the shim, `InvalidInput` in Rust).
const NULL_DEREF: i64 = -2;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BSplineIn {
    dim: u64,
    #[serde(default)]
    oob: bool,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HermiteIn {
    dim: u64,
    p1: [f64; 3],
    d1: [f64; 3],
    p2: [f64; 3],
    d2: [f64; 3],
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
struct CircleIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    domain: [f64; 2],
    radius: f64,
    #[serde(default)]
    inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Line(LineIn),
    Ellipse(EllipseIn),
    Circle(CircleIn),
}

/// A B-spline surface.
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
#[serde(tag = "kind", rename_all = "snake_case")]
enum BrepIn {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    Face { surface: BssIn, dom: [f64; 4] },
    Wire { curve: CurveIn, t0: f64, t1: f64 },
}

/// One edge's edits (`edge` a selector).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EditsIn {
    edge: i64,
    #[serde(default = "minus_one")]
    tol: f64,
    #[serde(default)]
    p0: Option<[f64; 3]>,
    #[serde(default)]
    p1: Option<[f64; 3]>,
    #[serde(default)]
    drop: bool,
}

fn minus_one() -> f64 {
    -1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Bbox {
        edge: i64,
        normal: bool,
        pseudo: bool,
        polar: bool,
        tight: bool,
    },
    Tight {
        edge: i64,
    },
    Closest {
        edge: i64,
        nt: f64,
        offset: [f64; 3],
    },
    Radial {
        eu: i64,
        classify: i64,
        nparam: f64,
    },
    RadialCurve {
        eu: i64,
        nt: f64,
        dir: [f64; 3],
        len: f64,
        orient: u8,
        force: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    edits: Option<EditsIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- encoding ---------------------------------------------------------------------

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
        }
        CurveIn::Ellipse(e) => {
            v.extend([3.0, 3.0]);
            for p in [e.c, e.x, e.y] {
                v.extend_from_slice(&p);
            }
            v.extend([e.domain[0], e.domain[1], e.radii[0], e.radii[1], b(e.inside_out)]);
        }
        CurveIn::Circle(e) => {
            v.extend([4.0, 3.0]);
            for p in [e.c, e.x, e.y] {
                v.extend_from_slice(&p);
            }
            v.extend([e.domain[0], e.domain[1], e.radius, b(e.inside_out)]);
        }
    }
}

fn push_surface(v: &mut Vec<f64>, s: &BssIn) -> Result<(), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let m = s
        .net
        .first()
        .and_then(|r| r.len().checked_sub(1))
        .ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != m + 1) {
        return Err(harness("ragged net"));
    }
    let r = s.u.len().checked_sub(1).ok_or_else(|| harness("no u knots"))?;
    let q = s.v.len().checked_sub(1).ok_or_else(|| harness("no v knots"))?;
    v.extend([
        0.0,
        b(s.oob),
        0.0,
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        r as f64,
        q as f64,
    ]);
    for row in &s.net {
        for p in row {
            v.extend_from_slice(p);
        }
    }
    v.extend(&s.u);
    v.extend(&s.v);
    Ok(())
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
fn path_of(br: &BrepIn) -> Result<String, HarnessError> {
    match br {
        BrepIn::File { file, .. } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

/// The B-rep block.
fn brep_block(br: &BrepIn) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match br {
        BrepIn::File { rebuild, .. } => v.extend([0.0, b(*rebuild)]),
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(4.0);
            curve_block(&mut v, curve);
            v.extend([*t0, *t1]);
        }
    }
    Ok(v)
}

/// Resolves a selector modulo `n` (`None` when there is nothing to select).
fn sel(s: i64, n: usize) -> Option<usize> {
    if n == 0 {
        return None;
    }
    Some(s.rem_euclid(n as i64) as usize)
}

/// The edit block (selector resolved against `n_edges`).
fn edit_block(e: Option<&EditsIn>, n_edges: usize) -> Vec<f64> {
    let Some(e) = e else {
        return vec![-1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    };
    let edge = sel(e.edge, n_edges).map_or(-1.0, |i| i as f64);
    let mut v = vec![edge, e.tol];
    for p in [e.p0, e.p1] {
        v.push(b(p.is_some()));
        v.extend(p.unwrap_or([0.0; 3]));
    }
    v.push(b(e.drop));
    v
}

// ---- the B-rep, as both sides see it ----------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn build_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
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
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
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
                    k,
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
        CurveIn::Circle(e) => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: e.inside_out,
            };
            Ok(Curve::from(
                Ellipse::new_circle(
                    k,
                    v3(e.c),
                    v3(e.x),
                    v3(e.y),
                    Extent1d::from_min_max(e.domain[0], e.domain[1]),
                    e.radius,
                    &opts,
                )
                .map_err(|e| harness(format!("circle: {e}")))?,
            ))
        }
    }
}

/// The case's B-rep: the edited C++ export imported into a Rust [`Brep`].
struct Scene {
    brep: Brep,
    /// The B-rep block and the resolved edit block.
    prefix: Vec<f64>,
    path: String,
    /// Loop edge uses, in id order.
    loop_eus: Vec<EdgeuseId>,
}

/// Builds the scene, or the build failure's output.
fn scene(k: &Kernel, i: &Inputs) -> Result<Result<Scene, Output>, HarnessError> {
    let path = path_of(&i.brep)?;
    let block = brep_block(&i.brep)?;
    let mut plain = block.clone();
    plain.extend(edit_block(None, 0));
    let first = ffi::eval(Op::Export, &path, &plain).map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi::counts(&first) else {
        let st = first.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let n_edges = c[7];
    let mut prefix = block;
    prefix.extend(edit_block(i.edits.as_ref(), n_edges));
    let ex = ffi::eval(Op::Export, &path, &prefix).map_err(|e| harness(format!("c++ export: {e}")))?;
    // the edge curve kinds follow the graph
    let split = ex
        .len()
        .checked_sub(n_edges)
        .ok_or_else(|| harness("export without curve kinds"))?;
    let (graph, kinds) = ex.split_at(split);
    let mut brep = crate::units::topology_edgeuse_p1::import(graph)?;
    for (e, &kind) in kinds.iter().enumerate() {
        let kind = kind as i64;
        let id = EdgeId::from_index(e as u32);
        match &i.brep {
            BrepIn::Wire { curve, .. } => {
                if kind != -1 {
                    let c = build_curve(k, curve)?;
                    brep.edge_mut(id).map_err(|e| harness(e.to_string()))?.curve = Some(c);
                }
            }
            _ => {
                if kind != -1 && kind != BSPLINE_CURVE_TYPE && kind != LINE_TYPE {
                    return Err(harness(format!("edge curve of type {kind} cannot be imported")));
                }
            }
        }
    }
    let loop_eus = brep
        .edgeuses()
        .ids()
        .filter(|&eu| eu1::is_loop_edgeuse(&brep, eu).unwrap_or(false))
        .collect();
    Ok(Ok(Scene {
        brep,
        prefix,
        path,
        loop_eus,
    }))
}

/// An op with its selectors resolved and derived inputs computed.
#[derive(Debug, Clone)]
enum Resolved {
    Bbox {
        edge: usize,
        req: BoundingBoxRequest,
        tight: bool,
    },
    Tight {
        edge: usize,
    },
    Closest {
        edge: usize,
        point: Vec3,
    },
    Radial {
        eu: usize,
        classify: usize,
        nparam: f64,
    },
    RadialCurve {
        eu: usize,
        t: f64,
        line: LineIn,
        orient: u8,
        force: bool,
    },
}

/// The edge curve's point at normalised edge parameter `nt` (`None` without curve or
/// when it cannot be evaluated) and the parameter.
fn edge_point(br: &Brep, e: EdgeId, nt: f64) -> Option<(f64, Vec3)> {
    let edge = br.edge(e).ok()?;
    let t = edge.interval.evaluate(nt);
    let p = edge.curve.as_ref()?.evaluate_point(t).ok()?;
    Some((t, p))
}

fn resolve(s: &Scene, op: &OpIn) -> Option<Resolved> {
    let n_e = s.brep.edges().len();
    let n_l = s.loop_eus.len();
    let eid = |i: usize| EdgeId::from_index(i as u32);
    Some(match *op {
        OpIn::Bbox {
            edge,
            normal,
            pseudo,
            polar,
            tight,
        } => Resolved::Bbox {
            edge: sel(edge, n_e)?,
            req: BoundingBoxRequest {
                normal,
                pseudo,
                polar,
            },
            tight,
        },
        OpIn::Tight { edge } => Resolved::Tight {
            edge: sel(edge, n_e)?,
        },
        OpIn::Closest { edge, nt, offset } => {
            let e = sel(edge, n_e)?;
            let base = edge_point(&s.brep, eid(e), nt).map_or(Vec3::ZERO, |(_, p)| p);
            Resolved::Closest {
                edge: e,
                point: base + v3(offset),
            }
        }
        OpIn::Radial {
            eu,
            classify,
            nparam,
        } => Resolved::Radial {
            eu: s.loop_eus.get(sel(eu, n_l)?)?.index() as usize,
            classify: s.loop_eus.get(sel(classify, n_l)?)?.index() as usize,
            nparam,
        },
        OpIn::RadialCurve {
            eu,
            nt,
            dir,
            len,
            orient,
            force,
        } => {
            let id = *s.loop_eus.get(sel(eu, n_l)?)?;
            let e = s.brep.edgeuse(id).ok()?.edge?;
            let (t, p) = edge_point(&s.brep, e, nt)?;
            Resolved::RadialCurve {
                eu: id.index() as usize,
                t,
                line: LineIn {
                    p: [p.x, p.y, p.z],
                    v: dir,
                    domain: [0.0, len],
                    scale: 1.0,
                    inside_out: false,
                },
                orient,
                force,
            }
        }
    })
}

fn no_entity() -> Output {
    Output::err("no_entity_of_that_kind")
}

// ---- C++ side ---------------------------------------------------------------------

fn op_args(r: &Resolved) -> (Op, Vec<f64>) {
    match r {
        Resolved::Bbox { edge, req, tight } => (
            Op::BBox,
            vec![
                *edge as f64,
                b(req.normal),
                b(req.pseudo),
                b(req.polar),
                b(*tight),
            ],
        ),
        Resolved::Tight { edge } => (Op::Tight, vec![*edge as f64]),
        Resolved::Closest { edge, point } => {
            (Op::Closest, vec![*edge as f64, point.x, point.y, point.z])
        }
        Resolved::Radial {
            eu,
            classify,
            nparam,
        } => (Op::Radial, vec![*eu as f64, *classify as f64, *nparam]),
        Resolved::RadialCurve {
            eu,
            t,
            line,
            orient,
            force,
        } => {
            let mut v = vec![*eu as f64, *t];
            curve_block(&mut v, &CurveIn::Line(line.clone()));
            v.extend([line.domain[0], line.domain[1], f64::from(*orient), b(*force)]);
            (Op::RadialCurve, v)
        }
    }
}

/// `failed` of a status slot.
fn failed_of(st: f64) -> i64 {
    if st == 0.0 {
        0
    } else if st == NULL_DEREF as f64 {
        NULL_DEREF
    } else {
        1
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let s = match scene(&k, i)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let Some(r) = resolve(&s, &i.op) else {
        return Ok(no_entity());
    };
    let (op, args) = op_args(&r);
    let mut input = s.prefix.clone();
    input.extend(args);
    let out = ffi::eval(op, &s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    let st = body.first().copied().ok_or_else(|| harness("c++: no status"))?;
    let failed = failed_of(st);
    let o = Output::new().int("failed", failed);
    if failed != 0 {
        return Ok(o);
    }
    let rest = body.get(1..).unwrap_or(&[]);
    Ok(match r {
        Resolved::Bbox { req, .. } => {
            let mut at = 0;
            let mut o = o;
            let mut take = |n: usize| {
                let v = rest.get(at..at + n).unwrap_or(&[]).to_vec();
                at += n;
                v
            };
            if req.normal {
                o = o.values("normal", take(6));
            }
            if req.pseudo {
                o = o.values("pseudo", take(15));
            }
            if req.polar {
                o = o.values("polar", take(14));
            }
            o
        }
        Resolved::Tight { .. } => o.values("box", rest.to_vec()),
        Resolved::Closest { .. } => {
            let found = rest.first().copied().unwrap_or(-1.0) as i64;
            let o = o.int("found", found);
            if found == 1 {
                o.values("closest", rest.get(1..).unwrap_or(&[]).to_vec())
            } else {
                o
            }
        }
        Resolved::Radial { .. } | Resolved::RadialCurve { .. } => {
            o.int("result", rest.first().copied().unwrap_or(-1.0) as i64)
        }
    })
}

// ---- Rust side --------------------------------------------------------------------

fn box_values(e: &Extent3d) -> Vec<f64> {
    (0..3)
        .map(|i| e.min_at(i))
        .chain((0..3).map(|i| e.max_at(i)))
        .collect()
}

fn boxes_output(o: Output, bx: &BoundingBoxes) -> Output {
    let mut o = o;
    if let Some(nb) = &bx.normal {
        o = o.values("normal", box_values(nb));
    }
    if let Some(pb) = &bx.pseudo {
        let mut v = Vec::new();
        for w in pb.basis() {
            v.extend([w.x, w.y, w.z]);
        }
        for iv in pb.intervals() {
            v.extend([iv.min(), iv.max()]);
        }
        o = o.values("pseudo", v);
    }
    if let Some(pl) = &bx.polar {
        let mut v = vec![f64::from(pl.state().code())];
        for w in pl.basis() {
            v.extend([w.x, w.y, w.z]);
        }
        let d = pl.domain();
        v.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
        o = o.values("polar", v);
    }
    o
}

/// A Rust error as an output: deferred, `-2` for `InvalidInput`, else failed.
fn rust_err(e: &SolidError) -> Output {
    if let Some(o) = Output::unported(e) {
        return o;
    }
    let failed = if matches!(e, SolidError::InvalidInput { .. }) {
        NULL_DEREF
    } else {
        1
    };
    Output::new().int("failed", failed)
}

fn orient_of(code: u8) -> OrientType {
    match code {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut s = match scene(&k, i)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let Some(r) = resolve(&s, &i.op) else {
        return Ok(no_entity());
    };
    let br = &mut s.brep;
    let eid = |i: usize| EdgeId::from_index(i as u32);
    let euid = |i: usize| EdgeuseId::from_index(i as u32);
    let ok = Output::new().int("failed", 0);
    Ok(match r {
        Resolved::Bbox { edge, req, tight } => {
            match qe::calculate_bounding_box(&k, br, eid(edge), req, tight) {
                Ok(bx) => boxes_output(ok, &bx),
                Err(e) => rust_err(&e),
            }
        }
        Resolved::Tight { edge } => match qe::calculate_tight_bounding_box(&k, br, eid(edge)) {
            Ok(bb) => ok.values("box", box_values(&bb.unwrap_or_else(Extent3d::new))),
            Err(e) => rust_err(&e),
        },
        Resolved::Closest { edge, point } => match qe::closest_point(&k, br, eid(edge), point) {
            Ok(Some(cp)) => ok
                .int("found", 1)
                .values("closest", vec![cp.parameter, cp.distance]),
            Ok(None) => ok.int("found", 0),
            Err(e) => rust_err(&e),
        },
        Resolved::Radial {
            eu,
            classify,
            nparam,
        } => {
            let Some(edge) = br.edgeuse(euid(eu)).ok().and_then(|x| x.edge) else {
                return Ok(Output::new().int("failed", NULL_DEREF));
            };
            match qe::radial_sector_classify(&k, br, edge, nparam, euid(eu), euid(classify)) {
                Ok(c) => ok.int("result", i64::from(c.code())),
                Err(e) => rust_err(&e),
            }
        }
        Resolved::RadialCurve {
            eu,
            t,
            line,
            orient,
            force,
        } => {
            let Some(edge) = br.edgeuse(euid(eu)).ok().and_then(|x| x.edge) else {
                return Ok(Output::new().int("failed", NULL_DEREF));
            };
            let ivl = Extent1d::from_min_max(line.domain[0], line.domain[1]);
            let c = build_curve(&k, &CurveIn::Line(line))?;
            match qe::radial_sector_of_curve_classify(
                &k,
                br,
                edge,
                euid(eu),
                t,
                &c,
                &ivl,
                orient_of(orient),
                force,
            ) {
                Ok(c) => ok.int("result", i64::from(c.code())),
                Err(e) => rust_err(&e),
            }
        }
    })
}

// ---- generation -------------------------------------------------------------------

/// The upstream `.smb` files used (all B-spline geometry, as unit
/// `topology.edgeuse_p1`).
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

fn scale_pick(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-4, 1e-2, 1.0, 1.0, 1.0, 1.0, 1e2, 1e5])
        .unwrap_or(&1.0)
}

fn unit_dir(rng: &mut Rng) -> [f64; 3] {
    let mut d = rng.point3(-1.0, 1.0);
    if d.iter().all(|c| c.abs() < 0.1) {
        d = [1.0, 0.0, 0.0];
    }
    d
}

/// A wire curve and its edge interval.
fn gen_wire(rng: &mut Rng) -> BrepIn {
    let scale = scale_pick(rng);
    let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
    let (curve, a, b) = match rng.below(6) {
        0 | 1 => {
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(4) as usize;
            let rational = rng.chance(0.25);
            let straight = rng.chance(0.2);
            let dir = unit_dir(rng);
            let cpts = (0..=n)
                .map(|i| {
                    let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
                    let k = if rational { w } else { 1.0 };
                    let base = if straight {
                        dir.map(|d| d * i as f64)
                    } else {
                        [
                            i as f64 + rng.uniform(-0.3, 0.3),
                            rng.uniform(-1.0, 1.0),
                            rng.uniform(-1.0, 1.0),
                        ]
                    };
                    [
                        (o[0] + base[0] * scale) * k,
                        (o[1] + base[1] * scale) * k,
                        (o[2] + base[2] * scale) * k,
                        w,
                    ]
                })
                .collect();
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0))
            };
            (
                CurveIn::Bspline(BSplineIn {
                    dim: 3,
                    oob: rng.chance(0.2),
                    cpts,
                    p,
                    u: clamped_knots(rng, p, n, a, b),
                }),
                a,
                b,
            )
        }
        2 => (
            CurveIn::Line(LineIn {
                p: o,
                v: unit_dir(rng).map(|c| c * scale),
                domain: [rng.uniform(-2.0, 0.0), rng.uniform(0.5, 3.0)],
                scale: if rng.chance(0.3) { rng.uniform(0.5, 3.0) } else { 1.0 },
                inside_out: rng.chance(0.2),
            }),
            -10.0,
            10.0,
        ),
        3 => {
            let d0 = rng.uniform(-90.0, 90.0);
            let d1 = d0 + rng.uniform(10.0, 360.0);
            (
                CurveIn::Circle(CircleIn {
                    c: o,
                    x: unit_dir(rng),
                    y: unit_dir(rng),
                    domain: [d0, d1],
                    radius: scale * rng.uniform(0.2, 3.0),
                    inside_out: rng.chance(0.2),
                }),
                0.0,
                1.0,
            )
        }
        4 => {
            let d0 = rng.uniform(-90.0, 90.0);
            let d1 = d0 + rng.uniform(10.0, 360.0);
            (
                CurveIn::Ellipse(EllipseIn {
                    c: o,
                    x: unit_dir(rng),
                    y: unit_dir(rng),
                    domain: [d0, d1],
                    radii: [scale * rng.uniform(0.2, 3.0), scale * rng.uniform(0.2, 3.0)],
                    inside_out: rng.chance(0.2),
                }),
                0.0,
                1.0,
            )
        }
        _ => (
            CurveIn::Hermite(HermiteIn {
                dim: 3,
                p1: o,
                d1: rng.point3(-3.0, 3.0).map(|c| c * scale),
                p2: rng.point3(-2.0, 2.0).map(|c| c * scale),
                d2: rng.point3(-3.0, 3.0).map(|c| c * scale),
            }),
            0.0,
            1.0,
        ),
    };
    let (t0, t1) = match &curve {
        CurveIn::Line(l) => (l.domain[0], l.domain[1]),
        _ if rng.chance(0.4) => (a, b),
        _ => (
            a + (b - a) * rng.uniform(0.0, 0.4),
            b - (b - a) * rng.uniform(0.0, 0.4),
        ),
    };
    BrepIn::Wire { curve, t0, t1 }
}

fn gen_face(rng: &mut Rng) -> BrepIn {
    let scale = scale_pick(rng);
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    [
                        i as f64 * scale,
                        j as f64 * scale,
                        rng.uniform(-0.5, 0.5) * scale,
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    let s = BssIn {
        oob: rng.chance(0.2),
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    };
    let dom = if rng.chance(0.5) {
        [0.0, 0.0, 1.0, 1.0]
    } else {
        [
            rng.uniform(0.0, 0.3),
            rng.uniform(0.0, 0.3),
            rng.uniform(0.7, 1.0),
            rng.uniform(0.7, 1.0),
        ]
    };
    BrepIn::Face { surface: s, dom }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

/// A normalised parameter: interior, an end, or just outside.
fn gen_nt(rng: &mut Rng) -> f64 {
    match rng.below(6) {
        0 => 0.0,
        1 => 1.0,
        2 => *rng.choose(&[-1e-9, 1.0 + 1e-9, 1e-12, 0.5]).unwrap_or(&0.5),
        _ => rng.uniform(0.0, 1.0),
    }
}

/// An offset from the curve: on it, at a tolerance, near or far, scaled.
fn gen_offset(rng: &mut Rng) -> [f64; 3] {
    let mag = *rng
        .choose(&[0.0, 1e-12, 1e-7, 1e-3, 0.1, 1.0, 10.0, 1e4])
        .unwrap_or(&1.0);
    rng.point3(-1.0, 1.0).map(|c| c * mag)
}

fn gen_edits(rng: &mut Rng, radial: bool) -> Option<EditsIn> {
    if !rng.chance(0.4) {
        return None;
    }
    let tol = *rng
        .choose(&[-1.0, 0.0, 1e-12, 1e-9, 1e-6, 1e-3, 0.1, 10.0])
        .unwrap_or(&-1.0);
    let mv = |rng: &mut Rng| -> Option<[f64; 3]> {
        rng.chance(0.3).then(|| {
            let mag = *rng.choose(&[0.0, 1e-9, 1e-3, 1.0, 100.0]).unwrap_or(&1.0);
            rng.point3(-1.0, 1.0).map(|c| c * mag)
        })
    };
    Some(EditsIn {
        edge: gen_sel(rng),
        tol,
        p0: mv(rng),
        p1: mv(rng),
        drop: !radial && rng.chance(0.1),
    })
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let brep = match rng.below(10) {
        0..=4 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"cone.smb")).to_string(),
            rebuild: rng.chance(0.2),
        },
        5 | 6 => gen_face(rng),
        _ => gen_wire(rng),
    };
    let has_loops = !matches!(brep, BrepIn::Wire { .. });
    let pick = if has_loops { rng.below(10) } else { rng.below(7) };
    let op = match pick {
        0 | 1 => OpIn::Bbox {
            edge: gen_sel(rng),
            normal: rng.chance(0.7),
            pseudo: rng.chance(0.5),
            polar: rng.chance(0.5),
            tight: rng.chance(0.4),
        },
        2 | 3 => OpIn::Tight { edge: gen_sel(rng) },
        4..=6 => OpIn::Closest {
            edge: gen_sel(rng),
            nt: gen_nt(rng),
            offset: gen_offset(rng),
        },
        7 | 8 => OpIn::Radial {
            eu: gen_sel(rng),
            classify: gen_sel(rng),
            nparam: gen_nt(rng),
        },
        _ => OpIn::RadialCurve {
            eu: gen_sel(rng),
            nt: rng.uniform(0.0, 1.0),
            dir: unit_dir(rng),
            len: *rng.choose(&[1e-3, 0.1, 1.0, 10.0]).unwrap_or(&1.0),
            orient: rng.below(3) as u8,
            force: rng.chance(0.5),
        },
    };
    let radial = matches!(op, OpIn::Radial { .. } | OpIn::RadialCurve { .. });
    Inputs {
        brep,
        edits: gen_edits(rng, radial),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.edge"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }
}
