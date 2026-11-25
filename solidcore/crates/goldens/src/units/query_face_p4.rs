//! Runner for unit `query.face_p4`. Ported from upstream `SmFace::ClassifyLoops`; checked
//! against its golden. Rust: `solidcore::query::face::p4`.
//!
//! **B-reps.** Each case builds a B-rep with the Rust kernel ([`own_brep`]: the Rust
//! reader, the shared face builder and this runner's curves and natural-face builders,
//! in upstream's call order and traversal order): an upstream `.smb` file, a
//! face over a B-spline surface or `SmPlane` (`CreateFaceFromSurface`), a face of 2D line
//! and circle UV trim curves and single vertex loops over a real `SmCylinder`,
//! `SmSphere`, `SmTorus` or `SmCone` or a B-spline (`MakeFaceWithCurves`, as `prog_test`
//! `my_test_tsurf_cyl_creation_fast`; lines spanning a periodic direction make the
//! no-area loops of faces without seam edge: cylinder circles, sphere caps and bands,
//! torus bands and diagonals), or the natural face of an analytic surface
//! (`CreateFaceInRegionFromSurface`, as `my_test_closed_sphere_properties`).
//!
//! **Face props.** `ClassifyLoops` reads the face props of its only caller,
//! `SmFaceProps::SetProps_Stage3`. With `props` `1` they are built as that caller
//! does (`SetProps_Stage2`, `ReSet(SM_PROPSTAGE_3)`, the loop props; the Rust side with
//! the port of unit `heal.face_props`, [`stage3_props`]) on the B-rep, and runs
//! `ClassifyLoops` on them. With `props` `0` the props are fresh and `ClassifyLoops`
//! computes them itself (`SetProps_Stage2`). The case (face, props, target loops and
//! the rejection guards) is resolved from the B-rep ([`setup`], in upstream's call
//! order).
//!
//! **Deferred.** Fresh props of a closed surface need `SmFace::HasSeamProblem`
//! (`topology.face_p4`), which reaches its unported callees.
//!
//! **Native geometry.** The analytic face surfaces and ellipse edge curves are the Rust
//! kernel's own (`CreateCanonical`, `SwapUV`, the reader); before, the importer rebuilt
//! them from the shim's exact records, and before that imported their B-spline parts
//! only (the loop bisector then flipped a loop classification in three cylinder cases,
//! now the named regressions `cyl_*` in `cases.json`). The gap caches are the Rust
//! B-rep's own: those the build's `init_gaps` walk fills, and those the props build and
//! `ClassifyLoops` compute (plan/findings/query.face_p4--gen0360-macos-exact-records.md).
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
use solidcore::intersect::curve_class::p1::{CrvDirOnFaceType, CurveClassification};
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::query::face::p4 as qf4;
use solidcore::surface::surface::Surface;
use solidcore::topology::Brep;
use solidcore::topology::{EdgeId, EdgeuseId, FaceId, LoopId, VertexId};

use super::topology_edgeuse_p1::{domain, gen_surface, SrfIn, FILES};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

/// The unit runner.
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
    /// A temporary curve (the seam curves of `SmFace::HasSeamProblem`).
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
    Cylinder {
        scale: f64,
        a: f64,
        c: f64,
        #[serde(default)]
        swap: bool,
    },
    /// `SmSphere` of radius `scale*a`.
    Sphere {
        scale: f64,
        a: f64,
        #[serde(default)]
        swap: bool,
    },
    /// `SmTorus` of radii `scale*a`, `scale*b`.
    Torus {
        scale: f64,
        a: f64,
        b: f64,
        #[serde(default)]
        swap: bool,
    },
    /// `SmCone` of bottom radius `scale*a`, semi-angle `b` degrees, axis clamped to
    /// `[0, scale*c]`.
    Cone {
        scale: f64,
        a: f64,
        b: f64,
        c: f64,
        #[serde(default)]
        swap: bool,
    },
}

/// A UV trim curve in fractions of the face domain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub(super) enum CurveIn {
    /// A line from `(a, b)` to `(c, d)`; `rev`: `SM_OT_OPPOSITE` in its loop.
    Line {
        a: f64,
        b: f64,
        c: f64,
        d: f64,
        #[serde(default)]
        rev: bool,
    },
    /// A counter-clockwise circle centred at `(a, b)`, radius `r` times the smaller
    /// domain side.
    Circle {
        a: f64,
        b: f64,
        r: f64,
        #[serde(default)]
        rev: bool,
    },
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

// ---- outputs ----------------------------------------------------------------------------

/// The out-params' starting values on both sides (the shim's `run_op` starts from the
/// same `SM_HNF_NO_LOOP_TREE_ROOT`, `SM_CD_LAMINA` and `[77]`): values `ClassifyLoops`
/// never leaves together with its status, so an early return is visible.
fn initial_out() -> ClassifyLoopsOut {
    ClassifyLoopsOut {
        classifiable: HealNotYetFaceType::NoLoopTreeRoot,
        classify_loop_ios: Vec::new(),
        classify_loop_io_slack: Vec::new(),
        start_type: CrvDirOnFaceType::Lamina,
        stop_type: CrvDirOnFaceType::Lamina,
        ordered_no_area_indices: vec![77],
        looptree: None,
    }
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

// ---- the Rust B-rep ---------------------------------------------------------------------

/// The shim's `read_any_surface`: a B-spline (or plane) over its natural domain, or a
/// real analytic surface made by its `CreateCanonical` at the default placement,
/// `SwapUV`'d when `swap`, over its natural domain with an unbounded direction (above
/// `1e5` wide) clamped to `[0, scale*c]`. The error is the build's (the shim's NULL
/// surface, then `SM_ERR`).
fn any_surface(
    k: &Kernel,
    s: &AnySrf,
) -> Result<Result<(Surface, Extent2d), SolidError>, HarnessError> {
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::surface::cone::Cone;
    use solidcore::surface::sphere::Sphere;
    use solidcore::surface::torus::{Torus, TorusOptions};
    const OP: &str = "difftest::query_face_p4::read_any_surface";
    if let AnySrf::Bspline { surface } = s {
        let s = super::topology_edgeuse_p1::surface_of(surface)?;
        return Ok(s.natural_uv_domain_with_nurb(k).map(|d| (s, d)));
    }
    let origin = Axis2Placement::default();
    let (made, scale, c, swap) = match s {
        AnySrf::Bspline { .. } => return Err(harness("unreachable surface kind")),
        AnySrf::Cylinder { scale, a, c, swap } => (
            Cone::create_canonical_cylinder(k, &origin, scale * a).map(Surface::from),
            *scale,
            *c,
            *swap,
        ),
        AnySrf::Sphere { scale, a, swap } => (
            Sphere::create_canonical(k, &origin, scale * a).map(Surface::from),
            *scale,
            0.0,
            *swap,
        ),
        AnySrf::Torus { scale, a, b, swap } => (
            Torus::create_canonical(
                k,
                &origin,
                scale * a,
                scale * b,
                TorusOptions::default(),
                None,
            )
            .map(Surface::from),
            *scale,
            0.0,
            *swap,
        ),
        AnySrf::Cone {
            scale,
            a,
            b,
            c,
            swap,
        } => (
            Cone::create_canonical_with_semi_angle(k, &origin, scale * a, *b, None, false, false)
                .map(Surface::from),
            *scale,
            *c,
            *swap,
        ),
    };
    // a failed CreateCanonical or SwapUV: the shim's NULL surface
    let Ok(mut surface) = made else {
        return Ok(Err(SolidError::sm_err(OP)));
    };
    if swap && surface.swap_uv(k).is_err() {
        return Ok(Err(SolidError::sm_err(OP)));
    }
    let dom = match surface.natural_uv_domain_with_nurb(k) {
        Ok(d) => d,
        Err(e) => return Ok(Err(e)),
    };
    let (mut lo, mut hi) = (dom.min(), dom.max());
    if hi.x - lo.x > 1e5 {
        lo.x = 0.0;
        hi.x = scale * c;
    }
    if hi.y - lo.y > 1e5 {
        lo.y = 0.0;
        hi.y = scale * c;
    }
    Ok(Ok((surface, Extent2d::from_points(lo, hi))))
}

/// The point at fractions `(a, b)` of `dom` (the shim's `at_frac`).
fn at_frac(dom: &Extent2d, a: f64, b: f64) -> Point2 {
    Point2::new(
        dom.min().x + a * (dom.max().x - dom.min().x),
        dom.min().y + b * (dom.max().y - dom.min().y),
    )
}

/// One `MakeFaceWithCurves` face of a [`BrepIn::Curves`] source (the shim's
/// `make_face_with_curves`): the surface, its UV line and circle trims and the vertex
/// loop points, on the infinite region.
fn curves_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: &AnySrf,
    loops: &[Vec<CurveIn>],
    points: &[[f64; 2]],
) -> Result<Result<(), SolidError>, HarnessError> {
    use solidcore::curve::curve::Curve;
    use solidcore::geom::axis2_placement::Axis2Placement;
    use solidcore::nurbs::b_spline_curve::{BSplineCurve, NurbCircleParam};
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
    use solidcore::topology::brep_cache::{arena_identity, CacheObject};
    let (srf, dom) = match any_surface(k, surface)? {
        Ok(x) => x,
        Err(e) => return Ok(Err(e)),
    };
    let mut uv = Vec::new();
    let mut orientations = Vec::new();
    for c in loops.iter().flatten() {
        let (curve, rev) = match c {
            CurveIn::Line { a, b, c, d, rev } => {
                let p = at_frac(&dom, *a, *b);
                let q = at_frac(&dom, *c, *d);
                (
                    BSplineCurve::create_line_segment(
                        2,
                        Point3::new(p.x, p.y, 0.0),
                        Point3::new(q.x, q.y, 0.0),
                    ),
                    *rev,
                )
            }
            CurveIn::Circle { a, b, r, rev } => {
                let ctr = at_frac(&dom, *a, *b);
                let side = (dom.max().x - dom.min().x).min(dom.max().y - dom.min().y);
                let mut frame = Axis2Placement::default();
                // UV curves lie in XY: SetCanonical takes the frame's X and Y axes.
                let _ = frame.set_canonical(
                    Point3::new(ctr.x, ctr.y, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(0.0, 1.0, 0.0),
                );
                (
                    BSplineCurve::create_circle_segment(
                        2,
                        &frame,
                        r * side,
                        0.0,
                        360.0,
                        NurbCircleParam::Quadratic,
                    ),
                    *rev,
                )
            }
        };
        match curve {
            Ok(c) => {
                uv.push(Curve::from(c));
                orientations.push(if rev {
                    OrientType::Opposite
                } else {
                    OrientType::Same
                });
            }
            Err(e) => return Ok(Err(e)),
        }
    }
    let mut loop_points = Vec::new();
    for [a, bb] in points {
        match srf.evaluate_point(k, at_frac(&dom, *a, *bb)) {
            Ok(p) => loop_points.push(p),
            Err(e) => return Ok(Err(e)),
        }
    }
    let input = FaceCurvesInput {
        loops: loops.iter().map(Vec::len).collect(),
        curves_3d: None,
        uv_curves: Some(uv),
        orientations,
        loop_points,
    };
    let mut state = ArenaNotifyState::new(b);
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut env = state.env(None);
    let region = b.infinite_region();
    Ok(make_face_with_curves(
        k,
        b,
        region,
        input,
        srf,
        &dom,
        OrientType::Same,
        None,
        &this_face,
        &mut env,
    )
    .map(|_| ()))
}

/// The natural face of a [`BrepIn::Natural`] source (`CreateFaceInRegionFromSurface`
/// on the infinite region).
fn natural_face(
    k: &mut Kernel,
    b: &mut Brep,
    surface: &AnySrf,
) -> Result<Result<(), SolidError>, HarnessError> {
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep_cache::{arena_identity, CacheObject};
    use solidcore::topology::topology_traverser::Marks;
    let (srf, dom) = match any_surface(k, surface)? {
        Ok(x) => x,
        Err(e) => return Ok(Err(e)),
    };
    let mut state = ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let mut marks = Marks::default();
    let this_face = |face| arena_identity(CacheObject::Face(face));
    let mut fenv = solidcore::topology::brep::p8::FaceBuildEnv {
        env: &mut env,
        marks: &mut marks,
        this_face: &this_face,
    };
    let region = b.infinite_region();
    Ok(solidcore::topology::brep::p8::create_face_in_region_from_surface(
        k,
        b,
        region,
        Some(srf),
        &dom,
        &mut fenv,
    )
    .map(|_| ()))
}

/// The Rust B-rep of a B-rep source of this unit's shim `build` (shared with unit
/// `query.face_p3`), made by the Rust kernel in the shim's traversal order (unit
/// `topology.face_p2`'s conventions: the same `walk` and `init_gaps` walk): an `.smb`
/// file read by the Rust reader, a face from a surface by [`crate::own_builder`], and
/// the curves and natural faces here. A failed build is `build_failed: <status>`.
pub(super) fn own_brep(
    brep: &BrepIn,
    validate_only: bool,
    init_gaps: bool,
) -> Result<Result<(Brep, [usize; 10]), Output>, HarnessError> {
    use super::topology_face_p2::{own_output, CONVENTIONS};
    let mut k = crate::runner_kernel();
    let built = match brep {
        BrepIn::File { .. } | BrepIn::Face { .. } => {
            return Ok(super::topology_face_p2::own_brep(
                &mut k,
                brep,
                &brep_path(brep)?,
                validate_only,
                init_gaps,
            )?
            .map_err(|o| failed_as_build(&o)))
        }
        BrepIn::Curves {
            copies,
            surface,
            loops,
            points,
        } => {
            let mut bad = None;
            let raw = crate::own_builder::in_new_brep(&mut k, |k, b| {
                solidcore::topology::brep::p2::set_tolerance(k, b, 1e-7, false, true)?;
                let mut first = Ok(());
                for _ in 0..*copies {
                    let r = match curves_face(k, b, surface, loops, points) {
                        Ok(r) => r,
                        Err(h) => {
                            bad = Some(h);
                            return Err(SolidError::invalid_input("difftest", "bad source"));
                        }
                    };
                    if first.is_ok() {
                        first = r;
                    }
                }
                first
            })?;
            if let Some(h) = bad {
                return Err(h);
            }
            crate::own_builder::finish(&mut k, raw, validate_only, init_gaps, &CONVENTIONS)?
        }
        BrepIn::Natural { surface } => {
            let mut bad = None;
            let raw = crate::own_builder::in_new_brep(&mut k, |k, b| {
                solidcore::topology::brep::p2::set_tolerance(k, b, 1e-7, false, true)?;
                match natural_face(k, b, surface) {
                    Ok(r) => r,
                    Err(h) => {
                        bad = Some(h);
                        Err(SolidError::invalid_input("difftest", "bad source"))
                    }
                }
            })?;
            if let Some(h) = bad {
                return Err(h);
            }
            crate::own_builder::finish(&mut k, raw, validate_only, init_gaps, &CONVENTIONS)?
        }
    };
    Ok(own_output(built).map_err(|o| failed_as_build(&o)))
}

/// A failed build's output in this unit's form (`build_failed: <status>`; an unported
/// deferral unchanged).
fn failed_as_build(o: &Output) -> Output {
    match o.error.as_deref().and_then(|n| n.strip_prefix("brep_build_status_")) {
        Some(status) => Output::err(format!("build_failed: {status}")),
        None => o.clone(),
    }
}

// ---- the case, resolved on the Rust B-rep (the shim's `setup`) ---------------------------

/// The deferral or failure output of a Rust error the shim's case setup cannot ignore:
/// an unported callee defers the case, anything else is a Rust error.
pub(super) fn setup_error(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::kernel(&e, format!("rust_error: {e}")))
}

/// Whether the shim ignores a status of the case setup's props calls (it discards
/// `SetProps_Stage2`'s status, and `SmLoopProps::SetProps` always succeeds upstream):
/// an upstream status. An unported callee or any other Rust error (an invalid input
/// included: it can be a limitation of the runner's handles) is not ignored.
fn ignored_status(e: &SolidError) -> bool {
    solidcore::error::upstream_status(e).is_some()
}

/// The face props `SetProps_Stage3` has built when it calls `ClassifyLoops`, and its
/// closed loops: the shim's `new SmFaceProps(face, face_index)`, `SetProps_Stage2(face,
/// face_index, SM_HO_ALL)` (on `b` in place, its status ignored) and `stage3_prefix`
/// (`SmFaceProps.cpp`, `SetProps_Stage3`, from `ReSet(SM_PROPSTAGE_3)` to the closed
/// loop count, as the library's `build_loop_props` ports it). `Err(output)` for an
/// error the shim has no counterpart for (an unported callee defers the case).
pub(super) fn stage3_props<O: LoopObject>(
    k: &Kernel,
    b: &mut Brep,
    face: FaceId,
    face_index: u64,
) -> Result<(FaceProps<O>, Vec<LoopId>), Box<Output>> {
    use solidcore::heal::face_props::{ContainmentType, LoopuseType};
    use solidcore::heal::face_props::HealerOpType;
    let mut props: FaceProps<O> = FaceProps::for_face(face, face_index);
    if let Err(e) = props.set_props_stage2_mut(k, b, face, face_index, HealerOpType::All) {
        if !ignored_status(&e) {
            return Err(Box::new(setup_error(&e)));
        }
    }
    // stage3_prefix
    props.reset(PropStage::Stage3);
    props.prop_stage = PropStage::Stage3A;
    let Some(m_face) = props.face else {
        return Err(Box::new(Output::err("rust_error: props without face")));
    };
    let loops = match solidcore::topology::face::p2::loops(b, m_face, false, None) {
        Ok(l) => l,
        Err(e) => return Err(Box::new(setup_error(&e))),
    };
    let mut loop_props = vec![LoopProps::new(); loops.len()];
    let mut closed_loops = Vec::new();
    for (lp, &l) in loop_props.iter_mut().zip(&loops) {
        if let Err(e) = lp.set_props(k, b, l, &props) {
            if !ignored_status(&e) {
                return Err(Box::new(setup_error(&e)));
            }
        }
        if lp.loopuse_type == LoopuseType::Vertexuse {
            if (lp.seam_crossing_cnt_u > 0 || lp.seam_crossing_cnt_v > 0)
                && lp.vertex_loop_on_pole.is_true()
            {
                closed_loops.push(l);
            }
        } else if lp.closed_3d.is_true() || lp.closed_ptrs.is_true() {
            closed_loops.push(l);
        } else {
            lp.containment = ContainmentType::WireLoop;
        }
    }
    props.loop_props = loop_props;
    props.closed_loop_cnt = closed_loops.len() as u64;
    Ok((props, closed_loops))
}

/// The shim's `fresh_props_closed`: whether fresh props of the selected face would be on
/// a closed surface (`SetProps_Stage2` on a separate Rust build of the same source, its
/// status ignored; a failed build counts as closed, as the shim's).
fn fresh_props_closed(k: &Kernel, i: &Inputs) -> Result<Result<bool, Output>, HarnessError> {
    use solidcore::heal::face_props::HealerOpType;
    let Ok((mut b, _)) = own_brep(&i.brep, i.validate_only, i.init_gaps)? else {
        return Ok(Ok(true));
    };
    let n = b.faces().len();
    if n == 0 {
        return Ok(Ok(true));
    }
    let fi = i.face as usize % n;
    let face = FaceId::from_index(u32::try_from(fi).map_err(|_| harness("bad face"))?);
    let mut props: FaceProps<H> = FaceProps::for_face(face, fi as u64);
    if let Err(e) = props.set_props_stage2_mut(k, &mut b, face, fi as u64, HealerOpType::All) {
        if !ignored_status(&e) {
            return Ok(Err(setup_error(&e)));
        }
    }
    Ok(Ok(props.closed_u != Tri::False || props.closed_v != Tri::False))
}

/// The shim's `staged_guard`: the inputs `ClassifyLoops` would read outside its arrays
/// with (a touch list of odd length, a touch connected to something other than a loop, a
/// touch loop index outside the target loops, an empty seam classification of a loop
/// crossing that seam, more target loops than loop props on a closed surface).
fn staged_guard(p: &FaceProps<H>, n_target: usize) -> bool {
    let n_target = n_target as u64;
    for l in [
        &p.done_touch_list_u,
        &p.done_touch_list_v,
        &p.raw_touch_list_u,
        &p.raw_touch_list_v,
    ] {
        if l.len() % 2 != 0 {
            return true;
        }
        for t in l {
            if t.connected_to.is_some_and(|c| c.as_loop().is_none()) {
                return true;
            }
            if t.loop_index >= n_target {
                return true;
            }
        }
    }
    let closed = p.closed_u != Tri::False || p.closed_v != Tri::False;
    if closed && n_target > p.loop_props.len() as u64 {
        return true;
    }
    let empty = |c: &Option<CurveClassification<H>>| c.as_ref().is_none_or(|c| c.size() == 0);
    for lp in &p.loop_props {
        if p.closed_u != Tri::False && lp.seam_crossing_cnt_u > 0 && empty(&p.crv_class_u) {
            return true;
        }
        if p.closed_v != Tri::False && lp.seam_crossing_cnt_v > 0 && empty(&p.crv_class_v) {
            return true;
        }
    }
    false
}

/// A resolved case: the face, its props and the target loops, or the case's output.
type CaseOf = Result<(FaceId, FaceProps<H>, Vec<LoopId>), Output>;

/// The case on `b` as the shim's `setup` resolves it (`face_sel props_mode
/// loops_mode`): the face, its props and the target loops; `Err(rejected)` where the
/// shim rejects it, `Err(output)` for an error of the Rust setup.
fn setup(
    k: &Kernel,
    b: &mut Brep,
    i: &Inputs,
) -> Result<CaseOf, HarnessError> {
    use solidcore::topology::face::p2::loops as face_loops;
    let rejected = || Ok(Err(Output::err("rejected")));
    let n_faces = b.faces().len();
    if n_faces == 0 {
        return rejected();
    }
    let face_index = i.face as usize % n_faces;
    let face_of = |x: usize| u32::try_from(x).map(FaceId::from_index).map_err(|_| harness("bad face"));
    let face = face_of(face_index)?;
    let (props, base) = if i.props == 1 {
        match stage3_props::<H>(k, b, face, face_index as u64) {
            Ok(x) => x,
            Err(o) => return Ok(Err(*o)),
        }
    } else {
        match fresh_props_closed(k, i)? {
            Ok(true) => return rejected(),
            Ok(false) => {}
            Err(o) => return Ok(Err(o)),
        }
        let base = match face_loops(b, face, false, None) {
            Ok(l) => l,
            Err(e) => return Ok(Err(setup_error(&e))),
        };
        (FaceProps::for_face(face, face_index as u64), base)
    };
    // loops_mode: 0 empty (ClassifyLoops gets them), 1 base, 2 base reversed, 3 base
    // without its last loop, 4 base with its first loop replaced by a loop of the next
    // face (prepended to an empty base)
    let mut loops = Vec::new();
    match i.loops {
        1 | 3 | 4 => {
            let mut first = 0;
            if i.loops == 4 {
                let other = face_of((face_index + 1) % n_faces)?;
                let ol = match face_loops(b, other, false, None) {
                    Ok(l) => l,
                    Err(e) => return Ok(Err(setup_error(&e))),
                };
                match ol.first() {
                    Some(&l) if other != face => loops.push(l),
                    _ => return rejected(),
                }
                first = usize::from(!base.is_empty());
            }
            let n = base.len() - usize::from(i.loops == 3 && !base.is_empty());
            loops.extend(base.iter().take(n).skip(first));
        }
        2 => loops.extend(base.iter().rev()),
        _ => {}
    }
    if i.props == 1 {
        let mut n_target = loops.len();
        if n_target == 0 {
            n_target = match face_loops(b, face, false, None) {
                Ok(l) => l.len(),
                Err(e) => return Ok(Err(setup_error(&e))),
            };
        }
        if staged_guard(&props, n_target) {
            return rejected();
        }
    }
    Ok(Ok((face, props, loops)))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    // the B-rep: the Rust kernel's own build of the source (no C++ state)
    let (mut b, _) = match own_brep(&i.brep, i.validate_only, i.init_gaps)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    // the case (rejection, face, props, target loops), resolved from that B-rep as the
    // shim's `setup` resolves it
    let (face, mut props, mut loops) = match setup(&k, &mut b, i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };

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
                SolidError::InvalidInput { reason, .. } | SolidError::PortInvariant { reason, .. }
                    if *reason == qf4::NOT_THIS_FACE || *reason == qf4::NO_SURFACE =>
                {
                    SM_ERR_INVALID_INPUT
                }
                e if solidcore::error::upstream_status(e) == Some(SM_ERR) => f64::from(SM_ERR),
                _ => return Ok(Output::kernel(&e, format!("rust_error: {e}"))),
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
            *x = *rng
                .choose(&[1e-4, 1e-6, 0.999_9, 0.999_999])
                .unwrap_or(&0.01);
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
        vec![CurveIn::Line {
            a,
            b,
            c,
            d,
            rev: false,
        }]
    }
}

pub(super) fn rect(rng: &mut Rng) -> Vec<CurveIn> {
    let u0 = rng.uniform(0.1, 0.4);
    let v0 = rng.uniform(0.1, 0.4);
    let u1 = u0 + rng.uniform(0.1, 0.4);
    let v1 = v0 + rng.uniform(0.1, 0.4);
    let mut l = vec![
        CurveIn::Line {
            a: u0,
            b: v0,
            c: u1,
            d: v0,
            rev: false,
        },
        CurveIn::Line {
            a: u1,
            b: v0,
            c: u1,
            d: v1,
            rev: false,
        },
        CurveIn::Line {
            a: u1,
            b: v1,
            c: u0,
            d: v1,
            rev: false,
        },
        CurveIn::Line {
            a: u0,
            b: v1,
            c: u0,
            d: v0,
            rev: false,
        },
    ];
    if rng.chance(0.5) {
        // clockwise
        l = l
            .into_iter()
            .rev()
            .map(|c| match c {
                CurveIn::Line { a, b, c, d, .. } => CurveIn::Line {
                    a: c,
                    b: d,
                    c: a,
                    d: b,
                    rev: false,
                },
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
            CurveIn::Line {
                a: p.0,
                b: p.1,
                c: q.0,
                d: q.1,
                rev: false,
            }
        })
        .collect();
    if cw {
        l = l
            .into_iter()
            .rev()
            .map(|c| match c {
                CurveIn::Line { a, b, c, d, .. } => CurveIn::Line {
                    a: c,
                    b: d,
                    c: a,
                    d: b,
                    rev: false,
                },
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
            loops.push(straddle(
                u0 + du,
                u1 - du,
                v0 + dv,
                v1 - dv,
                u_dir,
                rng.chance(0.8),
            ));
        }
        if rng.chance(0.3) {
            loops.push(span(rng.uniform(0.92, 0.98), u_dir, rng.chance(0.5), false));
        }
    } else {
        for (j, level) in levels(rng, n).into_iter().enumerate() {
            let alternate = j % 2 == 1;
            let backward = if rng.chance(0.85) {
                alternate
            } else {
                rng.chance(0.5)
            };
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

    /// #18342: the loops of a natural cylinder face 3.5e-3 tall (scale 1e-3) classify.
    /// Its complete seams send `ClassifyLoops` through the bisector, an axial line over
    /// the ±1234567 sentinel range; its two circle crossings, 1.43e-9 apart in the
    /// bisector's parameter, were merged into one boundary, so the touches were only
    /// [In, Out] and the loop tree had no root (`classifiable` 3, `NoLoopTreeRoot`).
    #[test]
    fn short_natural_cylinder_classifies_through_both_circle_crossings() {
        let i = Inputs {
            brep: BrepIn::Natural {
                surface: AnySrf::Cylinder {
                    scale: 1e-3,
                    a: 0.6278941237582317,
                    c: 3.538075172749348,
                    swap: false,
                },
            },
            validate_only: false,
            init_gaps: true,
            face: 32,
            props: 1,
            loops: 2,
        };
        let out = rust(&i).unwrap();
        assert_eq!(out.error, None);
        assert_eq!(out.ints.get("classifiable"), Some(&vec![0]));
        assert_eq!(
            out.ints.get("looptree").and_then(|t| t.first()),
            Some(&1),
            "{:?}",
            out.ints.get("looptree")
        );
    }

    /// `i` with its cylinder or cone `scale` multiplied by `f`.
    fn scaled(i: &Inputs, f: f64) -> Inputs {
        let mut i = i.clone();
        if let BrepIn::Curves { surface, .. } | BrepIn::Natural { surface } = &mut i.brep {
            if let AnySrf::Cylinder { scale, .. } | AnySrf::Cone { scale, .. } = surface {
                *scale *= f;
            }
        }
        i
    }

    /// `ClassifyLoops` of `i` gives the same answer as the same face 1000 times larger:
    /// there no two crossings lie within the parameter snap band of the bisector or of
    /// its ends, so the larger face is an independent reference.
    fn assert_same_as_scaled_up(i: &Inputs) -> Output {
        let out = rust(i).unwrap();
        let big = rust(&scaled(i, 1e3)).unwrap();
        assert_eq!(out.error, None);
        assert_eq!(big.error, None);
        assert_eq!(out.ints, big.ints, "scale {:?}", i.brep);
        assert_eq!(out.values, big.values, "scale {:?}", i.brep);
        out
    }

    /// #18342: `cyl_natural_1e-3_gen0157` itself (`swap: true`). Its bisector's ends leave
    /// the face across no seam image an edge use could bound, so their type is `None`
    /// (1), as for the same face at scale 1. Before the fix the two circle crossings
    /// merged into the bisector's ends and both were typed `SeamNoEdgeuse` (9).
    #[test]
    fn short_natural_cylinder_gen0157_bisector_ends_are_none() {
        let i = Inputs {
            brep: BrepIn::Natural {
                surface: AnySrf::Cylinder {
                    scale: 1e-3,
                    a: 0.6278941237582317,
                    c: 3.538075172749348,
                    swap: true,
                },
            },
            validate_only: false,
            init_gaps: true,
            face: 32,
            props: 1,
            loops: 2,
        };
        let out = assert_same_as_scaled_up(&i);
        assert_eq!(out.ints.get("classifiable"), Some(&vec![0]));
        assert_eq!(out.ints.get("start_stop"), Some(&vec![1, 1]));
    }

    /// #18342: two opposite u-isolines 1e-6 apart in normalized v on a cylinder (the
    /// faces of generated cases 84 and 331 with every loop as target), 3.1e-6 and 2.6e-6
    /// apart in 3D against a classification pair tolerance of 2e-7, are a no-area pair:
    /// each crossing keeps its own point of the bisector.
    #[test]
    fn close_opposite_isolines_classify_as_a_no_area_pair() {
        for (a, c, v0, v1, face) in [
            (0.7503165792421282, 3.1025197902685333, 0.08420134538839802, 0.08420234538839802, 50),
            (1.7416486248859733, 2.5778465526264056, 0.3680146569383668, 0.3680156569383668, 30),
        ] {
            let line = |a: f64, b: f64, c: f64, d: f64| {
                serde_json::json!({"t": "line", "a": a, "b": b, "c": c, "d": d, "rev": false})
            };
            let i: Inputs = serde_json::from_value(serde_json::json!({
                "brep": {
                    "kind": "curves",
                    "copies": 1,
                    "surface": {"kind": "cylinder", "scale": 1.0, "a": a, "c": c, "swap": false},
                    "loops": [[line(0.0, v0, 1.0, v0)], [line(1.0, v1, 0.0, v1)]],
                    "points": []
                },
                "init_gaps": false,
                "face": face,
                "props": 1,
                "loops": 1
            }))
            .unwrap();
            let out = assert_same_as_scaled_up(&i);
            assert_eq!(out.ints.get("classifiable"), Some(&vec![0]));
            assert_eq!(out.ints.get("ordered_no_area"), Some(&vec![0, 1]));
            assert_eq!(out.ints.get("start_stop"), Some(&vec![1, 1]));
        }
    }

    /// #18342: four parallel circles on a cone at scale 1e-3 (generated case 73) nest as
    /// they do on the same cone at scale 1. The seam classification's crossings lie
    /// 2.0e-9 to 2.1e-9 from its start in the curve's parameter but about 5e-3 from it
    /// in 3D; compared with the face's UV tolerance they were taken as touches on the
    /// curve's start (`Lamina`), so loop 3 dropped out of the tree.
    #[test]
    fn cone_parallel_circles_at_small_scale_nest_as_at_scale_one() {
        let mut rng = Rng::new(CaseContext::seed_for("query.face_p4", 73));
        let i = gen_case(&mut rng);
        let out = assert_same_as_scaled_up(&i);
        assert_eq!(out.ints.get("ordered_no_area"), Some(&vec![3, 2, 1, 0]));
    }

    /// #18342: a cone face at scale 1e-3 (generated case 381) gives the answer of the
    /// same face at scale 1. Loops 0 and 1 are opposite u-isolines 1e-9 apart in
    /// normalized v, each with one vertex on the U seam; the two vertices coincide to the
    /// scaled zero (gaps 2.2e-19 and 0, seam parameters 1.0e-18 apart). The seam
    /// classification used to keep the vertex with the smaller gap, rounding noise that
    /// gave the crossing to loop 1 at this scale and to loop 0 at scale 1; with loop 1
    /// crossed once, its seam walk had no entry and `ClassifyLoops` returned `SM_ERR`.
    /// Now the first vertex (loop 0's) is kept at both scales: loop 0 is the seam pass's
    /// no-area loop, the bisector meets loop 1 at no crossing (`NoClassifyZeroCrosscnt`),
    /// and the ends are the pole without a vertex and none. Stage 3 reads that as not yet
    /// healable (1040) at both scales, base's answer.
    #[test]
    fn coincident_seam_vertices_on_a_small_cone_classify_as_at_scale_one() {
        use solidcore::error::heal::HealFailure;
        use solidcore::heal::face_props::HealerOpType;
        let mut rng = Rng::new(CaseContext::seed_for("query.face_p4", 381));
        let i = gen_case(&mut rng);
        let out = assert_same_as_scaled_up(&i);
        assert_eq!(out.values.get("status"), Some(&vec![0.0]));
        assert_eq!(
            out.ints.get("classifiable"),
            Some(&vec![HealNotYetFaceType::NoClassifyZeroCrosscnt.code()])
        );
        assert_eq!(
            out.ints.get("start_stop"),
            Some(&vec![
                CrvDirOnFaceType::PoleNoVertex as i64,
                CrvDirOnFaceType::None as i64
            ])
        );
        assert_eq!(out.ints.get("ordered_no_area"), Some(&vec![0]));

        // the healer's view: Stage 3 of the same face, all its loops, at both scales
        for i in [i.clone(), scaled(&i, 1e3)] {
            let k = Kernel::default();
            let (mut b, _) = own_brep(&i.brep, i.validate_only, i.init_gaps)
                .unwrap()
                .unwrap();
            let fi = i.face as usize % b.faces().len();
            let face = FaceId::from_index(u32::try_from(fi).unwrap());
            let mut props: FaceProps<H> = FaceProps::for_face(face, fi as u64);
            let r = props.set_props_stage3_mut(&k, &mut b, face, fi as u64, HealerOpType::All);
            let e = r.unwrap_err();
            assert!(
                matches!(e.heal_failure(), Some(HealFailure::NotYetHealable)),
                "{e:?}"
            );
            assert!(props.has_not_yet_problems());
        }
    }

    /// Closed-form bounds on the distance between edges 0 and 1 of `i`'s B-rep, two full
    /// circles: `(lower, upper, zone_tol_3d)`. Every point of edge 0 is at least `lower`
    /// from edge 1, and each point of either lies within `upper` of the other.
    ///
    /// For circles `c_i + r_i u_i(θ)`, `u_i = cos θ x_i + sin θ y_i`, the points at the
    /// same angle θ differ by `Δc + Δr u_0 + r1 (u_0 - u_1)` (`Δc = c0 - c1`,
    /// `Δr = r0 - r1`). As `|u_0| = 1` and `Δc·u_0 = Δc_⊥0·u_0` (`Δc_⊥0` the part of `Δc`
    /// in circle 0's plane), `|Δc + Δr u_0|² <= |Δc|² + Δr² + 2 |Δr| |Δc_⊥0|`, and
    /// `|u_0 - u_1| <= |x0 - x1| + |y0 - y1|`: the root of the first plus `r1` times the
    /// second bounds the distance. With `n` the unit axis of circle 1 and
    /// `e = |x0·n| + |y0·n|` (circle 0's tilt to it), a point `p` of circle 0 lies
    /// `|(p - c1)·n| >= |(c0 - c1)·n| - r0 e` off circle 1's plane and at a distance from
    /// its axis within `|r0 - r1| - |(c0 - c1)_⊥| - r0 e` of `r1`; either is a lower
    /// bound on its distance to circle 1. The rounding of these few products and sums is
    /// a few ulps (1e-16 relative) of the largest term, far below the closest margin
    /// asserted: at ×10 the upper bound is 9.945e-9 against 1e-8, 0.55% below it.
    ///
    /// Premises asserted: both edge curves are ellipses with equal radii (circles), and
    /// each edge is the whole circle: a positive interval whose ends evaluate to the same
    /// point, on a 360-degree domain.
    fn circle_gap_bounds(i: &Inputs) -> (f64, f64, f64) {
        use solidcore::curve::curve::p2::CurveKind;
        use solidcore::topology::edge as topo_edge;
        let k = Kernel::default();
        let (mut b, _) = own_brep(&i.brep, i.validate_only, i.init_gaps).unwrap().unwrap();
        let (_face, props, _loops) = setup(&k, &mut b, i).unwrap().unwrap();
        let edges = solidcore::topology::brep::p1::edges(&b, None).unwrap();
        let circles: Vec<(Point3, Point3, Point3, f64)> = edges
            .iter()
            .take(2)
            .map(|&e| {
                let c = topo_edge::evaluation_curve(&b, e).unwrap().unwrap();
                let iv = topo_edge::interval(&b, e).unwrap();
                let CurveKind::Ellipse(el) = c.kind() else { panic!("edge {e:?} is no ellipse") };
                let can = el.canonical();
                assert_eq!(can.radius_x, can.radius_y, "edge {e:?} is no circle");
                assert_eq!(can.domain.max() - can.domain.min(), 360.0, "edge {e:?}");
                assert!(iv.max() > iv.min(), "edge {e:?}");
                let (a, z) = (c.evaluate_point(&k, iv.min()).unwrap(), c.evaluate_point(&k, iv.max()).unwrap());
                assert!(a.distance(z) <= 1e-12 * can.radius_x, "edge {e:?} is no whole circle");
                let pos = can.position;
                (pos.origin(), pos.x_axis(), pos.y_axis(), can.radius_x)
            })
            .collect();
        let [(c0, x0, y0, r0), (c1, x1, y1, r1)] = circles[..] else { panic!("two edges") };
        let dc = c0 - c1;
        let n0 = x0.cross(y0);
        let n0 = n0 * (1.0 / n0.length());
        let dr = (r0 - r1).abs();
        let in_plane0 = (dc - n0 * dc.dot(n0)).length();
        let upper = (dc.dot(dc) + dr * dr + 2.0 * dr * in_plane0).sqrt()
            + r1 * ((x0 - x1).length() + (y0 - y1).length());
        let n = x1.cross(y1);
        let n = n * (1.0 / n.length());
        let tilt = x0.dot(n).abs() + y0.dot(n).abs();
        let axial = dc.dot(n).abs();
        let radial = (dc - n * dc.dot(n)).length();
        let lower = (axial - r0 * tilt).max((r0 - r1).abs() - radial - r0 * tilt);
        (lower, upper, props.zone_tol_3d)
    }

    /// #18342: generated case 309, a cone at scale 1e-3 whose loops 0 and 1 are two
    /// circles 9.9e-10 apart (a hundredth of the face's 1e-7 zone tolerance). It gives
    /// the answer of the same face 3 and 10 times larger, where the circles are still
    /// within a tenth of the tolerance: `NotYet` with both bisector ends typed as at every
    /// larger scale ([8, 1]). The face 1000 times larger is no reference here: its
    /// circles are 9.9e-7 apart, ten times the tolerance, and classify as distinct loops.
    /// Before the fix the coincident seam vertices of loops 0 and 1 picked the classified
    /// vertex by rounding noise, and the face at scales 1 and 3 was refused (`SM_ERR`).
    #[test]
    fn coincident_circles_on_a_small_cone_gen0309_classify_as_at_three_and_ten_times() {
        let mut rng = Rng::new(CaseContext::seed_for("query.face_p4", 309));
        let i = gen_case(&mut rng);
        let out = rust(&i).unwrap();
        assert_eq!(out.error, None);
        assert_eq!(out.values.get("status"), Some(&vec![0.0]));
        assert_eq!(out.ints.get("classifiable"), Some(&vec![HealNotYetFaceType::NoClassifyZeroCrosscnt.code()]));
        assert_eq!(
            out.ints.get("start_stop"),
            Some(&vec![CrvDirOnFaceType::PoleNoVertex as i64, CrvDirOnFaceType::None as i64])
        );
        for f in [1.0, 3.0, 10.0] {
            let (_, gap, tol) = circle_gap_bounds(&scaled(&i, f));
            assert!(gap < 0.1 * tol, "x{f}: {gap:e} {tol:e}");
            if f > 1.0 {
                let o = rust(&scaled(&i, f)).unwrap();
                assert_eq!(o.error, None);
                assert_eq!(o.ints, out.ints, "x{f}");
            }
        }
        let (gap, _, tol) = circle_gap_bounds(&scaled(&i, 1e3));
        assert!(gap > 5.0 * tol, "x1e3: {gap:e} {tol:e}");
    }

    /// #18342: generated case 22 (a cone at scale 1e-3) gives the answer of the same face
    /// at scale 1: its start is the pole without a vertex (`PoleNoVertex`, 8), not the
    /// `Pole` (7) of a loop vertex that the parameter snap merged into the start.
    #[test]
    fn small_cone_gen0022_start_is_the_pole_without_a_vertex_as_at_scale_one() {
        let mut rng = Rng::new(CaseContext::seed_for("query.face_p4", 22));
        let out = assert_same_as_scaled_up(&gen_case(&mut rng));
        assert_eq!(
            out.ints.get("start_stop").and_then(|s| s.first()),
            Some(&(CrvDirOnFaceType::PoleNoVertex as i64))
        );
    }

    /// #18342: generated case 247 (a natural cylinder at scale 1e-3) gives the answer of
    /// the same face at scale 1, `scratch_size` included: its boundary loop crosses no v
    /// seam, so `ClassifyLoops` builds the bisector, whose final reset leaves one
    /// interval. Before the fix two snapped crossings sent it down the seam path (0).
    #[test]
    fn natural_cylinder_gen0247_uses_the_bisector_as_at_scale_one() {
        let mut rng = Rng::new(CaseContext::seed_for("query.face_p4", 247));
        let out = assert_same_as_scaled_up(&gen_case(&mut rng));
        assert_eq!(out.ints.get("scratch_size"), Some(&vec![1]));
        assert_eq!(out.ints.get("classifiable"), Some(&vec![0]));
    }

    /// A circular UV trim spans both axes and winds in the requested direction.
    #[test]
    fn circular_trims_keep_their_uv_bounds_and_orientation() {
        use super::super::topology_edgeuse_p1::BssIn;
        use solidcore::topology::face::p2::loops;
        use solidcore::topology::face::p3::compute_loop_orientation;

        for rev in [false, true] {
            let mut k = Kernel::default();
            let mut b = Brep::new_with_infinite_region().unwrap();
            let surface = AnySrf::Bspline {
                surface: SrfIn::Bspline(BssIn {
                    oob: false,
                    form: 0,
                    net: vec![
                        vec![[0.0, 0.0, 0.0, 1.0], [0.0, 1.0, 0.0, 1.0]],
                        vec![[1.0, 0.0, 0.0, 1.0], [1.0, 1.0, 0.0, 1.0]],
                    ],
                    p: 1,
                    q: 1,
                    u: vec![0.0, 0.0, 1.0, 1.0],
                    v: vec![0.0, 0.0, 1.0, 1.0],
                }),
            };
            // A unit square's UV domain: centre (0.3, 0.5), radius 0.1.
            let trims = vec![vec![CurveIn::Circle {
                a: 0.3,
                b: 0.5,
                r: 0.1,
                rev,
            }]];
            curves_face(&mut k, &mut b, &surface, &trims, &[])
                .unwrap()
                .unwrap();
            assert_eq!(b.faces().len(), 1);
            let face = FaceId::from_index(0);
            let loops = loops(&b, face, false, None).unwrap();
            assert_eq!(loops.len(), 1);
            let mut bounds = Extent2d::new();
            let orientation = compute_loop_orientation(
                &k,
                &mut b,
                face,
                loops[0],
                Some(&mut bounds),
                false,
            )
            .unwrap();
            assert!(
                (bounds.min().x - 0.2).abs() < 1e-12
                    && (bounds.max().x - 0.4).abs() < 1e-12
                    && (bounds.min().y - 0.4).abs() < 1e-12
                    && (bounds.max().y - 0.6).abs() < 1e-12,
                "rev={rev}: circular trim bounds {bounds:?}"
            );
            assert_eq!(
                orientation,
                if rev { OrientType::Opposite } else { OrientType::Same }
            );
        }
    }

    /// Most generated cases reach a real classification (not a build failure or a
    /// rejection).
    #[test]
    fn generated_cases_classify() {
        let mut rng = Rng::new(11);
        let mut why: BTreeMap<String, usize> = BTreeMap::new();
        let n = 200;
        let mut classified = 0;
        for _ in 0..n {
            let i = gen_case(&mut rng);
            let c = rust(&i).unwrap();
            let key = if let Some(e) = &c.error {
                let kind = serde_json::to_value(&i.brep).unwrap()["kind"].clone();
                format!("error {e} {kind} props {} loops {}", i.props, i.loops)
            } else if let Some(d) = &c.deferred {
                format!("deferred {}", d.unit)
            } else {
                let ios = c.ints.get("classify_loop_ios").map_or(0, Vec::len);
                *why.entry(format!("  start_stop {:?}", c.ints.get("start_stop")))
                    .or_default() += 1;
                for ch in c
                    .ints
                    .get("classify_loop_ios")
                    .cloned()
                    .unwrap_or_default()
                    .chunks(6)
                {
                    *why.entry(format!("  io type/orient/area/cont {:?}", ch.get(1..)))
                        .or_default() += 1;
                }
                *why.entry(format!(
                    "  tree len {:?}",
                    c.ints.get("looptree").and_then(|t| t.first())
                ))
                .or_default() += 1;
                if ios > 0 {
                    classified += 1;
                }
                format!(
                    "status {} classifiable {:?} ios>0 {}",
                    c.values
                        .get("status")
                        .and_then(|v| v.first())
                        .copied()
                        .unwrap_or(-1.0),
                    c.ints.get("classifiable"),
                    ios > 0
                )
            };
            *why.entry(key).or_default() += 1;
        }
        for (k, v) in &why {
            eprintln!("{v:4} {k}");
        }
        assert!(
            classified * 2 > n,
            "only {classified} of {n} classify loops"
        );
    }
}
