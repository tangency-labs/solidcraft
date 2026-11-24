//! Runner for unit `query.face_p3`. Ported from upstream `SmFace`'s classifications;
//! checked against its golden. Rust: `solidcore::query::face::p3` (and, for `sm_FaceClassifyUVLine`,
//! `solidcore::topology::face::p2::calculate_an_internal_point`).
//!
//! **B-reps.** Each case makes a B-rep with the Rust kernel (unit `query.face_p4`'s
//! `own_brep`, in upstream's traversal order: an upstream `.smb` file,
//! `CreateFaceFromSurface` over a B-spline or `SmPlane`, `MakeFaceWithCurves` with line
//! and circle UV trims and vertex loops over a B-spline or an `SmCylinder` / `SmSphere` /
//! `SmTorus` / `SmCone`, or an analytic surface's natural face) and resolves the case on
//! it (the face, `SetProps_Stage3`'s props and closed loops for the bisector, the sector
//! line's edge uses, parameters and ends: [`resolve`], in upstream's call order).
//!
//! **Operations.**
//! - `tight`: `SmFace::CalculateTightBoundingBox` of a face: status and box.
//! - `internal`: `SmFace::CalculateAnInternalPoint`, the only caller of the file-static
//!   `sm_FaceClassifyUVLine`: status and point.
//! - `bisector`: `sms_FindAndClassifyLoopBisector` on the face props and closed loops
//!   `SetProps_Stage3` builds (as unit `query.face_p4`'s props mode 1), with chosen loops marked as
//!   classified: status, seam flags, closure, whether a curve was built, the UV trim
//!   line's ends and the classification's intervals (bounds and start / mid / end point
//!   classes and vertices).
//! - `sector`: `SmFace::SectorClassifyCurveOn` of a line (3D or UV) from a point where it
//!   crosses an edge's interior or a vertex (resolved to edge uses,
//!   parameters, orientations and the line's ends): status, inside, possible problem and
//!   the problem.
//!
//! A case outside the operation's defined inputs (see [`resolve`]) is `rejected`.

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

use super::query_face_p4::{
    gen_scale, gen_seam_face, own_brep, rect, setup_error, stage3_props,
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

/// The unit runner.
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

fn status_err(status: f64) -> Output {
    Output::err(format!("status_{status}"))
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
/// status for a failure that has one, else an error no C++ output has (a visible failure).
fn rust_err(e: &SolidError) -> Output {
    if let Some(d) = Output::unported(e) {
        return d;
    }
    match crate::output::recorded_status(e) {
        Some(code) => status_err(f64::from(code)),
        None => Output::err(format!("rust_error: {e}")),
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
    let k = crate::runner_kernel();
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
    if matches!(resolved, Resolved::Face(_)) && !matches!(i.op, OpIn::Internal) {
        // the body boxed, kept when a test captures it (`dump::capture`)
        crate::dump::body(&k, "tight", &b);
    }
    Ok(match resolved {
        Resolved::Face(f) => match &i.op {
            OpIn::Internal => {
                // the reference shim's `drop_trim_cache(f)` before
                // `CalculateAnInternalPoint`
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
                SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. }
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        gen_case(rng).ok().and_then(|c| serde_json::to_value(c).ok())
    }
}

/// The face tight boxes of the golden rows WR-588's search moved (#17685), each side checked
/// against a certified bound on its face's extreme on that side.
///
/// # What is certified
///
/// A face is its surface `S` over the region `Omega` of its uv domain that its trim loops
/// bound: each edge use's pcurve, and the straight uv segment closing a loop between two of
/// them where their ends differ (the face as `mass::enclosure` integrates it). On a side `d`
/// (`+-x`, `+-y`, `+-z`) the face's extreme is `max f` over `Omega`, `f = d . S`. Its greatest
/// point lies on the boundary of `Omega`, or inside it where `grad f = 0`. So with every
/// quantity enclosed in rounded interval arithmetic (`Ivl`; the surface's partials over a
/// uv box, `nurbs_partials_over_box`; a pcurve's derivatives over a parameter interval,
/// `curve_derivs_over`):
/// - **Boundary.** On a piece of a pcurve or closing segment over `T`, `g(t) = f(c(t))` lies
///   below `g(t_m) + g'(T) (T - t_m)` (mean value form, `g' = f_u u' + f_v v'`), and below
///   `f` over the uv box of `c(T)`.
/// - **Inside.** A uv cell where `f_u` or `f_v` is enclosed away from 0 holds no point with
///   `grad f = 0`, and is set aside. Any other cell is bounded by `f` over it (its range, and
///   its mean value form about its middle), unless it is clear of `Omega`: no pcurve piece's
///   enclosure meets it and its winding number round the loops is 0. The winding number of a
///   point clear of every piece's box is the polygon's through the pieces' ends, since each
///   piece and its chord lie in a convex box without the point.
/// - **Refining.** Items are refined greatest bound first; `L` is the greatest value
///   enclosed at a point of the face (a boundary point, or the middle of a cell inside
///   `Omega`), an item whose bound is at most `L` is set aside, and `U`, the greatest bound
///   left when the work stops, holds `max f`.
///
/// The surface is read in its NURBS form (`as_b_spline`, what the box's own searches read);
/// a uv box is clamped to the surface's knots, where its evaluation clamps.
///
/// Run: `cargo test --profile goldens -p goldens --lib tight_box_certificate -- --ignored --nocapture`.
#[cfg(test)]
mod tight_box_certificate {
    use std::cmp::Ordering;
    use std::collections::BinaryHeap;

    use solidcore::blend::fillet_section::certificate::{
        curve_derivs_over, nurbs_partials_over_box, NurbsCurve,
    };
    use solidcore::blend::fillet_section::interval::Ivl;
    use solidcore::blend::fillet_section::station::{as_b_spline, Nurbs};
    use solidcore::kernel::Kernel;
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::query::face::p3 as qf3;
    use solidcore::topology::edgeuse::{p1 as eu1, p3 as eu3};
    use solidcore::topology::{Brep, FaceId};

    /// A piece of the boundary: a pcurve walked forward (`dir` 1) or backward.
    struct Piece {
        cn: NurbsCurve,
        dir: f64,
    }

    /// A face as the certificate reads it.
    struct Face {
        cn: Nurbs,
        /// The surface's knot range, `[u0, v0, u1, v1]`.
        range: [f64; 4],
        /// The loops, each its pieces in walking order.
        loops: Vec<Vec<Piece>>,
    }

    fn knot(u: &[f64], i: usize) -> f64 {
        u.get(i).copied().unwrap_or(f64::NAN)
    }

    fn neg(x: Ivl) -> Ivl {
        Ivl::new(-x.hi, -x.lo)
    }

    fn segment(a: [f64; 2], z: [f64; 2]) -> NurbsCurve {
        NurbsCurve {
            p: 1,
            n: 2,
            u: vec![0.0, 0.0, 1.0, 1.0],
            pw: vec![a[0], a[1], 0.0, 1.0, z[0], z[1], 0.0, 1.0],
        }
    }

    fn param_range(c: &NurbsCurve) -> (f64, f64) {
        (knot(&c.u, c.p), knot(&c.u, c.n))
    }

    /// The uv point's enclosure at `t`.
    fn point_at(c: &NurbsCurve, t: f64) -> Option<(Ivl, Ivl)> {
        let d = curve_derivs_over(c, Ivl::from(t), 0, None).ok()?;
        let p = d.first()?;
        Some((p.x, p.y))
    }

    fn face_of(k: &Kernel, b: &mut Brep, f: FaceId) -> Result<Face, String> {
        let e = |x: solidcore::SolidError| x.to_string();
        let surface = b.face(f).map_err(e)?.surface.clone().ok_or("no surface")?;
        let bs = as_b_spline(k, &surface).map_err(e)?.ok_or("no NURBS form")?;
        let cn = Nurbs::of(&bs).map_err(e)?;
        let range = [
            knot(&cn.u, cn.p),
            knot(&cn.v, cn.q),
            knot(&cn.u, cn.nu),
            knot(&cn.v, cn.nv),
        ];
        let up = solidcore::topology::face::p2::upward_faceuse(b, f)
            .map_err(e)?
            .ok_or("no upward faceuse")?;
        let mut loops = Vec::new();
        for lu in eu1::faceuse_loopuses(b, up, false, None).map_err(e)? {
            if solidcore::topology::loopuse::is_vertex_loopuse(b, lu).map_err(e)? {
                continue;
            }
            let mut curves = Vec::new();
            for eu in eu1::loopuse_edgeuses(b, lu).map_err(e)? {
                let dir = if eu1::orientation(b, eu).map_err(e)? == OrientType::Same {
                    1.0
                } else {
                    -1.0
                };
                let fetch =
                    eu3::get_or_create_uv_trim_curve(k, b, eu, false, false, true).map_err(e)?;
                fetch.status.map_err(e)?;
                let c = eu1::uv_trim_curve(b, eu).map_err(e)?.0.ok_or("no pcurve")?;
                let bs = c.b_spline_base().ok_or("a pcurve with no NURBS form")?;
                curves.push((NurbsCurve::of(bs).map_err(e)?, dir));
            }
            // the walk's ends of each curve, and the closing segments between them
            let mut ends = Vec::new();
            for (c, dir) in &curves {
                let (t0, t1) = param_range(c);
                let (a, z) = if *dir > 0.0 { (t0, t1) } else { (t1, t0) };
                let pa = point_at(c, a).ok_or("a pcurve end")?;
                let pz = point_at(c, z).ok_or("a pcurve end")?;
                ends.push(([pa.0.mid(), pa.1.mid()], [pz.0.mid(), pz.1.mid()]));
            }
            let n = curves.len();
            let mut pieces = Vec::new();
            for (i, (c, dir)) in curves.into_iter().enumerate() {
                let prev = ends[(i + n - 1) % n].1;
                let this = ends[i].0;
                if prev != this {
                    pieces.push(Piece {
                        cn: segment(prev, this),
                        dir: 1.0,
                    });
                }
                pieces.push(Piece { cn: c, dir });
            }
            loops.push(pieces);
        }
        if loops.is_empty() {
            return Err("a face without trim loops".into());
        }
        Ok(Face { cn, range, loops })
    }

    fn clamp(x: Ivl, lo: f64, hi: f64) -> Ivl {
        Ivl::new(x.lo.max(lo).min(hi), x.hi.min(hi).max(lo))
    }

    /// `f = sign * S[axis]` and its uv partials over the uv box (clamped to the knots).
    fn f_over(face: &Face, axis: usize, sign: f64, u: Ivl, v: Ivl) -> Option<[Ivl; 3]> {
        let [u0, v0, u1, v1] = face.range;
        let (u, v) = (clamp(u, u0, u1), clamp(v, v0, v1));
        let p = nurbs_partials_over_box(&face.cn, u.lo, u.hi, v.lo, v.hi, 1, 0).ok()?;
        let pick = |a: usize, b: usize| {
            let w = p.at(a, b);
            let x = match axis {
                0 => w.x,
                1 => w.y,
                _ => w.z,
            };
            if sign > 0.0 {
                x
            } else {
                neg(x)
            }
        };
        let out = [pick(0, 0), pick(1, 0), pick(0, 1)];
        out.iter().all(|x| x.is_finite()).then_some(out)
    }

    fn holds_zero(x: Ivl) -> bool {
        x.lo <= 0.0 && 0.0 <= x.hi
    }

    fn meets(a: (Ivl, Ivl), u: Ivl, v: Ivl) -> bool {
        a.0.lo <= u.hi && u.lo <= a.0.hi && a.1.lo <= v.hi && v.lo <= a.1.hi
    }

    /// The winding number of the loops round the cell, `None` when a piece may meet it.
    fn winding(face: &Face, u: Ivl, v: Ivl) -> Option<i64> {
        let p = [u.mid(), v.mid()];
        let angle = |a: [f64; 2], z: [f64; 2]| {
            let (ax, ay, zx, zy) = (a[0] - p[0], a[1] - p[1], z[0] - p[0], z[1] - p[1]);
            (ax * zy - ay * zx).atan2(ax * zx + ay * zy)
        };
        let mut total = 0.0;
        for lp in &face.loops {
            for piece in lp {
                let (t0, t1) = param_range(&piece.cn);
                // (interval, depth), walked in the piece's direction
                let mut stack = vec![(t0, t1, 0u32)];
                let mut leaves = Vec::new();
                while let Some((a, z, depth)) = stack.pop() {
                    let d = curve_derivs_over(&piece.cn, Ivl::new(a, z), 0, None).ok()?;
                    let (pa, pz) = (point_at(&piece.cn, a)?, point_at(&piece.cn, z)?);
                    // the piece, its ends' boxes and so its chord between their middles lie
                    // in this convex box
                    let bx = (
                        d.first()?.x.hull(pa.0).hull(pz.0),
                        d.first()?.y.hull(pa.1).hull(pz.1),
                    );
                    if !meets(bx, u, v) {
                        leaves.push((a, z));
                        continue;
                    }
                    if depth >= 30 {
                        return None;
                    }
                    let m = 0.5 * (a + z);
                    stack.push((a, m, depth + 1));
                    stack.push((m, z, depth + 1));
                }
                leaves.sort_by(|x, y| x.0.total_cmp(&y.0));
                // each leaf's chord between its ends' middles (shared with the next leaf)
                for (a, z) in leaves {
                    let pa = point_at(&piece.cn, a)?;
                    let pz = point_at(&piece.cn, z)?;
                    let (ma, mz) = ([pa.0.mid(), pa.1.mid()], [pz.0.mid(), pz.1.mid()]);
                    total += piece.dir * angle(ma, mz);
                }
            }
        }
        // (the leaves' shared ends are the same mid point, and a closing segment starts at
        // the mid of the curve end before it: the polygon is closed)
        Some((total / std::f64::consts::TAU).round() as i64)
    }

    #[derive(Clone, Copy, Debug)]
    enum What {
        Bnd { lp: usize, piece: usize, t: (f64, f64) },
        Cell { u: (f64, f64), v: (f64, f64) },
    }

    #[derive(Clone, Copy, Debug)]
    struct Item {
        upper: f64,
        what: What,
        depth: u32,
    }

    impl PartialEq for Item {
        fn eq(&self, o: &Self) -> bool {
            self.upper.total_cmp(&o.upper) == Ordering::Equal
        }
    }
    impl Eq for Item {}
    impl PartialOrd for Item {
        fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
            Some(self.cmp(o))
        }
    }
    impl Ord for Item {
        fn cmp(&self, o: &Self) -> Ordering {
            self.upper.total_cmp(&o.upper)
        }
    }

    /// What [`extreme`] proved: `lower <= max f <= upper`.
    #[derive(Debug, Clone)]
    pub struct Bound {
        pub lower: f64,
        pub upper: f64,
        pub items: usize,
        /// Where `lower` is taken.
        pub lower_at: String,
    }

    const MAX_ITEMS: usize = 400_000;
    const MAX_DEPTH: u32 = 48;

    /// A boundary piece's bound over `t`, and a value taken at its middle.
    fn bnd(face: &Face, axis: usize, sign: f64, piece: &Piece, t: (f64, f64)) -> (f64, f64) {
        let tm = 0.5 * (t.0 + t.1);
        let Ok(d) = curve_derivs_over(&piece.cn, Ivl::new(t.0, t.1), 1, None) else {
            return (f64::INFINITY, f64::NEG_INFINITY);
        };
        let (Some(c0), Some(c1)) = (d.first(), d.get(1)) else {
            return (f64::INFINITY, f64::NEG_INFINITY);
        };
        let Some(over) = f_over(face, axis, sign, c0.x, c0.y) else {
            return (f64::INFINITY, f64::NEG_INFINITY);
        };
        let mid = point_at(&piece.cn, tm).and_then(|(u, v)| f_over(face, axis, sign, u, v));
        let Some(mid) = mid else {
            return (over[0].hi, f64::NEG_INFINITY);
        };
        // where the box is clamped, the clamped coordinate's derivative may be 0
        let [u0, v0, u1, v1] = face.range;
        let du = if c0.x.lo < u0 || c0.x.hi > u1 { c1.x.hull(Ivl::from(0.0)) } else { c1.x };
        let dv = if c0.y.lo < v0 || c0.y.hi > v1 { c1.y.hull(Ivl::from(0.0)) } else { c1.y };
        let g1 = over[1] * du + over[2] * dv;
        let mv = mid[0] + g1 * Ivl::new(t.0 - tm, t.1 - tm);
        let mv = mv.hi.max(mid[0].hi); // (rounding of t - tm is held by the hull with mid)
        (over[0].hi.min(mv), mid[0].lo)
    }

    /// A cell's bound, and whether it may hold a greatest point of the face: `None` when it
    /// holds none (`grad f` away from 0, or clear of the face).
    fn cell(face: &Face, axis: usize, sign: f64, u: (f64, f64), v: (f64, f64)) -> Option<(f64, f64)> {
        let (iu, iv) = (Ivl::new(u.0, u.1), Ivl::new(v.0, v.1));
        let Some(over) = f_over(face, axis, sign, iu, iv) else {
            return Some((f64::INFINITY, f64::NEG_INFINITY));
        };
        let [u0, v0, u1, v1] = face.range;
        let clamped_u = u.0 < u0 || u.1 > u1;
        let clamped_v = v.0 < v0 || v.1 > v1;
        if (!holds_zero(over[1]) && !clamped_u) || (!holds_zero(over[2]) && !clamped_v) {
            return None;
        }
        let (mu, mv) = (0.5 * (u.0 + u.1), 0.5 * (v.0 + v.1));
        let mid = f_over(face, axis, sign, Ivl::from(mu), Ivl::from(mv));
        let mut upper = over[0].hi;
        if let (Some(mid), false, false) = (mid, clamped_u, clamped_v) {
            let m = mid[0] + over[1] * Ivl::new(u.0 - mu, u.1 - mu) + over[2] * Ivl::new(v.0 - mv, v.1 - mv);
            upper = upper.min(m.hi.max(mid[0].hi));
        }
        let lower = match winding(face, iu, iv) {
            Some(0) => return None,
            Some(_) => mid.map_or(f64::NEG_INFINITY, |m| m[0].lo),
            None => f64::NEG_INFINITY,
        };
        Some((upper, lower))
    }

    /// Certified bounds on the face's greatest `sign * S[axis]`, refined until `upper - lower`
    /// is at most `target` or the work's limits.
    fn extreme(face: &Face, axis: usize, sign: f64, target: f64) -> Bound {
        let mut heap = BinaryHeap::new();
        let mut lower = f64::NEG_INFINITY;
        let mut lower_at = String::new();
        let mut settled = f64::NEG_INFINITY; // items at the depth limit
        let mut raise = |lower: &mut f64, lo: f64, at: &dyn Fn() -> String| {
            if lo > *lower {
                *lower = lo;
                lower_at = at();
            }
        };
        for (li, lp) in face.loops.iter().enumerate() {
            for (pi, piece) in lp.iter().enumerate() {
                let (t0, t1) = param_range(&piece.cn);
                let mut cuts: Vec<f64> = piece.cn.u.iter().copied().filter(|&x| x > t0 && x < t1).collect();
                cuts.dedup();
                let mut a = t0;
                for z in cuts.into_iter().chain([t1]) {
                    let (up, lo) = bnd(face, axis, sign, piece, (a, z));
                    raise(&mut lower, lo, &|| format!("loop {li} piece {pi} t {}", 0.5 * (a + z)));
                    heap.push(Item { upper: up, what: What::Bnd { lp: li, piece: pi, t: (a, z) }, depth: 0 });
                    a = z;
                }
            }
        }
        // the cells: the boundary's uv hull, within the knots
        let (mut ulo, mut vlo, mut uhi, mut vhi) = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        for lp in &face.loops {
            for piece in lp {
                let (t0, t1) = param_range(&piece.cn);
                if let Ok(d) = curve_derivs_over(&piece.cn, Ivl::new(t0, t1), 0, None) {
                    if let Some(p) = d.first() {
                        ulo = ulo.min(p.x.lo);
                        uhi = uhi.max(p.x.hi);
                        vlo = vlo.min(p.y.lo);
                        vhi = vhi.max(p.y.hi);
                    }
                }
            }
        }
        let [u0, v0, u1, v1] = face.range;
        let (ulo, uhi, vlo, vhi) = (ulo.max(u0), uhi.min(u1), vlo.max(v0), vhi.min(v1));
        if ulo < uhi && vlo < vhi {
            if let Some((up, lo)) = cell(face, axis, sign, (ulo, uhi), (vlo, vhi)) {
                raise(&mut lower, lo, &|| "root cell".to_string());
                heap.push(Item { upper: up, what: What::Cell { u: (ulo, uhi), v: (vlo, vhi) }, depth: 0 });
            }
        }
        let mut items = 0;
        while let Some(it) = heap.pop() {
            let top = it.upper.max(settled);
            if top <= lower + target || items >= MAX_ITEMS {
                heap.push(it);
                break;
            }
            if it.upper <= lower {
                continue;
            }
            if it.depth >= MAX_DEPTH {
                settled = settled.max(it.upper);
                continue;
            }
            items += 1;
            match it.what {
                What::Bnd { lp, piece, t } => {
                    let pc = &face.loops[lp][piece];
                    let m = 0.5 * (t.0 + t.1);
                    for half in [(t.0, m), (m, t.1)] {
                        let (up, lo) = bnd(face, axis, sign, pc, half);
                        raise(&mut lower, lo, &|| format!("loop {lp} piece {piece} t {}", 0.5 * (half.0 + half.1)));
                        heap.push(Item { upper: up, what: What::Bnd { lp, piece, t: half }, depth: it.depth + 1 });
                    }
                }
                What::Cell { u, v } => {
                    let (mu, mv) = (0.5 * (u.0 + u.1), 0.5 * (v.0 + v.1));
                    for cu in [(u.0, mu), (mu, u.1)] {
                        for cv in [(v.0, mv), (mv, v.1)] {
                            if let Some((up, lo)) = cell(face, axis, sign, cu, cv) {
                                raise(&mut lower, lo, &|| format!("inside cell u {cu:?} v {cv:?}"));
                                heap.push(Item { upper: up, what: What::Cell { u: cu, v: cv }, depth: it.depth + 1 });
                            }
                        }
                    }
                }
            }
        }
        let left = heap.peek().map_or(f64::NEG_INFINITY, |it| it.upper);
        Bound { lower, upper: left.max(settled).max(lower), items, lower_at }
    }

    /// The moved rows: unit, case.
    const ROWS: &[(&str, &str)] = &[
        ("query.face_p3", "gen0010"),
        ("query.face_p3", "gen0072"),
        ("query.face_p3", "gen0088"),
        ("query.face_p3", "gen0104"),
        ("query.face_p3", "gen0112"),
        ("query.face_p3", "gen0133"),
        ("query.face_p3", "gen0226"),
        ("query.face_p3", "gen0292"),
        ("query.face_p3", "gen0328"),
        ("query.face_p3", "gen0330"),
        ("query.face_p3", "gen0336"),
        ("query.face_p3", "gen0367"),
        ("query.face_p3", "gen0378"),
        ("query.face_p3", "gen0381"),
        ("query.face_p3", "gen0422"),
        ("query.face_p3", "gen0447"),
        ("query.face_p3", "gen0484"),
        ("query.face_p3", "tight_cone_f0"),
        ("query.face_p3", "tight_cone_f0_rebuild"),
        ("query.face_p1", "gen0054"),
        ("query.face_p1", "gen0055"),
        ("query.face_p1", "gen0215"),
        ("query.face_p1", "gen0644"),
        ("query.brep", "gen0128"),
        ("query.brep", "gen0549"),
        ("query.brep", "gen0571"),
    ];

    #[test]
    #[ignore = "re-runs the moved cases of three golden units and refines each face's bounds"]
    fn moved_tight_box_rows_hold_their_faces() {
        let ctx = crate::CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let filter = std::env::var("TIGHT_CERT_FILTER").ok();
        let reg = crate::registry();
        let mut report = Vec::new();
        let mut failures = Vec::new();
        for &(unit_id, name) in ROWS {
            if filter.as_deref().is_some_and(|f| !name.contains(f)) {
                continue;
            }
            let unit = reg.iter().find(|u| u.id() == unit_id).expect("unit");
            let cases = unit.cases(&ctx).expect("cases");
            let Some(case) = cases.iter().find(|c| c.name == name) else {
                report.push(format!("{unit_id} {name}: no such case"));
                continue;
            };
            crate::dump::capture();
            let _ = unit.run_rust(case);
            let captured = crate::dump::take();
            // each body boxed once is enough: the steps box the same body
            for cap in captured.iter().take(1) {
                let Ok(body) = &cap.brep else {
                    report.push(format!("{unit_id} {name}: no copy"));
                    continue;
                };
                let k = crate::runner_kernel();
                let faces = solidcore::topology::brep::p1::faces(body, None).expect("faces");
                for f in faces {
                    let mut b = body.clone_standing_for_self().expect("copy");
                    let Ok(bx) = qf3::calculate_tight_bounding_box(&k, &b, f) else {
                        continue;
                    };
                    let face = match face_of(&k, &mut b, f) {
                        Ok(x) => x,
                        Err(e) => {
                            report.push(format!("{unit_id} {name} face {}: {e}", f.index()));
                            continue;
                        }
                    };
                    let (lo, hi) = (bx.min(), bx.max());
                    for axis in 0..3 {
                        for sign in [1.0, -1.0] {
                            let side = sign * if sign > 0.0 { hi.component(axis) } else { lo.component(axis) };
                            let scale = lo.component(axis).abs().max(hi.component(axis).abs()).max(1e-300);
                            let bd = extreme(&face, axis, sign, 1e-13 * scale);
                            let short = bd.upper - side;
                            let tag = format!("{name} face {} {}{}", f.index(), if sign > 0.0 { "+" } else { "-" }, ["x", "y", "z"][axis]);
                            // every side holds its face to within 1e-9 of its size (the
                            // refinement's budget; most to 1e-13)
                            if short > 1e-9 * scale {
                                failures.push(format!(
                                    "{tag}: short of its face's certified bound by {short:e}"
                                ));
                            }
                            report.push(format!(
                                "{unit_id} {name} face {} {}{}: side {:.17e} certified [{:.17e}, {:.17e}] side short of upper by {:.3e} ({:.2e} rel) items {}",
                                f.index(),
                                if sign > 0.0 { "+" } else { "-" },
                                ["x", "y", "z"][axis],
                                sign * side,
                                bd.lower,
                                bd.upper,
                                short,
                                short / scale,
                                bd.items
                            ));
                            report.push(format!("    lower at {}", bd.lower_at));
                            if short > 1e-6 * scale {
                                for (li, lp) in face.loops.iter().enumerate() {
                                    for (pi, pc) in lp.iter().enumerate() {
                                        let (t0, t1) = param_range(&pc.cn);
                                        report.push(format!(
                                            "    loop {li} piece {pi}: degree {} points {} dir {} from {:?} to {:?}",
                                            pc.cn.p, pc.cn.n, pc.dir, point_at(&pc.cn, t0), point_at(&pc.cn, t1)
                                        ));
                                    }
                                }
                                let mut kk = crate::runner_kernel();
                                let mut bb = body.clone_standing_for_self().expect("copy");
                                let check = solidcore::topology::brep::validation::check_brep(
                                    &mut kk,
                                    &mut bb,
                                    solidcore::topology::brep::validation::CheckLevel::Level2,
                                );
                                report.push(format!(
                                    "    check_brep Level 2: {:?}",
                                    check.map(|c| c.lines())
                                ));
                            }
                        }
                    }
                }
            }
        }
        for l in &report {
            eprintln!("{l}");
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generated cases reach every operation, and a good share of them runs to a
    /// result (neither rejected nor deferred).
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
            let r = rust(&i).unwrap();
            let key = if let Some(d) = &r.deferred {
                format!("{op} {kind} deferred {} {}", d.unit, d.symbol)
            } else if let Some(e) = &r.error {
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

    /// #18342: on every plane face of the adversarial bisector fixtures, the bisector, the
    /// uv line from u = -1234567 to 1234567 at the face's middle v, enters and leaves the
    /// face at `u_min` and `u_max` of its domain: its boundaries are the closed form
    /// 0.5 + u / 2469134, to 1 ulp. The 0.001-wide faces (`adv_bis_plane{0..3}_2`) leave
    /// at 0.5 + 0.001 / 2469134 = 0.5000000004050003; before the fix that crossing, 4.05e-10
    /// of parameter from the entry at 0.5 but 1e-3 away in uv, snapped onto it.
    #[test]
    fn plane_bisector_boundaries_are_the_closed_form() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/query.face_p3/review_adversarial.json");
        let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut seen = 0;
        for c in fixture["cases"].as_array().unwrap() {
            let name = c["name"].as_str().unwrap();
            if !name.starts_with("adv_bis_plane") {
                continue;
            }
            let dom = &c["inputs"]["brep"]["dom"];
            let (u_min, u_max) = (dom[0].as_f64().unwrap(), dom[2].as_f64().unwrap());
            let i: Inputs = serde_json::from_value(c["inputs"].clone()).unwrap();
            let out = rust(&i).unwrap();
            assert_eq!(out.error, None, "{name}");
            let b = out.values.get("interval_bounds").unwrap();
            let near = |got: f64, want: f64| (got - want).abs() <= want.abs() * f64::EPSILON;
            let (enter, leave) = (0.5 + u_min / 2469134.0, 0.5 + u_max / 2469134.0);
            assert_eq!(b.len(), 6, "{name} {b:?}");
            assert_eq!((b[0], b[5]), (0.0, 1.0), "{name} {b:?}");
            assert!(b[1] == b[2] && near(b[1], enter), "{name} {b:?} enter {enter}");
            assert!(b[3] == b[4] && near(b[3], leave), "{name} {b:?} leave {leave}");
            seen += 1;
        }
        assert_eq!(seen, 16);
    }
}

