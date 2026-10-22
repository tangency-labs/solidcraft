//! Runner for unit `query.face_p1`. C++: `solidcore_ffi::units::query_face_p1`. Rust:
//! `solidcore::query::face::p1`.
//!
//! **Curve API.** `SmApiEvaluateCurve` on a null curve, an `SmLine` or an
//! `SmHermiteCurve`, every optional-output mask; status and the (seeded) outputs.
//!
//! **B-rep ops.** The C++ side builds a real `SmBrep` from unit `topology.edgeuse_p1`'s
//! B-rep block (an upstream `.smb` file with or without rebuilt UV trim curves, a face
//! on a B-spline surface or a real `SmPlane`, a wire edge, the wire network, isolated
//! vertices, or faces and networks mixed) and runs `SmFace` / `SmShell` /
//! `SmRegion::CalculateBoundingBox` (explicit `bTight` or the default argument),
//! `SmFace::Point3DClassify` or `SmFace::EdgeCurveOnClassify` on it. The Rust side
//! imports that unit's export (traversal order) and patches in each face's primary face
//! use `m_pFU`, which the export lacks (the shim's `FaceFu` op). Entity selectors are
//! taken modulo the entity count (`-2` on both sides when there is none).
//!
//! The classified 3D point is computed from the imported B-rep, identically for both
//! sides: the face surface at a fraction of the face's UV domain plus an offset. A case
//! touching a face surface or edge curve the import holds only as its B-spline part
//! (cones, spheres, tori, extrusions, circles, ...; an `SmSurfOfRevolution` face is
//! imported exactly) is deferred: to the surface unit whose Rust kind is missing,
//! otherwise as the shared export's harness limitation
//! (`harness:analytic-geometry-export`, as in `topology.brep_p1`).
//!
//! **Outputs.** Boxes: status and the box (seeded with a sentinel box, so what a failing
//! call leaves is compared). `Point3DClassify`: status and the classification (class,
//! object kind and index, gap, source tolerance, pre-snap parameter, curve and surface
//! parameters, parameters-set flag, face kind and index). `EdgeCurveOnClassify`: status,
//! inside, possible problem and problem code (seeded, so a null edge leaves them).

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::p2::{BSPLINE_CURVE_TYPE, CRV_ON_SURF_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::assert_array::labels::FACE_TYPE;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::PointClassification;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p1;
use solidcore::topology::{
    topology_traverser, Brep, EdgeId, FaceId, FaceuseId, LoopId, RegionId, ShellId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::query_face_p1::{self as ffi, Op};
use solidcore_ffi::units::topology_edgeuse_p1 as graph;

use super::topology_edgeuse_p1::{
    domain, fixture_input, gen_brep, gen_surface, import_with_types, BrepIn, ImportTypes,
};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1200;

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- inputs -----------------------------------------------------------------------------

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Parameter {
    Number(f64),
    Special(String),
}

impl Parameter {
    fn value(&self) -> Result<f64, HarnessError> {
        match self {
            Self::Number(x) => Ok(*x),
            Self::Special(x) if x == "NaN" => Ok(f64::NAN),
            Self::Special(x) if x == "+inf" => Ok(f64::INFINITY),
            Self::Special(x) if x == "-inf" => Ok(f64::NEG_INFINITY),
            Self::Special(_) => Err(harness("unknown special parameter")),
        }
    }
}

/// A curve API case.
#[derive(Serialize, Deserialize)]
struct CurveInput {
    kind: i32,
    data: [f64; 12],
    parameter: Parameter,
    mask: i32,
}

/// A box case: `target` 0 face, 1 shell, 2 region; `index` a selector.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoxInput {
    brep: Value,
    target: u8,
    index: usize,
    tight: bool,
    use_default: bool,
}

/// The guess of a `Point3DClassify` case: none, or the classified UV point plus
/// `(du, dv)` (fractions of the UV domain size).
#[derive(Clone, Copy, Serialize, Deserialize)]
struct Guess {
    du: f64,
    dv: f64,
}

/// A `Point3DClassify` case. The point is the face surface at the UV domain fraction
/// `(u, v)` plus `offset`.
#[derive(Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // upstream's SmBoolean arguments
struct PointInput {
    brep: Value,
    face: usize,
    u: f64,
    v: f64,
    offset: [f64; 3],
    src_tol: f64,
    do_bnd: bool,
    #[serde(default)]
    guess: Option<Guess>,
    #[serde(default)]
    tgt_only: bool,
    #[serde(default)]
    has_loops: bool,
    #[serde(default)]
    loops: Vec<usize>,
    #[serde(default = "yes")]
    uv_ok: bool,
    pc_tol: f64,
}

const fn yes() -> bool {
    true
}

/// An `EdgeCurveOnClassify` case: the line segment `p -> q` (in the face's parameter
/// space for `dim` 2, z ignored) crossing edge `edge` (`-1`: NULL) at `edge_param`.
#[derive(Serialize, Deserialize)]
struct EdgeOnInput {
    brep: Value,
    face: usize,
    edge: i64,
    dim: u8,
    p: [f64; 3],
    q: [f64; 3],
    edge_param: f64,
    interval: [f64; 2],
    same: bool,
    tol: f64,
    #[serde(default = "yes")]
    uv_ok: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum Input {
    Box(BoxInput),
    Point(PointInput),
    EdgeOn(EdgeOnInput),
    Curve(CurveInput),
}

// ---- the imported B-rep -----------------------------------------------------------------

/// The case's B-rep imported from the C++ export, with each face's primary face use
/// (shared with the `query.brep` runner).
pub(crate) struct Model {
    pub(crate) path: String,
    pub(crate) block: Vec<f64>,
    pub(crate) brep: Brep,
    pub(crate) types: ImportTypes,
}

pub(crate) fn model(brep: &Value) -> Result<Model, HarnessError> {
    let (path, block) = fixture_input(brep.clone())?;
    let export = graph::eval(graph::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let (mut b, types) = import_with_types(&export)?;
    let fus = ffi::eval(Op::FaceFu, &path, &block, &[])
        .map_err(|e| harness(format!("c++ face uses: {e}")))?;
    if fus.len() != b.faces().len() {
        return Err(harness("face use count differs from the face count"));
    }
    for (i, fu) in fus.iter().enumerate() {
        let f = FaceId::from_index(u32::try_from(i).map_err(|_| harness("face index"))?);
        b.face_mut(f).map_err(|e| harness(e.to_string()))?.faceuse = if *fu < 0.0 {
            None
        } else {
            Some(FaceuseId::from_index(*fu as u32))
        };
    }
    // Upstream face surfaces are owned by their faces (their zone tolerance is the
    // face's, and point solves test trim containment through the face); the import
    // leaves them unowned.
    for i in 0..b.faces().len() {
        let f = FaceId::from_index(u32::try_from(i).map_err(|_| harness("face index"))?);
        if let Some(s) = b
            .face_mut(f)
            .map_err(|e| harness(e.to_string()))?
            .surface
            .as_mut()
        {
            s.set_owner(Some(ObjectRef {
                type_code: FACE_TYPE,
                key: ObjectKey(i as u64),
            }));
        }
        // the face's tolerance, as the face upstream answers GetTolerance with
        b.sync_face_surface_tolerance(f)
            .map_err(|e| harness(e.to_string()))?;
    }
    Ok(Model {
        path,
        block,
        brep: b,
        types,
    })
}

fn pick(sel: usize, n: usize) -> Option<u32> {
    (n > 0).then(|| (sel % n) as u32)
}

const EXACT_SURFACES: [i64; 2] = [15002, 15011];

/// The deferral label of geometry the shared B-rep export flattens to its B-spline part
/// (a harness limitation, as in `topology.brep_p1`).
const HARNESS_ANALYTIC_IMPORT: &str = "harness:analytic-geometry-export";

/// The deferral of a case that reads a face surface or edge curve the import holds only
/// as its B-spline part (see the module docs).
pub(crate) fn inexact(m: &Model, faces: &[FaceId], edges: &[EdgeId]) -> Option<Output> {
    for f in faces {
        let st = m.types.inexact_surface(f.index() as usize).unwrap_or(-1);
        if st >= 0 && !EXACT_SURFACES.contains(&st) {
            let (unit, symbol) = match st {
                // the gap is the missing `SurfaceKind` variant (the `SmSurface` dispatch,
                // unit `surface.surface_p2`; #76), not the kinds' own units (all ported)
                15012 | 15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone)"),
                15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere)"),
                15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus)"),
                15030 => (
                    "surface.surface_p2",
                    "SurfaceKind::SurfOfExtrusion (SmSurfOfExtrusion)",
                ),
                _ => (
                    HARNESS_ANALYTIC_IMPORT,
                    "analytic face surface in the B-rep export",
                ),
            };
            return Some(Output::deferred(unit, symbol));
        }
    }
    for e in edges {
        let ct = m
            .types
            .edge_curve_types
            .get(e.index() as usize)
            .copied()
            .unwrap_or(-1);
        if ct >= 0 && ![BSPLINE_CURVE_TYPE, LINE_TYPE, CRV_ON_SURF_TYPE].contains(&ct) {
            return Some(Output::deferred(
                HARNESS_ANALYTIC_IMPORT,
                "analytic edge curve in the B-rep export",
            ));
        }
    }
    None
}

/// The face and its edges.
fn face_and_edges(b: &Brep, f: FaceId) -> (Vec<FaceId>, Vec<EdgeId>) {
    (
        vec![f],
        topology_traverser::face_edges(b, f).unwrap_or_default(),
    )
}

/// Every face and edge of the B-rep.
pub(crate) fn everything(b: &Brep) -> (Vec<FaceId>, Vec<EdgeId>) {
    (
        b.faces().iter().map(|(id, _)| id).collect(),
        b.edges().iter().map(|(id, _)| id).collect(),
    )
}

fn none_output() -> Output {
    Output::new().int("status", -2)
}

/// The status of a Rust result: `0`, or the `SmStatus` code; `None` for an error with no
/// upstream status.
fn status_of(r: &Result<(), SolidError>) -> Option<i64> {
    match r {
        Ok(()) => Some(0),
        Err(SolidError::Upstream { code, .. }) => Some(i64::from(*code)),
        Err(_) => None,
    }
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

// ---- boxes --------------------------------------------------------------------------------

const SEED_MIN: [f64; 3] = [-17., -19., -23.];
const SEED_MAX: [f64; 3] = [29., 31., 37.];

fn box_output(status: i64, values: &[f64]) -> Output {
    Output::new()
        .int("status", status)
        .values("box", values.to_vec())
}

fn run_box(input: &BoxInput, cpp: bool) -> Result<Output, HarnessError> {
    let m = model(&input.brep)?;
    let b = &m.brep;
    if cpp {
        let args = [
            f64::from(input.target),
            input.index as f64,
            flag(input.tight),
            flag(input.use_default),
        ];
        let out = ffi::eval(Op::Box, &m.path, &m.block, &args)
            .map_err(|e| harness(format!("c++: {e}")))?;
        let (status, rest) = out
            .split_first()
            .ok_or_else(|| harness("empty c++ output"))?;
        if *status == -2.0 {
            return Ok(none_output());
        }
        return Ok(box_output(*status as i64, rest));
    }
    let n = match input.target {
        0 => b.faces().len(),
        1 => b.shells().len(),
        2 => b.regions().len(),
        _ => return Err(harness("unknown box target")),
    };
    let Some(id) = pick(input.index, n) else {
        return Ok(none_output());
    };
    let (faces, edges) = if input.target == 0 {
        face_and_edges(b, FaceId::from_index(id))
    } else {
        everything(b)
    };
    if let Some(o) = inexact(&m, &faces, &edges) {
        return Ok(o);
    }
    let mut k = Kernel::default();
    let tight = input.tight && !input.use_default;
    let mut bx = Extent3d::from_points(
        Vec3::new(SEED_MIN[0], SEED_MIN[1], SEED_MIN[2]),
        Vec3::new(SEED_MAX[0], SEED_MAX[1], SEED_MAX[2]),
    );
    let r = match input.target {
        0 => p1::calculate_bounding_box(&k, b, FaceId::from_index(id), tight).map(|x| bx = x),
        1 => p1::calculate_shell_bounding_box_into(
            &mut k,
            b,
            ShellId::from_index(id),
            tight,
            &mut bx,
        ),
        _ => p1::calculate_region_bounding_box_into(
            &mut k,
            b,
            RegionId::from_index(id),
            tight,
            &mut bx,
        ),
    };
    let Some(status) = status_of(&r) else {
        return Ok(r.map_or_else(|e| rust_err(&e), |()| Output::new()));
    };
    let (lo, hi) = (bx.min(), bx.max());
    Ok(box_output(status, &[lo.x, lo.y, lo.z, hi.x, hi.y, hi.z]))
}

// ---- Point3DClassify ------------------------------------------------------------------

/// The resolved arguments of a point case: face, point, guess UV, loops.
struct PointArgs {
    face: FaceId,
    point: Vec3,
    guess: Option<Point2>,
    loops: Vec<LoopId>,
}

fn point_args(k: &Kernel, m: &Model, i: &PointInput) -> Result<Option<PointArgs>, HarnessError> {
    let b = &m.brep;
    let Some(f) = pick(i.face, b.faces().len()) else {
        return Ok(None);
    };
    let face = FaceId::from_index(f);
    let rec = b.face(face).map_err(|e| harness(e.to_string()))?;
    let dom = rec.uv_domain;
    let surface = rec
        .surface
        .as_ref()
        .ok_or_else(|| harness("face without surface"))?;
    let size = dom.size();
    let uv = Point2::new(dom.min().x + i.u * size.x, dom.min().y + i.v * size.y);
    let s = surface
        .evaluate_point(k, uv)
        .map_err(|e| harness(format!("surface point: {e}")))?;
    let point = Vec3::new(s.x + i.offset[0], s.y + i.offset[1], s.z + i.offset[2]);
    let guess = i
        .guess
        .map(|g| Point2::new(uv.x + g.du * size.x, uv.y + g.dv * size.y));
    let n_loops = b.loops().len();
    let loops = i
        .loops
        .iter()
        .filter_map(|&l| pick(l, n_loops).map(LoopId::from_index))
        .collect();
    Ok(Some(PointArgs {
        face,
        point,
        guess,
        loops,
    }))
}

fn pc_block(pc: &PointClassification<EntityRef>) -> Vec<f64> {
    let object = |o: Option<EntityRef>| match o {
        None => (0.0, -1.0),
        Some(EntityRef::Face(f)) => (1.0, f64::from(f.index())),
        Some(EntityRef::Edge(e)) => (2.0, f64::from(e.index())),
        Some(EntityRef::Vertex(v)) => (3.0, f64::from(v.index())),
        Some(_) => (-3.0, -1.0),
    };
    let (kind, index) = object(pc.object());
    let (face_kind, face_index) = object(pc.face());
    let uv = pc.uv_param();
    vec![
        f64::from(pc.point_class().code()),
        kind,
        index,
        pc.gap_3d(&Kernel::default(), &NoGeometry, None)
            .unwrap_or(f64::NAN),
        pc.src_zone_tol_3d(),
        pc.pre_snap_param(),
        pc.t_param(),
        uv.x,
        uv.y,
        flag(pc.are_parameters_set()),
        face_kind,
        face_index,
    ]
}

/// No geometry behind the B-rep handles (only the stored gap is read; shared with the
/// `query.brep` runner).
pub(crate) struct NoGeometry;

impl solidcore::intersect::point_class::ClassGeometry<EntityRef> for NoGeometry {
    fn curve(&self, _o: EntityRef) -> Option<&Curve> {
        None
    }
    fn surface(&self, _o: EntityRef) -> Option<&solidcore::surface::surface::Surface> {
        None
    }
    fn volume(&self, _o: EntityRef) -> Option<&solidcore::surface::volume::Volume> {
        None
    }
    fn point(&self, _o: EntityRef) -> Option<Vec3> {
        None
    }
}

fn run_point(k: &Kernel, i: &PointInput, cpp: bool) -> Result<Output, HarnessError> {
    let m = model(&i.brep)?;
    let Some(a) = point_args(k, &m, i)? else {
        return Ok(none_output());
    };
    if cpp {
        let mut args = vec![
            f64::from(a.face.index()),
            a.point.x,
            a.point.y,
            a.point.z,
            i.src_tol,
            flag(i.do_bnd),
            flag(a.guess.is_some()),
            a.guess.map_or(0.0, |g| g.x),
            a.guess.map_or(0.0, |g| g.y),
            flag(i.tgt_only),
            flag(i.has_loops),
            a.loops.len() as f64,
        ];
        args.extend(a.loops.iter().map(|l| f64::from(l.index())));
        args.extend([flag(i.uv_ok), i.pc_tol]);
        let out = ffi::eval(Op::Point3d, &m.path, &m.block, &args)
            .map_err(|e| harness(format!("c++: {e}")))?;
        let (status, rest) = out
            .split_first()
            .ok_or_else(|| harness("empty c++ output"))?;
        return Ok(Output::new()
            .int("status", *status as i64)
            .values("pc", rest.to_vec()));
    }
    let (faces, edges) = face_and_edges(&m.brep, a.face);
    if let Some(o) = inexact(&m, &faces, &edges) {
        return Ok(o);
    }
    let k = Kernel::default();
    let mut b = m.brep;
    let mut pc = PointClassification::new(zone_tol3d(&k, i.pc_tol));
    let opts = p1::Point3dClassifyOptions {
        guess_uv: a.guess,
        tgt_loops_only: i.tgt_only,
        tgt_loops: i.has_loops.then_some(a.loops.as_slice()),
        uv_space_okay: i.uv_ok,
    };
    let r = p1::point_3d_classify(
        &k,
        &mut b,
        a.face,
        a.point,
        zone_tol3d(&k, i.src_tol),
        i.do_bnd,
        &mut pc,
        opts,
    );
    let Some(status) = status_of(&r) else {
        return Ok(r.map_or_else(|e| rust_err(&e), |()| Output::new()));
    };
    Ok(Output::new()
        .int("status", status)
        .values("pc", pc_block(&pc)))
}

// ---- EdgeCurveOnClassify --------------------------------------------------------------

fn edge_on_output(status: i64, inside: bool, problem: bool, why: i64) -> Output {
    Output::new()
        .int("status", status)
        .ints("answer", vec![i64::from(inside), i64::from(problem), why])
}

fn run_edge_on(i: &EdgeOnInput, cpp: bool) -> Result<Output, HarnessError> {
    let m = model(&i.brep)?;
    let b = &m.brep;
    let n_edges = b.edges().len();
    let Some(f) = pick(i.face, b.faces().len()) else {
        return Ok(none_output());
    };
    if i.edge >= 0 && n_edges == 0 {
        return Ok(none_output());
    }
    let face = FaceId::from_index(f);
    let edge = (i.edge >= 0)
        .then(|| pick(i.edge as usize, n_edges).map(EdgeId::from_index))
        .flatten();
    if cpp {
        let mut args = vec![
            f64::from(face.index()),
            edge.map_or(-1.0, |e| f64::from(e.index())),
        ];
        args.push(f64::from(i.dim));
        args.extend(i.p);
        args.extend(i.q);
        args.extend([
            i.edge_param,
            i.interval[0],
            i.interval[1],
            if i.same { 0.0 } else { 1.0 },
            i.tol,
            flag(i.uv_ok),
        ]);
        let out = ffi::eval(Op::EdgeOn, &m.path, &m.block, &args)
            .map_err(|e| harness(format!("c++: {e}")))?;
        return Ok(match out.as_slice() {
            [s, ..] if *s == -2.0 => none_output(),
            [s, cs] if *s == -3.0 => Output::err(format!("line_segment_status_{cs}")),
            [s, inside, problem, why] => {
                edge_on_output(*s as i64, *inside != 0.0, *problem != 0.0, *why as i64)
            }
            _ => return Err(harness("malformed c++ output")),
        });
    }
    let (faces, edges) = face_and_edges(b, face);
    if let Some(o) = inexact(&m, &faces, &edges) {
        return Ok(o);
    }
    let k = Kernel::default();
    let [px, py, pz] = i.p;
    let [qx, qy, qz] = i.q;
    let line = match BSplineCurve::create_line_segment(
        u64::from(i.dim),
        Vec3::new(px, py, pz),
        Vec3::new(qx, qy, qz),
    ) {
        Ok(c) => Curve::from(c),
        Err(SolidError::Upstream { code, .. }) => {
            return Ok(Output::err(format!(
                "line_segment_status_{}",
                f64::from(code)
            )))
        }
        Err(e) => return Ok(rust_err(&e)),
    };
    let opts = p1::EdgeCurveOnOptions {
        uv_space_okay: i.uv_ok,
        ..p1::EdgeCurveOnOptions::new(
            i.edge_param,
            Extent1d::from_min_max(i.interval[0], i.interval[1]),
            if i.same {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
            i.tol,
        )
    };
    let mut b = m.brep;
    let r = p1::edge_curve_on_classify(&k, &mut b, face, edge, &line, &opts);
    Ok(match r {
        Ok(c) => edge_on_output(0, c.inside, c.possible_problem, c.problem as i64),
        // the seeded outputs survive a failure
        Err(SolidError::Upstream { code, .. }) => edge_on_output(i64::from(code), true, true, 7),
        Err(e) => rust_err(&e),
    })
}

// ---- curve API ------------------------------------------------------------------------------

fn curve_output(status: i32, values: [f64; 9]) -> Output {
    Output::new()
        .int("status", i64::from(status))
        .values("out", values.to_vec())
}

fn run_curve(k: &Kernel, input: &CurveInput, cpp: bool) -> Result<Output, HarnessError> {
    // Inputs the FFI refuses before any C++ runs: nothing runs on either side.
    if !(0..=2).contains(&input.kind)
        || !(0..=7).contains(&input.mask)
        || input.data.iter().any(|x| !x.is_finite())
    {
        return Ok(Output::rejected(
            "curve kind, data or output mask outside the shim's inputs",
        ));
    }
    if cpp {
        let (status, values) = ffi::evaluate_curve(
            input.kind,
            &input.data,
            input.parameter.value()?,
            input.mask,
        )
        .map_err(|e| harness(e.to_string()))?;
        // the shim reports the raw SmStatus: SM_SUCCESS is 1000
        return Ok(curve_output(
            if status == 1000 { 0 } else { status },
            values,
        ));
    }
    let [px, py, pz, dx, dy, dz, qx, qy, qz, ex, ey, ez] = input.data;
    let p = Vec3::new(px, py, pz);
    let d = Vec3::new(dx, dy, dz);
    let q = Vec3::new(qx, qy, qz);
    let e = Vec3::new(ex, ey, ez);
    let curve: Option<Curve> = match input.kind {
        0 => None,
        1 => Some(Line::from_points(p, q, 3).into()),
        2 => Some(HermiteCurve::new(p, d, q, e, 3).into()),
        _ => return Err(harness("unknown curve kind")),
    };
    let mut values = [11., 13., 17., 19., 23., 29., 31., 37., 41.];
    match p1::evaluate_curve(k, curve.as_ref(), input.parameter.value()?) {
        Ok(result) => {
            for ((i, vector), destination) in
                result.iter().enumerate().zip(values.as_chunks_mut::<3>().0)
            {
                if input.mask & (1 << i) != 0 {
                    *destination = [vector.x, vector.y, vector.z];
                }
            }
            Ok(curve_output(0, values))
        }
        Err(SolidError::InvalidInput { .. }) => Ok(curve_output(1007, values)),
        Err(SolidError::Upstream { code, .. }) => Ok(curve_output(code, values)),
        Err(e) => Ok(rust_err(&e)),
    }
}

fn run(k: &Kernel, case: &Case, cpp: bool) -> Result<Output, HarnessError> {
    let input = case.parse::<Input>()?;
    match input {
        Input::Box(i) => run_box(&i, cpp),
        Input::Point(i) => run_point(k, &i, cpp),
        Input::EdgeOn(i) => run_edge_on(&i, cpp),
        Input::Curve(i) => run_curve(k, &i, cpp),
    }
}

// ---- generation -------------------------------------------------------------------------------

/// The upstream `.smb` files whose faces the export carries exactly (see the
/// `query.face_p2` runner).
const EXACT_FILES: [&str; 7] = [
    "Endcap.smb",
    "Mouse.smb",
    "box_with_large_fillet.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
];

pub(crate) fn gen_face_brep(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.7) {
        BrepIn::File {
            file: (*rng.choose(&EXACT_FILES).unwrap_or(&"plane_square.smb")).to_string(),
            rebuild: rng.chance(0.7),
        }
    } else {
        let s = gen_surface(rng);
        let [u0, v0, u1, v1] = domain(&s);
        let (du, dv) = (u1 - u0, v1 - v0);
        let dom = if rng.chance(0.5) {
            [u0, v0, u1, v1]
        } else {
            [
                u0 + du * rng.uniform(0.0, 0.3),
                v0 + dv * rng.uniform(0.0, 0.3),
                u1 - du * rng.uniform(0.0, 0.3),
                v1 - dv * rng.uniform(0.0, 0.3),
            ]
        };
        BrepIn::Face { surface: s, dom }
    }
}

fn to_value<T: Serialize>(x: &T) -> Option<Value> {
    serde_json::to_value(x).ok()
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-10, 1e-8, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2])
        .unwrap_or(&1e-6)
}

fn gen_frac(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 0.0,
        1 => 1.0,
        2 => 0.5,
        3 => rng.uniform(-0.3, 0.0),
        4 => rng.uniform(1.0, 1.3),
        _ => rng.unit(),
    }
}

fn gen_offset(rng: &mut Rng) -> [f64; 3] {
    let scale = *rng
        .choose(&[0.0, 0.0, 1e-12, 1e-9, 1e-7, 1e-5, 1e-3, 0.1, 10.0])
        .unwrap_or(&0.0);
    [
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
        rng.uniform(-1.0, 1.0) * scale,
    ]
}

fn gen_box(rng: &mut Rng) -> Option<Value> {
    let brep = if rng.chance(0.6) {
        gen_face_brep(rng)
    } else {
        gen_brep(rng)
    };
    let tight = rng.chance(0.15);
    to_value(&BoxInput {
        brep: to_value(&brep)?,
        target: rng.below(3) as u8,
        index: rng.below(1 << 20) as usize,
        tight,
        use_default: rng.chance(0.3),
    })
}

fn gen_point(rng: &mut Rng) -> Option<Value> {
    let brep = gen_face_brep(rng);
    let guess = match rng.below(4) {
        0 => Some(Guess { du: 0.0, dv: 0.0 }),
        1 => Some(Guess {
            du: rng.uniform(-0.1, 0.1),
            dv: rng.uniform(-0.1, 0.1),
        }),
        _ => None,
    };
    let has_loops = rng.chance(0.2);
    to_value(&PointInput {
        brep: to_value(&brep)?,
        face: rng.below(1 << 20) as usize,
        u: gen_frac(rng),
        v: gen_frac(rng),
        offset: gen_offset(rng),
        src_tol: gen_tol(rng),
        do_bnd: rng.chance(0.5),
        guess,
        tgt_only: rng.chance(0.2),
        has_loops,
        loops: (0..rng.below(3))
            .map(|_| rng.below(1 << 20) as usize)
            .collect(),
        uv_ok: rng.chance(0.8),
        pc_tol: gen_tol(rng),
    })
}

fn gen_edge_on(rng: &mut Rng) -> Option<Value> {
    let brep = if rng.chance(0.8) {
        gen_face_brep(rng)
    } else {
        gen_brep(rng)
    };
    let dim = if rng.chance(0.5) { 2 } else { 3 };
    let pt = |rng: &mut Rng| {
        [
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
            rng.uniform(-2.0, 2.0),
        ]
    };
    let (p, q) = (pt(rng), pt(rng));
    to_value(&EdgeOnInput {
        brep: to_value(&brep)?,
        face: rng.below(1 << 20) as usize,
        edge: if rng.chance(0.1) {
            -1
        } else {
            rng.below(1 << 20) as i64
        },
        dim,
        p,
        q,
        edge_param: rng.uniform(0.0, 1.0),
        interval: [0.0, 1.0],
        same: rng.chance(0.5),
        tol: gen_tol(rng),
        uv_ok: rng.chance(0.8),
    })
}

fn gen_curve(rng: &mut Rng) -> Option<Value> {
    let scale = match rng.below(5) {
        0 => 1e-200,
        1 => 1e-6,
        2 => 1.0,
        3 => 1e6,
        _ => 1e100,
    };
    let mut data = [0.0; 12];
    for item in &mut data {
        *item = rng.uniform(-100.0, 100.0) * scale;
    }
    to_value(&CurveInput {
        kind: 1 + rng.below(2) as i32,
        data,
        parameter: Parameter::Number(rng.unit()),
        mask: 1 + rng.below(7) as i32,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.face_p1"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        default_cases(
            self,
            &CaseContext {
                seeds: ctx.seeds.max(MIN_GENERATED),
                ..ctx.clone()
            },
        )
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        run(k, case, true)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        run(k, case, false)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        match rng.below(10) {
            0 | 1 => gen_curve(rng),
            2..=4 => gen_box(rng),
            5..=8 => gen_point(rng),
            _ => gen_edge_on(rng),
        }
    }
}
