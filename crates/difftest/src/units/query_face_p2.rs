//! Runner for unit `query.face_p2`. C++: `solidcore_ffi::units::query_face_p2`. Rust:
//! `solidcore::query::face::p2`.
//!
//! **B-rep ops.** The C++ side builds a real `SmBrep` per call (an upstream `.smb` test
//! file, optionally with rebuilt UV trim curves, or `CreateFaceFromSurface` over a
//! B-spline surface or a real `SmPlane`), optionally overwrites stored face, edge and
//! vertex tolerances, then runs `SmFace::PointClassify`, `sm_ClassifyParamOnFaces` (a
//! verbatim copy) or `SmFace::CurveOnClassify`. There is no Rust B-rep reader or builder
//! yet, so the Rust side imports the edited graph the shim exports (unit
//! `topology.face_p2`'s layout and importer: topology, tolerances, UV domains, planes and
//! B-spline surfaces, B-spline and line curves, UV trim curves) and runs the port on it.
//! A face whose surface, or one of whose loop edges' curves, is an `SmBSplineSurface` /
//! `SmBSplineCurve` subclass the export carries only as its B-spline part is not
//! compared (both sides report `inexact_geometry`).
//!
//! **Points.** The classified parameter point is computed from the imported B-rep
//! (identically for both sides, then passed to both): a fraction of the face's UV
//! domain (`frac`, values outside `[0, 1]` reach outside the domain), a point at the
//! normalised parameter of a loop edge use's UV trim curve (`trim`), or explicit
//! coordinates (`abs`), each plus an offset `(du, dv)`. Selectors are taken modulo the
//! entity count.
//!
//! **Trimmed surface caches.** Reading a file can leave a trimmed surface cache on a face
//! surface. The port has no object caches, so the shim deletes the cache of every face
//! surface an op classifies against first: both sides classify through the face's
//! vertices and loops, and `PointClassify`'s cache branch is not compared.
//!
//! **Outputs.** `status` (`0` or the `SmStatus` code), whether the face surface still
//! has a trimmed surface cache (always `0`), and the classification: class, object kind and export
//! index, gap, source tolerance, pre-snap parameter, curve and surface parameters and
//! the parameters-set flag. `CurveOnClassify` cases are deferred (its callees are in
//! units not ported yet); `SmProtoVertex` cases compare `ContainsPoint`, `DistanceTo`
//! and the capture distances.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind, PointClassification};
use solidcore::kernel::Kernel;
use solidcore::query::face::p2 as qf;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::proto_topology::ProtoTopologyManager;
use solidcore::topology::{Brep, EdgeuseId, FaceId, LoopId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::query_face_p2::{self as ffi, Op};

use super::topology_edgeuse_p1::{brep_block_of, domain, gen_surface, path_of, sel, BrepIn, FILES};
use super::topology_face_p2::{import, Exact};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 300;

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------------

/// A parameter point (see the module docs).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct At {
    /// `frac`, `trim` or `abs`.
    mode: String,
    /// `frac`: the u fraction; `trim`: the normalised trim curve parameter; `abs`: u.
    a: f64,
    /// `frac`: the v fraction; `abs`: v.
    #[serde(default)]
    b: f64,
    /// `trim`: the edge use selector among the face's loop edge uses.
    #[serde(default)]
    sel: i64,
    /// Offset added to u.
    #[serde(default)]
    du: f64,
    /// Offset added to v.
    #[serde(default)]
    dv: f64,
}

/// A B-rep edit of the entity `sel` (modulo the count of its kind): `kind` 0 face, 1
/// edge, 2 vertex: its stored zone tolerance set to `value`; 3: the UV trim curves of
/// the face built (`GetOrCreateUVTrimCurve`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Edit {
    kind: u8,
    sel: i64,
    value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    PointClassify {
        face: i64,
        at: At,
        #[serde(default)]
        src_tol: f64,
        #[serde(default)]
        do_bnd: bool,
        #[serde(default)]
        use_uv: bool,
        #[serde(default)]
        tgt_only: bool,
        #[serde(default)]
        has_loops: bool,
        #[serde(default)]
        loops: Vec<i64>,
        #[serde(default)]
        pc_tol: f64,
        #[serde(default = "yes")]
        uv_ok: bool,
    },
    ClassifyParam {
        face1: i64,
        face2: i64,
        at: At,
        tol: f64,
        /// `None`: the face's own UV domain.
        #[serde(default)]
        dom1: Option<[f64; 4]>,
        #[serde(default)]
        dom2: Option<[f64; 4]>,
    },
    CurveOnClassify {
        face: i64,
        dim: u8,
        from: At,
        to: At,
        tol: f64,
        do_pc: bool,
        big_gaps: bool,
        check3d: bool,
    },
    ProtoVertex {
        approx: f64,
        capture: f64,
        override_tol: f64,
        p: [f64; 3],
        tol: f64,
        q: [f64; 3],
    },
}

const fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(default)]
    edits: Vec<Edit>,
    #[serde(flatten)]
    op: OpIn,
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- the imported B-rep -----------------------------------------------------------------

/// The edited B-rep, imported from the shim's export.
struct Model {
    brep: Brep,
    exact: Exact,
    /// The edit block (selectors resolved).
    edit_block: Vec<f64>,
}

/// The B-rep block of the case.
fn block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    brep_block_of(i.brep.as_ref(), i.validate_only, i.init_gaps)
}

/// Builds the case's B-rep on the C++ side, applies the edits and imports it;
/// `Err(Output)` when it could not be built (the same failure on both sides).
fn model(i: &Inputs) -> Result<Result<Model, Output>, HarnessError> {
    let path = path_of(i.brep.as_ref())?;
    let mut edit_block = vec![i.edits.len() as f64];
    for e in &i.edits {
        if e.sel < 0 {
            return Err(harness("negative edit selector"));
        }
        edit_block.extend([f64::from(e.kind.min(3)), e.sel as f64, e.value]);
    }
    let mut input = block(i)?;
    input.extend(&edit_block);
    let ex = ffi::eval(Op::Export, &path, &input).map_err(|e| harness(format!("c++ export: {e}")))?;
    if ex.first() != Some(&0.0) {
        return Ok(Err(Output::err(format!(
            "brep_build_status_{}",
            ex.get(1).copied().unwrap_or(0.0)
        ))));
    }
    let (brep, exact) = import(&ex)?;
    Ok(Ok(Model {
        brep,
        exact,
        edit_block,
    }))
}

/// The face selected by `s`.
fn face_of(m: &Model, s: i64) -> Result<FaceId, HarnessError> {
    let f = sel(s, m.brep.faces().len());
    u32::try_from(f)
        .map(FaceId::from_index)
        .map_err(|_| harness("no face"))
}

/// The edge uses of the loops of face `f`'s face use, in loop order.
fn face_edgeuses(b: &Brep, f: FaceId) -> Result<Vec<EdgeuseId>, HarnessError> {
    let fu = b
        .face(f)
        .map_err(|e| harness(e.to_string()))?
        .faceuse
        .ok_or_else(|| harness("face without face use"))?;
    eu1::faceuse_edgeuses(b, fu, false, None).map_err(|e| harness(e.to_string()))
}

/// Whether face `f`'s surface and the curves of its loop edges were imported exactly.
fn face_exact(m: &Model, f: FaceId) -> Result<bool, HarnessError> {
    if !m.exact.faces.get(f.index() as usize).copied().unwrap_or(false) {
        return Ok(false);
    }
    for eu in face_edgeuses(&m.brep, f)? {
        let e = m
            .brep
            .edgeuse(eu)
            .map_err(|e| harness(e.to_string()))?
            .edge
            .ok_or_else(|| harness("edge use without edge"))?;
        if !m.exact.edges.get(e.index() as usize).copied().unwrap_or(false) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The parameter point `at` on face `f`.
fn point_of(m: &Model, f: FaceId, at: &At) -> Result<Point2, HarnessError> {
    let face = m.brep.face(f).map_err(|e| harness(e.to_string()))?;
    let dom = face.uv_domain;
    let (min, max) = (dom.min(), dom.max());
    let base = match at.mode.as_str() {
        "frac" => Point2::new(min.x + at.a * (max.x - min.x), min.y + at.b * (max.y - min.y)),
        "abs" => Point2::new(at.a, at.b),
        "trim" => {
            let eus = face_edgeuses(&m.brep, f)?;
            let eu = usize::try_from(sel(at.sel, eus.len()))
                .ok()
                .and_then(|i| eus.get(i).copied());
            let curve = match eu {
                Some(eu) => eu1::uv_trim_curve(&m.brep, eu)
                    .map_err(|e| harness(e.to_string()))?
                    .0,
                None => None,
            };
            match curve {
                Some(c) => {
                    let ivl = c.natural_interval();
                    let t = ivl.min() + at.a * (ivl.max() - ivl.min());
                    let p = c.evaluate_point(t).map_err(|e| harness(e.to_string()))?;
                    Point2::new(p.x, p.y)
                }
                // no trim curve: the domain centre
                None => Point2::new(min.x + 0.5 * (max.x - min.x), min.y + 0.5 * (max.y - min.y)),
            }
        }
        x => return Err(harness(format!("bad point mode {x}"))),
    };
    Ok(Point2::new(base.x + at.du, base.y + at.dv))
}

fn dom_of(m: &Model, f: FaceId, d: Option<[f64; 4]>) -> Result<[f64; 4], HarnessError> {
    if let Some(d) = d {
        Ok(d)
    } else {
        let dom = m.brep.face(f).map_err(|e| harness(e.to_string()))?.uv_domain;
        Ok([dom.min().x, dom.min().y, dom.max().x, dom.max().y])
    }
}

// ---- the op on both sides ---------------------------------------------------------------

/// The resolved arguments of a B-rep op (after the B-rep and edit blocks), and the
/// faces it reads.
fn op_args(m: &Model, op: &OpIn) -> Result<(Op, Vec<f64>, Vec<FaceId>), HarnessError> {
    match op {
        OpIn::PointClassify {
            face,
            at,
            src_tol,
            do_bnd,
            use_uv,
            tgt_only,
            has_loops,
            loops,
            pc_tol,
            uv_ok,
        } => {
            let f = face_of(m, *face)?;
            let uv = point_of(m, f, at)?;
            let mut v = vec![f64::from(f.index()), uv.x, uv.y, *src_tol, flag(*do_bnd), flag(*use_uv)];
            v.extend([flag(*tgt_only), flag(*has_loops)]);
            let n_loops = m.brep.loops().len();
            let ls: Vec<f64> = loops
                .iter()
                .map(|&l| sel(l, n_loops))
                .filter(|&l| l >= 0)
                .map(|l| l as f64)
                .collect();
            v.push(ls.len() as f64);
            v.extend(ls);
            v.extend([*pc_tol, flag(*uv_ok)]);
            Ok((Op::PointClassify, v, vec![f]))
        }
        OpIn::ClassifyParam {
            face1,
            face2,
            at,
            tol,
            dom1,
            dom2,
        } => {
            let f1 = face_of(m, *face1)?;
            let f2 = face_of(m, *face2)?;
            let uv = point_of(m, f1, at)?;
            let mut v = vec![uv.x, uv.y, *tol, f64::from(f1.index())];
            v.extend(dom_of(m, f1, *dom1)?);
            v.push(f64::from(f2.index()));
            v.extend(dom_of(m, f2, *dom2)?);
            Ok((Op::ClassifyParam, v, vec![f1, f2]))
        }
        OpIn::CurveOnClassify {
            face,
            dim,
            from,
            to,
            tol,
            do_pc,
            big_gaps,
            check3d,
        } => {
            let f = face_of(m, *face)?;
            let a = point_of(m, f, from)?;
            let b = point_of(m, f, to)?;
            let (p, q) = if *dim == 2 {
                ([a.x, a.y, 0.0], [b.x, b.y, 0.0])
            } else {
                let s = m
                    .brep
                    .face(f)
                    .map_err(|e| harness(e.to_string()))?
                    .surface
                    .as_ref()
                    .ok_or_else(|| harness("face without surface"))?;
                let p = s.evaluate_point(a).map_err(|e| harness(e.to_string()))?;
                let q = s.evaluate_point(b).map_err(|e| harness(e.to_string()))?;
                ([p.x, p.y, p.z], [q.x, q.y, q.z])
            };
            let mut v = vec![f64::from(f.index()), f64::from(*dim)];
            v.extend(p);
            v.extend(q);
            v.extend([*tol, flag(*do_pc), flag(*big_gaps), flag(*check3d), 1.0]);
            Ok((Op::CurveOnClassify, v, vec![f]))
        }
        OpIn::ProtoVertex { .. } => Err(harness("not a B-rep op")),
    }
}

/// The C++ point classification block as outputs under `prefix`.
fn put_pc(o: Output, prefix: &str, pc: &[f64]) -> Output {
    o.values(&format!("{prefix}pc"), pc.to_vec())
}

fn proto_input(op: &OpIn) -> Option<Vec<f64>> {
    match op {
        OpIn::ProtoVertex {
            approx,
            capture,
            override_tol,
            p,
            tol,
            q,
        } => {
            let mut v = vec![*approx, *capture, *override_tol];
            v.extend(p);
            v.push(*tol);
            v.extend(q);
            Some(v)
        }
        _ => None,
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if let Some(v) = proto_input(&i.op) {
        let o = ffi::eval(Op::ProtoVertex, "", &v).map_err(|e| harness(format!("c++: {e}")))?;
        return Ok(Output::new().values("proto", o));
    }
    let m = match model(i)? {
        Ok(m) => m,
        Err(o) => return Ok(o),
    };
    let (op, args, faces) = op_args(&m, &i.op)?;
    for &f in &faces {
        if !face_exact(&m, f)? {
            return Ok(Output::err("inexact_geometry"));
        }
    }
    let path = path_of(i.brep.as_ref())?;
    let mut input = block(i)?;
    input.extend(&m.edit_block);
    input.extend(&args);
    let o = ffi::eval(op, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = o.get(1..).ok_or_else(|| harness("empty c++ output"))?;
    match op {
        Op::PointClassify => {
            let (head, pc) = body.split_at(2.min(body.len()));
            let status = head.first().copied().unwrap_or(-9.0);
            let tsc = head.get(1).copied().unwrap_or(-9.0);
            Ok(put_pc(
                Output::new().scalar("status", status).scalar("has_trim_cache", tsc),
                "",
                pc,
            ))
        }
        Op::ClassifyParam => Ok(Output::new().values("which", body.to_vec())),
        _ => Ok(Output::new().values("curve_classification", body.to_vec())),
    }
}

/// The point classification block of the shim (`put_pc`).
fn pc_block(pc: &PointClassification<EntityRef>) -> Vec<f64> {
    let (kind, index) = match pc.object() {
        None => (0.0, -1.0),
        Some(EntityRef::Face(f)) => (1.0, f64::from(f.index())),
        Some(EntityRef::Edge(e)) => (2.0, f64::from(e.index())),
        Some(EntityRef::Vertex(v)) => (3.0, f64::from(v.index())),
        Some(_) => (-3.0, -1.0),
    };
    let uv = pc.uv_param();
    vec![
        f64::from(pc.point_class().code()),
        kind,
        index,
        pc.gap_3d(&Kernel::default(), &NoGeometry, None).unwrap_or(f64::NAN),
        pc.src_zone_tol_3d(),
        pc.pre_snap_param(),
        pc.t_param(),
        uv.x,
        uv.y,
        flag(pc.are_parameters_set()),
    ]
}

/// No geometry behind the B-rep handles (only the stored gap is read).
struct NoGeometry;

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
    fn point(&self, _o: EntityRef) -> Option<solidcore::math::Point3> {
        None
    }
}

/// The handles of a Rust curve classification (`CurveOnClassify`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum H {
    Face(FaceId),
    Surface(FaceId),
    Curve,
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Face(_) => ClassObjectKind::Face,
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve => ClassObjectKind::Curve,
        }
    }
}

struct Objects<'a> {
    brep: &'a Brep,
    curve: Curve,
}

impl qf::CurveOnClassifyObjects<H> for Objects<'_> {
    fn face(&self, f: FaceId) -> H {
        H::Face(f)
    }
    fn surface(&self, f: FaceId) -> Option<H> {
        self.brep
            .face(f)
            .ok()
            .and_then(|r| r.surface.as_ref())
            .map(|_| H::Surface(f))
    }
    fn curve(&self, curve: H) -> Option<&Curve> {
        (curve == H::Curve).then_some(&self.curve)
    }
    fn loop_id(&self, _tgt_loop: H) -> Option<LoopId> {
        // the classification has no target loops
        None
    }
}

/// A Rust error as the case outcome: deferred for [`SolidError::Unported`], else an
/// error no C++ output has (a visible failure).
fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    if let OpIn::ProtoVertex {
        approx,
        capture,
        override_tol,
        p,
        tol,
        q,
    } = &i.op
    {
        let h = EntityRef::Face(FaceId::from_index(0));
        let mut mgr: ProtoTopologyManager<EntityRef> =
            match ProtoTopologyManager::new(h, h, h, *approx, *capture, *override_tol) {
                Ok(m) => m,
                Err(e) => return Ok(rust_err(&e)),
            };
        let [px, py, pz] = *p;
        let [qx, qy, qz] = *q;
        let id = match mgr.new_proto_vertex(Vec3::new(px, py, pz), *tol) {
            Ok(id) => id,
            Err(e) => return Ok(rust_err(&e)),
        };
        let pv = mgr
            .proto_vertex(id)
            .ok_or_else(|| harness("proto vertex missing"))?;
        let qp = Vec3::new(qx, qy, qz);
        let out = vec![
            flag(qf::proto_vertex_contains_point(&mgr, pv, qp)),
            qf::proto_vertex_distance_to(pv, qp),
            qf::proto_vertex_capture_distance(&mgr),
            mgr.capture_distance(),
        ];
        return Ok(Output::new().values("proto", out));
    }
    let m = match model(i)? {
        Ok(m) => m,
        Err(o) => return Ok(o),
    };
    let (op, args, faces) = op_args(&m, &i.op)?;
    for &f in &faces {
        if !face_exact(&m, f)? {
            return Ok(Output::err("inexact_geometry"));
        }
    }
    let b = &m.brep;
    match (&i.op, op) {
        (
            OpIn::PointClassify {
                src_tol,
                do_bnd,
                use_uv,
                tgt_only,
                has_loops,
                pc_tol,
                uv_ok,
                ..
            },
            Op::PointClassify,
        ) => {
            let f = faces.first().copied().ok_or_else(|| harness("no face"))?;
            let uv = Point2::new(
                args.get(1).copied().unwrap_or(0.0),
                args.get(2).copied().unwrap_or(0.0),
            );
            let n = args.get(8).copied().unwrap_or(0.0) as usize;
            let loops: Vec<LoopId> = args
                .get(9..9 + n)
                .unwrap_or(&[])
                .iter()
                .map(|&l| LoopId::from_index(l as u32))
                .collect();
            let mut pc = PointClassification::new(zone_tol3d(&k, *pc_tol));
            let opts = qf::PointClassifyOptions {
                src_zone_tol_3d: zone_tol3d(&k, *src_tol),
                do_boundary_intersections: *do_bnd,
                use_uv_classification: *use_uv,
                tgt_loops_only: *tgt_only,
                tgt_loops: has_loops.then_some(loops.as_slice()),
                uv_space_okay: *uv_ok,
                trim_cache: None,
            };
            let status = match qf::point_classify(&k, b, f, uv, &mut pc, opts) {
                Ok(()) => 0.0,
                Err(SolidError::Upstream { code, .. }) => f64::from(code),
                Err(e) => return Ok(rust_err(&e)),
            };
            Ok(put_pc(
                Output::new().scalar("status", status).scalar("has_trim_cache", 0.0),
                "",
                &pc_block(&pc),
            ))
        }
        (OpIn::ClassifyParam { tol, .. }, Op::ClassifyParam) => {
            let [f1, f2] = [faces.first(), faces.get(1)].map(Option::<&FaceId>::copied);
            let (Some(f1), Some(f2)) = (f1, f2) else {
                return Err(harness("no faces"));
            };
            let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
            let uv = Point2::new(a(0), a(1));
            let d1 = Extent2d::from_points(Point2::new(a(4), a(5)), Point2::new(a(6), a(7)));
            let d2 = Extent2d::from_points(Point2::new(a(9), a(10)), Point2::new(a(11), a(12)));
            match qf::classify_param_on_faces(&k, b, uv, zone_tol3d(&k, *tol), f1, &d1, f2, &d2) {
                Ok(w) => Ok(Output::new().values("which", vec![f64::from(w)])),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        (
            OpIn::CurveOnClassify {
                dim,
                tol,
                do_pc,
                big_gaps,
                check3d,
                ..
            },
            Op::CurveOnClassify,
        ) => {
            let f = faces.first().copied().ok_or_else(|| harness("no face"))?;
            let a = |i: usize| args.get(i).copied().unwrap_or(0.0);
            let p = Vec3::new(a(2), a(3), a(4));
            let q = Vec3::new(a(5), a(6), a(7));
            let line = Line::bounded(
                p,
                q - p,
                &Extent1d::from_min_max(0.0, (q - p).length()),
                &BoundedLineOptions {
                    dim: u64::from(*dim),
                    ..BoundedLineOptions::default()
                },
            );
            let objects = Objects {
                brep: b,
                curve: Curve::from(line),
            };
            let mut cc: CurveClassification<H> = CurveClassification::new(&k);
            cc.set_curve(Some(H::Curve), false);
            cc.set_src_zone_tol_3d(zone_tol3d(&k, *tol));
            match qf::curve_on_classify(&k, b, f, &objects, *do_pc, &mut cc, *big_gaps, *check3d, true) {
                Ok(_) => Err(harness("CurveOnClassify is expected to defer")),
                Err(e) => Ok(rust_err(&e)),
            }
        }
        _ => Err(harness("op mismatch")),
    }
}

// ---- generation -------------------------------------------------------------------------

fn gen_at(rng: &mut Rng) -> At {
    let scale = *rng
        .choose(&[0.0, 0.0, 1e-12, 1e-9, 1e-7, 1e-6, 1e-5, 1e-4, 1e-3, 1e-2, 0.1])
        .unwrap_or(&0.0);
    let off = |rng: &mut Rng| if rng.chance(0.5) { scale } else { -scale };
    match rng.below(10) {
        0..=3 => At {
            mode: "frac".into(),
            a: rng.uniform(-0.1, 1.1),
            b: rng.uniform(-0.1, 1.1),
            sel: 0,
            du: 0.0,
            dv: 0.0,
        },
        4 => At {
            mode: "frac".into(),
            a: *rng.choose(&[0.0, 0.5, 1.0]).unwrap_or(&0.0),
            b: *rng.choose(&[0.0, 0.5, 1.0]).unwrap_or(&0.0),
            sel: 0,
            du: off(rng),
            dv: off(rng),
        },
        5..=7 => At {
            mode: "trim".into(),
            a: rng.unit(),
            b: 0.0,
            sel: rng.below(64) as i64,
            du: off(rng),
            dv: off(rng),
        },
        _ => At {
            mode: "trim".into(),
            a: *rng.choose(&[0.0, 1.0]).unwrap_or(&0.0),
            b: 0.0,
            sel: rng.below(64) as i64,
            du: off(rng),
            dv: off(rng),
        },
    }
}

/// The upstream `.smb` files whose faces the export carries exactly (B-spline surfaces
/// and curves; the other files of `topology.edgeuse_p1`'s list have surfaces of
/// revolution, extrusion, cones, spheres or tori, which the export carries only as
/// their B-spline parts). `Mouse.smb` and `box_with_large_fillet.smb` have several
/// faces (two and one of them inexact).
const EXACT_FILES: [&str; 7] = [
    "Endcap.smb",
    "Mouse.smb",
    "box_with_large_fillet.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
];

fn gen_brep(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.8) {
        let files: &[&str] = if rng.chance(0.95) { &EXACT_FILES } else { &FILES };
        BrepIn::File {
            file: (*rng.choose(files).unwrap_or(&"plane_square.smb")).to_string(),
            rebuild: rng.chance(0.8),
        }
    } else {
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
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 0.0, 1e-9, 1e-6, 1e-4, 1e-3, 1e-2, 0.1]).unwrap_or(&0.0)
}

/// Tolerance edits, and the UV trim curves of face `face` built with probability
/// `trims`.
fn gen_edits(rng: &mut Rng, face: i64, trims: f64) -> Vec<Edit> {
    let mut out = Vec::new();
    if rng.chance(trims) {
        out.push(Edit {
            kind: 3,
            sel: face,
            value: 0.0,
        });
    }
    if rng.chance(0.6) {
        return out;
    }
    out.extend((0..=rng.below(3)).map(|_| Edit {
        kind: rng.below(3) as u8,
        sel: rng.below(64) as i64,
        value: *rng.choose(&[0.0, 1e-9, 1e-6, 1e-4, 1e-2, 0.1, 1.0]).unwrap_or(&1e-6),
    }));
    out
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let kind = rng.below(20);
    if kind == 0 {
        let scale = *rng.choose(&[1e-6, 1e-3, 1.0, 1e3]).unwrap_or(&1.0);
        let p = rng.point3(-1.0, 1.0).map(|c| c * scale);
        let d = rng.point3(-1.0, 1.0).map(|c| c * scale * rng.uniform(0.0, 2.0));
        return Inputs {
            brep: None,
            validate_only: false,
            init_gaps: false,
            edits: Vec::new(),
            op: OpIn::ProtoVertex {
                approx: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-3),
                capture: scale * gen_tol(rng) * 10.0,
                override_tol: gen_tol(rng),
                p,
                tol: scale * gen_tol(rng) * 10.0,
                q: [p[0] + d[0], p[1] + d[1], p[2] + d[2]],
            },
        };
    }
    let brep = gen_brep(rng);
    let op = match kind {
        1..=3 => OpIn::ClassifyParam {
            face1: rng.below(64) as i64,
            face2: rng.below(64) as i64,
            at: gen_at(rng),
            tol: gen_tol(rng),
            dom1: None,
            dom2: if rng.chance(0.2) {
                Some([-1.0, -1.0, 1.0, 1.0])
            } else {
                None
            },
        },
        4 => OpIn::CurveOnClassify {
            face: rng.below(64) as i64,
            dim: if rng.chance(0.5) { 2 } else { 3 },
            from: gen_at(rng),
            to: gen_at(rng),
            tol: gen_tol(rng),
            do_pc: rng.chance(0.5),
            big_gaps: rng.chance(0.2),
            check3d: rng.chance(0.2),
        },
        _ => {
            let tgt_only = rng.chance(0.25);
            OpIn::PointClassify {
                face: rng.below(64) as i64,
                at: gen_at(rng),
                src_tol: gen_tol(rng),
                do_bnd: rng.chance(0.5),
                use_uv: rng.chance(0.5),
                tgt_only,
                has_loops: tgt_only && rng.chance(0.7),
                loops: (0..rng.below(3)).map(|_| rng.below(64) as i64).collect(),
                pc_tol: gen_tol(rng),
                uv_ok: rng.chance(0.8),
            }
        }
    };
    // the face classified (the first of ClassifyParam): build its trim curves mostly
    let face = match &op {
        OpIn::PointClassify { face, .. } | OpIn::CurveOnClassify { face, .. } => *face,
        OpIn::ClassifyParam { face1, .. } => *face1,
        OpIn::ProtoVertex { .. } => 0,
    };
    let trims = if matches!(brep, BrepIn::Face { .. }) { 0.9 } else { 0.5 };
    let mut edits = gen_edits(rng, face, trims);
    if let OpIn::ClassifyParam { face2, .. } = &op {
        if rng.chance(trims) {
            edits.push(Edit {
                kind: 3,
                sel: *face2,
                value: 0.0,
            });
        }
    }
    Inputs {
        brep: Some(brep),
        validate_only: false,
        init_gaps: rng.chance(0.1),
        edits,
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.face_p2"
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
