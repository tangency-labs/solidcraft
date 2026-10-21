//! Runner for unit `topology.face_p4`. C++: `solidcore_ffi::units::topology_face_p4`.
//! Rust: `solidcore::topology::face::p4`.
//!
//! Every op runs on a B-rep of unit `topology.vertexuse` (upstream `.smb` files, faces of
//! B-spline surfaces or real `SmPlane`s): the C++ side builds a real `SmBrep`, gives its
//! faces and edges the plain B-spline copies of analytic geometry (what the Rust import
//! rebuilds, keeping the owners) and runs the `SmFace` member on the selected face; the
//! Rust side imports the graph that unit's shim exports and runs the port. Both sides
//! produce one flat record (`out`, laid out as in the ffi module): traversal indices,
//! flags, statuses, line/face solutions and seam classification point classes.
//!
//! **Selectors.** The face is taken modulo the face count, the loop modulo the loop
//! count (`-1` stays "no target array"), a `Connected` target modulo its kind's count.
//! `Sector` picks the `edge`-th edge of the face (modulo its edge count) and the vertex
//! at the end it starts from (`at_start`), so the curve leaves a face vertex; the
//! selectors are resolved on the Rust import, whose traversal order is the export's.
//!
//! **Not compared.** Attribute filters (`pOptAttributeId`): both sides pass none. The
//! solutions' objects of `Line` (the surface pointer upstream, a handle here).
//!
//! Deferred: `Line` ranges (`sm_FaceClassifyUVLine`, `query.face_p3`), `Sector` past
//! its single-sector shortcuts (`SmFace::SectorClassifyCurveOn`, `query.face_p3`) and
//! `Seam` of a periodic face (`SmFaceProps::SetProps_Stage2`'s seam call,
//! `heal.face_props`, and `SmCurveClassification::GetLoopCrossings`,
//! `intersect.curve_class_p4`).

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject, SolutionType};
use solidcore::heal::face_props::LoopObject;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{ClassGeometry, ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec3};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p2::CurveOnClassifyObjects;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::face::p4::{self as fp4, FindVertexSectorArgs, HasSeamProblemOptions};
use solidcore::topology::{
    ArenaId, Brep, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId,
    VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_face_p4::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse as fixture;

use super::topology_vertexuse::{self as shared, BrepIn};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// The `.smb` files the systematic cases run on (those of unit `topology.vertexuse`).
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

/// Which upward-use getter a `List` op calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ListKind {
    Edges,
    Vertices,
    Loopuses,
    Vertexuses,
    Edgeuses,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Brep,
    List {
        kind: ListKind,
        tgt_only: bool,
        /// `-1`: no target array.
        #[serde(rename = "loop")]
        loop_: i64,
    },
    Connected {
        /// Export slot `0..=9`, `-1`: NULL.
        kind: i64,
        index: i64,
    },
    Line {
        p: [f64; 3],
        v: [f64; 3],
        #[serde(default)]
        ivl: Option<[f64; 2]>,
        fire: bool,
        /// `None`: the face's tolerance.
        #[serde(default)]
        tol: Option<f64>,
    },
    Sector {
        edge: i64,
        #[serde(default)]
        vertex: i64,
        use_uv: bool,
        at_start: bool,
        opposite: bool,
        /// `None`: the face's tolerance.
        #[serde(default)]
        tol: Option<f64>,
        validate: bool,
        uv_ok: bool,
    },
    Seam {
        outer: bool,
        /// `-1`: unknown, `0`/`1`: known.
        periodic_u: i64,
        periodic_v: i64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    face: i64,
    #[serde(flatten)]
    op: OpIn,
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
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

/// The op code and its arguments after the face index (selectors resolved).
fn op_args(op: &OpIn) -> (Op, Vec<f64>) {
    match op {
        OpIn::Brep => (Op::Brep, vec![]),
        OpIn::List {
            kind,
            tgt_only,
            loop_,
        } => {
            let op = match kind {
                ListKind::Edges => Op::Edges,
                ListKind::Vertices => Op::Vertices,
                ListKind::Loopuses => Op::UpLoopuses,
                ListKind::Vertexuses => Op::UpVertexuses,
                ListKind::Edgeuses => Op::UpEdgeuses,
            };
            (op, vec![flag(*tgt_only), *loop_ as f64])
        }
        OpIn::Connected { kind, index } => (Op::Connected, vec![*kind as f64, *index as f64]),
        OpIn::Line {
            p,
            v,
            ivl,
            fire,
            tol,
        } => {
            let mut a: Vec<f64> = p.iter().chain(v.iter()).copied().collect();
            let [t0, t1] = ivl.unwrap_or([0.0, 0.0]);
            a.extend([
                flag(ivl.is_some()),
                t0,
                t1,
                flag(*fire),
                flag(tol.is_none()),
                tol.unwrap_or(0.0),
            ]);
            (Op::Line, a)
        }
        OpIn::Sector {
            edge,
            vertex,
            use_uv,
            at_start,
            opposite,
            tol,
            validate,
            uv_ok,
        } => (
            Op::Sector,
            vec![
                *vertex as f64,
                *edge as f64,
                flag(*use_uv),
                flag(*at_start),
                flag(*opposite),
                tol.unwrap_or(0.0),
                flag(*validate),
                flag(*uv_ok),
                flag(tol.is_none()),
            ],
        ),
        OpIn::Seam {
            outer,
            periodic_u,
            periodic_v,
        } => (
            Op::Seam,
            vec![flag(*outer), *periodic_u as f64, *periodic_v as f64],
        ),
    }
}

/// A cached export with its key (the `.smb` path and the block's bits).
type CachedExport = (String, Vec<u64>, Rc<Vec<f64>>);

thread_local! {
    /// The last few exports (both sides of a case export the same B-rep).
    static EXPORTS: RefCell<Vec<CachedExport>> = const { RefCell::new(Vec::new()) };
}

/// The export of a B-rep.
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

/// A resolved case (inputs and the B-rep export), or the output of a case that ends
/// before the op.
type Resolved = Result<(Inputs, Rc<Vec<f64>>), Output>;

/// The edges of a face in the import (`GetEdges`), for the `Sector` selectors.
fn face_edges(ex: &[f64], f: FaceId) -> Result<Vec<EdgeId>, HarnessError> {
    let (b, _) = shared::import(ex)?;
    fp4::edges(&b, f, None, false, None).map_err(|e| harness(format!("rust import: {e}")))
}

fn resolve(i: &Inputs) -> Result<Resolved, HarnessError> {
    let ex = export_of(&i.brep)?;
    let Some(c) = fixture::counts(&ex) else {
        return Ok(Err(Output::new().values("out", ex.to_vec())));
    };
    let [_, _, _, n_f, _, n_l, ..] = c;
    if n_f == 0 {
        return Ok(Err(Output::err("no_face")));
    }
    let mut r = i.clone();
    r.face = shared::sel(i.face.max(0), n_f);
    match &mut r.op {
        OpIn::List { loop_, .. } => {
            *loop_ = if *loop_ < 0 || n_l == 0 {
                -1
            } else {
                shared::sel(*loop_, n_l)
            };
        }
        OpIn::Connected { kind, index } => {
            if *kind >= 0 {
                *kind = (*kind).rem_euclid(10);
                let n = c.get(*kind as usize).copied().unwrap_or(0);
                if n == 0 {
                    *kind = -1;
                    *index = 0;
                } else {
                    *index = shared::sel((*index).max(0), n);
                }
            } else {
                *index = 0;
            }
        }
        OpIn::Sector {
            edge,
            vertex,
            at_start,
            ..
        } => {
            let f = FaceId::from_index(r.face as u32);
            let edges = face_edges(&ex, f)?;
            if edges.is_empty() {
                return Ok(Err(Output::err("no_face_edge")));
            }
            let e = edges
                .get(shared::sel((*edge).max(0), edges.len()) as usize)
                .copied()
                .ok_or_else(|| harness("edge selector"))?;
            let (b, _) = shared::import(&ex)?;
            let [v0, v1] = b.edge(e).map_err(|e| harness(e.to_string()))?.vertices;
            *edge = i64::from(e.index());
            *vertex = i64::from(if *at_start { v0 } else { v1 }.index());
        }
        _ => {}
    }
    Ok(Ok((r, ex)))
}

fn input_of(i: &Inputs) -> Result<(Op, String, Vec<f64>), HarnessError> {
    let (op, args) = op_args(&i.op);
    let mut v = vec![i.face as f64];
    v.extend(args);
    v.extend(brep_block(&i.brep)?);
    Ok((op, shared::path_of(&i.brep)?, v))
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (r, _) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let (op, path, input) = input_of(&r)?;
    let out = ffi::eval(op, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
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

/// The record of a list op: `n idx*n`, or `-2` for the null dereference upstream.
fn put_list<I: ArenaId>(o: &mut Vec<f64>, r: Result<Vec<I>, SolidError>) -> Result<(), Fail> {
    match r {
        Ok(v) => {
            o.push(v.len() as f64);
            o.extend(v.iter().map(|x| f64::from(x.index())));
        }
        Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
        Err(e) => return Err(fail(&e)),
    }
    Ok(())
}

/// The `Line` op's solution object: the face surface's natural domain.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SurfaceHandle(Extent2d);

impl SolutionObject for SurfaceHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        Some(self.0)
    }
}

/// The classification handles of `Seam`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum H {
    Face(FaceId),
    Vertex(VertexId),
    Edge(EdgeId),
    Vertexuse(VertexuseId),
    Edgeuse(EdgeuseId),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
    Surface(FaceId),
    Loop(LoopId),
    Faceuse(FaceuseId),
    Loopuse(LoopuseId),
    Temp(usize),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Face(_) => ClassObjectKind::Face,
            Self::Vertex(_) => ClassObjectKind::Vertex,
            Self::Edge(_) => ClassObjectKind::Edge,
            Self::Vertexuse(_) => ClassObjectKind::Vertexuse,
            Self::Edgeuse(_) => ClassObjectKind::Edgeuse,
            Self::EdgeCurve(_) | Self::Trim(_) | Self::Temp(_) => ClassObjectKind::Curve,
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Loop(_) => ClassObjectKind::Loop,
            Self::Faceuse(_) => ClassObjectKind::Faceuse,
            Self::Loopuse(_) => ClassObjectKind::Loopuse,
        }
    }
    fn temporary(index: usize, kind: ClassObjectKind) -> Option<Self> {
        (kind == ClassObjectKind::Curve).then_some(Self::Temp(index))
    }
}

impl LoopObject for H {
    fn from_loop(l: LoopId) -> Self {
        Self::Loop(l)
    }
}

/// The B-rep behind the `Seam` handles.
struct Objects<'a> {
    k: &'a Kernel,
    brep: &'a Brep,
    /// A copy of `brep` for the vertex/face gaps, made at the first lookup: upstream's
    /// `const` `GetVertexFaceGap` fills the vertex use's lazy cache.
    gaps: RefCell<Option<Brep>>,
    /// The first error of a gap lookup (its `Unported` defers the case).
    gap_error: RefCell<Option<SolidError>>,
}

impl<'a> Objects<'a> {
    fn new(k: &'a Kernel, brep: &'a Brep) -> Self {
        Self {
            k,
            brep,
            gaps: RefCell::new(None),
            gap_error: RefCell::new(None),
        }
    }

    /// The face whose surface is `surface`.
    fn face_of_surface(&self, surface: &Surface) -> Option<FaceId> {
        self.brep
            .faces()
            .iter()
            .find(|(_, r)| r.surface.as_ref().is_some_and(|s| std::ptr::eq(s, surface)))
            .map(|(f, _)| f)
    }

    /// `pVertex->GetVertexuseOfFace(pFace)->GetVertexFaceGap()`: its `GetLength()`, or
    /// `0.0` without a gap.
    fn gap(&self, v: VertexId, f: FaceId) -> Result<Option<f64>, SolidError> {
        let Some(vu) = solidcore::topology::vertex::vertexuse_of_face(self.brep, v, Some(f))?
        else {
            return Ok(None);
        };
        let mut gaps = self.gaps.borrow_mut();
        let b = gaps.get_or_insert_with(|| self.brep.clone());
        let gap = solidcore::topology::vertexuse::vertex_face_gap(self.k, b, vu, false)?;
        Ok(Some(gap.map_or(0.0, |g| g.gap_3d)))
    }
}

impl CurveOnClassifyObjects<H> for Objects<'_> {
    fn face(&self, f: FaceId) -> H {
        H::Face(f)
    }
    fn vertex(&self, v: VertexId) -> H {
        H::Vertex(v)
    }
    fn edge(&self, e: EdgeId) -> H {
        H::Edge(e)
    }
    fn vertexuse(&self, vu: VertexuseId) -> H {
        H::Vertexuse(vu)
    }
    fn edgeuse(&self, eu: EdgeuseId) -> H {
        H::Edgeuse(eu)
    }
    fn edge_curve(&self, e: EdgeId) -> H {
        H::EdgeCurve(e)
    }
    fn trim_curve(&self, eu: EdgeuseId) -> H {
        H::Trim(eu)
    }
    fn face_surface(&self, f: FaceId) -> Option<H> {
        self.brep
            .face(f)
            .ok()
            .and_then(|r| r.surface.as_ref())
            .map(|_| H::Surface(f))
    }
    fn loop_id(&self, tgt_loop: H) -> Option<LoopId> {
        match tgt_loop {
            H::Loop(l) => Some(l),
            _ => None,
        }
    }
}

impl ClassGeometry<H> for Objects<'_> {
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        match o {
            H::Vertex(v) => self.brep.vertex(v).ok().map(|v| v.point),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        match o {
            H::Vertex(v) => self.brep.vertex(v).ok().map(|v| v.tolerance),
            H::Face(f) => self.brep.face(f).ok().map(|f| f.tolerance),
            H::Edge(e) => self.brep.edge(e).ok().map(|e| e.tolerance),
            _ => None,
        }
    }
    fn topology_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        let e = match o {
            H::Face(f) => EntityRef::Face(f),
            H::Vertex(v) => EntityRef::Vertex(v),
            H::Edge(e) => EntityRef::Edge(e),
            H::Vertexuse(vu) => EntityRef::Vertexuse(vu),
            H::Edgeuse(eu) => EntityRef::Edgeuse(eu),
            H::Loop(l) => EntityRef::Loop(l),
            H::Faceuse(fu) => EntityRef::Faceuse(fu),
            H::Loopuse(lu) => EntityRef::Loopuse(lu),
            H::EdgeCurve(_) | H::Trim(_) | H::Surface(_) | H::Temp(_) => return None,
        };
        Some((self.brep, e))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<H> {
        Some(match e {
            EntityRef::Face(f) => H::Face(f),
            EntityRef::Vertex(v) => H::Vertex(v),
            EntityRef::Edge(e) => H::Edge(e),
            EntityRef::Vertexuse(vu) => H::Vertexuse(vu),
            EntityRef::Edgeuse(eu) => H::Edgeuse(eu),
            EntityRef::Loop(l) => H::Loop(l),
            EntityRef::Faceuse(fu) => H::Faceuse(fu),
            EntityRef::Loopuse(lu) => H::Loopuse(lu),
            _ => return None,
        })
    }
    fn vertex_face_gap(&self, vertex: H, surface: &Surface, face: Option<H>) -> Option<f64> {
        let H::Vertex(v) = vertex else {
            return None;
        };
        let f = match face {
            Some(H::Face(f)) => f,
            Some(_) => return None,
            None => self.face_of_surface(surface)?,
        };
        match self.gap(v, f) {
            Ok(g) => g,
            Err(e) => {
                self.gap_error.borrow_mut().get_or_insert(e);
                None
            }
        }
    }
    fn curve(&self, o: H) -> Option<&Curve> {
        match o {
            H::EdgeCurve(e) => self.brep.edge(e).ok().and_then(|e| e.curve.as_ref()),
            H::Trim(eu) => eu1::uv_trim_curve(self.brep, eu).ok().and_then(|(c, _)| c),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match o {
            H::Surface(f) => self.brep.face(f).ok().and_then(|f| f.surface.as_ref()),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, o: H) -> Option<Point3> {
        match o {
            H::Vertex(v) => self.brep.vertex(v).ok().map(|v| v.point),
            _ => None,
        }
    }
}

fn put_class(o: &mut Vec<f64>, c: &CurveClassification<H>) {
    o.push(flag(c.tgt_loops_only()));
    o.push(c.size() as f64);
    for i in c.intervals() {
        o.push(f64::from(i.start.point_class() as i32));
        o.push(f64::from(i.mid.point_class() as i32));
        o.push(f64::from(i.end.point_class() as i32));
    }
}

/// The entity of an export slot and index.
fn target(kind: i64, index: i64) -> Option<EntityRef> {
    let i = index as u32;
    Some(match kind {
        0 => EntityRef::Region(RegionId::from_index(i)),
        1 => EntityRef::Shell(ShellId::from_index(i)),
        2 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        3 => EntityRef::Face(FaceId::from_index(i)),
        4 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        5 => EntityRef::Loop(LoopId::from_index(i)),
        6 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        7 => EntityRef::Edge(EdgeId::from_index(i)),
        8 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        9 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

/// Whether `Sector`'s upstream path would dereference null (the shim's guard).
fn sector_null_deref(b: &Brep, f: FaceId, v: VertexId, e: EdgeId) -> Result<bool, SolidError> {
    if b.edge(e)?.curve.is_none() || b.face(f)?.surface.is_none() {
        return Ok(true);
    }
    let up = b.face(f)?.faceuse;
    for vu in solidcore::topology::vertex::vertexuses(b, v)? {
        if solidcore::topology::vertexuse::faceuse(b, vu)? != up
            || solidcore::topology::vertexuse::is_loop_vertexuse(b, vu)?
        {
            continue;
        }
        let Some(eu) = solidcore::topology::vertexuse::edgeuse(b, vu)? else {
            return Ok(true);
        };
        if b.edgeuse(eu)?.edge.is_none() {
            return Ok(true);
        }
        let Some(cw) = b.edgeuse(eu)?.cw else {
            return Ok(true);
        };
        if b.edgeuse(cw)?.edge.is_none() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_lines)]
fn rust_record(i: &Inputs, ex: &[f64]) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let (mut b, _) = shared::import(ex)?;
    let f = FaceId::from_index(i.face as u32);
    let mut o = vec![0.0];
    let loops_of = |l: i64| (l >= 0).then(|| vec![LoopId::from_index(l as u32)]);
    match &i.op {
        OpIn::Brep => o.push(flag(fp4::brep(&b, f).map_err(|e| fail(&e))?.is_some())),
        OpIn::List {
            kind,
            tgt_only,
            loop_,
        } => {
            let tgts = loops_of(*loop_);
            let t = tgts.as_deref();
            match kind {
                ListKind::Edges => put_list(&mut o, fp4::edges(&b, f, None, *tgt_only, t))?,
                ListKind::Vertices => put_list(&mut o, fp4::vertices(&b, f, None, *tgt_only, t))?,
                ListKind::Loopuses => put_list(&mut o, fp4::upward_loopuses(&b, f, *tgt_only, t))?,
                ListKind::Vertexuses => {
                    put_list(&mut o, fp4::upward_vertexuses(&b, f, *tgt_only, t))?;
                }
                ListKind::Edgeuses => put_list(&mut o, fp4::upward_edgeuses(&b, f, *tgt_only, t))?,
            }
        }
        OpIn::Connected { kind, index } => match fp4::is_connected_to(&b, f, target(*kind, *index))
        {
            Ok(c) => o.push(flag(c)),
            Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
            Err(e) => return Err(fail(&e)),
        },
        OpIn::Line {
            p,
            v,
            ivl,
            fire,
            tol,
        } => {
            let face = b.face(f).map_err(|e| fail(&e))?;
            let Some(surface) = face.surface.as_ref() else {
                o.push(NULL_DEREF);
                return Ok(o);
            };
            let handle = SurfaceHandle(surface.natural_uv_domain());
            let uv_domain = face.uv_domain;
            let tol = tol.unwrap_or(face.tolerance);
            let interval = ivl.map(|[t0, t1]| Extent1d::from_min_max(t0, t1));
            let [px, py, pz] = *p;
            let [vx, vy, vz] = *v;
            match fp4::global_line_intersect(
                &k,
                &mut b,
                f,
                handle,
                &uv_domain,
                Point3::new(px, py, pz),
                Vec3::new(vx, vy, vz),
                interval.as_ref(),
                *fire,
                tol,
            ) {
                Ok(sols) => {
                    o.extend([0.0, sols.len() as f64]);
                    for s in sols.as_slice() {
                        o.push(f64::from(s.solution_type as i32));
                        o.extend(s.start.parameters.iter().take(3));
                        if s.solution_type == SolutionType::RangeOfValues {
                            o.extend(s.end.parameters.iter().take(3));
                        } else {
                            o.extend([0.0; 3]);
                        }
                    }
                }
                Err(SolidError::Upstream { code, .. }) => o.extend([f64::from(code), 0.0]),
                Err(e) => return Err(fail(&e)),
            }
        }
        OpIn::Sector {
            edge,
            vertex,
            use_uv,
            at_start,
            opposite,
            tol,
            validate,
            uv_ok,
        } => {
            let e = EdgeId::from_index(*edge as u32);
            let v = VertexId::from_index(*vertex as u32);
            if sector_null_deref(&b, f, v, e).map_err(|e| fail(&e))? {
                o.push(NULL_DEREF);
                return Ok(o);
            }
            let edge_rec = b.edge(e).map_err(|e| fail(&e))?;
            let curve = edge_rec
                .curve
                .clone()
                .ok_or_else(|| harness("edge without curve"))?;
            let interval = edge_rec.interval;
            let primary = edge_rec.edgeuses.first().copied();
            let uv_curve = match primary {
                Some(eu) if *use_uv => eu1::uv_trim_curve(&b, eu).map_err(|e| fail(&e))?.0.cloned(),
                _ => None,
            };
            let (at, end) = if *at_start {
                (interval.min(), interval.max())
            } else {
                (interval.max(), interval.min())
            };
            let face_tol = b.face(f).map_err(|e| fail(&e))?.tolerance;
            let args = FindVertexSectorArgs {
                vertex: v,
                curve: &curve,
                uv_curve: uv_curve.as_ref(),
                param_at_vertex: at,
                param_ivl_end: end,
                orientation: if *opposite {
                    OrientType::Opposite
                } else {
                    OrientType::Same
                },
                zone_tol_3d: tol.unwrap_or(face_tol),
                validate_sector: *validate,
                uv_space_okay: *uv_ok,
            };
            match fp4::find_vertex_sector(&k, &mut b, f, args) {
                Ok(s) => o.extend([
                    0.0,
                    s.vertexuse.map_or(-1.0, |vu| f64::from(vu.index())),
                    flag(s.possible_problem),
                    f64::from(s.problem as i32),
                ]),
                Err(SolidError::Upstream { code, .. }) => {
                    o.extend([f64::from(code), -1.0, 0.0, 0.0]);
                }
                Err(e) => return Err(fail(&e)),
            }
        }
        OpIn::Seam {
            outer,
            periodic_u,
            periodic_v,
        } => {
            let known = |p: i64| (p >= 0).then_some(p == 1);
            let opts = HasSeamProblemOptions {
                outer_loop_only: *outer,
                periodic_u: known(*periodic_u),
                periodic_v: known(*periodic_v),
                face_props: None,
            };
            let objects = Objects::new(&k, &b);
            let s = fp4::has_seam_problem(&k, &b, f, &objects, opts).map_err(|e| {
                // a failed gap lookup surfaces as part 5's unresolved-gap error
                match objects.gap_error.borrow_mut().take() {
                    Some(g) => fail(&g),
                    None => fail(&e),
                }
            })?;
            o.extend([
                flag(s.has_problem),
                f64::from(s.problem.crossed as i32),
                f64::from(s.problem.missing as i32),
                f64::from(s.problem.near_miss as i32),
            ]);
            put_class(&mut o, &s.class_u);
            put_class(&mut o, &s.class_v);
        }
    }
    Ok(o)
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let (r, ex) = match resolve(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    match rust_record(&r, &ex) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- cases ------------------------------------------------------------------------------

const LISTS: [ListKind; 5] = [
    ListKind::Edges,
    ListKind::Vertices,
    ListKind::Loopuses,
    ListKind::Vertexuses,
    ListKind::Edgeuses,
];

/// Every op variant for one face selector `s` (`v` varies the flags and selectors).
fn ops_for(s: i64, v: usize) -> Vec<OpIn> {
    let bit = |k: usize| (v >> k) & 1 == 1;
    let mut ops = vec![OpIn::Brep];
    for (j, kind) in LISTS.into_iter().enumerate() {
        ops.push(OpIn::List {
            kind,
            tgt_only: bit(j),
            loop_: if bit(j + 1) { s + j as i64 } else { -1 },
        });
    }
    for kind in -1..10 {
        ops.push(OpIn::Connected {
            kind,
            index: s + v as i64 % 7,
        });
    }
    // lines through the face's neighbourhood: along the axes and a skew direction
    let c = (v % 5) as f64 - 2.0;
    for (j, dir) in [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [0.3, -0.5, 0.8],
    ]
    .into_iter()
    .enumerate()
    {
        ops.push(OpIn::Line {
            p: [c, -c * 0.5, c * 0.25],
            v: dir,
            ivl: bit(j).then_some([-50.0, 50.0]),
            fire: bit(j + 1),
            tol: None,
        });
    }
    for j in 0..4usize {
        ops.push(OpIn::Sector {
            edge: s + j as i64,
            vertex: 0,
            use_uv: bit(j),
            at_start: bit(j + 1),
            opposite: bit(j + 2),
            tol: None,
            validate: bit(j + 3),
            uv_ok: !bit(j + 4),
        });
    }
    ops.push(OpIn::Seam {
        outer: bit(0),
        periodic_u: (v % 3) as i64 - 1,
        periodic_v: ((v / 3) % 3) as i64 - 1,
    });
    ops
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    // only B-reps with faces: a wire or the wire network has no face to run on
    let brep = loop {
        let b = shared::gen_brep(rng);
        if matches!(b, BrepIn::File { .. } | BrepIn::Face { .. }) {
            break b;
        }
    };
    let s = rng.below(64) as i64;
    let mut ops = ops_for(s, rng.below(1 << 10) as usize);
    // a random line through the box around the origin
    ops.push(OpIn::Line {
        p: rng.point3(-20.0, 20.0),
        v: rng.point3(-1.0, 1.0),
        ivl: None,
        fire: rng.below(2) == 1,
        tol: None,
    });
    let op = rng.choose(&ops).cloned().unwrap_or(OpIn::Brep);
    Inputs { brep, face: s, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.face_p4"
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

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = default_cases(self, ctx)?;
        // every op on the first faces of every upstream file, with and without rebuilt
        // UV trim curves
        for file in FILES {
            for rebuild in [false, true] {
                for s in 0..6i64 {
                    for (j, op) in ops_for(s, s as usize * 7 + usize::from(rebuild) * 3)
                        .into_iter()
                        .enumerate()
                    {
                        let inputs = Inputs {
                            brep: BrepIn::File {
                                file: file.to_string(),
                                rebuild,
                            },
                            face: s,
                            op,
                        };
                        v.push(Case {
                            unit: self.id().to_string(),
                            name: format!("{file}_{}_f{s}_op{j}", u8::from(rebuild)),
                            source: crate::CaseSource::Fixture {
                                path: "built-in: systematic cases".to_string(),
                                index: v.len(),
                                upstream_ref: Some(
                                    "tests/prog_test (the pt_TestFiles B-reps)".to_string(),
                                ),
                            },
                            inputs: serde_json::to_value(inputs)
                                .map_err(|e| harness(e.to_string()))?,
                        });
                    }
                }
            }
        }
        // GlobalLineIntersect's by-value SmZoneTol3d: negative and SM_USE_DEFAULT
        // tolerances become fabs / the system zone tolerance (review of PR #239: a
        // negative tolerance rejected every planar hit)
        for file in FILES {
            for s in 0..3i64 {
                for p in [[0.2, 0.1, 1.0], [0.0, 0.0, 0.0]] {
                    for dir in [[0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]] {
                        for tol in [-1e-6, -1e-2, solidcore::geom::tol::USE_DEFAULT] {
                            let inputs = Inputs {
                                brep: BrepIn::File {
                                    file: file.to_string(),
                                    rebuild: false,
                                },
                                face: s,
                                op: OpIn::Line {
                                    p,
                                    v: dir,
                                    ivl: None,
                                    fire: false,
                                    tol: Some(tol),
                                },
                            };
                            v.push(Case {
                                unit: self.id().to_string(),
                                name: format!("{file}_f{s}_tol{}", v.len()),
                                source: crate::CaseSource::Fixture {
                                    path: "built-in: zone tolerance cases".to_string(),
                                    index: v.len(),
                                    upstream_ref: None,
                                },
                                inputs: serde_json::to_value(inputs)
                                    .map_err(|e| harness(e.to_string()))?,
                            });
                        }
                    }
                }
            }
        }
        Ok(v)
    }
}
