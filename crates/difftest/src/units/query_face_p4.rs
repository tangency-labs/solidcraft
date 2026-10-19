//! Runner for unit `query.face_p4`. C++: `solidcore_ffi::units::query_face_p4`. Rust:
//! `solidcore::query::face::p4`.
//!
//! **B-reps.** The C++ side builds a real `SmBrep` per call: an upstream `.smb` file, a
//! face over a B-spline surface or `SmPlane` (`CreateFaceFromSurface`), a face of 2D line
//! and circle UV trim curves and single vertex loops over a real `SmCylinder`,
//! `SmSphere`, `SmTorus` or `SmCone` or a B-spline (`MakeFaceWithCurves`, as `prog_test`
//! `my_test_tsurf_cyl_creation_fast`; lines spanning a periodic direction make the
//! no-area loops of faces without seam edge: cylinder circles, sphere caps and bands,
//! torus bands and diagonals), or the natural face of an analytic surface
//! (`CreateFaceInRegionFromSurface`, as `my_test_closed_sphere_properties`). There is no
//! Rust B-rep builder, so the Rust side imports the graph the shim exports (unit
//! `topology.face_p2`'s layout and importer).
//!
//! **Face props.** `ClassifyLoops` reads the face props of its only caller,
//! `SmFaceProps::SetProps_Stage3`. With `props` `1` the shim builds them as that caller
//! does (`SetProps_Stage2`, `ReSet(SM_PROPSTAGE_3)`, the loop props) and exports the
//! values `ClassifyLoops` reads (stage, closed flags, poles, the loops' seam crossing
//! and winding counts, the seam classifications' end point classes and vertices, the
//! seam done-touch lists); both sides then run `ClassifyLoops` on those same props (the
//! Rust side imports them; the seam classifications' non-vertex point classifications
//! carry no object, which `ClassifyLoops` never reads). The target loops are exported
//! too. With `props` `0` the props are fresh and `ClassifyLoops` computes them itself
//! (`SetProps_Stage2`; the Rust side runs the port of unit `heal.face_props`).
//!
//! **Deferred.** A target loop that crosses no seam the face props classified needs
//! `sms_FindAndClassifyLoopBisector` (`query.face_p3`) and `GetLoopCrossings`
//! (`intersect.curve_class_p4`); fresh props of a closed surface need
//! `SmFace::HasSeamProblem` (`topology.face_p4`).
//!
//! **Rejected** (both sides report `rejected`): the inputs the FFI wrapper documents,
//! where `ClassifyLoops` would read outside its arrays.
//!
//! **Outputs.** The status (`0` or the `SmStatus` code), the target loops after the call,
//! `reClassifiable`, the props' not-yet type, one record per target loop (loop, loop use
//! type, orientation, degenerate and area flags, containment), the start and stop types,
//! the ordered no-area indices, the loop tree in pre-order and the size of the scratch
//! classification after the call. The out-params start from the same values on both
//! sides, so an early return compares what it leaves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::error::{EntityRef, SolidError, SM_ERR};
use solidcore::heal::face_props::{
    ClassifyLoopsOut, FaceProps, HealNotYetFaceType, LoopObject, LoopProps, Looptree, PropStage,
    Tri,
};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::intersect::curve_class::p1::{CrvDirOnFaceType, CurveClassification, TouchData};
use solidcore::intersect::point_class::PointClassType;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::query::face::p4 as qf4;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId, LoopId, VertexId};
use solidcore_ffi::units::query_face_p4::{self as ffi, Op};

use super::topology_edgeuse_p1::{domain, flag, gen_surface, push_surface, SrfIn, FILES};
use super::topology_face_p2::import;
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// The difftest runner.
pub struct Runner;

/// The classification handles: B-rep topology, and the face's surface and the two
/// curves of the loop bisector (which `EntityRef` cannot name).
#[derive(Debug, Clone, Copy, PartialEq)]
enum H {
    Topo(EntityRef),
    Surface(FaceId),
    Curve(bool),
    EdgeCurve(EdgeId),
    Trim(EdgeuseId),
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Topo(e) => e.class_kind(),
            Self::Surface(_) => ClassObjectKind::Surface,
            Self::Curve(_) | Self::EdgeCurve(_) | Self::Trim(_) => ClassObjectKind::Curve,
        }
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

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------------

/// A surface of the curve-built and natural faces.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum AnySrf {
    /// A B-spline surface (its natural domain).
    Bspline { surface: SrfIn },
    /// `SmCylinder` of radius `scale*a`, axis clamped to `[0, scale*c]`.
    Cylinder { scale: f64, a: f64, c: f64, #[serde(default)] swap: bool },
    /// `SmSphere` of radius `scale*a`.
    Sphere { scale: f64, a: f64, #[serde(default)] swap: bool },
    /// `SmTorus` of radii `scale*a`, `scale*b`.
    Torus { scale: f64, a: f64, b: f64, #[serde(default)] swap: bool },
    /// `SmCone` of bottom radius `scale*a`, semi-angle `b` degrees, axis clamped to
    /// `[0, scale*c]`.
    Cone { scale: f64, a: f64, b: f64, c: f64, #[serde(default)] swap: bool },
}

/// A UV trim curve in fractions of the face domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(super) enum CurveIn {
    /// A line from `(a, b)` to `(c, d)`; `rev`: `SM_OT_OPPOSITE` in its loop.
    Line { a: f64, b: f64, c: f64, d: f64, #[serde(default)] rev: bool },
    /// A counter-clockwise circle centred at `(a, b)`, radius `r` times the smaller
    /// domain side.
    Circle { a: f64, b: f64, r: f64, #[serde(default)] rev: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum BrepIn {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    /// `CreateFaceFromSurface(surface, dom)`.
    Face { surface: SrfIn, dom: [f64; 4] },
    /// `MakeFaceWithCurves`, `copies` times.
    Curves {
        #[serde(default = "one")]
        copies: u8,
        surface: AnySrf,
        loops: Vec<Vec<CurveIn>>,
        #[serde(default)]
        points: Vec<[f64; 2]>,
    },
    /// `CreateFaceInRegionFromSurface` over the natural domain.
    Natural { surface: AnySrf },
}

pub(super) const fn one() -> u8 {
    1
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
    /// `0` fresh props, `1` as `SetProps_Stage3` builds them.
    #[serde(default = "one")]
    props: u8,
    /// The target loops (see the FFI wrapper).
    #[serde(default)]
    loops: u8,
}

fn push_any(v: &mut Vec<f64>, s: &AnySrf) -> Result<(), HarnessError> {
    let sw = |b: bool| flag(b);
    match s {
        AnySrf::Bspline { surface } => push_surface(v, surface)?,
        AnySrf::Cylinder { scale, a, c, swap } => v.extend([10.0, *scale, *a, 0.0, *c, sw(*swap)]),
        AnySrf::Sphere { scale, a, swap } => v.extend([11.0, *scale, *a, 0.0, 0.0, sw(*swap)]),
        AnySrf::Torus { scale, a, b, swap } => v.extend([12.0, *scale, *a, *b, 0.0, sw(*swap)]),
        AnySrf::Cone { scale, a, b, c, swap } => v.extend([13.0, *scale, *a, *b, *c, sw(*swap)]),
    }
    Ok(())
}

/// The B-rep block of `brep` (shared with unit `query.face_p3`, whose shim builds
/// B-reps with this unit's builders).
pub(super) fn brep_input(
    brep: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match brep {
        BrepIn::File { rebuild, .. } => v.extend([0.0, flag(*rebuild)]),
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Curves {
            copies,
            surface,
            loops,
            points,
        } => {
            v.extend([4.0, f64::from(*copies)]);
            push_any(&mut v, surface)?;
            v.push(loops.len() as f64);
            v.extend(loops.iter().map(|l| l.len() as f64));
            v.push(loops.iter().map(Vec::len).sum::<usize>() as f64);
            for c in loops.iter().flatten() {
                match c {
                    CurveIn::Line { a, b, c, d, rev } => {
                        v.extend([0.0, *a, *b, *c, *d, if *rev { 2.0 } else { 1.0 }]);
                    }
                    CurveIn::Circle { a, b, r, rev } => {
                        v.extend([1.0, *a, *b, *r, 0.0, if *rev { 2.0 } else { 1.0 }]);
                    }
                }
            }
            v.push(points.len() as f64);
            v.extend(points.iter().flatten());
        }
        BrepIn::Natural { surface } => {
            v.push(5.0);
            push_any(&mut v, surface)?;
        }
    }
    v.extend([flag(validate_only), flag(init_gaps)]);
    Ok(v)
}

fn input_of(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = brep_input(&i.brep, i.validate_only, i.init_gaps)?;
    v.extend([f64::from(i.face), f64::from(i.props), f64::from(i.loops)]);
    Ok(v)
}

fn path_of(i: &Inputs) -> Result<String, HarnessError> {
    brep_path(&i.brep)
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
pub(super) fn brep_path(brep: &BrepIn) -> Result<String, HarnessError> {
    match brep {
        BrepIn::File { file, .. } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

fn run_ffi(op: Op, i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    ffi::eval(op, &path_of(i)?, &input_of(i)?).map_err(|e| harness(format!("c++: {e}")))
}

// ---- outputs ----------------------------------------------------------------------------

/// The out-params' starting values on both sides: values `ClassifyLoops` never leaves
/// in them, so an early return is visible.
fn initial_out() -> ClassifyLoopsOut {
    ClassifyLoopsOut {
        classifiable: HealNotYetFaceType::NoLoopTreeRoot,
        classify_loop_ios: Vec::new(),
        start_type: CrvDirOnFaceType::Lamina,
        stop_type: CrvDirOnFaceType::Lamina,
        ordered_no_area_indices: vec![77],
        looptree: None,
    }
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
        let n = self.int()?;
        usize::try_from(n)
            .ok()
            .filter(|&n| n <= 1 << 20)
            .ok_or_else(|| harness("bad count in shim output"))
    }

    fn ints(&mut self, n: usize) -> Result<Vec<i64>, HarnessError> {
        (0..n).map(|_| self.int()).collect()
    }
}

fn loop_id(i: i64) -> Result<LoopId, HarnessError> {
    u32::try_from(i)
        .map(LoopId::from_index)
        .map_err(|_| harness("bad loop index"))
}

fn index_of(l: Option<LoopId>) -> i64 {
    l.map_or(-1, |l| i64::from(l.index()))
}

fn tri_code(t: Tri) -> i64 {
    t.code()
}

fn orient_code(o: OrientType) -> i64 {
    solidcore::heal::face_props::orient_code(o)
}

/// The loop tree in pre-order (item, child subtree, sibling subtree), as the shim's
/// `put_tree`.
fn tree_block(tree: Option<&Looptree>) -> Result<Vec<i64>, HarnessError> {
    let Some(tree) = tree else {
        return Ok(vec![0]);
    };
    let mut order = Vec::new();
    let mut num = vec![-1i64; tree.len()];
    let mut stack: Vec<_> = tree.root().into_iter().collect();
    while let Some(id) = stack.pop() {
        if order.len() > 100_000 {
            return Err(harness("looptree too large"));
        }
        let item = tree.item(id).map_err(|e| harness(e.to_string()))?;
        if let Some(slot) = num.get_mut(id.0) {
            *slot = order.len() as i64;
        }
        order.push(id);
        stack.extend(item.sibling);
        stack.extend(item.child);
    }
    let mut out = vec![order.len() as i64];
    let link = |i: Option<solidcore::heal::face_props::ItemId>| {
        i.map_or(-1, |i| num.get(i.0).copied().unwrap_or(-3))
    };
    let io = |i: Option<usize>| i.map_or(-1, |i| i as i64);
    for id in order {
        let it = tree.item(id).map_err(|e| harness(e.to_string()))?;
        out.extend([
            i64::from(it.pole_vertex),
            index_of(it.loop_),
            io(it.classify_loop_io),
            i64::from(it.partner_pole_vertex),
            index_of(it.partner_loop),
            io(it.partner_classify_loop_io),
            link(it.child),
            link(it.sibling),
        ]);
    }
    Ok(out)
}

fn result_output(
    status: f64,
    loops: Vec<i64>,
    not_yet: i64,
    out: &ClassifyLoopsOut,
    tree: Vec<i64>,
    scratch_size: i64,
) -> Output {
    let mut ios = Vec::new();
    for io in &out.classify_loop_ios {
        ios.extend([
            index_of(io.loop_),
            io.loopuse_type.code(),
            orient_code(io.orient),
            tri_code(io.degen_loop),
            tri_code(io.area_loop),
            io.containment.code(),
        ]);
    }
    Output::new()
        .scalar("status", status)
        .ints("loops", loops)
        .int("classifiable", out.classifiable.code())
        .int("not_yet", not_yet)
        .ints("classify_loop_ios", ios)
        .ints(
            "start_stop",
            vec![
                i64::from(out.start_type.code()),
                i64::from(out.stop_type.code()),
            ],
        )
        .ints(
            "ordered_no_area",
            out.ordered_no_area_indices
                .iter()
                .map(|&i| i as i64)
                .collect::<Vec<_>>(),
        )
        .ints("looptree", tree)
        .int("scratch_size", scratch_size)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let o = run_ffi(Op::Classify, i)?;
    let mut rd = Rd { v: &o, at: 0 };
    if rd.num()? != 0.0 {
        return Ok(Output::err(format!("build_failed: {}", rd.num()?)));
    }
    if rd.num()? != 0.0 {
        return Ok(Output::err("rejected"));
    }
    let status = rd.num()?;
    let n = rd.count()?;
    let loops = rd.ints(n)?;
    let classifiable = rd.int()?;
    let not_yet = rd.int()?;
    let n_io = rd.count()?;
    let ios = rd.ints(6 * n_io)?;
    let start = rd.int()?;
    let stop = rd.int()?;
    let n_na = rd.count()?;
    let no_area = rd.ints(n_na)?;
    let n_tree = rd.count()?;
    let mut tree = vec![n_tree as i64];
    tree.extend(rd.ints(8 * n_tree)?);
    let scratch = rd.int()?;
    Ok(Output::new()
        .scalar("status", status)
        .ints("loops", loops)
        .int("classifiable", classifiable)
        .int("not_yet", not_yet)
        .ints("classify_loop_ios", ios)
        .ints("start_stop", vec![start, stop])
        .ints("ordered_no_area", no_area)
        .ints("looptree", tree)
        .int("scratch_size", scratch))
}

/// The imported seam classification: `n` intervals whose end point classifications
/// carry the exported class (and vertex).
fn classification(
    k: &Kernel,
    rd: &mut Rd<'_>,
) -> Result<Option<CurveClassification<H>>, HarnessError> {
    if rd.int()? == 0 {
        return Ok(None);
    }
    let n = rd.count()?;
    let mut cc = CurveClassification::new(k);
    cc.set_size(k, n).map_err(|e| harness(e.to_string()))?;
    for i in 0..n {
        let iv = cc.at_mut(i).map_err(|e| harness(e.to_string()))?;
        for end in 0..2 {
            let class = rd.int()?;
            let vertex = rd.int()?;
            let class = i32::try_from(class)
                .ok()
                .and_then(PointClassType::from_code)
                .ok_or_else(|| harness("bad point class"))?;
            let object = u32::try_from(vertex)
                .ok()
                .map(|v| H::Topo(EntityRef::Vertex(VertexId::from_index(v))));
            let pc = if end == 0 { &mut iv.start } else { &mut iv.end };
            pc.set_class_object(class, object);
        }
    }
    Ok(Some(cc))
}

fn touches(rd: &mut Rd<'_>) -> Result<Vec<TouchData<H>>, HarnessError> {
    let n = rd.count()?;
    let mut v = Vec::new();
    for _ in 0..n {
        let conn = rd.int()?;
        let dir = rd.int()?;
        let loop_index = rd.int()?;
        let connected_to = match conn {
            -1 => None,
            c if c >= 0 => Some(H::Topo(EntityRef::Loop(loop_id(c)?))),
            _ => return Err(harness("touch connected to a non-loop (guard missed it)")),
        };
        v.push(TouchData {
            connected_to,
            loop_dir: i32::try_from(dir)
                .ok()
                .and_then(CrvDirOnFaceType::from_code)
                .ok_or_else(|| harness("bad touch direction"))?,
            loop_index: u64::try_from(loop_index).map_err(|_| harness("bad loop index"))?,
            param: 0.0,
            pt_classify_index: 0,
            pt_classify: None,
        });
    }
    Ok(v)
}

fn tri(code: i64) -> Result<Tri, HarnessError> {
    Ok(match code {
        0 => Tri::False,
        1 => Tri::True,
        2 => Tri::Unsure,
        _ => return Err(harness("bad SmBoolean")),
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let o = run_ffi(Op::Export, i)?;
    if o.first() != Some(&0.0) {
        return Ok(Output::err(format!(
            "build_failed: {}",
            o.get(1).copied().unwrap_or(-9.0)
        )));
    }
    let (graph, block) = ffi::split_export(&o).ok_or_else(|| harness("bad export"))?;
    let mut rd = Rd { v: block, at: 0 };
    if rd.num()? != 0.0 {
        return Ok(Output::err("rejected"));
    }
    let (b, _exact) = import(graph)?;
    let face_index = rd.int()?;
    let face = FaceId::from_index(u32::try_from(face_index).map_err(|_| harness("bad face"))?);

    // the props: imported (props 1) or fresh (props 0)
    let stage = rd.int()?;
    let props_face_index = rd.int()?;
    let closed_u = tri(rd.int()?)?;
    let closed_v = tri(rd.int()?)?;
    let poles = rd.int()?;
    let not_yet = rd.int()?;
    let n_lp = rd.count()?;
    let mut loop_props = Vec::new();
    for _ in 0..n_lp {
        let mut lp = LoopProps::new();
        lp.seam_crossing_cnt_u = rd.int()? as u64;
        lp.seam_winding_cnt_u = rd.int()? as u64;
        lp.seam_crossing_cnt_v = rd.int()? as u64;
        lp.seam_winding_cnt_v = rd.int()? as u64;
        loop_props.push(lp);
    }
    let crv_class_u = classification(&k, &mut rd)?;
    let crv_class_v = classification(&k, &mut rd)?;
    let done_u = touches(&mut rd)?;
    let done_v = touches(&mut rd)?;
    let n_loops = rd.count()?;
    let mut loops = rd
        .ints(n_loops)?
        .into_iter()
        .map(loop_id)
        .collect::<Result<Vec<_>, _>>()?;

    let mut props: FaceProps<H> = FaceProps::for_face(face, props_face_index as u64);
    if i.props == 1 {
        props.prop_stage = PropStage::from_code(stage).ok_or_else(|| harness("bad stage"))?;
        props.closed_u = closed_u;
        props.closed_v = closed_v;
        props.poles = poles as u64;
        props.not_yet_face_type =
            HealNotYetFaceType::from_code(not_yet).ok_or_else(|| harness("bad not-yet type"))?;
        props.loop_props = loop_props;
        props.crv_class_u = crv_class_u;
        props.crv_class_v = crv_class_v;
        props.done_touch_list_u = done_u;
        props.done_touch_list_v = done_v;
    }

    let mut out = initial_out();
    let mut scratch: CurveClassification<H> = CurveClassification::new(&k);
    let r = qf4::classify_loops(&k, &b, face, &mut loops, &mut out, &mut scratch, &mut props);
    let status = match r {
        Ok(()) => 0.0,
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            match &e {
                SolidError::InvalidInput { reason, .. }
                    if *reason == qf4::NOT_THIS_FACE || *reason == qf4::NO_SURFACE =>
                {
                    SM_ERR_INVALID_INPUT
                }
                SolidError::Upstream { code, .. } if *code == SM_ERR => f64::from(SM_ERR),
                _ => return Ok(Output::err(format!("rust_error: {e}"))),
            }
        }
    };
    let tree = tree_block(out.looptree.as_ref())?;
    Ok(result_output(
        status,
        loops.iter().map(|l| i64::from(l.index())).collect(),
        props.not_yet_face_type.code(),
        &out,
        tree,
        scratch.size() as i64,
    ))
}

// ---- generated cases ----------------------------------------------------------------------

pub(super) fn gen_scale(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0)
}

/// Sorted distinct levels in `(0, 1)`, with a chance of near-coincident and near-pole
/// levels.
fn levels(rng: &mut Rng, n: usize) -> Vec<f64> {
    let mut v: Vec<f64> = (0..n).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.15) {
        if let Some(x) = v.first_mut() {
            *x = *rng.choose(&[1e-4, 1e-6, 0.999_9, 0.999_999]).unwrap_or(&0.01);
        }
    }
    if n > 1 && rng.chance(0.1) {
        let a = v.first().copied().unwrap_or(0.5);
        if let Some(x) = v.get_mut(1) {
            *x = a + *rng.choose(&[1e-3, 1e-6, 1e-9]).unwrap_or(&1e-3);
        }
    }
    v.sort_by(f64::total_cmp);
    v.dedup();
    v.retain(|x| *x > 0.0 && *x < 1.0);
    v
}

/// A loop spanning the periodic direction (`u_dir`: lines of constant v from u 0 to 1)
/// at `level`, forward or backward.
fn span(level: f64, u_dir: bool, backward: bool, rev: bool) -> Vec<CurveIn> {
    let (a, b, c, d) = if u_dir {
        if backward {
            (1.0, level, 0.0, level)
        } else {
            (0.0, level, 1.0, level)
        }
    } else if backward {
        (level, 1.0, level, 0.0)
    } else {
        (level, 0.0, level, 1.0)
    };
    if rev {
        vec![CurveIn::Line {
            a: c,
            b: d,
            c: a,
            d: b,
            rev: true,
        }]
    } else {
        vec![CurveIn::Line { a, b, c, d, rev: false }]
    }
}

pub(super) fn rect(rng: &mut Rng) -> Vec<CurveIn> {
    let u0 = rng.uniform(0.1, 0.4);
    let v0 = rng.uniform(0.1, 0.4);
    let u1 = u0 + rng.uniform(0.1, 0.4);
    let v1 = v0 + rng.uniform(0.1, 0.4);
    let mut l = vec![
        CurveIn::Line { a: u0, b: v0, c: u1, d: v0, rev: false },
        CurveIn::Line { a: u1, b: v0, c: u1, d: v1, rev: false },
        CurveIn::Line { a: u1, b: v1, c: u0, d: v1, rev: false },
        CurveIn::Line { a: u0, b: v1, c: u0, d: v0, rev: false },
    ];
    if rng.chance(0.5) {
        // clockwise
        l = l
            .into_iter()
            .rev()
            .map(|c| match c {
                CurveIn::Line { a, b, c, d, .. } => CurveIn::Line { a: c, b: d, c: a, d: b, rev: false },
                other @ CurveIn::Circle { .. } => other,
            })
            .collect();
    }
    l
}

/// A loop of lines around `[u0, 1] + [0, u1] x [v0, v1]`: straddling the periodic
/// direction's seam (crossing it twice), counter-clockwise unless `cw`; `u_dir` false
/// swaps the coordinates.
fn straddle(u0: f64, u1: f64, v0: f64, v1: f64, u_dir: bool, cw: bool) -> Vec<CurveIn> {
    let pts = [
        ((u0, v0), (1.0, v0)),
        ((0.0, v0), (u1, v0)),
        ((u1, v0), (u1, v1)),
        ((u1, v1), (0.0, v1)),
        ((1.0, v1), (u0, v1)),
        ((u0, v1), (u0, v0)),
    ];
    let tr = |(a, b): (f64, f64)| if u_dir { (a, b) } else { (b, a) };
    let mut l: Vec<CurveIn> = pts
        .iter()
        .map(|&(p, q)| {
            let (p, q) = (tr(p), tr(q));
            CurveIn::Line { a: p.0, b: p.1, c: q.0, d: q.1, rev: false }
        })
        .collect();
    if cw {
        l = l
            .into_iter()
            .rev()
            .map(|c| match c {
                CurveIn::Line { a, b, c, d, .. } => CurveIn::Line { a: c, b: d, c: a, d: b, rev: false },
                other @ CurveIn::Circle { .. } => other,
            })
            .collect();
    }
    l
}

pub(super) fn gen_seam_face(rng: &mut Rng) -> BrepIn {
    let scale = gen_scale(rng);
    let swap = rng.chance(0.3);
    let kind = rng.below(5);
    let surface = match kind {
        0 | 1 => AnySrf::Cylinder {
            scale,
            a: rng.uniform(0.5, 2.0),
            c: rng.uniform(1.0, 5.0),
            swap,
        },
        2 => AnySrf::Sphere {
            scale,
            a: rng.uniform(0.5, 3.0),
            swap,
        },
        3 => {
            let a = rng.uniform(2.0, 4.0);
            AnySrf::Torus {
                scale,
                a,
                b: a * rng.uniform(0.1, 0.5),
                swap,
            }
        }
        _ => {
            let a = rng.uniform(1.0, 3.0);
            let semi = rng.uniform(10.0, 45.0);
            AnySrf::Cone {
                scale,
                a,
                b: semi,
                c: rng.uniform(0.1, 0.45) * a / semi.to_radians().tan(),
                swap,
            }
        }
    };
    // the periodic direction in fractions: u unless swapped (torus: either)
    let mut u_dir = !swap;
    if kind == 3 && rng.chance(0.5) {
        u_dir = !u_dir;
    }
    let n = 1 + rng.below(4) as usize;
    let mut loops: Vec<Vec<CurveIn>> = Vec::new();
    if rng.chance(0.3) {
        // area loops straddling the seam: one, or an outer and a nested one (either
        // orientation), with a chance of no-area loops beside them
        let (u0, u1) = (rng.uniform(0.55, 0.8), rng.uniform(0.2, 0.45));
        let (v0, v1) = (rng.uniform(0.1, 0.3), rng.uniform(0.7, 0.9));
        loops.push(straddle(u0, u1, v0, v1, u_dir, rng.chance(0.2)));
        if rng.chance(0.5) {
            let du = rng.uniform(0.02, 0.1);
            let dv = rng.uniform(0.02, 0.1);
            loops.push(straddle(u0 + du, u1 - du, v0 + dv, v1 - dv, u_dir, rng.chance(0.8)));
        }
        if rng.chance(0.3) {
            loops.push(span(rng.uniform(0.92, 0.98), u_dir, rng.chance(0.5), false));
        }
    } else {
        for (j, level) in levels(rng, n).into_iter().enumerate() {
            let alternate = j % 2 == 1;
            let backward = if rng.chance(0.85) { alternate } else { rng.chance(0.5) };
            loops.push(span(level, u_dir, backward, rng.chance(0.2)));
        }
    }
    if kind == 3 && rng.chance(0.25) {
        // a diagonal winding once about both seams
        loops.push(vec![CurveIn::Line {
            a: 0.0,
            b: 0.0,
            c: 1.0,
            d: 1.0,
            rev: false,
        }]);
    }
    if rng.chance(0.05) {
        loops.push(rect(rng));
    }
    if rng.chance(0.03) {
        loops.push(vec![CurveIn::Circle {
            a: 0.5,
            b: 0.5,
            r: rng.uniform(0.01, 0.2),
            rev: rng.chance(0.5),
        }]);
    }
    if rng.chance(0.15) {
        loops.reverse();
    }
    let mut points = Vec::new();
    if kind == 2 && rng.chance(0.4) {
        // a vertex loop at a pole
        let at_end = rng.chance(0.5);
        points.push(if swap {
            [if at_end { 1.0 } else { 0.0 }, 0.5]
        } else {
            [0.5, if at_end { 1.0 } else { 0.0 }]
        });
    }
    BrepIn::Curves {
        copies: if rng.chance(0.08) { 2 } else { 1 },
        surface,
        loops,
        points,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let roll = rng.below(100);
    let brep = if roll < 80 {
        gen_seam_face(rng)
    } else if roll < 90 {
        let scale = gen_scale(rng);
        BrepIn::Natural {
            surface: match rng.below(3) {
                0 => AnySrf::Sphere {
                    scale,
                    a: rng.uniform(0.5, 3.0),
                    swap: rng.chance(0.5),
                },
                1 => AnySrf::Torus {
                    scale,
                    a: 3.0,
                    b: rng.uniform(0.3, 1.5),
                    swap: rng.chance(0.5),
                },
                _ => AnySrf::Cylinder {
                    scale,
                    a: rng.uniform(0.5, 2.0),
                    c: rng.uniform(1.0, 5.0),
                    swap: rng.chance(0.5),
                },
            },
        }
    } else if roll < 95 {
        let s = gen_surface(rng);
        let [u0, v0, u1, v1] = domain(&s);
        let t = |a: f64, b: f64, f: f64| a + (b - a) * f;
        let (fa, fb) = (rng.uniform(0.0, 0.3), rng.uniform(0.7, 1.0));
        BrepIn::Face {
            dom: [t(u0, u1, fa), t(v0, v1, fa), t(u0, u1, fb), t(v0, v1, fb)],
            surface: s,
        }
    } else {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"sphere.smb")).to_string(),
            rebuild: false,
        }
    };
    let two_faces = matches!(brep, BrepIn::Curves { copies: 2, .. } | BrepIn::File { .. });
    // fresh props are rejected on a closed surface: only for the open faces
    let staged = match brep {
        BrepIn::Curves { .. } | BrepIn::Natural { .. } => true,
        _ => rng.chance(0.5),
    };
    let loops = if two_faces && rng.chance(0.5) {
        4
    } else if rng.chance(0.05) {
        3
    } else {
        *rng.choose(&[0u8, 0, 1, 1, 1, 2]).unwrap_or(&0)
    };
    Inputs {
        brep,
        validate_only: rng.chance(0.1),
        init_gaps: rng.chance(0.2),
        face: rng.below(64) as u32,
        props: u8::from(staged),
        loops,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.face_p4"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Most generated cases reach a real classification on the C++ side (not a build
    /// failure or a rejection), and the Rust side compares most of those.
    #[test]
    fn generated_cases_classify() {
        let mut rng = Rng::new(11);
        let mut why: BTreeMap<String, usize> = BTreeMap::new();
        let n = 200;
        let mut classified = 0;
        for _ in 0..n {
            let i = gen_case(&mut rng);
            let c = cpp(&i).unwrap();
            let r = rust(&i).unwrap();
            let key = if let Some(e) = &c.error {
                let kind = serde_json::to_value(&i.brep).unwrap()["kind"].clone();
                format!("cpp error {e} {kind} props {} loops {}", i.props, i.loops)
            } else if let Some(d) = &r.deferred {
                format!("deferred {}", d.unit)
            } else {
                let ios = c.ints.get("classify_loop_ios").map_or(0, Vec::len);
                *why.entry(format!("  start_stop {:?}", c.ints.get("start_stop"))).or_default() += 1;
                for ch in c.ints.get("classify_loop_ios").cloned().unwrap_or_default().chunks(6) {
                    *why.entry(format!("  io type/orient/area/cont {:?}", ch.get(1..))).or_default() += 1;
                }
                *why.entry(format!("  tree len {:?}", c.ints.get("looptree").and_then(|t| t.first()))).or_default() += 1;
                if ios > 0 {
                    classified += 1;
                }
                format!(
                    "compared status {} classifiable {:?} ios>0 {}",
                    c.values.get("status").and_then(|v| v.first()).copied().unwrap_or(-1.0),
                    c.ints.get("classifiable"),
                    ios > 0
                )
            };
            *why.entry(key).or_default() += 1;
        }
        for (k, v) in &why {
            eprintln!("{v:4} {k}");
        }
        assert!(classified * 2 > n, "only {classified} of {n} classify loops");
    }
}
