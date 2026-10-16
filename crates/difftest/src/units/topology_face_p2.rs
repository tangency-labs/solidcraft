//! Runner for unit `topology.face_p2`. C++: `solidcore_ffi::units::topology_face_p2`.
//! Rust: `solidcore::topology::face::p2`.
//!
//! **B-rep ops.** As the `topology.vertexuse` runner, the C++ side builds a real
//! `SmBrep` per call: read from an upstream `.smb` test file (optionally rebuilding its
//! UV trim curves), a wire edge from a B-spline curve (`CreateWireEdgeFromCurve`), a face
//! from a B-spline surface or a real `SmPlane` (`CreateFaceFromSurface`), or the wire
//! network of `prog_test` `my_test_topology`. There is no Rust B-rep reader or builder
//! yet (units `topology.brep_*`), so the Rust side imports the graph the shim exports in
//! traversal order (raw members, including each face's face use, UV domain,
//! rectangular-trim flag, tolerance and surface owner, `SmPlane`s as planes, and each
//! vertex use's cached vertex/face gap) into a [`Brep`] and runs the ported functions on
//! it; results are compared as traversal indices, flags, statuses, values and curve
//! blocks (the shim's `put_curve` layout). Selectors are taken modulo the entity count
//! (`-1` selecting null where allowed).
//!
//! **Inexact geometry.** The export carries plain B-spline surfaces, `SmPlane`s,
//! B-spline curves and `SmLine`s exactly; another `SmBSplineSurface` subclass (the
//! revolution, extrusion, cone, sphere and torus surfaces of several upstream files) or
//! `SmBSplineCurve` subclass (an edge's ellipse) arrives as its B-spline part only. Ops
//! that evaluate such a face surface (`Points`, `FixUv`, `BestUv`, `Connected`) or, for
//! `BestUv`, such an edge curve are not compared: both sides report `inexact_geometry`.
//!
//! **`GetPointsInFace` with more than one point** draws its V dither from
//! `srand(time(NULL))` / `rand()` upstream and from `kernel.rng` in the port
//! (`plan/findings.md`); such cases compare what does not depend on the draw: the
//! point count, the U values and, on a rectangular face, each V's grid slot
//! (`round(v / dVInc)`, the dither being under half a slot).
//!
//! **Geometry ops** (`sm_CheckSeamCrossing`, `SmPrecisePropertyLayout`,
//! `SmODEIsoCurveLengthIFEO::Evaluate`, `SmODEAreaIFEO::Evaluate`, verbatim copies in the
//! shim) run on B-spline surfaces and real `SmPlane`s and B-spline UV curves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::ObjectKey;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::solution_array::ObjectKind;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::p1::CurveClassification;
use solidcore::intersect::point_class::{ClassObject, ClassObjectKind, PointClassType};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::{DropCurveFail, B_SPLINE_SURFACE_TYPE};
use solidcore::surface::surface::Surface;
use solidcore::topology::face::p2 as fp2;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId,
    Loop, LoopId, Loopuse, LoopuseId, LoopuseStart, ObjectHandle, Region, RegionId, Shell,
    ShellId, Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::SolidError;
use solidcore_ffi::units::topology_face_p2::{self as ffi, Op};

use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmTopoTypes.h` codes.
const SHELL_TYPE: i64 = 16013;
const LOOPUSE_TYPE: i64 = 16004;
const EDGEUSE_TYPE: i64 = 16005;
const VERTEXUSE_TYPE: i64 = 16006;

/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [16002, 16005, 16001, 16010, 12060, 11001, 16013, 16022, 16999];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// Generated cases beyond the default `--seeds` (the B-rep ops are cheap).
const EXTRA_GENERATED: usize = 1500;

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve: `dim`, control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvIn {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default)]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
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
    Wire { curve: CrvIn, t0: f64, t1: f64 },
    Face { surface: SrfIn, dom: [f64; 4] },
    Network,
}

/// A `sm_FindBestUVCurve` candidate: `kind` 0 null, 1 `curve`, 2 a copy of the edge
/// use's own UV trim curve; `orient` an `SmOrientType`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cand {
    kind: u8,
    #[serde(default)]
    curve: Option<CrvIn>,
    orient: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    FaceQuery {
        f: i64,
        tgt_only: bool,
        has_loops: bool,
        loops: Vec<i64>,
    },
    Set {
        f: i64,
        dom: [f64; 4],
        fu: i64,
    },
    Points {
        f: i64,
        count: u64,
        /// A UV domain set on the face first (`SetUVDomain`).
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    Internal {
        f: i64,
    },
    FromFace {
        f: i64,
        orient: u8,
        want_3d: bool,
        want_uv: bool,
    },
    Trim {
        f: i64,
        tol: Option<f64>,
        curves_3d: Option<Vec<Option<CrvIn>>>,
        uv_curves: Option<Vec<Option<CrvIn>>>,
        want_3d: bool,
        want_uv: bool,
    },
    FixUv {
        eu: i64,
        prev: i64,
        vertex: i64,
        tol: f64,
        dist: f64,
        face_dom: bool,
        dom: [f64; 4],
    },
    BestUv {
        eu: i64,
        cands: Vec<Cand>,
    },
    Connected {
        eu1: i64,
        eu2: i64,
        singularity_ok: bool,
        seam_ok: bool,
    },
    Rescue {
        f: i64,
        to_null: bool,
    },
    RemoveSuspect {
        ends: Vec<[i64; 2]>,
    },
    SeamCross {
        uv1: [f64; 2],
        uv2: [f64; 2],
        dom: [f64; 4],
        closed_u: bool,
        closed_v: bool,
        shift: [f64; 2],
    },
    Layout {
        flags: u64,
    },
    OdeIso {
        surface: SrfIn,
        u: f64,
        t: f64,
        origin: [f64; 3],
        thickness: f64,
        flags: u64,
        n: usize,
    },
    OdeArea {
        surface: SrfIn,
        curve: CrvIn,
        t: f64,
        min_v: f64,
        accuracy: f64,
        y_scale: Vec<f64>,
        origin: [f64; 3],
        thickness: f64,
        flags: u64,
        dydx: Vec<f64>,
    },
}

impl OpIn {
    fn is_brep(&self) -> bool {
        !matches!(
            self,
            Self::SeamCross { .. } | Self::Layout { .. } | Self::OdeIso { .. } | Self::OdeArea { .. }
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

// ---- encoding ---------------------------------------------------------------------------

fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("curve without points"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("curve without knots"))?;
    v.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        v.extend(p);
    }
    v.extend(&c.u);
    Ok(())
}

fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline(b) => {
            let n = b.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            v.extend([
                0.0,
                f64::from(u8::from(b.oob)),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                (b.u.len() - 1) as f64,
                (b.v.len() - 1) as f64,
            ]);
            for row in &b.net {
                for p in row {
                    v.extend(p);
                }
            }
            v.extend(&b.u);
            v.extend(&b.v);
        }
    }
    Ok(())
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
fn path_of(i: &Inputs) -> Result<String, HarnessError> {
    match &i.brep {
        Some(BrepIn::File { file, .. }) => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

/// The B-rep block.
fn brep_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    let brep = i.brep.as_ref().ok_or_else(|| harness("B-rep op without B-rep"))?;
    match brep {
        BrepIn::File { rebuild, .. } => {
            v.push(0.0);
            v.push(flag(*rebuild));
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
    }
    v.push(flag(i.validate_only));
    v.push(flag(i.init_gaps));
    Ok(v)
}

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
fn sel(s: i64, n: usize) -> i64 {
    if s < 0 || n == 0 {
        -1
    } else {
        (s as u64 % n as u64) as i64
    }
}

/// The C++ export of the case's B-rep.
fn export(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let path = path_of(i)?;
    ffi::eval(Op::Export, &path, &brep_block(i)?).map_err(|e| harness(format!("c++ export: {e}")))
}

/// A build failure as an operation error.
fn build_failure(export: &[f64]) -> Output {
    Output::err(format!("brep_build_status_{}", export.get(1).copied().unwrap_or(0.0)))
}


// ---- import -----------------------------------------------------------------------------

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
            .ok_or_else(|| harness("export too short"))?;
        self.at += 1;
        Ok(x)
    }
    fn idx(&mut self) -> Result<i64, HarnessError> {
        Ok(self.num()? as i64)
    }
    fn usize(&mut self) -> Result<usize, HarnessError> {
        usize::try_from(self.idx()?).map_err(|_| harness("negative count"))
    }
    fn list(&mut self) -> Result<Vec<i64>, HarnessError> {
        let n = self.usize()?;
        (0..n).map(|_| self.idx()).collect()
    }
}

fn opt<T>(i: i64, f: impl Fn(u32) -> T) -> Result<Option<T>, HarnessError> {
    match i {
        -1 => Ok(None),
        x if x >= 0 => Ok(Some(f(u32::try_from(x).map_err(|_| harness("index overflow"))?))),
        x => Err(harness(format!("unregistered pointer ({x}) in export"))),
    }
}

fn orient_of(code: i64) -> Result<OrientType, HarnessError> {
    match code {
        0 => Ok(OrientType::Unknown),
        1 => Ok(OrientType::Same),
        2 => Ok(OrientType::Opposite),
        x => Err(harness(format!("bad orientation {x}"))),
    }
}

/// An exported curve block (`None` for `-1`). A B-spline or line is rebuilt exactly;
/// another `SmBSplineCurve` kind is an error for a UV trim curve (`uv`) and its B-spline
/// part for an edge curve (edge curves are only evaluated on deferred paths).
fn read_curve(rd: &mut Rd<'_>, uv: bool) -> Result<Option<SmCurve>, HarnessError> {
    let ty = rd.num()?;
    if ty == -1.0 {
        return Ok(None);
    }
    if ty < 0.0 {
        return Err(harness("export: a curve is not an SmBSplineCurve"));
    }
    let ty = ty as i64;
    let dim = rd.num()? as u64;
    let has_nurb = rd.num()? != 0.0;
    let base = if has_nurb {
        let n = rd.usize()?;
        let p = rd.usize()?;
        let m = rd.usize()?;
        let mut pw = Vec::new();
        for _ in 0..=n {
            pw.push(CPoint::new(rd.num()?, rd.num()?, rd.num()?, rd.num()?));
        }
        let mut u = Vec::new();
        for _ in 0..=m {
            u.push(rd.num()?);
        }
        let cur = Curve::new(pw, p, u).map_err(|e| harness(format!("export curve: {e}")))?;
        BSplineCurve::from_nurb(cur, dim, false)
    } else {
        BSplineCurve::new(dim)
    };
    if ty == LINE_TYPE {
        let point = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let vector = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let a = rd.num()?;
        let b = rd.num()?;
        let scale = rd.num()?;
        let inside_out = rd.num()? != 0.0;
        let dom = Extent1d::from_raw(a, b);
        return Ok(Some(SmCurve::from(Line::from_parts(
            base, point, vector, dom, scale, inside_out,
        ))));
    }
    if ty != BSPLINE_CURVE_TYPE && uv {
        return Err(harness(format!("export: UV trim curve of type {ty}")));
    }
    Ok(Some(SmCurve::from(base)))
}

fn read_surface(rd: &mut Rd<'_>) -> Result<(Option<Surface>, bool), HarnessError> {
    let kind = rd.num()?;
    if kind == -1.0 {
        return Ok((None, true));
    }
    if kind == 1.0 {
        // an SmPlane: origin, axes, scales, analytic domain
        let o = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let x = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let y = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let sc = Vec2::new(rd.num()?, rd.num()?);
        let d = Extent2d::from_bounds(rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        return Ok((Some(Surface::from(Plane::new(o, x, y, sc, &d))), true));
    }
    if kind != 0.0 {
        return Err(harness("export: a surface is not a B-spline or plane"));
    }
    // an SmBSplineSurface subclass other than SmPlane: only its B-spline part is
    // imported, so its evaluations are not the upstream ones
    let exact = rd.num()? == B_SPLINE_SURFACE_TYPE as f64;
    let oob = rd.num()? != 0.0;
    let form = rd.num()?;
    let n = rd.usize()?;
    let m = rd.usize()?;
    let p = rd.usize()?;
    let q = rd.usize()?;
    let r = rd.usize()?;
    let s = rd.usize()?;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            row.push(CPoint::new(rd.num()?, rd.num()?, rd.num()?, rd.num()?));
        }
        net.push(row);
    }
    let u = (0..=r).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..=s).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
    let nl = NlSurface::new(net, p, q, u, v).map_err(|e| harness(format!("export surface: {e}")))?;
    let mut b = BSplineSurface::from_nurb(nl, false);
    *b.out_of_bounds_enabled_mut() = oob;
    let form = BSplineSurfaceForm::from_code(form as u64).ok_or_else(|| harness("bad surface form"))?;
    b.set_b_spline_surface_form(form);
    Ok((Some(Surface::from(b)), exact))
}

/// Whether each face's surface and each edge's curve was imported exactly (an
/// `SmBSplineSurface` subclass other than `SmPlane`, or an `SmBSplineCurve` subclass
/// other than `SmLine`, is imported as its B-spline part).
pub(crate) struct Exact {
    pub(crate) faces: Vec<bool>,
    pub(crate) edges: Vec<bool>,
}

/// Builds the Rust B-rep from the shim's export (see the module docs; shared with unit
/// `query.face_p2`, whose shim exports the same layout).
pub(crate) fn import(ex: &[f64]) -> Result<(Brep, Exact), HarnessError> {
    let c = ffi::counts(ex).ok_or_else(|| harness("import of a failed build"))?;
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let mut rd = Rd { v: ex, at: 1 };
    let validate_only = rd.num()? != 0.0;
    rd.at = 12;

    // Region shell lists, shell types and face-use lists as exported (void flags
    // are not exported and stay 0); wire/vertex shell heads are set after import.
    let mut regions = Vec::new();
    for _ in 0..n_r {
        let in_brep = rd.num()? != 0.0;
        let shells = super::topology_vertexuse::region_shell_list(rd.list()?)?;
        regions.push(Region {
            in_brep,
            shells,
            ..Region::default()
        });
    }
    let mut shells = Vec::new();
    for _ in 0..n_s {
        let region = opt(rd.idx()?, RegionId::from_index)?;
        let (shell_type, members) =
            super::topology_vertexuse::shell_members(rd.idx()?, rd.list()?)?;
        shells.push(Shell {
            faces: Vec::new(),
            region,
            shell_type,
            members,
        });
    }
    let mut faceuses = Vec::new();
    for _ in 0..n_fu {
        let shell = opt(rd.idx()?, ShellId::from_index)?;
        let face = opt(rd.idx()?, FaceId::from_index)?;
        let mate = opt(rd.idx()?, FaceuseId::from_index)?;
        let orientation = orient_of(rd.idx()?)?;
        let loopuses = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, LoopuseId::from_index)?.ok_or_else(|| harness("null loop use")))
            .collect::<Result<Vec<_>, _>>()?;
        faceuses.push(Faceuse {
            shell,
            face,
            mate,
            orientation,
            loopuses,
            sibling_links: None,
        });
    }
    let mut surfaces = Vec::new();
    let mut exact_faces = Vec::new();
    for f in 0..n_f {
        let (mut surface, exact) = read_surface(&mut rd)?;
        exact_faces.push(exact);
        let faceuse = opt(rd.idx()?, FaceuseId::from_index)?;
        let dom = Extent2d::from_bounds(rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        let rect = rd.idx()?;
        let tolerance = rd.num()?;
        let owned = rd.num()? != 0.0;
        if owned {
            if let Some(s) = surface.as_mut() {
                s.set_owner(Some(ObjectRef {
                    type_code: fp2::TYPE,
                    key: ObjectKey(1 + f as u64),
                }));
            }
        }
        surfaces.push((surface, faceuse, dom, rect, tolerance));
    }
    let mut loopuses = Vec::new();
    for _ in 0..n_lu {
        let faceuse = opt(rd.idx()?, FaceuseId::from_index)?;
        let loop_ = opt(rd.idx()?, LoopId::from_index)?;
        let mate = opt(rd.idx()?, LoopuseId::from_index)?;
        let ty = rd.idx()?;
        let start = rd.idx()?;
        let start = match ty {
            EDGEUSE_TYPE => LoopuseStart::Edgeuse(opt(start, EdgeuseId::from_index)?),
            VERTEXUSE_TYPE => LoopuseStart::Vertexuse(opt(start, VertexuseId::from_index)?),
            _ => LoopuseStart::Unknown,
        };
        let orientation = orient_of(rd.idx()?)?;
        loopuses.push(Loopuse {
            faceuse,
            loop_,
            mate,
            start,
            orientation,
        });
    }
    let mut loop_lus = Vec::new();
    for _ in 0..n_l {
        loop_lus.push(opt(rd.idx()?, LoopuseId::from_index)?);
    }
    let mut edgeuses = Vec::new();
    for _ in 0..n_eu {
        let edge = opt(rd.idx()?, EdgeId::from_index)?;
        let ty = rd.idx()?;
        let sorlu = rd.idx()?;
        let owner = match ty {
            LOOPUSE_TYPE => EdgeuseOwner::Loopuse(opt(sorlu, LoopuseId::from_index)?),
            SHELL_TYPE => EdgeuseOwner::Shell(opt(sorlu, ShellId::from_index)?),
            _ => EdgeuseOwner::Unknown,
        };
        let cw = opt(rd.idx()?, EdgeuseId::from_index)?;
        let ccw = opt(rd.idx()?, EdgeuseId::from_index)?;
        let vertexuse = opt(rd.idx()?, VertexuseId::from_index)?;
        let orientation = orient_of(rd.idx()?)?;
        let uv_trim_curve = read_curve(&mut rd, true)?;
        rd.idx()?; // the curve's owner: the holding edge use
        let has_fail = rd.num()? != 0.0;
        let fail_status = rd.idx()?;
        let fail_brep = rd.num()? != 0.0;
        let ef_init = rd.num()? != 0.0;
        let ccw_init = rd.num()? != 0.0;
        let mut r = Edgeuse {
            edge,
            owner,
            ccw,
            cw,
            vertexuse,
            orientation,
            uv_trim_curve,
            ..Edgeuse::default()
        };
        let me = EdgeuseId::from_index(u32::try_from(edgeuses.len()).map_err(|_| harness("overflow"))?);
        if has_fail {
            let f = DropCurveFail::<ObjectHandle> {
                fail_status: i32::try_from(fail_status).map_err(|_| harness("fail status"))?,
                brep: fail_brep.then_some(ObjectHandle {
                    kind: ObjectKind::Other,
                    key: ObjectKey(0),
                }),
                ..DropCurveFail::default()
            };
            r.drop_curve_fail = Some(Box::new(f));
        }
        if ef_init {
            // the gap value itself is never compared here
            r.max_edge_face_gap.set_edgeuse(Some(me));
        }
        if ccw_init {
            r.edge_ccw_edge_gap.edgeuse0 = Some(me);
            r.edge_ccw_edge_gap.edgeuse1 = ccw.or(Some(me));
        }
        edgeuses.push(r);
    }
    let mut edges = Vec::new();
    let mut exact_edges = Vec::new();
    for _ in 0..n_e {
        let tolerance = rd.num()?;
        let a = rd.num()?;
        let b = rd.num()?;
        let list = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, EdgeuseId::from_index)?.ok_or_else(|| harness("null edge use")))
            .collect::<Result<Vec<_>, _>>()?;
        let exact_curve = matches!(rd.v.get(rd.at).copied(), Some(t) if t == -1.0 || t == BSPLINE_CURVE_TYPE as f64 || t == LINE_TYPE as f64);
        exact_edges.push(exact_curve);
        let curve = read_curve(&mut rd, false)?;
        let interval = Extent1d::from_raw(a, b);
        edges.push((tolerance, interval, list, curve));
    }
    let mut vertexuses = Vec::new();
    for _ in 0..n_vu {
        let vertex = opt(rd.idx()?, VertexId::from_index)?;
        let ty = rd.idx()?;
        let own = rd.idx()?;
        let owner = match ty {
            EDGEUSE_TYPE => VertexuseOwner::Edgeuse(opt(own, EdgeuseId::from_index)?),
            LOOPUSE_TYPE => VertexuseOwner::Loopuse(opt(own, LoopuseId::from_index)?),
            SHELL_TYPE => VertexuseOwner::Shell(opt(own, ShellId::from_index)?),
            _ => VertexuseOwner::Unknown,
        };
        let face_init = rd.num()? != 0.0;
        let edge_init = rd.num()? != 0.0;
        let uv_point = Point2::new(rd.num()?, rd.num()?);
        let face_uv = Point2::new(rd.num()?, rd.num()?);
        let face_gap = rd.num()?;
        let me = VertexuseId::from_index(
            u32::try_from(vertexuses.len()).map_err(|_| harness("overflow"))?,
        );
        let mut r = Vertexuse {
            vertex,
            owner,
            uv_point,
            ..Vertexuse::default()
        };
        // the gap values themselves are never compared here
        if face_init {
            r.vertex_face_gap.vertexuse = Some(me);
            r.vertex_face_gap.face_uv = face_uv;
            r.vertex_face_gap.gap_3d = face_gap;
        }
        if edge_init {
            r.vertex_edge_gap.vertexuse = Some(me);
        }
        vertexuses.push(r);
    }
    let mut vertices = Vec::new();
    for _ in 0..n_v {
        let point = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let tolerance = rd.num()?;
        vertices.push(Vertex {
            point,
            tolerance,
            ..Vertex::default()
        });
    }
    let infinite_region = super::topology_vertexuse::read_shell_state(
        ex,
        &mut rd.at,
        &mut regions,
        &mut shells,
        &mut vertices,
        &mut faceuses,
    )?;
    if rd.at != ex.len() {
        return Err(harness("export: trailing data"));
    }

    // assemble (skeleton fields filled minimally)
    let mut b = Brep::new();
    b.set_validate_only(validate_only);
    b.set_infinite_region_record(infinite_region);
    for r in regions {
        b.add_region_record(r).map_err(|e| harness(e.to_string()))?;
    }
    let face_shell = |f: usize| -> ShellId {
        faceuses
            .iter()
            .find(|fu| fu.face == Some(FaceId::from_index(f as u32)) && fu.shell.is_some())
            .and_then(|fu| fu.shell)
            .unwrap_or(ShellId::from_index(0))
    };
    let face_shells: Vec<ShellId> = (0..n_f).map(face_shell).collect();
    for (i, mut s) in shells.into_iter().enumerate() {
        s.faces = face_shells
            .iter()
            .enumerate()
            .filter(|(_, sh)| sh.index() as usize == i)
            .map(|(f, _)| FaceId::from_index(f as u32))
            .collect();
        b.add_shell_record(s).map_err(|e| harness(e.to_string()))?;
    }
    for (f, (surface, faceuse, uv_domain, rect, tolerance)) in surfaces.into_iter().enumerate() {
        let shell = face_shells.get(f).copied().unwrap_or(ShellId::from_index(0));
        b.add_face_record(Face {
            surface,
            faceuse,
            uv_domain,
            rectangular_trim: i32::try_from(rect).map_err(|_| harness("rectangular trim flag"))?,
            tolerance,
            ..Face::new(shell)
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for lu in &loop_lus {
        let face = lu
            .and_then(|x| loopuses.get(x.index() as usize))
            .and_then(|l| l.faceuse)
            .and_then(|fu| faceuses.get(fu.index() as usize))
            .and_then(|fu| fu.face)
            .unwrap_or(FaceId::from_index(0));
        b.add_loop_record(Loop {
            face,
            edges: Vec::new(),
            loopuse: *lu,
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for (tolerance, interval, list, curve) in edges {
        let vertex_of = |eu: Option<&EdgeuseId>| -> Option<VertexId> {
            let vu = edgeuses.get(eu?.index() as usize)?.vertexuse?;
            vertexuses.get(vu.index() as usize)?.vertex
        };
        let v0 = vertex_of(list.first()).unwrap_or(VertexId::from_index(0));
        let v1 = vertex_of(list.get(1)).unwrap_or(v0);
        b.add_edge_record(Edge {
            vertices: [v0, v1],
            edgeuses: list,
            interval,
            tolerance,
            curve,
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for v in vertices {
        b.add_vertex_record(v).map_err(|e| harness(e.to_string()))?;
    }
    for fu in faceuses {
        b.add_faceuse_record(fu).map_err(|e| harness(e.to_string()))?;
    }
    for lu in loopuses {
        b.add_loopuse_record(lu).map_err(|e| harness(e.to_string()))?;
    }
    for eu in edgeuses {
        b.add_edgeuse_record(eu).map_err(|e| harness(e.to_string()))?;
    }
    for vu in vertexuses {
        b.add_vertexuse_record(vu).map_err(|e| harness(e.to_string()))?;
    }
    Ok((
        b,
        Exact {
            faces: exact_faces,
            edges: exact_edges,
        },
    ))
}

// ---- generation -------------------------------------------------------------------------

/// The upstream `.smb` files used (all B-spline geometry).
/// (`SmallBox.smb` and `Platform.smb` crash upstream's `SmBrep::ReadFromFile`; see
/// `plan/findings.md`.)
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
    let mut ks: Vec<f64> = (0..inner).map(|_| a + (b - a) * rng.uniform(0.05, 0.95)).collect();
    ks.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(ks);
    u.extend(vec![b; p + 1]);
    u
}

fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            let k = if rational { w } else { 1.0 };
            let x = (i as f64 + rng.uniform(-0.3, 0.3)) * scale;
            let y = rng.uniform(-1.0, 1.0) * scale;
            let z = if dim == 2 { NOZ } else { rng.uniform(-1.0, 1.0) * scale * k };
            [x * k, y * k, z, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) { (0.0, 1.0) } else { (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0)) };
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

fn gen_surface(rng: &mut Rng) -> SrfIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    if rng.chance(0.25) {
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let pinch = rng.chance(0.15);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (x, y) = if pinch && i == 0 { (0.0, 0.0) } else { (i as f64, j as f64) };
                    [x * scale, y * scale, rng.uniform(-0.5, 0.5) * scale, NOW]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: rng.chance(0.2),
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

fn domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Bspline(b) => [
            b.u.first().copied().unwrap_or(0.0),
            b.v.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
            b.v.last().copied().unwrap_or(1.0),
        ],
    }
}

fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(10) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smb")).to_string(),
            rebuild: rng.chance(0.25),
        },
        6 => {
            let c = gen_curve(rng, 3, 1.0);
            let (a, b) = (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0));
            let (t0, t1) = if rng.chance(0.5) {
                (a, b)
            } else {
                (a + (b - a) * rng.uniform(0.0, 0.3), b - (b - a) * rng.uniform(0.0, 0.3))
            };
            BrepIn::Wire { curve: c, t0, t1 }
        }
        7 | 8 => {
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
        _ => BrepIn::Network,
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}



// ---- op encoding ------------------------------------------------------------------------

fn push_opt_curve(v: &mut Vec<f64>, c: Option<&CrvIn>) -> Result<(), HarnessError> {
    match c {
        None => v.push(0.0),
        Some(c) => {
            v.push(1.0);
            push_curve(v, c)?;
        }
    }
    Ok(())
}

fn push_layout_free(v: &mut Vec<f64>, origin: &[f64; 3], thickness: f64, flags: u64) {
    v.extend(origin);
    v.push(thickness);
    v.push(flags as f64);
}

/// The B-rep entity counts an op's selectors resolve against.
#[derive(Clone, Copy)]
struct Counts {
    fu: usize,
    f: usize,
    l: usize,
    eu: usize,
    v: usize,
}

/// The op with its selectors resolved; `Err(name)` when the B-rep lacks the entity
/// kind the op needs.
fn resolve(op: &OpIn, c: &Counts) -> Result<OpIn, &'static str> {
    let face = |s: i64| -> Result<i64, &'static str> {
        if c.f == 0 {
            Err("no_face")
        } else {
            Ok(sel(s, c.f).max(0))
        }
    };
    let eu = |s: i64| -> Result<i64, &'static str> {
        if c.eu == 0 {
            Err("no_edgeuse")
        } else {
            Ok(sel(s, c.eu).max(0))
        }
    };
    Ok(match op.clone() {
        OpIn::FaceQuery {
            f,
            tgt_only,
            has_loops,
            loops,
        } => OpIn::FaceQuery {
            f: face(f)?,
            tgt_only,
            has_loops,
            loops: if c.l == 0 {
                Vec::new()
            } else {
                loops.iter().map(|&s| sel(s, c.l).max(0)).collect()
            },
        },
        OpIn::Set { f, dom, fu } => OpIn::Set {
            f: face(f)?,
            dom,
            fu: sel(fu, c.fu),
        },
        OpIn::Points { f, count, dom } => OpIn::Points {
            f: face(f)?,
            count,
            dom,
        },
        OpIn::Internal { f } => OpIn::Internal { f: face(f)? },
        OpIn::FromFace {
            f,
            orient,
            want_3d,
            want_uv,
        } => OpIn::FromFace {
            f: face(f)?,
            orient,
            want_3d,
            want_uv,
        },
        OpIn::Trim {
            f,
            tol,
            curves_3d,
            uv_curves,
            want_3d,
            want_uv,
        } => OpIn::Trim {
            f: face(f)?,
            tol,
            curves_3d,
            uv_curves,
            want_3d,
            want_uv,
        },
        OpIn::FixUv {
            eu: e,
            prev,
            vertex,
            tol,
            dist,
            face_dom,
            dom,
        } => OpIn::FixUv {
            eu: eu(e)?,
            prev: sel(prev, c.eu),
            vertex: sel(vertex, c.v),
            tol,
            dist,
            face_dom,
            dom,
        },
        OpIn::BestUv { eu: e, cands } => OpIn::BestUv { eu: eu(e)?, cands },
        OpIn::Connected {
            eu1,
            eu2,
            singularity_ok,
            seam_ok,
        } => OpIn::Connected {
            eu1: eu(eu1)?,
            eu2: eu(eu2)?,
            singularity_ok,
            seam_ok,
        },
        OpIn::Rescue { f, to_null } => OpIn::Rescue {
            f: face(f)?,
            to_null,
        },
        OpIn::RemoveSuspect { ends } => OpIn::RemoveSuspect {
            ends: ends
                .iter()
                .map(|[a, b]| [sel(*a, c.v), sel(*b, c.v)])
                .collect(),
        },
        other => other,
    })
}

/// The op code and its arguments (after the B-rep block for B-rep ops).
fn op_args(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let code = match op {
        OpIn::FaceQuery {
            f,
            tgt_only,
            has_loops,
            loops,
        } => {
            v.extend([*f as f64, flag(*tgt_only), flag(*has_loops), loops.len() as f64]);
            v.extend(loops.iter().map(|&l| l as f64));
            Op::FaceQuery
        }
        OpIn::Set { f, dom, fu } => {
            v.push(*f as f64);
            v.extend(dom);
            v.push(*fu as f64);
            Op::Set
        }
        OpIn::Points { f, count, dom } => {
            v.extend([*f as f64, *count as f64, flag(dom.is_some())]);
            if let Some(d) = dom {
                v.extend(d);
            }
            Op::Points
        }
        OpIn::Internal { f } => {
            v.push(*f as f64);
            Op::Internal
        }
        OpIn::FromFace {
            f,
            orient,
            want_3d,
            want_uv,
        } => {
            v.extend([*f as f64, f64::from(*orient), flag(*want_3d), flag(*want_uv)]);
            Op::FromFace
        }
        OpIn::Trim {
            f,
            tol,
            curves_3d,
            uv_curves,
            want_3d,
            want_uv,
        } => {
            v.extend([*f as f64, flag(tol.is_some()), tol.unwrap_or(0.0)]);
            for list in [curves_3d, uv_curves] {
                v.push(flag(list.is_some()));
                let l = list.as_deref().unwrap_or(&[]);
                v.push(l.len() as f64);
                for c in l {
                    push_opt_curve(&mut v, c.as_ref())?;
                }
            }
            v.extend([flag(*want_3d), flag(*want_uv)]);
            Op::Trim
        }
        OpIn::FixUv {
            eu,
            prev,
            vertex,
            tol,
            dist,
            face_dom,
            dom,
        } => {
            v.extend([*eu as f64, *prev as f64, *vertex as f64, *tol, *dist, flag(*face_dom)]);
            v.extend(dom);
            Op::FixUv
        }
        OpIn::BestUv { eu, cands } => {
            v.extend([*eu as f64, cands.len() as f64]);
            for c in cands {
                match (c.kind, &c.curve) {
                    (1, Some(curve)) => {
                        v.push(1.0);
                        push_curve(&mut v, curve)?;
                    }
                    (2, _) => v.push(2.0),
                    _ => v.push(0.0),
                }
                v.push(f64::from(c.orient));
            }
            Op::BestUv
        }
        OpIn::Connected {
            eu1,
            eu2,
            singularity_ok,
            seam_ok,
        } => {
            v.extend([*eu1 as f64, *eu2 as f64, flag(*singularity_ok), flag(*seam_ok)]);
            Op::Connected
        }
        OpIn::Rescue { f, to_null } => {
            v.extend([*f as f64, flag(*to_null)]);
            Op::Rescue
        }
        OpIn::RemoveSuspect { ends } => {
            v.push(ends.len() as f64);
            for [a, b] in ends {
                v.extend([*a as f64, *b as f64]);
            }
            Op::RemoveSuspect
        }
        OpIn::SeamCross {
            uv1,
            uv2,
            dom,
            closed_u,
            closed_v,
            shift,
        } => {
            v.extend(uv1);
            v.extend(uv2);
            v.extend(dom);
            v.extend([flag(*closed_u), flag(*closed_v)]);
            v.extend(shift);
            Op::SeamCross
        }
        OpIn::Layout { flags } => {
            v.push(*flags as f64);
            Op::Layout
        }
        OpIn::OdeIso {
            surface,
            u,
            t,
            origin,
            thickness,
            flags,
            n,
        } => {
            push_surface(&mut v, surface)?;
            v.extend([*u, *t]);
            push_layout_free(&mut v, origin, *thickness, *flags);
            v.push(*n as f64);
            Op::OdeIso
        }
        OpIn::OdeArea {
            surface,
            curve,
            t,
            min_v,
            accuracy,
            y_scale,
            origin,
            thickness,
            flags,
            dydx,
        } => {
            push_surface(&mut v, surface)?;
            push_curve(&mut v, curve)?;
            v.extend([*t, *min_v, *accuracy, y_scale.len() as f64]);
            v.extend(y_scale);
            push_layout_free(&mut v, origin, *thickness, *flags);
            v.push(dydx.len() as f64);
            v.extend(dydx);
            Op::OdeArea
        }
    };
    Ok((code, v))
}

// ---- comparison projections ---------------------------------------------------------------

/// `GetPointsInFace` output `status n uv[2n] pv[6n]`; for more than one point (a random
/// draw) the draw-invariant part `status n u* slot*` (the V grid slot on a rectangular
/// face, `0` otherwise).
fn project_points(body: &[f64], count: u64, face: &Face, dom: Option<[f64; 4]>) -> Vec<f64> {
    if count <= 1 || body.len() < 2 || body.first().copied() != Some(0.0) {
        return body.to_vec();
    }
    let n = body.get(1).copied().unwrap_or(0.0) as usize;
    let uv = body.get(2..2 + 2 * n).unwrap_or(&[]);
    let mut out = vec![0.0, n as f64];
    out.extend(uv.chunks(2).map(|c| c.first().copied().unwrap_or(0.0)));
    let v_len = match dom {
        Some(d) => Extent2d::from_bounds(d[0], d[1], d[2], d[3]).v_interval().length(),
        None => face.uv_domain.v_interval().length(),
    };
    let slot_size = if face.rectangular_trim == 1 {
        let t = count.min(30);
        let j = if (count / t) * t < count { count / t + 1 } else { count / t };
        v_len / (j + 1) as f64
    } else {
        0.0
    };
    out.extend(uv.chunks(2).map(|c| {
        if slot_size > 0.0 {
            (c.get(1).copied().unwrap_or(0.0) / slot_size).round()
        } else {
            0.0
        }
    }));
    out
}

/// Whether the op evaluates geometry that was not imported exactly ([`Exact`]): a
/// face's surface (`Points`, and the edge use ops through the edge use's face) or,
/// for `BestUv`, the edge's curve. Such cases are not compared (both sides report
/// `inexact_geometry`).
fn inexact(op: &OpIn, b: &Brep, exact: &Exact) -> bool {
    use solidcore::topology::edgeuse::p1 as eu1;
    let face_inexact = |f: Option<FaceId>| {
        f.is_some_and(|f| !exact.faces.get(f.index() as usize).copied().unwrap_or(true))
    };
    let eu_face = |e: i64| eu1::face(b, EdgeuseId::from_index(e as u32)).ok().flatten();
    let edge_inexact = |e: i64| {
        eu1::edge(b, EdgeuseId::from_index(e as u32))
            .ok()
            .flatten()
            .is_some_and(|e| !exact.edges.get(e.index() as usize).copied().unwrap_or(true))
    };
    match op {
        OpIn::Points { f, .. } => face_inexact(Some(FaceId::from_index(*f as u32))),
        OpIn::FixUv { eu, .. } => face_inexact(eu_face(*eu)),
        OpIn::BestUv { eu, .. } => face_inexact(eu_face(*eu)) || edge_inexact(*eu),
        OpIn::Connected { eu1: a, eu2: c, .. } => face_inexact(eu_face(*a)) || face_inexact(eu_face(*c)),
        _ => false,
    }
}

// ---- C++ side ---------------------------------------------------------------------------

fn counts_of(c: [usize; 10]) -> Counts {
    let [_, _, fu, f, _, l, eu, _, _, v] = c;
    Counts { fu, f, l, eu, v }
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if !i.op.is_brep() {
        let (code, args) = op_args(&i.op)?;
        let out = ffi::eval(code, "", &args).map_err(|e| harness(format!("c++: {e}")))?;
        return Ok(geom_output(&i.op, out));
    }
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let op = match resolve(&i.op, &counts_of(c)) {
        Ok(op) => op,
        Err(name) => return Ok(Output::err(name)),
    };
    let (b, exact) = import(&ex)?;
    if inexact(&op, &b, &exact) {
        return Ok(Output::err("inexact_geometry"));
    }
    let (code, args) = op_args(&op)?;
    let mut input = brep_block(i)?;
    input.extend(args);
    let path = path_of(i)?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    let body = match &op {
        OpIn::Points { f, count, dom } => {
            let face = b
                .face(FaceId::from_index(*f as u32))
                .map_err(|e| harness(e.to_string()))?;
            project_points(body, *count, face, *dom)
        }
        _ => body.to_vec(),
    };
    Ok(Output::new().values("state", body))
}

/// The geometry ops' output: their status normalised to `0` / `1` (any failure).
fn geom_output(op: &OpIn, mut out: Vec<f64>) -> Output {
    if matches!(op, OpIn::OdeIso { .. } | OpIn::OdeArea { .. }) {
        if let Some(s) = out.first_mut() {
            *s = flag(*s != 0.0);
        }
    }
    Output::new().values("state", out)
}

// ---- Rust side --------------------------------------------------------------------------

enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn rust_err(e: &SolidError) -> Result<Output, HarnessError> {
    match Output::unported(e) {
        Some(o) => Ok(o),
        None => Err(harness(format!("rust: {e}"))),
    }
}

fn idx<I: Copy>(o: Option<I>, f: impl Fn(I) -> u32) -> f64 {
    o.map_or(-1.0, |x| f64::from(f(x)))
}

/// An upstream status of a failed op: the `SmStatus` of an upstream error.
fn status_of(e: SolidError) -> Result<f64, Fail> {
    match e {
        SolidError::Upstream { code, .. } => Ok(f64::from(code)),
        other => Err(other.into()),
    }
}

fn rust_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(cur, c.dim, false)))
}

fn rust_surface(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::new(o[0], o[1], o[2]),
            Vec3::new(n[0], n[1], n[2]),
        ))),
        SrfIn::Bspline(b) => {
            let net = b
                .net
                .iter()
                .map(|r| r.iter().map(|p| CPoint::new(p[0], p[1], p[2], p[3])).collect())
                .collect();
            let nl = NlSurface::new(net, b.p, b.q, b.u.clone(), b.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = b.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(b.form))
                .ok_or_else(|| harness("bad surface form"))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

/// The shim's `put_curve` block of a curve.
fn put_curve(o: &mut Vec<f64>, c: Option<&SmCurve>) -> Result<(), Fail> {
    let Some(c) = c else {
        o.push(-1.0);
        return Ok(());
    };
    let (base, line) = match c.kind() {
        CurveKind::BSpline(b) => (std::borrow::Cow::Borrowed(b), None),
        CurveKind::Line(l) => (l.base_with_nurb()?, Some(l)),
        _ => {
            o.push(-2.0);
            return Ok(());
        }
    };
    o.push(c.type_code() as f64);
    o.push(base.dim() as f64);
    match base.gw_nurb() {
        Some(n) => {
            o.extend([1.0, n.n() as f64, n.degree() as f64, n.m() as f64]);
            for p in n.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend(n.knots());
        }
        None => o.push(0.0),
    }
    if let Some(l) = line {
        let lc = l.canonical_unit();
        o.extend([lc.point.x, lc.point.y, lc.point.z]);
        o.extend([lc.vector.x, lc.vector.y, lc.vector.z]);
        o.extend([lc.domain.min(), lc.domain.max(), lc.scale, flag(lc.inside_out)]);
    }
    Ok(())
}

fn orient(code: u8) -> OrientType {
    match code {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
    }
}

fn put_loops(o: &mut Vec<f64>, ls: &[LoopId]) {
    o.push(ls.len() as f64);
    o.extend(ls.iter().map(|l| f64::from(l.index())));
}

fn put_string(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn put_domain(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

/// A classification object of the `RemoveSuspect` op: a vertex index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct H(u32);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        ClassObjectKind::Vertex
    }
}

fn run_rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    if !i.op.is_brep() {
        return match run_rust_geom(k, &i.op) {
            Ok(o) => Ok(geom_output(&i.op, o)),
            Err(Fail::Solid(e)) => rust_err(&e),
            Err(Fail::Harness(h)) => Err(h),
        };
    }
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (mut b, exact) = import(&ex)?;
    let op = match resolve(&i.op, &counts_of(c)) {
        Ok(op) => op,
        Err(name) => return Ok(Output::err(name)),
    };
    if inexact(&op, &b, &exact) {
        return Ok(Output::err("inexact_geometry"));
    }
    match run_rust_op(&mut b, &op) {
        Ok(body) => {
            let body = match &op {
                OpIn::Points { f, count, dom } => {
                    let face = b
                        .face(FaceId::from_index(*f as u32))
                        .map_err(|e| harness(e.to_string()))?;
                    project_points(&body, *count, face, *dom)
                }
                _ => body,
            };
            Ok(Output::new().values("state", body))
        }
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// Whether `sm_FixUVTrimCurves` returns before dereferencing anything.
fn fix_returns_early(dist: f64, dom: &Extent2d) -> bool {
    let size = dom.max() - dom.min();
    dist < solidcore::math::EFF_ZERO / 10.0 || dist > size.x.min(size.y) / 20.0
}

#[allow(clippy::too_many_lines)]
fn run_rust_op(b: &mut Brep, op: &OpIn) -> Result<Vec<f64>, Fail> {
    use solidcore::topology::edgeuse::p1 as eu1;
    let mut k = Kernel::default();
    let fid = |x: i64| FaceId::from_index(x as u32);
    let eid = |x: i64| EdgeuseId::from_index(x as u32);
    let mut o = Vec::new();
    match op {
        OpIn::FaceQuery {
            f,
            tgt_only,
            has_loops,
            loops,
        } => {
            let f = fid(*f);
            o.push(flag(fp2::surface(b, f)?.is_some()));
            put_domain(&mut o, &fp2::uv_domain(b, f)?);
            o.push(f64::from(fp2::rectangular_trim(b, f)?));
            o.push(fp2::tolerance(b, f)?);
            let (fu1, fu2) = fp2::faceuses(b, f)?;
            o.push(idx(fu1, FaceuseId::index));
            o.push(idx(fu2, FaceuseId::index));
            o.push(idx(fp2::upward_faceuse(b, f)?, FaceuseId::index));
            let tl: Vec<LoopId> = loops.iter().map(|&l| LoopId::from_index(l as u32)).collect();
            put_loops(&mut o, &fp2::loops(b, f, *tgt_only, has_loops.then_some(tl.as_slice()))?);
            put_loops(&mut o, &fp2::outer_loops(b, f)?);
            o.push(fp2::TYPE as f64);
            o.push(fp2::class_type() as f64);
            o.extend(KIND_TYPES.iter().map(|&t| flag(fp2::is_kind_of(t))));
            put_string(&mut o, fp2::TYPE_STRING);
            put_string(&mut o, fp2::CLASS_STRING);
        }
        OpIn::Set { f, dom, fu } => {
            let f = fid(*f);
            fp2::set_uv_domain(b, f, &Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]))?;
            let fu = u32::try_from(*fu).ok().map(FaceuseId::from_index);
            fp2::set_faceuse(b, f, fu)?;
            put_domain(&mut o, &fp2::uv_domain(b, f)?);
            o.push(idx(fp2::upward_faceuse(b, f)?, FaceuseId::index));
            let (fu1, fu2) = fp2::faceuses(b, f)?;
            o.push(idx(fu1, FaceuseId::index));
            o.push(idx(fu2, FaceuseId::index));
            put_loops(&mut o, &fp2::loops(b, f, false, None)?);
        }
        OpIn::Points { f, count, dom } => {
            if let Some(d) = dom {
                fp2::set_uv_domain(b, fid(*f), &Extent2d::from_bounds(d[0], d[1], d[2], d[3]))?;
            }
            match fp2::points_in_face(&mut k, b, fid(*f), *count) {
            Ok(r) => {
                o.extend([0.0, r.uv.len() as f64]);
                for p in &r.uv {
                    o.extend([p.x, p.y]);
                }
                for p in &r.pv {
                    o.extend([p.x, p.y, p.z]);
                }
            }
            Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
            Err(e) => o.extend([status_of(e)?, 0.0]),
            }
        }
        OpIn::Internal { f } => match fp2::calculate_an_internal_point(&k, b, fid(*f)) {
            Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
            Err(e) => o.extend([status_of(e)?, 0.0, 0.0, 0.0]),
        },
        OpIn::FromFace {
            f,
            orient: or,
            want_3d,
            want_uv,
        } => match fp2::create_curves_from_face(&k, b, fid(*f), orient(*or), *want_3d, *want_uv) {
            Ok(r) => {
                o.push(0.0);
                o.push(r.curve_loops.len() as f64);
                o.extend(r.curve_loops.iter().map(|&n| n as f64));
                o.push(r.curve_orients.len() as f64);
                o.extend(r.curve_orients.iter().map(|&x| f64::from(x as u8)));
                o.push(r.loop_points.len() as f64);
                for p in &r.loop_points {
                    o.extend([p.x, p.y, p.z]);
                }
                for list in [&r.curves_3d, &r.uv_curves] {
                    let l = list.as_deref().unwrap_or(&[]);
                    o.push(l.len() as f64);
                    for c in l {
                        put_curve(&mut o, Some(c))?;
                    }
                }
            }
            Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
            Err(e) => o.extend([status_of(e)?, 0.0, 0.0, 0.0, 0.0, 0.0]),
        },
        OpIn::Trim {
            f,
            tol,
            curves_3d,
            uv_curves,
            want_3d,
            want_uv,
        } => {
            let conv = |l: &Option<Vec<Option<CrvIn>>>| -> Result<Option<Vec<Option<SmCurve>>>, HarnessError> {
                l.as_ref()
                    .map(|v| v.iter().map(|c| c.as_ref().map(rust_curve).transpose()).collect())
                    .transpose()
            };
            let c3 = conv(curves_3d)?;
            let cuv = conv(uv_curves)?;
            match fp2::create_curves_by_trimming(
                &k,
                b,
                fid(*f),
                *tol,
                c3.as_deref(),
                cuv.as_deref(),
                *want_3d,
                *want_uv,
            ) {
                Ok(r) => {
                    o.push(0.0);
                    match &r.curves_3d {
                        Some(l) => {
                            o.push(l.len() as f64);
                            for c in l {
                                put_curve(&mut o, Some(c))?;
                            }
                        }
                        None => o.push(-1.0),
                    }
                    match &r.uv_curves {
                        Some(l) => {
                            o.push(l.len() as f64);
                            for c in l {
                                put_curve(&mut o, c.as_ref())?;
                            }
                        }
                        None => o.push(-1.0),
                    }
                }
                Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
                Err(e) => o.extend([
                    status_of(e)?,
                    if *want_3d { 0.0 } else { -1.0 },
                    if *want_uv { 0.0 } else { -1.0 },
                ]),
            }
        }
        OpIn::FixUv {
            eu,
            prev,
            vertex,
            tol,
            dist,
            face_dom,
            dom,
        } => {
            let cur = eid(*eu);
            let prev = match u32::try_from(*prev) {
                Ok(p) => Some(EdgeuseId::from_index(p)),
                Err(_) => eu1::cw_edgeuse(b, cur)?,
            };
            let v = match u32::try_from(*vertex) {
                Ok(x) => Some(VertexId::from_index(x)),
                Err(_) => match eu1::vertexuse(b, cur)? {
                    Some(vu) => b.vertexuse(vu)?.vertex,
                    None => None,
                },
            };
            let dom = if *face_dom {
                match eu1::face(b, cur) {
                    Ok(Some(f)) => fp2::uv_domain(b, f)?,
                    _ => return Ok(vec![NULL_DEREF]),
                }
            } else {
                Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3])
            };
            let Some(v) = v else {
                return Ok(vec![NULL_DEREF]);
            };
            let uv_of = |b: &Brep, e: Option<EdgeuseId>| -> Result<Option<SmCurve>, Fail> {
                match e {
                    None => Ok(None),
                    Some(e) => Ok(eu1::uv_trim_curve(b, e)?.0.cloned()),
                }
            };
            let Some(p) = prev else {
                if fix_returns_early(*dist, &dom) {
                    o.extend([0.0, 0.0]);
                    put_curve(&mut o, uv_of(b, Some(cur))?.as_ref())?;
                    o.push(-1.0);
                    return Ok(o);
                }
                return Ok(vec![NULL_DEREF]);
            };
            match fp2::fix_uv_trim_curves(&k, b, cur, p, v, *tol, *dist, &dom) {
                Ok(fixed) => o.extend([0.0, flag(fixed)]),
                Err(SolidError::InvalidInput { .. }) => return Ok(vec![NULL_DEREF]),
                Err(e) => o.extend([status_of(e)?, 0.0]),
            }
            put_curve(&mut o, uv_of(b, Some(cur))?.as_ref())?;
            put_curve(&mut o, uv_of(b, Some(p))?.as_ref())?;
        }
        OpIn::BestUv { eu, cands } => {
            let e = eid(*eu);
            let own = eu1::uv_trim_curve(b, e)?.0.cloned();
            let mut slots: Vec<Option<SmCurve>> = Vec::new();
            let mut orients = Vec::new();
            for c in cands {
                slots.push(match (c.kind, &c.curve) {
                    (1, Some(curve)) => Some(rust_curve(curve)?),
                    (2, _) => own.clone(),
                    _ => None,
                });
                orients.push(orient(c.orient));
            }
            let surf = match eu1::face(b, e) {
                Ok(Some(f)) => match fp2::surface(b, f)? {
                    Some(s) => s.clone(),
                    None => return Ok(vec![NULL_DEREF]),
                },
                _ => return Ok(vec![NULL_DEREF]),
            };
            let before: Vec<bool> = slots.iter().map(Option::is_some).collect();
            match fp2::find_best_uv_curve(&k, b, &surf, e, &mut slots, &orients) {
                Ok(found) => {
                    let which = before
                        .iter()
                        .zip(&slots)
                        .position(|(was, now)| *was && now.is_none())
                        .filter(|_| found.is_some());
                    o.push(0.0);
                    o.push(which.map_or(-1.0, |w| w as f64));
                    put_curve(&mut o, found.as_ref())?;
                }
                Err(SolidError::InvalidInput { .. }) => return Ok(vec![NULL_DEREF]),
                Err(e) => o.extend([status_of(e)?, -1.0, -1.0]),
            }
            o.extend(slots.iter().map(|s| flag(s.is_some())));
        }
        OpIn::Connected {
            eu1: a,
            eu2: c,
            singularity_ok,
            seam_ok,
        } => match fp2::are_connected(&k, b, eid(*a), eid(*c), *singularity_ok, *seam_ok) {
            Ok(r) => o.extend([
                flag(r.connected),
                flag(r.on_singularity),
                flag(r.crosses_missing_seam),
            ]),
            Err(SolidError::InvalidInput { .. }) => o.push(NULL_DEREF),
            Err(e) => return Err(e.into()),
        },
        OpIn::Rescue { f, to_null } => {
            let f = fid(*f);
            let me = ObjectKey(1 + u64::from(f.index()));
            let face = b.face_mut(f)?;
            let Some(s) = face.surface.as_mut() else {
                return Ok(vec![NULL_DEREF]);
            };
            let owned = |s: &Surface| flag(s.owner().is_some_and(|x| x.key == me));
            o.push(owned(s));
            let rescue = fp2::RescueSurfaceOwner::new(s);
            s.set_owner(if *to_null {
                None
            } else {
                Some(ObjectRef {
                    type_code: 17001,
                    key: ObjectKey(0),
                })
            });
            o.push(flag(s.owner().is_none()));
            rescue.restore(s);
            o.push(owned(s));
        }
        OpIn::RemoveSuspect { ends } => {
            let mut cc: CurveClassification<H> = CurveClassification::new(&k);
            cc.set_size(&k, ends.len())?;
            for (n, [a, e]) in ends.iter().enumerate() {
                let ivl = cc.at_mut(n)?;
                if let Ok(a) = u32::try_from(*a) {
                    ivl.start.set_class_object(PointClassType::Vertex, Some(H(a)));
                }
                if let Ok(e) = u32::try_from(*e) {
                    ivl.end.set_class_object(PointClassType::Vertex, Some(H(e)));
                }
            }
            fp2::remove_suspect_split_at_seam_uv_trim_curves(b, &cc, |h| {
                Some(VertexId::from_index(h.0))
            })?;
            for (_, r) in b.edgeuses().iter() {
                o.push(flag(r.uv_trim_curve.is_some()));
            }
        }
        _ => return Err(harness("not a B-rep op").into()),
    }
    Ok(o)
}

fn run_rust_geom(k: &Kernel, op: &OpIn) -> Result<Vec<f64>, Fail> {
    let mut o = Vec::new();
    match op {
        OpIn::SeamCross {
            uv1,
            uv2,
            dom,
            closed_u,
            closed_v,
            shift,
        } => {
            let mut a = Point2::new(uv1[0], uv1[1]);
            let mut s = Vec2::new(shift[0], shift[1]);
            let r = fp2::check_seam_crossing(
                &mut a,
                Point2::new(uv2[0], uv2[1]),
                &Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]),
                *closed_u,
                *closed_v,
                &mut s,
            );
            o.extend([flag(r), a.x, a.y, s.x, s.y]);
        }
        OpIn::Layout { flags } => {
            let l = fp2::PrecisePropertyLayout::new(*flags);
            o.extend([l.num_components as f64, l.area as f64, l.volume as f64]);
            o.extend(l.moments.iter().map(|&m| m as f64));
        }
        OpIn::OdeIso {
            surface,
            u,
            t,
            origin,
            thickness,
            flags,
            n,
        } => {
            let s = rust_surface(surface)?;
            let l = fp2::PrecisePropertyLayout::new(*flags);
            let k = Kernel::default();
            let e = fp2::OdeIsoCurveLength::new(&k, &s, *u, Vec3::new(origin[0], origin[1], origin[2]), *thickness, &l);
            let mut d = vec![7.0; *n];
            let r = e.evaluate(k, *t, &mut d);
            match r {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(_) => o.push(1.0),
            }
            o.extend(d);
        }
        OpIn::OdeArea {
            surface,
            curve,
            t,
            min_v,
            accuracy,
            y_scale,
            origin,
            thickness,
            flags,
            dydx,
        } => {
            let s = rust_surface(surface)?;
            let c = rust_curve(curve)?;
            let l = fp2::PrecisePropertyLayout::new(*flags);
            let k = Kernel::default();
            let e = fp2::OdeArea {
                kernel: &k,
                surface: &s,
                uv_curve: &c,
                min_v: *min_v,
                iso_curve_accuracy: *accuracy,
                y_scale,
                origin: Vec3::new(origin[0], origin[1], origin[2]),
                face_thickness: *thickness,
                layout: &l,
            };
            let mut d = dydx.clone();
            match e.evaluate(k, *t, &mut d) {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Err(e.into()),
                Err(_) => o.push(1.0),
            }
            o.push(d.len() as f64);
            o.extend(d);
        }
        _ => return Err(harness("not a geometry op").into()),
    }
    Ok(o)
}

// ---- generated ops ----------------------------------------------------------------------

/// A 2D B-spline curve inside `[0, 1]^2` (UV space of most generated faces), sometimes
/// scaled out of it.
fn gen_uv_curve(rng: &mut Rng) -> CrvIn {
    let mut c = gen_curve(rng, 2, 1.0);
    let n = c.cpts.len().max(1) as f64;
    let (u0, v0) = (rng.uniform(-0.2, 0.8), rng.uniform(-0.2, 0.8));
    let (du, dv) = (rng.uniform(-0.6, 0.6), rng.uniform(-0.6, 0.6));
    for (i, p) in c.cpts.iter_mut().enumerate() {
        let k = if p[3] == NOW { 1.0 } else { p[3] };
        let t = i as f64 / (n - 1.0).max(1.0);
        p[0] = (u0 + du * t + rng.uniform(-0.05, 0.05)) * k;
        p[1] = (v0 + dv * t + rng.uniform(-0.05, 0.05)) * k;
    }
    c
}

fn gen_dom(rng: &mut Rng) -> [f64; 4] {
    match rng.below(4) {
        0 => [0.0, 0.0, 1.0, 1.0],
        1 => [-1.0, -2.0, 3.0, 0.5],
        2 => [0.25, 0.25, 0.25, 0.75], // degenerate in u
        _ => [rng.uniform(-5.0, 0.0), rng.uniform(-5.0, 0.0), rng.uniform(0.1, 5.0), rng.uniform(0.1, 5.0)],
    }
}

fn gen_flags(rng: &mut Rng) -> u64 {
    if rng.chance(0.3) {
        0x3f
    } else {
        rng.below(64)
    }
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng) -> OpIn {
    let f = gen_sel(rng);
    let eu = gen_sel(rng);
    match rng.below(40) {
        0..=3 => OpIn::FaceQuery {
            f,
            tgt_only: rng.chance(0.5),
            has_loops: rng.chance(0.5),
            loops: (0..rng.below(4)).map(|_| gen_sel(rng)).collect(),
        },
        4..=5 => OpIn::Set {
            f,
            dom: gen_dom(rng),
            fu: if rng.chance(0.2) { -1 } else { gen_sel(rng) },
        },
        6..=11 => OpIn::Points {
            f,
            count: *rng.choose(&[0, 1, 1, 1, 2, 3, 7, 30, 31, 61, 100]).unwrap_or(&1),
            dom: rng.chance(0.25).then(|| {
                let s = *rng.choose(&[1e-12, 1.1e-12, 1e-6, 1.0, 1e4]).unwrap_or(&1.0);
                [rng.uniform(-0.5, 0.5), rng.uniform(-0.5, 0.5), 0.0, 0.0]
                    .map(|x| x * s)
                    .into_iter()
                    .enumerate()
                    .map(|(i, x)| if i >= 2 { x + s * rng.uniform(0.5, 1.0) } else { x })
                    .collect::<Vec<_>>()
                    .try_into()
                    .unwrap_or([0.0, 0.0, 1.0, 1.0])
            }),
        },
        12 => OpIn::Internal { f },
        13..=14 => OpIn::FromFace {
            f,
            orient: rng.below(3) as u8,
            want_3d: rng.chance(0.5),
            want_uv: rng.chance(0.5),
        },
        15..=17 => {
            let n = rng.below(3) as usize;
            let uv = rng.chance(0.6);
            let degenerate = |rng: &mut Rng, mut c: CrvIn| {
                if rng.chance(0.5) {
                    // all control points equal: a degenerate curve (skipped upstream)
                    let p0 = c.cpts.first().copied().unwrap_or([0.0, 0.0, 0.0, NOW]);
                    c.cpts.fill(p0);
                }
                c
            };
            let uv_curves = uv.then(|| {
                (0..n)
                    .map(|_| {
                        if rng.chance(0.3) {
                            None
                        } else {
                            let c = gen_uv_curve(rng);
                            Some(degenerate(rng, c))
                        }
                    })
                    .collect()
            });
            let curves_3d = (!uv || rng.chance(0.3)).then(|| {
                (0..n.max(usize::from(uv) * n))
                    .map(|_| {
                        if !uv && rng.chance(0.1) {
                            None
                        } else {
                            let c = gen_curve(rng, 3, 1.0);
                            Some(degenerate(rng, c))
                        }
                    })
                    .collect::<Vec<_>>()
            });
            OpIn::Trim {
                f,
                tol: rng.chance(0.5).then(|| *rng.choose(&[0.0, 1e-7, 1e-3, 0.5]).unwrap_or(&0.0)),
                want_3d: rng.chance(0.5) && curves_3d.as_ref().is_none_or(|l| l.iter().all(Option::is_some)),
                curves_3d,
                uv_curves,
                want_uv: rng.chance(0.7),
            }
        }
        18..=25 => OpIn::FixUv {
            eu,
            prev: if rng.chance(0.85) { -1 } else { gen_sel(rng) },
            vertex: if rng.chance(0.85) { -1 } else { gen_sel(rng) },
            tol: *rng.choose(&[1e-5, 1e-3, 0.1]).unwrap_or(&1e-5),
            dist: *rng
                .choose(&[0.0, 1e-14, 1e-12, 1e-9, 1e-6, 1e-4, 1e-3, 0.01, 0.04, 0.2, 10.0])
                .unwrap_or(&1e-6),
            face_dom: rng.chance(0.8),
            dom: gen_dom(rng),
        },
        26..=31 => OpIn::BestUv {
            eu,
            cands: (0..=rng.below(4))
                .map(|_| {
                    let kind = *rng.choose(&[0u8, 1, 1, 2, 2]).unwrap_or(&1);
                    Cand {
                        kind,
                        curve: (kind == 1).then(|| gen_uv_curve(rng)),
                        orient: *rng.choose(&[1u8, 1, 2, 0]).unwrap_or(&1),
                    }
                })
                .collect(),
        },
        32..=33 => OpIn::Connected {
            eu1: eu,
            eu2: if rng.chance(0.5) { eu + 1 } else { gen_sel(rng) },
            singularity_ok: rng.chance(0.7),
            seam_ok: rng.chance(0.5),
        },
        34 => OpIn::Rescue {
            f,
            to_null: rng.chance(0.5),
        },
        35 => OpIn::RemoveSuspect {
            ends: (0..rng.below(4))
                .map(|_| {
                    [
                        if rng.chance(0.4) { -1 } else { gen_sel(rng) },
                        if rng.chance(0.4) { -1 } else { gen_sel(rng) },
                    ]
                })
                .collect(),
        },
        36 => {
            let dom = gen_dom(rng);
            let (lu, lv) = (dom[2] - dom[0], dom[3] - dom[1]);
            let p = |rng: &mut Rng| [dom[0] + lu * rng.uniform(-0.1, 1.1), dom[1] + lv * rng.uniform(-0.1, 1.1)];
            OpIn::SeamCross {
                uv1: p(rng),
                uv2: p(rng),
                dom,
                closed_u: rng.chance(0.6),
                closed_v: rng.chance(0.6),
                shift: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
            }
        }
        37 => OpIn::Layout { flags: rng.below(80) },
        38 => {
            let flags = gen_flags(rng);
            let need = ffi::layout_components(flags) + 1;
            OpIn::OdeIso {
                surface: gen_surface(rng),
                u: rng.uniform(-0.1, 1.1),
                t: rng.uniform(-0.1, 1.1),
                origin: if rng.chance(0.5) { [0.0; 3] } else { rng.point3(-2.0, 2.0) },
                thickness: if rng.chance(0.8) { 0.0 } else { *rng.choose(&[-0.5, 1e-13, 0.1]).unwrap_or(&0.1) },
                flags,
                n: need + rng.below(3) as usize,
            }
        }
        _ => {
            let flags = gen_flags(rng);
            let need = ffi::layout_components(flags);
            let mut curve = gen_uv_curve(rng);
            if rng.chance(0.6) {
                // a constant-u UV line: |du/dt| = 0, no inner integration
                let u = rng.uniform(0.0, 1.0);
                for p in &mut curve.cpts {
                    let k = if p[3] == NOW { 1.0 } else { p[3] };
                    p[0] = u * k;
                }
            }
            let (a, b) = (curve.u.first().copied().unwrap_or(0.0), curve.u.last().copied().unwrap_or(1.0));
            OpIn::OdeArea {
                surface: gen_surface(rng),
                t: a + (b - a) * rng.unit(),
                curve,
                min_v: rng.uniform(-0.1, 0.5),
                accuracy: *rng.choose(&[1e-6, 1e-3]).unwrap_or(&1e-6),
                y_scale: (0..need + rng.below(2) as usize).map(|_| rng.uniform(0.5, 2.0)).collect(),
                origin: if rng.chance(0.5) { [0.0; 3] } else { rng.point3(-2.0, 2.0) },
                thickness: if rng.chance(0.85) { 0.0 } else { 0.1 },
                flags,
                dydx: (0..rng.below(5)).map(|_| rng.uniform(-1.0, 1.0)).collect(),
            }
        }
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let op = gen_op(rng);
    let brep = op.is_brep().then(|| gen_brep(rng));
    Inputs {
        brep,
        validate_only: rng.chance(0.1),
        init_gaps: rng.chance(0.7),
        op,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.face_p2"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
let k = &Kernel::default();
        run_rust(k, &case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = crate::runner::default_cases(self, ctx)?;
        let mut rng = Rng::new(0xFACE_0002);
        for i in 0..EXTRA_GENERATED {
            let inputs = serde_json::to_value(gen_inputs(&mut rng))
                .map_err(|e| harness(e.to_string()))?;
            v.push(Case {
                unit: self.id().to_string(),
                name: format!("extra{i:05}"),
                source: crate::CaseSource::Generated { seed: 0xFACE_0002 },
                inputs,
            });
        }
        Ok(v)
    }
}


