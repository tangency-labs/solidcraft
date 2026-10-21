//! Runner for unit `blend.fillet_geom_p2`: the surface/surface intersection curve
//! between two points (`sm_SrfSrfIntersection`), the fillet vertex calculators
//! (`CalcRailIntRail`, `CalcRailIntEdgeuse`, `CalcVertOnCrossSection`, `CalcSetBackVert`),
//! the rail/rail interpolation edge (`CalcRailRailInterpolation`) and the split-face
//! bookkeeping of a fillet edge (`SmFilletEdge::UpdateSplitFace`).
//!
//! C++: `solidcore_ffi::units::blend_fillet_geom_p2` (upstream
//! `source/SMLib/src/SmFilletGeom.cpp`). Rust: `solidcore::blend::fillet_geom::p2`.
//!
//! `srf_srf` cases are two surfaces (B-spline, real `SmPlane`, real
//! `SmSurfOfRevolution`), optional start / end points, directions and a reference point,
//! the tolerances and the skip flag. Both sides produce the flat record of the FFI
//! module docs, split into keys: `cs` (the surfaces' construction statuses), `status`
//! (`0` = `SM_SUCCESS`, else the `SmStatus`; `-1` on the Rust side for an error upstream
//! cannot produce), the three curves (`c3d_*`, `uv1_*`, `uv2_*`: `type`, then the
//! B-spline data `has`, `dims`, `cpts`, `knots`) and `dir` (as the call left it). Many
//! cases take their points from the intersection curve itself (the Rust intersection
//! without points, sampled), so the trimming, reversal and acceptance paths are reached.
//!
//! The topology cases are `blend.fillet_geom_p1`'s pseudo-B-rep model with base
//! solvers (as in `blend.fillet_geom_p4`) and one query: a vertex calculator (no
//! corner: the shim refuses the block's opaque corner tags, upstream asks the corner
//! for its filleted vertex first), `rail_rail` (an edge between two vertices with
//! incoming rails, its original face a B-spline face, a real plane or a surface of
//! revolution; the edge's status, curve and its edge uses' UV curves are compared) or
//! `split_face` (the original face, then `UpdateSplitFace`; the face indices are
//! compared; only the path where the edge is not on the split face runs: the other
//! one reaches `ShouldSwapFaces`, `blend.fillet_geom_p6`, which the shim refuses).
//!
//! Deferred on the Rust side (unported callees): `sm_TwoPntsIntersection`'s point
//! intersector (`intersect.surface_intersector_p4`), `SmSurface::DropCurve`
//! (`intersect.surface_drop_lib_p3`) of a non-planar surface, `ShouldSwapFaces`
//! (`blend.fillet_geom_p6`). Every run has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_geom::p1::FilletSolverId;
use solidcore::blend::fillet_geom::p2::{srf_srf_intersection, SrfSrfOptions};
use solidcore::blend::fillet_solver::p1::FilletSolver;
use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::tol::zone_tol3d;
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::surface::surface::Surface;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::{edge, EdgeId, FaceId};
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_geom_p2::{self as ffi, Op};

use super::blend_fillet_geom_p1::{
    analytic_surface, dump_bcurve, gen_model, gen_surface, push_model, push_srf, scale_of,
    status, surface_of, AnalyticSrf, Crv, EdgeIn, EuIn, Model, RustModel, Srf,
};
use super::blend_fillet_geom_p4::{
    empty_model, gen_analytic, push_analytic, uv_line, vertex_at, SolverIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 300;

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `NL_NOW`: a non-rational control point's weight marker.
const NOW: f64 = -f64::MAX;

// ---- inputs ------------------------------------------------------------------------

/// A surface: B-spline, or a real `SmPlane` / `SmSurfOfRevolution`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SurfIn {
    BSpline(Srf),
    Analytic(AnalyticSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
enum Query {
    SrfSrf {
        s1: Box<SurfIn>,
        s2: Box<SurfIn>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        start: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        end_dir: Option<[f64; 3]>,
        tol: f64,
        ang: f64,
        #[serde(default)]
        skip: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ref_point: Option<[f64; 3]>,
    },
    RailIntRail {
        v: usize,
    },
    RailIntEdgeuse {
        v: usize,
        #[serde(default = "minus_one")]
        g: i64,
    },
    CrossSection {
        v: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        param: Option<f64>,
    },
    SetBack {
        v: usize,
    },
    RailRail {
        e: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        face: Option<SurfIn>,
        /// The original face's tolerance.
        #[serde(default)]
        face_tol: f64,
    },
    SplitFace {
        e: usize,
        set: i64,
        f1: i64,
        f2: i64,
    },
}

const fn minus_one() -> i64 {
    -1
}

impl Query {
    const fn op(&self) -> Op {
        match self {
            Self::SrfSrf { .. } => Op::SrfSrf,
            Self::RailIntRail { .. } => Op::RailIntRail,
            Self::RailIntEdgeuse { .. } => Op::RailIntEdgeuse,
            Self::CrossSection { .. } => Op::CrossSection,
            Self::SetBack { .. } => Op::SetBack,
            Self::RailRail { .. } => Op::RailRail,
            Self::SplitFace { .. } => Op::SplitFace,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default = "empty_model")]
    model: Model,
    #[serde(default)]
    solvers: Vec<SolverIn>,
    /// Per geometry: its solver index, `-1` (or missing) for none.
    #[serde(default)]
    geom_solvers: Vec<i64>,
    query: Query,
}

// ---- C++ side ------------------------------------------------------------------------

fn push_surf(o: &mut Vec<f64>, s: &SurfIn) -> Result<(), HarnessError> {
    match s {
        SurfIn::BSpline(srf) => {
            o.push(0.0);
            push_srf(o, srf)?;
        }
        SurfIn::Analytic(a) => push_analytic(o, a),
    }
    Ok(())
}

fn push_opt3(o: &mut Vec<f64>, p: Option<[f64; 3]>) {
    o.push(flag(p.is_some()));
    o.extend(p.unwrap_or([0.0; 3]));
}

fn geom_solver(i: &Inputs, g: usize) -> i64 {
    i.geom_solvers.get(g).copied().unwrap_or(-1)
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    if let Query::SrfSrf {
        s1,
        s2,
        start,
        end,
        dir,
        end_dir,
        tol,
        ang,
        skip,
        ref_point,
    } = &i.query
    {
        push_surf(&mut o, s1)?;
        push_surf(&mut o, s2)?;
        push_opt3(&mut o, *start);
        push_opt3(&mut o, *end);
        push_opt3(&mut o, *dir);
        push_opt3(&mut o, *end_dir);
        o.extend([*tol, *ang, flag(*skip)]);
        push_opt3(&mut o, *ref_point);
        return Ok(o);
    }
    push_model(&mut o, &i.model)?;
    o.push(i.solvers.len() as f64);
    for s in &i.solvers {
        o.extend([s.approx, s.ang, s.tang]);
    }
    for g in 0..i.model.geoms.len() {
        o.push(geom_solver(i, g) as f64);
    }
    match &i.query {
        Query::SrfSrf { .. } => {}
        Query::RailIntRail { v } | Query::SetBack { v } => o.push(*v as f64),
        Query::RailIntEdgeuse { v, g } => o.extend([*v as f64, *g as f64]),
        Query::CrossSection { v, param } => {
            o.extend([*v as f64, flag(param.is_some()), param.unwrap_or(0.0)]);
        }
        Query::RailRail { e, face, face_tol } => {
            o.push(*e as f64);
            match face {
                None => o.push(-1.0),
                Some(s) => {
                    push_surf(&mut o, s)?;
                    o.push(*face_tol);
                }
            }
        }
        Query::SplitFace { e, set, f1, f2 } => {
            o.extend([*e as f64, *set as f64, *f1 as f64, *f2 as f64]);
        }
    }
    Ok(o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let input = cpp_input(i)?;
    let flat = ffi::eval(i.query.op(), &input).map_err(|e| harness(e.to_string()))?;
    to_output(&i.query, &flat)
}

// ---- output splitting ---------------------------------------------------------------

struct Split<'a> {
    flat: &'a [f64],
    at: usize,
    out: Output,
}

impl Split<'_> {
    fn take(&mut self, k: usize) -> Result<Vec<f64>, HarnessError> {
        let part = self
            .at
            .checked_add(k)
            .and_then(|end| self.flat.get(self.at..end))
            .ok_or_else(|| harness(format!("record too short ({} values)", self.flat.len())))?;
        self.at += k;
        Ok(part.to_vec())
    }

    fn put(&mut self, key: &str, v: Vec<f64>) {
        let out = std::mem::take(&mut self.out);
        self.out = out.values(key, v);
    }

    fn vals(&mut self, key: &str, k: usize) -> Result<(), HarnessError> {
        let v = self.take(k)?;
        self.put(key, v);
        Ok(())
    }

    /// A curve record `type has [dim n p m cpts knots]` under `pfx`.
    fn curve(&mut self, pfx: &str) -> Result<(), HarnessError> {
        self.vals(&format!("{pfx}type"), 1)?;
        let has = self.take(1)?;
        let set = has.first().copied() == Some(1.0);
        self.put(&format!("{pfx}has"), has);
        if set {
            let dims = self.take(4)?;
            let cnt = |i: usize| dims.get(i).copied().unwrap_or(f64::NAN);
            let ok = |x: f64| x.fract() == 0.0 && (0.0..=1e6).contains(&x);
            let (n, m) = (cnt(1), cnt(3));
            if !(ok(n) && ok(m)) {
                return Err(harness(format!("bad curve dims {dims:?}")));
            }
            self.put(&format!("{pfx}dims"), dims);
            self.vals(&format!("{pfx}cpts"), 4 * (n as usize + 1))?;
            self.vals(&format!("{pfx}knots"), m as usize + 1)?;
        }
        Ok(())
    }
}

fn to_output(q: &Query, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut s = Split {
        flat,
        at: 0,
        out: Output::new(),
    };
    match q {
        Query::SrfSrf { .. } => {
            let cs = s.take(2)?;
            let built = cs.iter().all(|&c| c == 0.0);
            s.put("cs", cs);
            if built {
                s.vals("status", 1)?;
                s.curve("c3d_")?;
                s.curve("uv1_")?;
                s.curve("uv2_")?;
                s.vals("dir", 3)?;
            }
        }
        Query::RailIntRail { .. }
        | Query::RailIntEdgeuse { .. }
        | Query::CrossSection { .. }
        | Query::SetBack { .. } => {
            s.vals("status", 1)?;
            s.vals("point", 3)?;
            s.vals("fil_status", 1)?;
        }
        Query::RailRail { .. } => {
            let cs = s.take(1)?;
            let built = cs.first().copied() == Some(0.0);
            s.put("cs", cs);
            if built {
                s.vals("status", 1)?;
                s.vals("edge_status", 1)?;
                s.curve("edge_")?;
                s.curve("prim_uv_")?;
                s.curve("mate_uv_")?;
            }
        }
        Query::SplitFace { .. } => {
            s.vals("status", 1)?;
            s.vals("orig_face", 1)?;
            let n = s.take(1)?;
            let k = n.first().copied().unwrap_or(f64::NAN);
            if !(k.fract() == 0.0 && (0.0..=1e6).contains(&k)) {
                return Err(harness(format!("bad split face count {k}")));
            }
            s.put("n_split", n);
            s.vals("split", k as usize)?;
        }
    }
    if s.at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - s.at
        )));
    }
    Ok(s.out)
}

// ---- Rust side -----------------------------------------------------------------------

/// A Rust result: the flat record, or the unported callee that stopped it.
enum Rec {
    Flat(Vec<f64>),
    Deferred(SolidError),
}

/// `0` for success, else the status of the error; an unported callee defers.
fn st<T>(r: &solidcore::Result<T>) -> Result<f64, SolidError> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) if e.is_unported() => Err(e.clone()),
        Err(e) => Ok(status(e)),
    }
}

/// The B-spline part of a curve of a kind derived from `SmBSplineCurve`.
const fn b_spline_part(c: &Curve) -> Option<&BSplineCurve> {
    match c.kind() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => Some(l.base()),
        CurveKind::Ellipse(e) => Some(e.base()),
        CurveKind::Hyperbola(h) => Some(h.base()),
        CurveKind::Parabola(p) => Some(p.base()),
        _ => None,
    }
}

/// The curve record `type has [..]`.
fn dump_curve(o: &mut Vec<f64>, c: Option<&Curve>) {
    o.push(c.map_or(0.0, |c| c.type_code() as f64));
    dump_bcurve(o, c.and_then(b_spline_part));
}

fn surf(k: &Kernel, s: &SurfIn) -> Result<solidcore::Result<Surface>, HarnessError> {
    Ok(match s {
        SurfIn::BSpline(srf) => Ok(surface_of(srf)?),
        SurfIn::Analytic(a) => analytic_surface(k, a),
    })
}

fn rust_srf_srf(i: &Inputs) -> Result<Rec, HarnessError> {
    let Query::SrfSrf {
        s1,
        s2,
        start,
        end,
        dir,
        end_dir,
        tol,
        ang,
        skip,
        ref_point,
    } = &i.query
    else {
        return Err(harness("not a srf_srf query"));
    };
    let k = Kernel::default();
    let mut o = Vec::new();
    let build = |s: &SurfIn| -> Result<Result<Surface, f64>, HarnessError> {
        Ok(match surf(&k, s)? {
            Ok(s) => Ok(s),
            Err(e) if e.is_unported() => return Err(harness(e.to_string())),
            Err(e) => Err(status(&e)),
        })
    };
    let a = build(s1)?;
    let b = build(s2)?;
    let (a, b) = match (a, b) {
        (Ok(a), Ok(b)) => {
            o.extend([0.0, 0.0]);
            (a, b)
        }
        (a, b) => {
            o.push(a.err().unwrap_or(0.0));
            o.push(b.err().unwrap_or(0.0));
            return Ok(Rec::Flat(o));
        }
    };
    let mut d = v3(dir.unwrap_or([0.0; 3]));
    let r = srf_srf_intersection(
        &k,
        start.map(v3),
        end.map(v3),
        if dir.is_some() { Some(&mut d) } else { None },
        end_dir.map(v3),
        &a,
        &b,
        *tol,
        *ang,
        SrfSrfOptions {
            skip_two_pnts_intersection: *skip,
            ref_point: ref_point.map(v3),
        },
    );
    o.push(match st(&r) {
        Ok(x) => x,
        Err(e) => return Ok(Rec::Deferred(e)),
    });
    let curves = r.unwrap_or_default();
    dump_curve(&mut o, curves.curve_3d.as_ref());
    dump_curve(&mut o, curves.uv_curve1.as_ref());
    dump_curve(&mut o, curves.uv_curve2.as_ref());
    o.extend([d.x, d.y, d.z]);
    Ok(Rec::Flat(o))
}

fn rust(i: &Inputs) -> Result<Rec, HarnessError> {
    if matches!(i.query, Query::SrfSrf { .. }) {
        return rust_srf_srf(i);
    }
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(&i.model)?;
    let mut solvers: Vec<FilletSolver> = i
        .solvers
        .iter()
        .map(|s| FilletSolver::new(s.approx, s.ang, s.tang))
        .collect();
    for g in 0..i.model.geoms.len() {
        if let Ok(si) = u32::try_from(geom_solver(i, g)) {
            let gid = rm.geom(g)?;
            rm.fb.geom_mut(gid).map_err(h)?.solver = Some(FilletSolverId(si));
        }
    }
    let k = Kernel::default();
    let mut o = Vec::new();
    macro_rules! status_of {
        ($r:expr) => {
            match st(&$r) {
                Ok(x) => x,
                Err(e) => return Ok(Rec::Deferred(e)),
            }
        };
    }
    match &i.query {
        Query::SrfSrf { .. } => {}
        Query::RailIntRail { v }
        | Query::RailIntEdgeuse { v, .. }
        | Query::CrossSection { v, .. }
        | Query::SetBack { v } => {
            let vid = rm.vertex(*v)?;
            let r = match &i.query {
                Query::RailIntRail { .. } => {
                    rm.fb.calc_rail_int_rail(&k, vid, &solvers, &mut rm.target)
                }
                Query::RailIntEdgeuse { g, .. } => {
                    let g = match usize::try_from(*g) {
                        Ok(g) => Some(rm.geom(g)?),
                        Err(_) => None,
                    };
                    rm.fb
                        .calc_rail_int_edgeuse(&k, vid, g, &mut solvers, &mut rm.target)
                }
                Query::CrossSection { param, .. } => {
                    rm.fb
                        .calc_vert_on_cross_section(&k, vid, *param, &solvers, &mut rm.target)
                }
                _ => rm.fb.calc_set_back_vert(&k, vid, &solvers, &rm.target),
            };
            o.push(status_of!(r));
            let p: Point3 = rm.fb.vertex_point(vid).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(rm.fb.fillet_vertex(vid).map_err(h)?.status() as i64 as f64);
        }
        Query::RailRail { e, face, face_tol } => {
            let eid: EdgeId = rm.edge(*e)?;
            if let Some(s) = face {
                let surface = match surf(&k, s)? {
                    Ok(s) => s,
                    Err(e) if e.is_unported() => return Ok(Rec::Deferred(e)),
                    Err(e) => {
                        o.push(status(&e));
                        return Ok(Rec::Flat(o));
                    }
                };
                let domain = surface.natural_uv_domain();
                let shell = rm.target.add_shell().map_err(h)?;
                let f: FaceId = rm.target.add_face(shell).map_err(h)?;
                let fr = rm.target.face_mut(f).map_err(h)?;
                fr.surface = Some(surface);
                fr.uv_domain = domain;
                // SetTolerance(SmZoneTol3d(tol), FALSE, FALSE)
                fr.tolerance = zone_tol3d(&k, *face_tol);
                rm.fb
                    .fillet_edge_mut(eid)
                    .map_err(h)?
                    .set_original_face(Some(f));
            }
            o.push(0.0);
            let r = rm
                .fb
                .calc_rail_rail_interpolation(&k, eid, None, &solvers, &mut rm.target);
            o.push(status_of!(r));
            o.push(rm.fb.fillet_edge(eid).map_err(h)?.status() as i64 as f64);
            let brep = rm.fb.brep();
            dump_curve(&mut o, edge::curve(brep, eid).map_err(h)?);
            let prim = eu1::primary_edgeuse(brep, eid).map_err(h)?;
            let uv_of = |eu: Option<solidcore::topology::EdgeuseId>| -> Result<Option<&Curve>, HarnessError> {
                match eu {
                    None => Ok(None),
                    Some(eu) => Ok(eu1::uv_trim_curve(brep, eu).map_err(h)?.0),
                }
            };
            dump_curve(&mut o, uv_of(prim)?);
            let mate = match prim {
                Some(p) => eu1::mate(brep, p).map_err(h)?,
                None => None,
            };
            dump_curve(&mut o, uv_of(mate)?);
        }
        Query::SplitFace { e, set, f1, f2 } => {
            let eid = rm.edge(*e)?;
            let face = |x: i64| -> Result<Option<FaceId>, HarnessError> {
                match usize::try_from(x) {
                    Ok(i) => rm
                        .target_faces
                        .get(i)
                        .copied()
                        .map(Some)
                        .ok_or_else(|| harness("face index")),
                    Err(_) => Ok(None),
                }
            };
            let (fs, of, nf) = (face(*set)?, face(*f1)?, face(*f2)?);
            rm.fb.fillet_edge_mut(eid).map_err(h)?.set_original_face(fs);
            let r = rm.fb.update_edge_split_face(eid, of, nf);
            o.push(status_of!(r));
            let index = |f: Option<FaceId>| {
                f.and_then(|f| rm.target_faces.iter().position(|&x| x == f))
                    .map_or(-1.0, |i| i as f64)
            };
            let fe = rm.fb.fillet_edge(eid).map_err(h)?;
            o.push(index(fe.original_face()));
            o.push(fe.split_faces().len() as f64);
            for &f in fe.split_faces() {
                o.push(index(Some(f)));
            }
        }
    }
    Ok(Rec::Flat(o))
}

// ---- generation ----------------------------------------------------------------------

fn v3a(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// A plane through `o` with unit-ish axes `x`, `y` (its NURB made), domain around 0.
fn plane_in(o: [f64; 3], x: [f64; 3], y: [f64; 3], scale: f64, rng: &mut Rng) -> AnalyticSrf {
    let d = [rng.uniform(-1.5, -0.5), rng.uniform(-1.5, -0.5)];
    AnalyticSrf::Plane {
        origin: o,
        x_axis: x,
        y_axis: y,
        uv_scale: [scale, scale],
        domain: [d, [d[0] + rng.uniform(1.5, 3.0), d[1] + rng.uniform(1.5, 3.0)]],
        make_nurb: rng.chance(0.95),
    }
}

/// A unit vector perpendicular-ish to `n`.
fn perp(n: [f64; 3]) -> [f64; 3] {
    let a = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
    let c = [
        n[1] * a[2] - n[2] * a[1],
        n[2] * a[0] - n[0] * a[2],
        n[0] * a[1] - n[1] * a[0],
    ];
    let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    [c[0] / l, c[1] / l, c[2] / l]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit(rng: &mut Rng) -> [f64; 3] {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l > 0.2 && l <= 1.0 {
            return [v[0] / l, v[1] / l, v[2] / l];
        }
    }
}

/// A plane through `p` with normal `n`.
fn plane_through(p: [f64; 3], n: [f64; 3], scale: f64, rng: &mut Rng) -> AnalyticSrf {
    let x = perp(n);
    let y = cross(n, x);
    plane_in(p, x, y, scale, rng)
}

/// A B-spline surface standing up (its z and x swapped), crossing a lying one.
fn standing(mut s: Srf, shift: f64) -> Srf {
    for row in &mut s.net {
        for p in row.iter_mut() {
            let w = if p[3] == NOW { 1.0 } else { p[3] };
            let (x, z) = (p[0], p[2]);
            p[0] = z + shift * w;
            p[2] = x - 0.5 * shift * w;
        }
    }
    s
}

/// A pair of surfaces that (mostly) intersect.
fn gen_pair(k: &Kernel, rng: &mut Rng, scale: f64) -> (SurfIn, SurfIn) {
    match rng.below(20) {
        // plane / plane: crossing, sometimes parallel or coincident
        0..=6 => {
            let o = rng.point3(-scale, scale);
            let n1 = unit(rng);
            let n2 = match rng.below(8) {
                0 => n1,
                1 => [-n1[0], -n1[1], -n1[2]],
                _ => unit(rng),
            };
            let o2 = if rng.chance(0.1) {
                [o[0] + scale, o[1], o[2]]
            } else {
                o
            };
            (
                SurfIn::Analytic(plane_through(o, n1, scale, rng)),
                SurfIn::Analytic(plane_through(o2, n2, scale, rng)),
            )
        }
        // B-spline / plane through one of its points
        7..=9 => {
            let s = gen_surface(rng, scale);
            let p = surface_of(&s)
                .ok()
                .and_then(|rs| {
                    let d = rs.natural_uv_domain();
                    rs.evaluate_point(k, d.evaluate(rng.uniform(0.2, 0.8), rng.uniform(0.2, 0.8)))
                        .ok()
                })
                .map_or([0.5 * scale, 0.5 * scale, 0.0], v3a);
            let n = if rng.chance(0.5) {
                let mut n = unit(rng);
                n[2] *= 0.2;
                let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                [n[0] / l, n[1] / l, n[2] / l]
            } else {
                unit(rng)
            };
            let pl = SurfIn::Analytic(plane_through(p, n, scale, rng));
            if rng.chance(0.5) {
                (SurfIn::BSpline(s), pl)
            } else {
                (pl, SurfIn::BSpline(s))
            }
        }
        // surface of revolution / plane (circle, ellipse or lines)
        10..=12 => {
            let rev = gen_analytic(rng, scale);
            let rev = match rev {
                AnalyticSrf::Revolution { .. } => rev,
                plane @ AnalyticSrf::Plane { .. } => {
                    return (
                        SurfIn::Analytic(plane),
                        SurfIn::Analytic(plane_through(rng.point3(-scale, scale), unit(rng), scale, rng)),
                    )
                }
            };
            let h = rng.uniform(0.2, 0.8) * scale;
            let n = match rng.below(3) {
                0 => [0.0, 0.0, 1.0],
                1 => [1.0, 0.0, 0.0],
                _ => {
                    let mut n = unit(rng);
                    n[2] = n[2].abs() + 0.5;
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
                    [n[0] / l, n[1] / l, n[2] / l]
                }
            };
            let pl = SurfIn::Analytic(plane_through([0.0, 0.0, h], n, scale * 2.0, rng));
            if rng.chance(0.5) {
                (SurfIn::Analytic(rev), pl)
            } else {
                (pl, SurfIn::Analytic(rev))
            }
        }
        // B-spline / B-spline crossing
        _ => {
            let a = gen_surface(rng, scale);
            let b = standing(gen_surface(rng, scale), rng.uniform(0.3, 0.7) * scale);
            (SurfIn::BSpline(a), SurfIn::BSpline(b))
        }
    }
}

/// The Rust intersection curve of a pair without points (for sampling points on it).
fn probe_curve(s1: &SurfIn, s2: &SurfIn, tol: f64, ang: f64) -> Option<Curve> {
    let k = Kernel::default();
    let a = surf(&k, s1).ok()?.ok()?;
    let b = surf(&k, s2).ok()?.ok()?;
    srf_srf_intersection(&k, None, None, None, None, &a, &b, tol, ang, SrfSrfOptions::default())
        .ok()?
        .curve_3d
}

fn gen_srf_srf(k: &Kernel, rng: &mut Rng) -> Inputs {
    let scale = scale_of(rng);
    let (s1, s2) = gen_pair(k, rng, scale);
    let tol = scale * *rng.choose(&[1e-6, 1e-6, 1e-4, 1e-8, 1e-3, 1e-12, 1e-2]).unwrap_or(&1e-6);
    let ang = *rng
        .choose(&[0.5235987755982988, 0.1, 0.5, 0.01, 1.2])
        .unwrap_or(&0.5);
    let mut start = None;
    let mut end = None;
    let mut dir = None;
    let mut end_dir = None;
    let mut ref_point = None;
    let mut skip = rng.chance(0.3);
    match rng.below(10) {
        // no points: the longest curve
        0 | 1 => {}
        // a reference point near the surfaces
        2 => ref_point = Some(rng.point3(-scale, scale)),
        // points on the curve
        _ => {
            if let Some(c) = probe_curve(&s1, &s2, tol, ang) {
                let ivl = c.natural_interval();
                let at = |t: f64| c.evaluate(k, ivl.evaluate(t), 1, true, true).ok();
                let (t0, t1) = match rng.below(5) {
                    0 => (0.0, 1.0),
                    1 => (0.8, 0.2), // reversed
                    2 => (0.5, 0.5), // coincident
                    _ => (rng.uniform(0.0, 0.5), rng.uniform(0.5, 1.0)),
                };
                if let (Some(a), Some(b)) = (at(t0), at(t1)) {
                    let (pa, ta) = (a.first().copied(), a.get(1).copied());
                    let (pb, tb) = (b.first().copied(), b.get(1).copied());
                    let off = match rng.below(6) {
                        0 => tol * 0.5,
                        1 => tol * 50.0,
                        2 => tol * 1e4,
                        _ => 0.0,
                    };
                    start = pa.map(|p| [p.x + off, p.y, p.z - off]);
                    if rng.chance(0.75) {
                        end = pb.map(|p| [p.x, p.y - off, p.z]);
                    }
                    if rng.chance(0.8) {
                        let sign = if rng.chance(0.2) { -1.0 } else { 1.0 };
                        dir = ta.map(|t: Vec3| [sign * t.x, sign * t.y, sign * t.z]);
                        if rng.chance(0.05) {
                            dir = Some([0.0, 0.0, 0.0]);
                        }
                    }
                    if rng.chance(0.3) {
                        end_dir = tb.map(|t: Vec3| [t.x, t.y, t.z]);
                    }
                }
            } else {
                start = Some(rng.point3(-scale, scale));
                dir = Some(unit(rng));
                skip = false;
            }
        }
    }
    Inputs {
        model: empty_model(),
        solvers: Vec::new(),
        geom_solvers: Vec::new(),
        query: Query::SrfSrf {
            s1: Box::new(s1),
            s2: Box::new(s2),
            start,
            end,
            dir,
            end_dir,
            tol,
            ang,
            skip,
            ref_point,
        },
    }
}

/// A vertex calculator on p1's random model (the queried vertex without a corner).
fn gen_vertex_calc(rng: &mut Rng) -> Inputs {
    let mut model = gen_model(rng);
    let nv = model.vertices.len().max(1);
    let ng = model.geoms.len();
    let v = rng.below(nv as u64) as usize;
    if let Some(vx) = model.vertices.get_mut(v) {
        vx.corner = -1;
    }
    let ns = rng.below(3) as usize;
    let solvers = (0..ns)
        .map(|_| SolverIn {
            approx: *rng.choose(&[1e-6, 1e-3, 1e-9]).unwrap_or(&1e-6),
            ang: rng.uniform(0.01, 0.5),
            tang: rng.uniform(0.001, 0.1),
        })
        .collect::<Vec<_>>();
    let geom_solvers = (0..ng)
        .map(|_| {
            if ns > 0 && rng.chance(0.7) {
                rng.below(ns as u64) as i64
            } else {
                -1
            }
        })
        .collect();
    let query = match rng.below(4) {
        0 => Query::RailIntRail { v },
        1 => Query::RailIntEdgeuse {
            v,
            g: if ng > 0 && rng.chance(0.5) {
                rng.below(ng as u64) as i64
            } else {
                -1
            },
        },
        2 => Query::CrossSection {
            v,
            param: if rng.chance(0.5) {
                Some(rng.uniform(0.0, 1.0))
            } else {
                None
            },
        },
        _ => Query::SetBack { v },
    };
    Inputs {
        model,
        solvers,
        geom_solvers,
        query,
    }
}

/// A 3D line segment curve from `a` to `b`, extended a little past both ends.
fn seg(a: [f64; 3], b: [f64; 3], ext: f64) -> Crv {
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    Crv {
        dim: 3,
        cpts: vec![
            [a[0] - ext * d[0], a[1] - ext * d[1], a[2] - ext * d[2], NOW],
            [b[0] + ext * d[0], b[1] + ext * d[1], b[2] + ext * d[2], NOW],
        ],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A quadratic 3D curve from `a` to `b` through `m`.
fn arc3(a: [f64; 3], m: [f64; 3], b: [f64; 3]) -> Crv {
    Crv {
        dim: 3,
        cpts: vec![[a[0], a[1], a[2], NOW], [m[0], m[1], m[2], NOW], [b[0], b[1], b[2], NOW]],
        p: 2,
        u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
    }
}

/// A rail/rail interpolation edge `0 -> 1` with incoming rails from vertices `2` and `3`,
/// its original face a plane, a B-spline or a surface of revolution.
#[allow(clippy::too_many_lines)]
fn gen_rail_rail(rng: &mut Rng) -> Option<Inputs> {
    let scale = scale_of(rng);
    let k = Kernel::default();
    let face = match rng.below(6) {
        0..=2 => SurfIn::Analytic(AnalyticSrf::Plane {
            origin: rng.point3(-scale, scale),
            x_axis: [1.0, 0.0, rng.uniform(-0.2, 0.2)],
            y_axis: [0.0, 1.0, rng.uniform(-0.2, 0.2)],
            uv_scale: [scale, scale],
            // a domain far larger than the edge, so the drop rarely clamps
            domain: [[-10.0, -10.0], [10.0, 10.0]],
            make_nurb: rng.chance(0.95),
        }),
        3 | 4 => SurfIn::BSpline(gen_surface(rng, scale)),
        _ => SurfIn::Analytic(gen_analytic(rng, scale)),
    };
    let rs = surf(&k, &face).ok()?.ok()?;
    let d = rs.natural_uv_domain();
    let at = |a: f64, b: f64| -> Option<([f64; 3], [f64; 2])> {
        let uv = d.evaluate(a, b);
        rs.evaluate_point(&k, uv).ok().map(|p| (v3a(p), [uv.x, uv.y]))
    };
    // the edge's two vertices and the rails' far ends
    // fractions of the domain (the planes' domains are large: points near the middle)
    let big = matches!(face, SurfIn::Analytic(AnalyticSrf::Plane { .. }));
    let f = |x: f64| if big { 0.45 + 0.1 * x } else { x };
    let (p0, uv0) = at(f(rng.uniform(0.3, 0.45)), f(rng.uniform(0.2, 0.4)))?;
    let (p1, uv1) = at(f(rng.uniform(0.55, 0.7)), f(rng.uniform(0.2, 0.4)))?;
    let (p2, uv2) = at(f(rng.uniform(0.2, 0.4)), f(rng.uniform(0.6, 0.9)))?;
    let (p3, uv3) = at(f(rng.uniform(0.6, 0.8)), f(rng.uniform(0.6, 0.9)))?;
    let mut model = empty_model();
    let mate_type = 3; // SM_FV_MATE
    for (p, uv) in [(p0, uv0), (p1, uv1), (p2, uv2), (p3, uv3)] {
        let mut v = vertex_at(p);
        v.vtype = if rng.chance(0.85) { mate_type } else { rng.below(14) as i64 };
        v.uv = uv;
        if rng.chance(0.1) {
            // off the face
            v.point = [p[0] + 1e-3 * scale, p[1], p[2] - 1e-3 * scale];
        }
        model.vertices.push(v);
    }
    let rail = |rng: &mut Rng, from: usize, to: usize, pf: [f64; 3], pt: [f64; 3], uf: [f64; 2], ut: [f64; 2]| {
        let curve = if rng.chance(0.6) {
            seg(pf, pt, *rng.choose(&[0.0, 0.0, 0.1, 0.5]).unwrap_or(&0.0))
        } else {
            let m = [
                0.5 * (pf[0] + pt[0]) + rng.uniform(-0.1, 0.1) * scale,
                0.5 * (pf[1] + pt[1]) + rng.uniform(-0.1, 0.1) * scale,
                0.5 * (pf[2] + pt[2]),
            ];
            arc3(pf, m, pt)
        };
        let uv = if rng.chance(0.75) {
            Some(uv_line(uf, ut))
        } else {
            None
        };
        EdgeIn {
            vs: from,
            ve: to,
            etype: 9, // SM_FE_RAIL
            status: 0,
            corner: -1,
            curve: Some(curve),
            interval: Some([0.0, 1.0]),
            fbe: [-1, -1],
            eus: [
                EuIn {
                    uv_curve: None,
                    geom: -1,
                    vu_uv: [0.0, 0.0],
                },
                EuIn {
                    uv_curve: uv,
                    geom: -1,
                    vu_uv: [0.0, 0.0],
                },
            ],
        }
    };
    // the interpolation edge first (it is queried as edge 0)
    model.edges.push(EdgeIn {
        vs: 0,
        ve: 1,
        etype: 11, // SM_FE_RAIL_RAIL_INTERPOLATION
        status: 0,
        corner: -1,
        curve: None,
        interval: None,
        fbe: [-1, -1],
        eus: [
            EuIn {
                uv_curve: None,
                geom: -1,
                vu_uv: [0.0, 0.0],
            },
            EuIn {
                uv_curve: None,
                geom: -1,
                vu_uv: [0.0, 0.0],
            },
        ],
    });
    // incoming rails, either direction
    let r1 = if rng.chance(0.5) {
        rail(rng, 2, 0, p2, p0, uv2, uv0)
    } else {
        rail(rng, 0, 2, p0, p2, uv0, uv2)
    };
    let r2 = if rng.chance(0.5) {
        rail(rng, 3, 1, p3, p1, uv3, uv1)
    } else {
        rail(rng, 1, 3, p1, p3, uv1, uv3)
    };
    model.edges.push(r1);
    if rng.chance(0.97) {
        model.edges.push(r2);
    }
    Some(Inputs {
        model,
        solvers: Vec::new(),
        geom_solvers: Vec::new(),
        query: Query::RailRail {
            e: 0,
            face_tol: scale * *rng.choose(&[1e-6, 1e-6, 1e-4, 1e-9, 0.0]).unwrap_or(&1e-6),
            face: if rng.chance(0.03) {
                None
            } else {
                Some(face)
            },
        },
    })
}

/// `UpdateSplitFace` on p1's random model with target faces.
fn gen_split_face(rng: &mut Rng) -> Inputs {
    let mut model = gen_model(rng);
    let scale = scale_of(rng);
    while model.faces.len() < 2 {
        model.faces.push(gen_surface(rng, scale));
    }
    let nf = model.faces.len() as u64;
    let ne = model.edges.len().max(1) as u64;
    let e = rng.below(ne) as usize;
    let pick = |rng: &mut Rng| {
        if rng.chance(0.15) {
            -1
        } else {
            rng.below(nf) as i64
        }
    };
    let f1 = pick(rng);
    // the original face is never the split face (the shim refuses that path)
    let set = pick(rng);
    let f2 = pick(rng);
    Inputs {
        model,
        solvers: Vec::new(),
        geom_solvers: Vec::new(),
        query: Query::SplitFace { e, set, f1, f2 },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_geom_p2"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        cpp(&i)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        // an input the FFI refuses is a harness error on both sides
        ffi::validate(i.query.op(), &cpp_input(&i)?).map_err(|e| harness(e.to_string()))?;
        match rust(&i)? {
            Rec::Flat(flat) => to_output(&i.query, &flat),
            Rec::Deferred(e) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())))
            }
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        // keep only inputs the shim runs
        for _ in 0..64 {
            let inputs = match rng.below(20) {
                0..=12 => gen_srf_srf(k, rng),
                13 | 14 => gen_vertex_calc(rng),
                15..=18 => match gen_rail_rail(rng) {
                    Some(i) => i,
                    None => continue,
                },
                _ => gen_split_face(rng),
            };
            let Ok(flat) = cpp_input(&inputs) else {
                continue;
            };
            if ffi::eval(inputs.query.op(), &flat).is_ok() {
                return serde_json::to_value(inputs).ok();
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tallies the Rust outcome of every case per query (status `0`, an upstream error
    /// status, a deferral): with no failed case, the passes' success-path / error-path
    /// split. `CASE_SEEDS` sets the generated count.
    #[test]
    #[ignore = "census of the cases; run on demand"]
    fn case_census() {
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: std::env::var("CASE_SEEDS").map_or(MIN_GENERATED, |s| s.parse().unwrap()),
        };
        let cases = Runner.cases(&ctx).unwrap();
        let mut tally = std::collections::BTreeMap::<(String, String), usize>::new();
        for case in &cases {
            let i: Inputs = case.parse().unwrap();
            let qv = serde_json::to_value(&i.query).unwrap();
            let mut q = qv["q"].as_str().unwrap().to_string();
            for key in ["face", "s1", "s2"] {
                if let Some(f) = qv.get(key).and_then(|f| f.as_object()) {
                    let kind = f.keys().next().cloned().unwrap_or_default();
                    let sub = f
                        .get("analytic")
                        .and_then(|a| a.get("kind"))
                        .and_then(|k| k.as_str())
                        .unwrap_or("");
                    q = format!("{q} {key}={kind}{sub}");
                }
            }
            let kind = match Runner.run_rust(case) {
                Ok(o) if o.deferred.is_some() => {
                    format!("deferred {}", o.deferred.map(|d| d.unit).unwrap_or_default())
                }
                Ok(o) => match o.values.get("status").and_then(|v| v.first()).copied() {
                    Some(s) => {
                        if s == 0.0 {
                            "status0".to_string()
                        } else {
                            format!("status {s}")
                        }
                    }
                    None => "construction".to_string(),
                },
                Err(e) => format!("harness {e}"),
            };
            if std::env::var("CASE_SHOW").is_ok_and(|w| q.contains(&w) && kind.contains("1001")) {
                println!("{}", serde_json::to_string(&case.inputs).unwrap());
            }
            *tally.entry((q, kind)).or_default() += 1;
        }
        for ((q, k), n) in tally {
            println!("{n:5} {q} {k}");
        }
    }
}
