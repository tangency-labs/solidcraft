//! Runner for unit `blend.fillet_geom_p4`: the UV trim curve between two fillet vertices
//! (`sm_CreateUVCurve`), the fillet-surface intersection vertex calculators, the rail
//! index of a fillet vertex, `MakeFaceBrep` and `CalcBlendingGeom`.
//!
//! C++: `solidcore_ffi::units::blend_fillet_geom_p4` (upstream
//! `source/SMLib/src/SmFilletGeom.cpp`). Rust: `solidcore::blend::fillet_geom::p4`.
//!
//! A case is `blend.fillet_geom_p1`'s pseudo-B-rep model (built the same way on both
//! sides), base fillet solvers attached to its geometries (tolerances only) and one
//! query. Both sides produce the same flat record (layout in the FFI module docs), split
//! into keys: `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`, `1006`
//! `SM_ERR_NULL_POINTER`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an
//! error upstream cannot produce) and the query's values: for `uv_curve` the surface's
//! construction status (`cs`) and the three curves (`c3d_*`, `uv1_*`, `uv2_*`: control
//! points and knots), for `rail_index` the index, for the vertex calculators the
//! vertex's point and fillet status.
//!
//! Deferred on the Rust side (unported callees): the face makers' unported callees in
//! `make_face` (`topology.brep_p3`) and the
//! unported callees of `sm_SrfSrfIntersection` (`SmSurface::DropCurve`,
//! `intersect.surface_drop_lib_p3`, ...) where reached. The shim refuses the inputs that would make upstream dereference
//! what the harness cannot build (a real fillet corner, a solver or rail it needs);
//! the generator keeps only inputs both sides accept. Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_geom::p1::{CornerLookup, FilletGeomId, FilletSolverId};
use solidcore::blend::fillet_geom::p4::create_uv_curve;
use solidcore::blend::fillet_solver::p1::FilletSolver;
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::surface::surface::Surface;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_geom_p4::{self as ffi, Op};

use super::blend_fillet_geom_p1::{
    analytic_surface, b_spline_part, dump_bcurve, gen_curve, gen_cylinder, gen_model, gen_surface, push_model,
    push_srf, scale_of, status, surface_of, AnalyticSrf, Crv, EdgeIn, EuIn, GeomIn, Model,
    RustModel, Srf, VertexIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 300;

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs ------------------------------------------------------------------------

/// A base fillet solver's tolerances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct SolverIn {
    pub(crate) approx: f64,
    pub(crate) ang: f64,
    pub(crate) tang: f64,
}

/// The surface of a `uv_curve` query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UvSurface {
    /// A B-spline surface.
    BSpline(Srf),
    /// A real `SmPlane` or `SmSurfOfRevolution`.
    Analytic(AnalyticSrf),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
enum Query {
    UvCurve {
        sv: usize,
        ev: usize,
        two: bool,
        surface: UvSurface,
        tol: f64,
    },
    RailIndex {
        g: usize,
        v: i64,
    },
    RailIntExtended {
        v: usize,
    },
    Int2Fillets {
        v: usize,
    },
    Nx2 {
        v: usize,
    },
    MakeFace {
        g: usize,
    },
    Blending {
        g: usize,
    },
}

impl Query {
    const fn op(&self) -> Op {
        match self {
            Self::UvCurve { .. } => Op::UvCurve,
            Self::RailIndex { .. } => Op::RailIndex,
            Self::RailIntExtended { .. } => Op::RailIntExtended,
            Self::Int2Fillets { .. } => Op::Int2Fillets,
            Self::Nx2 { .. } => Op::Nx2,
            Self::MakeFace { .. } => Op::MakeFace,
            Self::Blending { .. } => Op::Blending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    model: Model,
    #[serde(default)]
    solvers: Vec<SolverIn>,
    /// Per geometry: its solver index, `-1` (or missing) for none.
    #[serde(default)]
    geom_solvers: Vec<i64>,
    query: Query,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CaseInputs {
    #[serde(flatten)]
    inputs: Inputs,
}

// ---- C++ side ------------------------------------------------------------------------

pub(crate) fn push_analytic(o: &mut Vec<f64>, a: &AnalyticSrf) {
    match a {
        AnalyticSrf::Plane {
            origin,
            x_axis,
            y_axis,
            uv_scale,
            domain,
            make_nurb,
        } => {
            o.push(1.0);
            o.extend_from_slice(origin);
            o.extend_from_slice(x_axis);
            o.extend_from_slice(y_axis);
            o.extend_from_slice(uv_scale);
            o.extend([domain[0][0], domain[0][1], domain[1][0], domain[1][1]]);
            o.push(flag(*make_nurb));
        }
        AnalyticSrf::Revolution {
            a,
            b,
            axis_point,
            axis_dir,
            swap,
        } => {
            o.push(2.0);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
            o.extend_from_slice(axis_point);
            o.extend_from_slice(axis_dir);
            o.push(flag(*swap));
        }
    }
}

fn geom_solver(i: &Inputs, g: usize) -> i64 {
    i.geom_solvers.get(g).copied().unwrap_or(-1)
}

fn cpp_input(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o = Vec::new();
    push_model(&mut o, &i.model)?;
    o.push(i.solvers.len() as f64);
    for s in &i.solvers {
        o.extend([s.approx, s.ang, s.tang]);
    }
    for g in 0..i.model.geoms.len() {
        o.push(geom_solver(i, g) as f64);
    }
    match &i.query {
        Query::UvCurve {
            sv,
            ev,
            two,
            surface,
            tol,
        } => {
            o.extend([*sv as f64, *ev as f64, flag(*two)]);
            match surface {
                UvSurface::BSpline(s) => {
                    o.push(0.0);
                    push_srf(&mut o, s)?;
                }
                UvSurface::Analytic(a) => push_analytic(&mut o, a),
            }
            o.push(*tol);
        }
        Query::RailIndex { g, v } => o.extend([*g as f64, *v as f64]),
        Query::RailIntExtended { v } | Query::Int2Fillets { v } | Query::Nx2 { v } => {
            o.push(*v as f64);
        }
        Query::MakeFace { g } | Query::Blending { g } => o.push(*g as f64),
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

    /// A curve record `has [dim n p m cpts knots]` under `pfx`.
    fn curve(&mut self, pfx: &str) -> Result<(), HarnessError> {
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
        Query::UvCurve { .. } => {
            let cs = s.take(1)?;
            let built = cs.first().copied() == Some(0.0);
            s.put("cs", cs);
            if built {
                s.vals("status", 1)?;
                s.curve("c3d_")?;
                s.curve("uv1_")?;
                s.curve("uv2_")?;
            }
        }
        Query::RailIndex { .. } => {
            s.vals("status", 1)?;
            s.vals("idx", 1)?;
        }
        Query::RailIntExtended { .. } | Query::Int2Fillets { .. } | Query::Nx2 { .. } => {
            s.vals("status", 1)?;
            s.vals("point", 3)?;
            s.vals("fil_status", 1)?;
        }
        Query::MakeFace { .. } | Query::Blending { .. } => s.vals("status", 1)?,
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

fn uv_surface(k: &Kernel, s: &UvSurface) -> Result<solidcore::Result<Surface>, HarnessError> {
    Ok(match s {
        UvSurface::BSpline(srf) => Ok(surface_of(srf)?),
        UvSurface::Analytic(a) => analytic_surface(k, a),
    })
}

fn rust(i: &Inputs) -> Result<Rec, HarnessError> {
    let h = |e: SolidError| harness(e.to_string());
    let mut rm = RustModel::build(&i.model)?;
    let solvers: Vec<FilletSolver> = i
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
    let mut k = Kernel::default();
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
        Query::UvCurve {
            sv,
            ev,
            two,
            surface,
            tol,
        } => {
            let surface = match uv_surface(&k, surface)? {
                Ok(s) => s,
                Err(e) if e.is_unported() => return Ok(Rec::Deferred(e)),
                Err(e) => {
                    o.push(status(&e));
                    return Ok(Rec::Flat(o));
                }
            };
            o.push(0.0);
            let r = create_uv_curve(
                &k,
                &rm.fb,
                rm.vertex(*sv)?,
                rm.vertex(*ev)?,
                *two,
                &surface,
                *tol,
            );
            o.push(status_of!(r));
            let curves = r.unwrap_or_default();
            dump_bcurve(&mut o, curves.curve_3d.as_ref().and_then(b_spline_part));
            dump_bcurve(&mut o, curves.uv_curve1.as_ref().and_then(b_spline_part));
            dump_bcurve(&mut o, curves.uv_curve2.as_ref());
        }
        Query::RailIndex { g, v } => {
            let v = match usize::try_from(*v) {
                Ok(v) => Some(rm.vertex(v)?),
                Err(_) => None,
            };
            let r = rm.fb.rail_index_of_vertex(rm.geom(*g)?, v);
            o.push(status_of!(r));
            o.push(r.map_or(0.0, |x| x as f64));
        }
        Query::RailIntExtended { v } | Query::Int2Fillets { v } | Query::Nx2 { v } => {
            let vid = rm.vertex(*v)?;
            let mut target = rm.target.clone();
            let r = match &i.query {
                Query::RailIntExtended { .. } => rm
                    .fb
                    .calc_rail_int_extended_edgeuse(&k, vid, CornerLookup::default(), &solvers, &mut target),
                Query::Int2Fillets { .. } => {
                    rm.fb.calc_fillet_int_2_fillets(&k, vid, CornerLookup::default(), &solvers, &target)
                }
                _ => rm.fb.calc_nx2_vert_fillet_x_fillet(&k, vid, &solvers),
            };
            o.push(status_of!(r));
            let p: Point3 = rm.fb.vertex_point(vid).map_err(h)?;
            o.extend([p.x, p.y, p.z]);
            o.push(rm.fb.fillet_vertex(vid).map_err(h)?.status() as i64 as f64);
        }
        Query::MakeFace { g } => {
            let gid: FilletGeomId = rm.geom(*g)?;
            // `new (ctx) SmBrep()`: the constructor makes the void infinite region the
            // face makers put the face in
            let mut brep = Brep::new_with_infinite_region().map_err(h)?;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attributes = solidcore::geom::attribute::AttributeStore::default();
            // distinct identities for the face makers' notifications, keyed as the
            // face reference make_face_brep passes (faces from 1)
            let key_of = |e: solidcore::EntityRef| {
                use solidcore::EntityRef as E;
                let (type_code, key) = match e {
                    E::Face(f) => (
                        solidcore::topology::face::p2::TYPE,
                        1 + u64::from(f.index()),
                    ),
                    E::Edge(x) => (
                        solidcore::topology::edge::TYPE,
                        1 << 40 | u64::from(x.index()),
                    ),
                    E::Edgeuse(x) => (16005, 2 << 40 | u64::from(x.index())),
                    _ => (0, 3 << 40),
                };
                solidcore::geom::object::ObjectRef {
                    type_code,
                    key: solidcore::geom::attribute::ObjectKey(key),
                }
            };
            let mut env = solidcore::topology::edge::NotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                context: None,
                key_of: &key_of,
            };
            let r = rm.fb.make_face_brep(&mut k, gid, &solvers, &mut brep, &mut env);
            o.push(status_of!(r));
        }
        Query::Blending { g } => {
            let gid = rm.geom(*g)?;
            let target = rm.target.clone();
            let r = rm.fb.calc_blending_geom(&k, gid, &solvers, &target);
            o.push(status_of!(r));
        }
    }
    Ok(Rec::Flat(o))
}

// ---- generation ----------------------------------------------------------------------

fn v3a(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// A vertex at `p` with no classification (the model's defaults).
pub(crate) fn vertex_at(p: [f64; 3]) -> VertexIn {
    VertexIn {
        point: p,
        vtype: 0,
        status: 0,
        corner: -1,
        pc_class: 100,
        pc_kind: 0,
        pc_index: -1,
        pc_sub: 0,
        pc_face: -1,
        t_param: 0.0,
        uv: [0.0, 0.0],
        mates: [-1, -1],
    }
}

pub(crate) fn empty_model() -> Model {
    Model {
        targets: Vec::new(),
        faces: Vec::new(),
        fillet_brep: Vec::new(),
        vertices: Vec::new(),
        geoms: Vec::new(),
        edges: Vec::new(),
    }
}

/// A random analytic surface: a plane (scaled UVs, mostly with its NURB) or a surface of
/// revolution of a line (cylinder-like when parallel to the axis, else cone-like).
pub(crate) fn gen_analytic(rng: &mut Rng, scale: f64) -> AnalyticSrf {
    if rng.chance(0.5) {
        let d = [rng.uniform(-1.0, 0.0), rng.uniform(-1.0, 0.0)];
        AnalyticSrf::Plane {
            origin: rng.point3(-scale, scale),
            x_axis: [1.0, rng.uniform(-0.3, 0.3), 0.0],
            y_axis: [rng.uniform(-0.3, 0.3), 1.0, rng.uniform(-0.3, 0.3)],
            uv_scale: [rng.uniform(0.5, 2.0) * scale, rng.uniform(0.5, 2.0) * scale],
            domain: [
                d,
                [d[0] + rng.uniform(0.5, 2.0), d[1] + rng.uniform(0.5, 2.0)],
            ],
            make_nurb: rng.chance(0.9),
        }
    } else {
        let r = rng.uniform(0.2, 1.0) * scale;
        let h = rng.uniform(0.5, 2.0) * scale;
        let cone = rng.chance(0.4);
        AnalyticSrf::Revolution {
            a: [r, 0.0, 0.0],
            b: [if cone { r * rng.uniform(0.2, 0.9) } else { r }, 0.0, h],
            axis_point: [0.0, 0.0, 0.0],
            axis_dir: [0.0, 0.0, 1.0],
            swap: rng.chance(0.3),
        }
    }
}

/// The UV point at fractions `(a, b)` of the surface's domain and its 3D point.
fn on_surface(k: &Kernel, s: &Surface, a: f64, b: f64) -> Option<[f64; 3]> {
    // (runner helper that does not return Result)
    let d = s.natural_uv_domain_if_built();
    s.evaluate_point(k, d.evaluate(a, b)).ok().map(v3a)
}

/// A `uv_curve` case: a surface, two vertex points (on it, near it, on its seam or
/// corners, coincident or off it) and a tolerance; sometimes with p1's random model
/// around the two vertices (rail edges reach the vertex-use branch).
fn gen_uv_curve(rng: &mut Rng) -> Option<Inputs> {
    let scale = scale_of(rng);
    let k = Kernel::default();
    let kind = rng.below(4);
    let surface = match kind {
        0 => UvSurface::BSpline(gen_surface(rng, scale)),
        1 => {
            let h = scale * rng.uniform(0.5, 2.0);
            UvSurface::BSpline(gen_cylinder(rng, scale, h))
        }
        _ => UvSurface::Analytic(gen_analytic(rng, scale)),
    };
    let rs = uv_surface(&k, &surface).ok()?.ok()?;
    let closed_u = kind == 1
        || matches!(
            &surface,
            UvSurface::Analytic(AnalyticSrf::Revolution { .. })
        );
    let two = closed_u && rng.chance(0.4) || rng.chance(0.05);
    let frac = |rng: &mut Rng| *rng.choose(&[0.0, 1.0, 0.5, 0.25]).unwrap_or(&0.5);
    let (a, b) = if two {
        // seam ends: corners in v, sometimes a cut corner (mid v) or swapped
        let v0 = if rng.chance(0.2) {
            rng.uniform(0.1, 0.9)
        } else {
            0.0
        };
        let v1 = if rng.chance(0.2) {
            rng.uniform(0.1, 0.9)
        } else {
            1.0
        };
        let u = if rng.chance(0.5) { 0.0 } else { 1.0 };
        let (p, q) = (on_surface(&k, &rs, u, v0)?, on_surface(&k, &rs, u, v1)?);
        if rng.chance(0.5) {
            (p, q)
        } else {
            (q, p)
        }
    } else {
        let pick = |rng: &mut Rng| {
            let (u, v) = match rng.below(5) {
                0 => (frac(rng), rng.uniform(0.0, 1.0)),
                1 => (rng.uniform(0.0, 1.0), frac(rng)),
                _ => (rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)),
            };
            on_surface(&k, &rs, u, v)
        };
        let p = pick(rng)?;
        let mut q = pick(rng)?;
        match rng.below(10) {
            // an iso line in u or v
            0 => {
                let u = rng.uniform(0.0, 1.0);
                return Some(gen_uv_from(
                    rng,
                    surface,
                    scale,
                    on_surface(&k, &rs, u, 0.1)?,
                    on_surface(&k, &rs, u, 0.8)?,
                    false,
                ));
            }
            1 => q = p, // coincident
            2 => {
                // slightly off the surface
                let e = scale * 1e-7;
                q = [q[0] + e, q[1] - e, q[2] + e];
            }
            3 => {
                // far off the surface (minimise fallback)
                q = [q[0] + scale, q[1], q[2] - scale];
            }
            _ => {}
        }
        (p, q)
    };
    Some(gen_uv_from(rng, surface, scale, a, b, two))
}

fn gen_uv_from(
    rng: &mut Rng,
    surface: UvSurface,
    scale: f64,
    a: [f64; 3],
    b: [f64; 3],
    two: bool,
) -> Inputs {
    let tol = scale
        * *rng
            .choose(&[1e-6, 1e-4, 1e-3, 1e-8, 1e-2, 1e-12])
            .unwrap_or(&1e-6);
    let (model, sv, ev) = if rng.chance(0.3) {
        // p1's random model with the two vertices first; rails of geometries reach the
        // vertex-use branch
        let mut m = gen_model(rng);
        if let Some(v) = m.vertices.get_mut(0) {
            v.point = a;
        }
        if let Some(v) = m.vertices.get_mut(1) {
            v.point = b;
        }
        for e in &mut m.edges {
            if rng.chance(0.5) {
                e.etype = 9; // SM_FE_RAIL
            }
        }
        (m, 0, 1)
    } else {
        let mut m = empty_model();
        m.vertices = vec![vertex_at(a), vertex_at(b)];
        (m, 0, 1)
    };
    Inputs {
        model,
        solvers: Vec::new(),
        geom_solvers: Vec::new(),
        query: Query::UvCurve {
            sv,
            ev,
            two,
            surface,
            tol,
        },
    }
}

/// A UV line curve over the unit square.
pub(crate) fn uv_line(a: [f64; 2], b: [f64; 2]) -> Crv {
    Crv {
        dim: 2,
        cpts: vec![[a[0], a[1], 0.0, -f64::MAX], [b[0], b[1], 0.0, -f64::MAX]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A topology query on p1's random model, with base solvers on some geometries and the
/// edges around the queried vertex biased to the types the calculators look for.
fn gen_topology(rng: &mut Rng) -> Inputs {
    let mut model = gen_model(rng);
    let nv = model.vertices.len();
    let ng = model.geoms.len();
    let ns = rng.below(3) as usize;
    let solvers = (0..ns)
        .map(|_| SolverIn {
            approx: *rng.choose(&[1e-6, 1e-3, 1e-9, 0.1]).unwrap_or(&1e-6),
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
        .collect::<Vec<_>>();
    let v = rng.below(nv as u64) as usize;
    let query = match rng.below(6) {
        0 => Query::RailIndex {
            g: rng.below(ng.max(1) as u64) as usize,
            v: if rng.chance(0.3) { -1 } else { v as i64 },
        },
        1 => Query::RailIntExtended { v },
        2 => Query::Int2Fillets { v },
        3 => Query::Nx2 { v },
        4 => Query::MakeFace {
            g: rng.below(ng.max(1) as u64) as usize,
        },
        _ => Query::Blending {
            g: rng.below(ng.max(1) as u64) as usize,
        },
    };
    // bias the edges at the vertex
    let wanted: &[i64] = match query {
        Query::RailIntExtended { .. } => &[5, 1],
        Query::Nx2 { .. } => &[1, 5],
        Query::Int2Fillets { .. } => &[4],
        Query::Blending { .. } => &[0, 1],
        _ => &[9],
    };
    for e in &mut model.edges {
        if (e.vs == v || e.ve == v || rng.chance(0.3)) && rng.chance(0.7) {
            e.etype = *rng.choose(wanted).unwrap_or(&14);
        }
        if rng.chance(0.2) {
            e.corner = -1;
        }
    }
    if let Some(vx) = model.vertices.get_mut(v) {
        if rng.chance(0.7) {
            vx.corner = -1;
        }
    }
    // geometries with real rails and UV curves for make_face
    if matches!(query, Query::MakeFace { .. }) && ng > 0 && rng.chance(0.5) {
        let g = match query {
            Query::MakeFace { g } => g,
            _ => 0,
        };
        let s = model.geoms.get(g).and_then(|x| x.surface.clone());
        let ne = model.edges.len();
        let scale = 1.0;
        for r in 0..2 {
            let e = EdgeIn {
                vs: rng.below(nv as u64) as usize,
                ve: rng.below(nv as u64) as usize,
                etype: 9,
                status: 0,
                corner: -1,
                curve: Some(gen_curve(rng, 3, scale)),
                interval: Some([0.0, 1.0]),
                fbe: [-1, -1],
                eus: [
                    EuIn {
                        uv_curve: if rng.chance(0.9) {
                            let v0 = if r == 0 { 0.1 } else { 0.9 };
                            Some(uv_line([0.0, v0], [1.0, v0 + rng.uniform(-0.05, 0.05)]))
                        } else {
                            None
                        },
                        geom: g as i64,
                        vu_uv: [0.0, 0.0],
                    },
                    EuIn {
                        uv_curve: None,
                        geom: -1,
                        vu_uv: [0.0, 0.0],
                    },
                ],
            };
            model.edges.push(e);
        }
        if let Some(geom) = model.geoms.get_mut(g) {
            *geom = GeomIn {
                gtype: if rng.chance(0.8) { 0 } else { 4 },
                status: 0,
                center: None,
                surface: s.or_else(|| Some(gen_surface(rng, 1.0))),
                rails: [ne as i64, ne as i64 + 1],
                other1: Vec::new(),
                other2: Vec::new(),
                sides: Vec::new(),
            };
        }
    }
    Inputs {
        model,
        solvers,
        geom_solvers,
        query,
    }
}

/// An edge between vertices `vs` and `ve` of type `etype` whose primary edge use is on
/// geometry `geom` (`-1`: none).
fn edge_in(rng: &mut Rng, vs: usize, ve: usize, etype: i64, geom: i64, scale: f64) -> EdgeIn {
    EdgeIn {
        vs,
        ve,
        etype,
        status: 0,
        corner: -1,
        curve: if rng.chance(0.9) {
            Some(gen_curve(rng, 3, scale))
        } else {
            None
        },
        interval: Some([0.0, 1.0]),
        fbe: [-1, -1],
        eus: [
            EuIn {
                uv_curve: None,
                geom,
                vu_uv: [0.0, 0.0],
            },
            EuIn {
                uv_curve: None,
                geom: -1,
                vu_uv: [0.0, 0.0],
            },
        ],
    }
}

fn geom_in(rng: &mut Rng, gtype: i64, surface: bool, scale: f64) -> GeomIn {
    GeomIn {
        gtype,
        status: 0,
        center: None,
        surface: if surface {
            Some(gen_surface(rng, scale))
        } else {
            None
        },
        rails: [-1, -1],
        other1: Vec::new(),
        other2: Vec::new(),
        sides: Vec::new(),
    }
}

/// A small designed model for one topology query: the queried vertex `0` with edges of
/// the types the routine looks for (and others before them), geometries with or without
/// surfaces and solvers, rails with or without UV curves, processed or not neighbours.
#[allow(clippy::too_many_lines)]
fn gen_crafted(rng: &mut Rng) -> Inputs {
    let scale = scale_of(rng);
    let nv = 2 + rng.below(4) as usize;
    let mut model = empty_model();
    for _ in 0..nv {
        let mut v = vertex_at(rng.point3(-scale, scale));
        v.status = *rng.choose(&[0, 1, 1, 2]).unwrap_or(&0);
        v.vtype = rng.below(14) as i64;
        model.vertices.push(v);
    }
    let ng = 1 + rng.below(2) as usize;
    for _ in 0..ng {
        let gtype = *rng.choose(&[0, 0, 1, 4, 5]).unwrap_or(&0);
        let surface = rng.chance(0.7);
        model.geoms.push(geom_in(rng, gtype, surface, scale));
    }
    let solvers = vec![SolverIn {
        approx: scale * *rng.choose(&[1e-6, 1e-4, 1e-9]).unwrap_or(&1e-6),
        ang: rng.uniform(0.01, 0.5),
        tang: rng.uniform(0.001, 0.1),
    }];
    let geom_solvers = (0..ng)
        .map(|_| if rng.chance(0.85) { 0 } else { -1 })
        .collect::<Vec<_>>();
    let pick_geom = |rng: &mut Rng| -> i64 {
        if rng.chance(0.85) {
            rng.below(ng as u64) as i64
        } else {
            -1
        }
    };
    let other = |rng: &mut Rng| 1 + rng.below(nv as u64 - 1) as usize;
    // some unrelated edges at vertex 0 first (GetEdges order)
    for _ in 0..rng.below(3) {
        let o = other(rng);
        let t = *rng.choose(&[3, 6, 9, 14, 12]).unwrap_or(&14);
        let g = pick_geom(rng);
        let e = if rng.chance(0.5) {
            edge_in(rng, 0, o, t, g, scale)
        } else {
            edge_in(rng, o, 0, t, g, scale)
        };
        model.edges.push(e);
    }
    let query = match rng.below(6) {
        0 => {
            // CalcRailIntExtendedEdgeuse: a fillet/side-face edge
            if rng.chance(0.9) {
                let o = other(rng);
                let g = pick_geom(rng);
                let e = if rng.chance(0.5) {
                    edge_in(rng, 0, o, 5, g, scale)
                } else {
                    edge_in(rng, o, 0, 5, g, scale)
                };
                model.edges.push(e);
            }
            Query::RailIntExtended { v: 0 }
        }
        1 => {
            // sm_CalcNx2VertFilletXFillet: cross-section edges, then a side-face edge
            for _ in 0..rng.below(3) {
                let o = other(rng);
                let g = pick_geom(rng);
                model.edges.push(edge_in(rng, 0, o, 1, g, scale));
            }
            let o = other(rng);
            model.edges.push(edge_in(rng, o, 0, 5, -1, scale));
            if let Some(v) = model.vertices.get_mut(o) {
                v.status = i64::from(rng.chance(0.3));
            }
            Query::Nx2 { v: 0 }
        }
        2 => {
            // CalcFilletInt2Fillets without a corner
            let o = other(rng);
            let g = pick_geom(rng);
            model.edges.push(edge_in(rng, 0, o, 4, g, scale));
            Query::Int2Fillets { v: 0 }
        }
        q => {
            // rails for GetRailIndexOfVertex, MakeFaceBrep and CalcBlendingGeom
            let g = rng.below(ng as u64) as usize;
            let rail_type = match q {
                5 => *rng.choose(&[0, 0, 9, 1]).unwrap_or(&0),
                _ => 9,
            };
            let ne = model.edges.len();
            for r in 0..2 {
                let (a, b) = (other(rng), other(rng));
                let mut e = edge_in(rng, a, b, rail_type, g as i64, scale);
                if rng.chance(0.85) {
                    let v0 = if r == 0 { 0.1 } else { 0.9 };
                    e.eus[0].uv_curve =
                        Some(uv_line([0.0, v0], [1.0, v0 + rng.uniform(-0.05, 0.05)]));
                }
                if r == 1 && rng.chance(0.2) {
                    // a rail through the queried vertex
                    e.vs = 0;
                }
                model.edges.push(e);
            }
            if let Some(geom) = model.geoms.get_mut(g) {
                geom.rails = [ne as i64, ne as i64 + 1];
                if rng.chance(0.1) {
                    geom.rails[1] = -1;
                }
            }
            match q {
                3 => Query::RailIndex {
                    g,
                    v: if rng.chance(0.3) {
                        -1
                    } else {
                        rng.below(nv as u64) as i64
                    },
                },
                4 => Query::MakeFace { g },
                _ => Query::Blending { g },
            }
        }
    };
    Inputs {
        model,
        solvers,
        geom_solvers,
        query,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_geom_p4"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let c: CaseInputs = case.parse()?;
        cpp(&c.inputs)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse::<CaseInputs>()?.inputs;
        // an input the FFI refuses is a harness error on both sides (the hang regimes)
        ffi::validate(i.query.op(), &cpp_input(&i)?).map_err(|e| harness(e.to_string()))?;
        match rust(&i)? {
            Rec::Flat(flat) => to_output(&i.query, &flat),
            Rec::Deferred(e) => {
                Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())))
            }
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // keep only inputs the shim runs (it refuses those upstream would dereference a
        // corner, solver or rail the harness cannot provide for)
        for _ in 0..64 {
            let inputs = match rng.below(20) {
                0..=10 => match gen_uv_curve(rng) {
                    Some(i) => i,
                    None => continue,
                },
                11..=13 => gen_topology(rng),
                _ => gen_crafted(rng),
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
