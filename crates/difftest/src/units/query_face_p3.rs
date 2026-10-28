//! Runner for unit `query.face_p3`. C++: `solidcore_ffi::units::query_face_p3`. Rust:
//! `solidcore::query::face::p3` (and, for `sm_FaceClassifyUVLine`,
//! `solidcore::topology::face::p2::calculate_an_internal_point`).
//!
//! **B-reps.** The C++ side builds a real `SmBrep` per call with unit `query.face_p4`'s
//! builders (an upstream `.smb` file, `CreateFaceFromSurface` over a B-spline or `SmPlane`,
//! `MakeFaceWithCurves` with line and circle UV trims and vertex loops over a B-spline or a
//! real `SmCylinder` / `SmSphere` / `SmTorus` / `SmCone`, or an analytic surface's natural
//! face). The Rust side makes the same B-rep with the Rust kernel (unit `query.face_p4`'s
//! `own_brep`, in the shim's traversal order) and resolves the case on it as the shim
//! does (the face, `SetProps_Stage3`'s props and closed loops for the bisector, the sector
//! line's edge uses, parameters and ends: [`resolve`], in the shim's call order), so no
//! C++ state reaches its geometry, topology or case. Every face is compared: the native geometry needs
//! no `inexact_geometry` skip.
//!
//! **Operations.**
//! - `tight`: `SmFace::CalculateTightBoundingBox` of a face: status and box.
//! - `internal`: `SmFace::CalculateAnInternalPoint`, the only caller of the file-static
//!   `sm_FaceClassifyUVLine`: status and point.
//! - `bisector`: `sms_FindAndClassifyLoopBisector` on the face props and closed loops
//!   `SetProps_Stage3` builds (each side builds its own, as unit `query.face_p4`'s
//!   props mode 1), with chosen loops marked as
//!   classified: status, seam flags, closure, whether a curve was built, the UV trim
//!   line's ends and the classification's intervals (bounds and start / mid / end point
//!   classes and vertices).
//! - `sector`: `SmFace::SectorClassifyCurveOn` of a line (3D or UV) from a point where it
//!   crosses an edge's interior or a vertex (resolved by the shim to edge uses,
//!   parameters, orientations and the line's ends): status, inside, possible problem and
//!   the problem.
//!
//! A case the shim rejects (see the FFI wrapper) is `rejected` on both sides.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::line::Line;
use solidcore::error::{EntityRef, SolidError};
use solidcore::geom::extent1d::Extent1d;
use solidcore::heal::face_props::{FaceProps, LoopObject};
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind, PointClassification};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p3 as qf3;
use solidcore::query::face::p4::ZEROED_CLASSIFY_LOOP_IO;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId, LoopId, VertexId};
use solidcore_ffi::units::query_face_p3::{self as ffi, Kind};

use super::query_face_p4::{
    brep_input, brep_path, gen_scale, gen_seam_face, own_brep, rect, setup_error, stage3_props,
    AnySrf, BrepIn, CurveIn,
};
use super::topology_edgeuse_p1::{domain, gen_surface, FILES};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i64 = 1007;

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Tight,
    Internal,
    Bisector {
        #[serde(default)]
        target: u32,
        #[serde(default)]
        mask: u8,
        #[serde(default)]
        containment: u8,
    },
    Sector {
        #[serde(default)]
        loop_sel: u32,
        #[serde(default)]
        eu: u32,
        /// `0` edge interior, `1` start vertex with the clockwise neighbour, `2` with the
        /// radial edge use.
        #[serde(default)]
        mode: u8,
        #[serde(default = "half")]
        frac: f64,
        /// `0` 3D line from the crossing, `1` the reverse, `2` UV line.
        #[serde(default)]
        curve: u8,
        ta: f64,
        tb: f64,
        #[serde(default = "tol")]
        curve_tol: f64,
        #[serde(default = "minus_one")]
        edge_tol: f64,
        #[serde(default)]
        singular: bool,
        #[serde(default)]
        sa: f64,
        #[serde(default)]
        sb: f64,
        #[serde(default = "yes")]
        uv_ok: bool,
    },
}

const fn half() -> f64 {
    0.5
}
const fn tol() -> f64 {
    1e-7
}
const fn minus_one() -> f64 {
    -1.0
}
const fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    /// The face selector (modulo the face count).
    #[serde(default)]
    face: u32,
    #[serde(flatten)]
    op: OpIn,
}

fn kind_of(op: &OpIn) -> Kind {
    match op {
        OpIn::Tight => Kind::Tight,
        OpIn::Internal => Kind::Internal,
        OpIn::Bisector { .. } => Kind::Bisector,
        OpIn::Sector { .. } => Kind::Sector,
    }
}

fn input_of(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = brep_input(&i.brep, i.validate_only, i.init_gaps)?;
    v.push(f64::from(i.face));
    let flag = |b: bool| f64::from(u8::from(b));
    match &i.op {
        OpIn::Tight | OpIn::Internal => {}
        OpIn::Bisector { target, mask, containment } => {
            v.extend([f64::from(*target), f64::from(*mask), f64::from(*containment)]);
        }
        OpIn::Sector {
            loop_sel,
            eu,
            mode,
            frac,
            curve,
            ta,
            tb,
            curve_tol,
            edge_tol,
            singular,
            sa,
            sb,
            uv_ok,
        } => v.extend([
            f64::from(*loop_sel),
            f64::from(*eu),
            f64::from(*mode),
            *frac,
            f64::from(*curve),
            *ta,
            *tb,
            *curve_tol,
            *edge_tol,
            flag(*singular),
            *sa,
            *sb,
            flag(*uv_ok),
        ]),
    }
    Ok(v)
}

fn run_ffi(i: &Inputs, export_only: bool) -> Result<Vec<f64>, HarnessError> {
    ffi::eval(kind_of(&i.op), export_only, &brep_path(&i.brep)?, &input_of(i)?)
        .map_err(|e| harness(format!("c++: {e}")))
}

/// Sequential reader of shim output.
struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = self
            .v
            .get(self.at)
            .copied()
            .ok_or_else(|| harness("shim output too short"))?;
        self.at += 1;
        Ok(x)
    }

    fn int(&mut self) -> Result<i64, HarnessError> {
        Ok(self.num()? as i64)
    }

    fn count(&mut self) -> Result<usize, HarnessError> {
        usize::try_from(self.int()?)
            .ok()
            .filter(|&n| n <= 1 << 20)
            .ok_or_else(|| harness("bad count in shim output"))
    }

    fn pt(&mut self) -> Result<Point3, HarnessError> {
        Ok(Point3::new(self.num()?, self.num()?, self.num()?))
    }
}

// ---- the resolved cases -------------------------------------------------------------------

/// The case the shim resolves from its B-rep, resolved by the Rust side from its own.
enum Resolved {
    Face(FaceId),
    Bisector(Box<BisectorCase>),
    Sector(SectorCase),
}

struct BisectorCase {
    face: FaceId,
    target: usize,
    loops: Vec<LoopId>,
    mask: u32,
    containment: i64,
    props: FaceProps<H>,
}

struct SectorCase {
    face: FaceId,
    eu1: EdgeuseId,
    eu2: EdgeuseId,
    p: [f64; 2],
    tol: [f64; 2],
    o: [OrientType; 2],
    dim: u64,
    start: Point3,
    end: Point3,
    curve_orient: OrientType,
    curve_tol: f64,
    singular: bool,
    sing_uv: Point2,
    uv_ok: bool,
}

/// The drop's solution object (`DropPoint` with no solution array: never stored).
#[derive(Debug, Clone, Copy, PartialEq)]
struct DropObj;

impl solidcore::geom::solution_array::SolutionObject for DropObj {
    fn kind(&self) -> solidcore::geom::solution_array::ObjectKind {
        solidcore::geom::solution_array::ObjectKind::Surface
    }
}

/// The shim's `face_frac`: the point at fractions `(a, b)` of the face's UV domain.
fn face_frac(b: &Brep, f: FaceId, a: f64, c: f64) -> solidcore::Result<Point2> {
    let d = solidcore::topology::face::p2::uv_domain(b, f)?;
    Ok(Point2::new(
        d.min().x + a * (d.max().x - d.min().x),
        d.min().y + c * (d.max().y - d.min().y),
    ))
}

/// The case of `i` on the Rust B-rep `b` (the Rust kernel's own build of the source,
/// unit `query.face_p4`'s [`own_brep`]), resolved as the shim's `run` resolves it on
/// its B-rep (face selection, `bisector_setup`, `sector_setup`, in their order); no C++
/// state reaches it. `Err(output)` for a rejected case (`rejected`, as the shim's) or
/// an error of the Rust setup (an unported callee defers the case).
fn resolve(k: &Kernel, b: &mut Brep, i: &Inputs) -> Result<Result<Resolved, Output>, HarnessError> {
    let rejected = || Ok(Err(Output::err("rejected")));
    let n_faces = b.faces().len();
    if n_faces == 0 {
        return rejected();
    }
    let face_index = i.face as usize % n_faces;
    let face = FaceId::from_index(u32::try_from(face_index).map_err(|_| harness("bad face"))?);
    match &i.op {
        OpIn::Tight | OpIn::Internal => Ok(Ok(Resolved::Face(face))),
        OpIn::Bisector { target, mask, containment } => {
            Ok(bisector_setup(k, b, face, face_index, *target, *mask, *containment).map_err(|o| *o))
        }
        OpIn::Sector { .. } => sector_setup(k, b, face, &i.op),
    }
}

/// The shim's `bisector_setup`: `SetProps_Stage3`'s props and closed loops (unit
/// `query.face_p4`'s props mode 1), the target index (modulo the closed loop count),
/// the mask and containment; rejected without closed loops or surface, or with a pole
/// and fewer than four pole points.
fn bisector_setup(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    face_index: usize,
    target: u32,
    mask: u8,
    containment: u8,
) -> Result<Resolved, Box<Output>> {
    let (props, loops) = stage3_props::<H>(k, b, face, face_index as u64)?;
    let surface = match solidcore::topology::face::p2::surface(b, face) {
        Ok(s) => s.is_some(),
        Err(e) => return Err(Box::new(setup_error(&e))),
    };
    if loops.is_empty() || !surface {
        return Err(Box::new(Output::err("rejected")));
    }
    let target = target as usize % loops.len();
    // the pole points are read at their four slots
    if props.poles != 0 && props.pole_points.len() < 4 {
        return Err(Box::new(Output::err("rejected")));
    }
    Ok(Resolved::Bisector(Box::new(BisectorCase {
        face,
        target,
        loops,
        mask: u32::from(mask),
        containment: i64::from(containment),
        props,
    })))
}

/// The shim's `sector_setup`: the edge uses, parameters, tolerances and orientations
/// where the line crosses an edge's interior (mode 0) or the start vertex of an edge use
/// (with its clockwise neighbour, mode 1, or radial, mode 2), and the line (3D from the
/// crossing, its reverse, or the UV line), in the shim's call order.
#[allow(clippy::too_many_lines)] // one shim routine
fn sector_setup(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    op: &OpIn,
) -> Result<Result<Resolved, Output>, HarnessError> {
    use solidcore::topology::{edge, edgeuse::p1 as eu_mod, face::p2 as face_p2, loopuse};
    let OpIn::Sector {
        loop_sel,
        eu: eu_sel,
        mode,
        frac,
        curve: curve_kind,
        ta,
        tb,
        curve_tol,
        edge_tol,
        singular,
        sa,
        sb,
        uv_ok,
    } = op
    else {
        return Err(harness("not a sector case"));
    };
    let rejected = || Ok(Err(Output::err("rejected")));
    let fail = |e: &SolidError| Ok(Err(setup_error(e)));
    macro_rules! r {
        ($e:expr) => {
            match $e {
                Ok(x) => x,
                Err(e) => return fail(&e),
            }
        };
    }
    let has_surface = r!(face_p2::surface(b, face)).is_some();
    let loops = r!(face_p2::loops(b, face, false, None));
    if !has_surface || loops.is_empty() {
        return rejected();
    }
    let Some(&l) = loops.get(*loop_sel as usize % loops.len()) else {
        return rejected();
    };
    if r!(loopuse::loop_loopuse(b, l)).is_none() {
        return rejected();
    }
    let eus = r!(loopuse::loop_edgeuses(b, l));
    let Some(&eu) = eus.get(*eu_sel as usize % eus.len().max(1)) else {
        return rejected();
    };
    let Some(e) = r!(eu_mod::edge(b, eu)) else {
        return rejected();
    };
    if r!(edge::curve(b, e)).is_none() {
        return rejected();
    }
    let ivl = r!(edge::interval(b, e));
    let (eu1, eu2, p1, p2, o1, o2);
    if *mode == 0 {
        // crossing an edge's interior: both edge uses the same
        eu1 = eu;
        eu2 = eu;
        p1 = ivl.evaluate(*frac);
        p2 = p1;
        o1 = OrientType::Same;
        o2 = OrientType::Opposite;
    } else {
        // at the start vertex of eu: eu (SAME) and its clockwise neighbour (OPPOSITE), or
        // eu's radial edge use (mode 2)
        eu1 = eu;
        p1 = if r!(eu_mod::orientation(b, eu)) == OrientType::Same {
            ivl.min()
        } else {
            ivl.max()
        };
        o1 = OrientType::Same;
        let other = if *mode == 1 {
            r!(eu_mod::cw_edgeuse(b, eu))
        } else {
            r!(eu_mod::radial(b, eu))
        };
        let Some(other) = other else {
            return rejected();
        };
        let Some(e2) = r!(eu_mod::edge(b, other)) else {
            return rejected();
        };
        if r!(edge::curve(b, e2)).is_none() {
            return rejected();
        }
        eu2 = other;
        let ivl2 = r!(edge::interval(b, e2));
        p2 = if r!(eu_mod::orientation(b, other)) == OrientType::Same {
            ivl2.max()
        } else {
            ivl2.min()
        };
        o2 = OrientType::Opposite;
    }
    let cross = {
        let Some(c) = r!(edge::curve(b, e)) else {
            return rejected();
        };
        r!(c.evaluate_point(k, p1))
    };
    // SmTol::GetZoneTol3d of an edge: its GetTolerance
    let tol1 = if *edge_tol < 0.0 { r!(edge::tolerance(b, e)) } else { *edge_tol };
    let tol2 = if *edge_tol < 0.0 {
        match r!(eu_mod::edge(b, eu2)) {
            Some(e2) => r!(edge::tolerance(b, e2)),
            None => return rejected(),
        }
    } else {
        *edge_tol
    };
    // the curve: a line from the crossing to the face point at fractions (ta, tb) (kind 0),
    // the reverse line classified at its max end (kind 1), or the UV line between their
    // parameters (kind 2)
    let tuv = r!(face_frac(b, face, *ta, *tb));
    let (dim, start, end, curve_orient);
    {
        let Some(srf) = r!(face_p2::surface(b, face)) else {
            return rejected();
        };
        if *curve_kind == 2 {
            let dom = r!(srf.natural_uv_domain_with_nurb(k));
            let opts = solidcore::surface::surface::p5::DropPointOptions::default();
            let dropped = match srf.drop_point::<DropObj>(k, cross, &dom, &opts) {
                Ok(d) => d,
                Err(e) if Output::unported(&e).is_some() => return fail(&e),
                // a failed DropPoint status rejects the case, as the shim's
                Err(_) => return rejected(),
            };
            if !dropped.success {
                return rejected();
            }
            let cuv = dropped.result_uv;
            dim = 2;
            start = Point3::new(cuv.x, cuv.y, 0.0);
            end = Point3::new(tuv.x, tuv.y, 0.0);
            curve_orient = OrientType::Same;
        } else {
            let target = match srf.evaluate_point(k, tuv) {
                Ok(p) => p,
                Err(e) if Output::unported(&e).is_some() => return fail(&e),
                Err(_) => return rejected(),
            };
            dim = 3;
            start = if *curve_kind == 1 { target } else { cross };
            end = if *curve_kind == 1 { cross } else { target };
            curve_orient = if *curve_kind == 1 { OrientType::Opposite } else { OrientType::Same };
        }
    }
    if start.distance(end) < 1e-9 {
        return rejected();
    }
    let sing_uv = r!(face_frac(b, face, *sa, *sb));
    Ok(Ok(Resolved::Sector(SectorCase {
        face,
        eu1,
        eu2,
        p: [p1, p2],
        tol: [tol1, tol2],
        o: [o1, o2],
        dim,
        start,
        end,
        curve_orient,
        curve_tol: *curve_tol,
        singular: *singular,
        sing_uv,
        uv_ok: *uv_ok,
    })))
}

// ---- C++ side -----------------------------------------------------------------------------

fn status_err(status: f64) -> Output {
    Output::err(format!("status_{status}"))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let o = run_ffi(i, false)?;
    let mut rd = Rd { v: &o, at: 0 };
    if rd.num()? != 0.0 {
        return Ok(Output::err(format!("build_failed: {}", rd.num()?)));
    }
    if rd.num()? != 0.0 {
        return Ok(Output::err("rejected"));
    }
    let status = rd.num()?;
    Ok(match &i.op {
        OpIn::Tight => {
            if status != 0.0 {
                return Ok(status_err(status));
            }
            let min = rd.pt()?;
            let max = rd.pt()?;
            box_output(min, max)
        }
        OpIn::Internal => {
            if status != 0.0 {
                return Ok(status_err(status));
            }
            Output::new().values("point", rd.pt()?.to_array())
        }
        OpIn::Bisector { .. } => {
            if status != 0.0 {
                return Ok(status_err(status));
            }
            let start = rd.int()?;
            let stop = rd.int()?;
            let closed = rd.int()?;
            let has_curve = rd.int()?;
            let line = if has_curve != 0 {
                vec![rd.num()?, rd.num()?, rd.num()?, rd.num()?]
            } else {
                Vec::new()
            };
            let n = rd.count()?;
            let mut bounds = Vec::new();
            let mut classes = Vec::new();
            for _ in 0..n {
                bounds.extend([rd.num()?, rd.num()?]);
                for _ in 0..6 {
                    classes.push(rd.int()?);
                }
            }
            bisector_output(start, stop, closed, has_curve, line, bounds, classes)
        }
        OpIn::Sector { .. } => {
            if status != 0.0 {
                return Ok(status_err(status));
            }
            let inside = rd.int()?;
            let problem = rd.int()?;
            let kind = rd.int()?;
            Output::new().ints("sector", vec![inside, problem, kind])
        }
    })
}

fn box_output(min: Point3, max: Point3) -> Output {
    Output::new()
        .values("min", min.to_array())
        .values("max", max.to_array())
}

fn bisector_output(
    start: i64,
    stop: i64,
    closed: i64,
    has_curve: i64,
    line: Vec<f64>,
    bounds: Vec<f64>,
    classes: Vec<i64>,
) -> Output {
    Output::new()
        .ints("flags", vec![start, stop, closed, has_curve])
        .values("uv_line", line)
        .values("interval_bounds", bounds)
        .ints("interval_classes", classes)
}

// ---- Rust side ----------------------------------------------------------------------------

/// A Rust error as the case outcome: deferred for [`SolidError::Unported`], the upstream
/// status for an `Upstream` error, else an error no C++ output has (a visible failure).
fn rust_err(e: &SolidError) -> Output {
    if let Some(d) = Output::unported(e) {
        return d;
    }
    match e {
        SolidError::Upstream { code, .. } => status_err(f64::from(*code)),
        _ => Output::err(format!("rust_error: {e}")),
    }
}

/// The handles of the Rust classifications: B-rep topology, the face's surface and the
/// two curves a classification makes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Topo(EntityRef),
    Surface(FaceId),
    Curve(bool),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
    /// A temporary curve (the seam curves of `SmFace::HasSeamProblem`, which the props'
    /// `SetProps_Stage2` runs on a closed surface).
    Temp(usize),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Topo(e) => e.class_kind(),
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) | Self::Temp(_) => {
                ClassObjectKind::Curve
            }
        }
    }

    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        (kind == ClassObjectKind::Curve).then_some(Self::Temp(index))
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self::Topo(EntityRef::Loop(l))
    }

    fn as_loop(&self) -> Option<LoopId> {
        match self {
            Self::Topo(e) => e.as_loop(),
            _ => None,
        }
    }

    fn as_vertex(&self) -> Option<VertexId> {
        match self {
            Self::Topo(e) => e.as_vertex(),
            _ => None,
        }
    }

    fn from_face(f: FaceId) -> Option<Self> {
        Some(Self::Topo(EntityRef::Face(f)))
    }

    fn from_face_surface(f: FaceId) -> Option<Self> {
        Some(Self::Surface(f))
    }

    fn from_classify_curve(uv: bool) -> Option<Self> {
        Some(Self::Curve(uv))
    }

    fn from_topology(t: EntityRef) -> Option<Self> {
        Some(Self::Topo(t))
    }

    fn as_topology(&self) -> Option<EntityRef> {
        match self {
            Self::Topo(t) => Some(*t),
            _ => None,
        }
    }

    fn from_edge_curve(e: EdgeId) -> Option<Self> {
        Some(Self::EdgeCurve(e))
    }

    fn as_edge_curve(&self) -> Option<EdgeId> {
        match self {
            Self::EdgeCurve(e) => Some(*e),
            _ => None,
        }
    }

    fn from_trim_curve(eu: EdgeuseId) -> Option<Self> {
        Some(Self::Trim(eu))
    }

    fn as_trim_curve(&self) -> Option<EdgeuseId> {
        match self {
            Self::Trim(eu) => Some(*eu),
            _ => None,
        }
    }
}

fn point_class_pair(pc: &PointClassification<H>) -> [i64; 2] {
    let class = i64::from(pc.point_class().code());
    let vertex = match pc.vertex_object() {
        Some(H::Topo(EntityRef::Vertex(v))) if pc.point_class() == solidcore::intersect::point_class::PointClassType::Vertex => {
            i64::from(v.index())
        }
        _ => -1,
    };
    [class, vertex]
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    // the B-rep: the Rust kernel's own build of the source (no C++ state)
    let (mut b, _) = match own_brep(&i.brep, i.validate_only, i.init_gaps)? {
        Ok(x) => x,
        Err(out) => return Ok(out),
    };
    // the case, resolved from that B-rep as the shim resolves it
    let resolved = match resolve(&k, &mut b, i)? {
        Ok(x) => x,
        Err(out) => return Ok(out),
    };
    Ok(match resolved {
        Resolved::Face(f) => match &i.op {
            OpIn::Internal => {
                // the shim's `drop_trim_cache(f)` before `CalculateAnInternalPoint`
                // (`harness/cpp/shim/query_face_p3.cpp`)
                super::query_face_p1::drop_trim_cache(&b, f)?;
                match solidcore::topology::face::p2::calculate_an_internal_point(&k, &mut b, f) {
                    Ok(p) => Output::new().values("point", p.to_array()),
                    Err(e) => rust_err(&e),
                }
            }
            _ => match qf3::calculate_tight_bounding_box(&k, &b, f) {
                Ok(bx) => box_output(bx.min(), bx.max()),
                Err(e) => rust_err(&e),
            },
        },
        Resolved::Bisector(c) => rust_bisector(&k, &b, &c)?,
        Resolved::Sector(c) => {
            let curve = Curve::from(Line::from_points(c.start, c.end, c.dim));
            let input = qf3::SectorClassifyInput {
                eu1: qf3::SectorEdgeuse {
                    edgeuse: c.eu1,
                    param: c.p[0],
                    zone_tol_3d: c.tol[0],
                    orientation: c.o[0],
                },
                eu2: qf3::SectorEdgeuse {
                    edgeuse: c.eu2,
                    param: c.p[1],
                    zone_tol_3d: c.tol[1],
                    orientation: c.o[1],
                },
                curve: &curve,
                curve_interval: curve.natural_interval(),
                curve_orientation: c.curve_orient,
                curve_zone_tol_3d: c.curve_tol,
                on_singularity: c.singular,
                singularity_uv: c.sing_uv,
                uv_space_okay: c.uv_ok,
            };
            match qf3::sector_classify_curve_on(&k, &mut b, c.face, &input) {
                Ok(r) => Output::new().ints(
                    "sector",
                    vec![
                        i64::from(r.inside),
                        i64::from(r.possible_problem),
                        r.problem as i64,
                    ],
                ),
                Err(e) => rust_err(&e),
            }
        }
    })
}

fn rust_bisector(k: &Kernel, b: &Brep, c: &BisectorCase) -> Result<Output, HarnessError> {
    let props = &c.props;
    let containment = solidcore::heal::face_props::ContainmentType::from_code(c.containment)
        .ok_or_else(|| harness("bad containment"))?;
    let mut ios = vec![ZEROED_CLASSIFY_LOOP_IO; c.loops.len()];
    let mut marked = std::collections::BTreeSet::new();
    for (idx, (l, io)) in c.loops.iter().zip(ios.iter_mut()).enumerate() {
        if idx < 32 && c.mask & (1 << idx) != 0 {
            marked.insert(*l);
            io.containment = containment;
        }
    }
    let mut objects: qf3::LoopObjectCurves<H> =
        qf3::LoopObjectCurves::new(b, c.face).map_err(|e| harness(e.to_string()))?;
    let mut cc: CurveClassification<H> = CurveClassification::new(k);
    let r = qf3::find_and_classify_loop_bisector(
        k,
        b,
        c.face,
        c.target,
        &c.loops,
        &ios,
        &marked,
        props,
        &mut objects,
        &mut cc,
    );
    let r = match r {
        Ok(r) => r,
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            return Ok(match &e {
                SolidError::InvalidInput { reason, .. }
                    if *reason == "FaceUVDomain Not contained by SurfaceUVDomain" =>
                {
                    status_err(SM_ERR_INVALID_INPUT as f64)
                }
                _ => rust_err(&e),
            });
        }
    };
    let closed = match r.closed_3d {
        None => 3,
        Some(c) => i64::from(c),
    };
    let line = match objects.classify_uv_curve() {
        Some(l) if r.has_curve => {
            let ivl: Extent1d = l.natural_interval();
            let a = l.evaluate_point(k, ivl.min()).map_err(|e| harness(e.to_string()))?;
            let z = l.evaluate_point(k, ivl.max()).map_err(|e| harness(e.to_string()))?;
            vec![a.x, a.y, z.x, z.y]
        }
        _ => Vec::new(),
    };
    let mut bounds = Vec::new();
    let mut classes = Vec::new();
    for ivl in cc.intervals() {
        bounds.extend([ivl.interval.min(), ivl.interval.max()]);
        for pc in [&ivl.start, &ivl.mid, &ivl.end] {
            classes.extend(point_class_pair(pc));
        }
    }
    Ok(bisector_output(
        i64::from(r.start_on_seam),
        i64::from(r.stop_on_seam),
        closed,
        i64::from(r.has_curve),
        line,
        bounds,
        classes,
    ))
}

// ---- generated cases ----------------------------------------------------------------------

/// A flat or curved B-spline surface over `[0, 1]^2` of degree 1..3 (JSON, as unit
/// `topology.edgeuse_p1`'s `SrfIn`): `bulge` 0 makes it planar.
fn bspline(rng: &mut Rng, scale: f64, bulge: f64) -> Result<super::topology_edgeuse_p1::SrfIn, HarnessError> {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(2) as usize;
    let m = q + rng.below(2) as usize;
    let knots = |deg: usize, cnt: usize, rng: &mut Rng| {
        let mut u = vec![0.0; deg + 1];
        let mut inner: Vec<f64> = (0..cnt - deg).map(|_| rng.uniform(0.2, 0.8)).collect();
        inner.sort_by(f64::total_cmp);
        u.extend(inner);
        u.extend(vec![1.0; deg + 1]);
        u
    };
    let now = solidcore::nurbs::math::NOW;
    let net: Vec<Vec<[f64; 4]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let z = if bulge == 0.0 { 0.0 } else { rng.uniform(-bulge, bulge) };
                    [i as f64 * scale, j as f64 * scale, z * scale, now]
                })
                .collect()
        })
        .collect();
    let v = serde_json::json!({
        "bspline": {
            "net": net,
            "p": p,
            "q": q,
            "u": knots(p, n, rng),
            "v": knots(q, m, rng),
        }
    });
    serde_json::from_value(v).map_err(|e| harness(e.to_string()))
}

fn gen_face(rng: &mut Rng, tight: bool) -> Result<BrepIn, HarnessError> {
    let roll = rng.below(100);
    if tight && roll < 35 {
        // a plane face (CreateFaceFromSurface), whose box is that of its edges
        let scale = gen_scale(rng);
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        let lo = [rng.uniform(-1.0, 0.5) * scale, rng.uniform(-1.0, 0.5) * scale];
        let hi = [lo[0] + rng.uniform(0.01, 1.0) * scale, lo[1] + rng.uniform(0.01, 1.0) * scale];
        let surface: super::topology_edgeuse_p1::SrfIn =
            serde_json::from_value(serde_json::json!({"plane": {"o": o, "n": n}}))
                .map_err(|e| harness(e.to_string()))?;
        return Ok(BrepIn::Face {
            dom: [lo[0], lo[1], hi[0], hi[1]],
            surface,
        });
    }
    Ok(if roll < 50 {
        // a B-spline face trimmed by rectangles / circles (holes)
        let scale = gen_scale(rng);
        let bulge = *rng.choose(&[0.0, 0.0, 0.3, 1.0]).unwrap_or(&0.3);
        let surface = AnySrf::Bspline {
            surface: bspline(rng, scale, bulge)?,
        };
        let mut loops = vec![vec![
            CurveIn::Line { a: 0.05, b: 0.05, c: 0.95, d: 0.05, rev: false },
            CurveIn::Line { a: 0.95, b: 0.05, c: 0.95, d: 0.95, rev: false },
            CurveIn::Line { a: 0.95, b: 0.95, c: 0.05, d: 0.95, rev: false },
            CurveIn::Line { a: 0.05, b: 0.95, c: 0.05, d: 0.05, rev: false },
        ]];
        if rng.chance(0.4) {
            loops = vec![vec![CurveIn::Circle {
                a: 0.5,
                b: 0.5,
                r: rng.uniform(0.2, 0.45),
                rev: false,
            }]];
        }
        if rng.chance(0.4) {
            loops.push(vec![CurveIn::Circle {
                a: rng.uniform(0.4, 0.6),
                b: rng.uniform(0.4, 0.6),
                r: rng.uniform(0.02, 0.1),
                rev: true,
            }]);
        }
        if rng.chance(0.2) {
            loops = vec![rect(rng)];
        }
        BrepIn::Curves {
            copies: 1,
            surface,
            loops,
            points: Vec::new(),
        }
    } else if roll < 75 {
        // CreateFaceFromSurface over a B-spline or plane
        let s = gen_surface(rng);
        let [u0, v0, u1, v1] = domain(&s);
        let t = |a: f64, b: f64, f: f64| a + (b - a) * f;
        let (fa, fb) = if rng.chance(0.2) {
            (0.0, 1.0)
        } else {
            (rng.uniform(0.0, 0.4), rng.uniform(0.6, 1.0))
        };
        BrepIn::Face {
            dom: [t(u0, u1, fa), t(v0, v1, fa), t(u0, u1, fb), t(v0, v1, fb)],
            surface: s,
        }
    } else if roll < 90 {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"sphere.smb")).to_string(),
            rebuild: rng.chance(0.2),
        }
    } else {
        // analytic faces: compared where the geometry the operation reads is exact
        gen_seam_face(rng)
    })
}

fn gen_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let roll = rng.below(100);
    let brep = gen_face(rng, roll < 55)?;
    let op = if roll < 55 {
        OpIn::Tight
    } else if roll < 63 {
        OpIn::Internal
    } else if roll < 75 {
        OpIn::Bisector {
            target: rng.below(4) as u32,
            mask: if rng.chance(0.3) { rng.below(16) as u8 } else { 0 },
            containment: *rng.choose(&[0u8, 1, 2, 5, 6]).unwrap_or(&1),
        }
    } else {
        OpIn::Sector {
            loop_sel: rng.below(3) as u32,
            eu: rng.below(8) as u32,
            // (the radial mode with a singularity returns before the binormals)
            mode: if rng.chance(0.3) { 2 } else { rng.below(3) as u8 },
            frac: *rng.choose(&[0.5, 0.25, 0.0, 1.0, 1e-6]).unwrap_or(&0.5),
            curve: rng.below(3) as u8,
            ta: rng.uniform(0.0, 1.0),
            tb: rng.uniform(0.0, 1.0),
            curve_tol: *rng.choose(&[1e-7, 1e-5, 1e-3, 0.0]).unwrap_or(&1e-7),
            edge_tol: *rng.choose(&[-1.0, -1.0, 1e-7, 1e-4]).unwrap_or(&-1.0),
            singular: rng.chance(0.4),
            sa: rng.uniform(0.0, 1.0),
            sb: *rng.choose(&[0.0, 1.0, 0.5]).unwrap_or(&0.0),
            uv_ok: rng.chance(0.7),
        }
    };
    Ok(Inputs {
        brep,
        validate_only: rng.chance(0.05),
        init_gaps: rng.chance(0.1),
        face: rng.below(64) as u32,
        op,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.face_p3"
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
        gen_case(rng).ok().and_then(|c| serde_json::to_value(c).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generated cases reach every operation, and a good share of them is compared
    /// (neither rejected nor deferred).
    #[test]
    fn generated_cases_compare() {
        let mut rng = Rng::new(7);
        let mut why: BTreeMap<String, usize> = BTreeMap::new();
        let n = 200;
        let mut compared = 0;
        for _ in 0..n {
            let i = gen_case(&mut rng).unwrap();
            let op = serde_json::to_value(&i).unwrap()["op"].clone();
            let kind = serde_json::to_value(&i.brep).unwrap()["kind"].clone();
            let c = cpp(&i).unwrap();
            let r = rust(&i).unwrap();
            let key = if let Some(d) = &r.deferred {
                format!("{op} {kind} deferred {} {}", d.unit, d.symbol)
            } else if let Some(e) = &c.error {
                format!("{op} {kind} error {e}")
            } else {
                compared += 1;
                format!("{op} {kind} compared")
            };
            *why.entry(key).or_default() += 1;
        }
        for (k, v) in &why {
            eprintln!("{v:4} {k}");
        }
        assert!(compared * 10 > n, "only {compared} of {n} compared");
    }
}
