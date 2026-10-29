//! Runner for unit `topology.vertex`. C++: `solidcore_ffi::units::topology_vertex`.
//! Rust: `solidcore::topology::vertex`.
//!
//! The C++ side builds a real `SmBrep` per call, as the `topology.vertexuse` runner
//! does: read from an upstream `.smb` test file (optionally rebuilding its UV trim
//! curves), a wire edge from a B-spline curve (`CreateWireEdgeFromCurve`), a face from a
//! B-spline surface or a plane (`CreateFaceFromSurface`), or the wire network of
//! `prog_test` `my_test_topology`, or a chain of wire edges. The Rust side builds every
//! non-file source itself ([`crate::own_builder`] under this shim's [`CONVENTIONS`], a
//! chain by [`build_chain`]). For an `.smb` file it imports the graph the shim exports
//! in traversal order (raw members, including each vertex's vertex use list in upstream list order,
//! each vertex use's cached gaps, each edge's curve identity and each face's upward face
//! use) into a [`Brep`] and runs the ported functions on it; their results are compared
//! as traversal indices, flags, statuses and values. `init_gaps` has the C++ side compute the cached
//! gaps before the op, so that cached gaps and their resets are observable. A B-rep
//! whose edges share a curve object, or whose edge curves are not B-splines or lines,
//! is a harness error (the Rust side could not represent it). An analytic face surface
//! (cone, sphere, ...) travels exactly as its upstream database record when that record
//! is lossless (`sc_analytic_export.h`, read by [`crate::util::read_exact_surface`]);
//! any other `SmBSplineSurface` subclass is held only as its B-spline part, and the
//! solving ops on such a B-rep are deferred.
//!
//! The vertex selector is taken modulo the vertex count (so generated cases need no
//! counts); other selectors likewise, `-1` selecting null.


use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::geom::solution_array::ObjectKind;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::query::poly::PolyVertexId;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::surface::surface::Surface;
use solidcore::topology::topology::Topology;
use solidcore::topology::vertex::{self as vx, VertexGap};
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, ObjectHandle, Region, RegionId, Shell, ShellId,
    Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_vertex::{self as ffi, Op};

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

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
const KIND_TYPES: [i64; 9] = [
    16021, 16010, 16001, 12060, 11001, 16006, 16016, 16013, 16999,
];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: i64 = -2;

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
    Wire {
        curve: CrvIn,
        t0: f64,
        t1: f64,
    },
    Face {
        surface: SrfIn,
        dom: [f64; 4],
    },
    Network,
    /// A chain of wire edges (closing back to its first vertex when `closed`).
    Chain {
        #[serde(default)]
        closed: bool,
        segs: Vec<SegIn>,
    },
}

/// A chain segment.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "seg", rename_all = "snake_case")]
enum SegIn {
    /// A line segment from `a` to `b`: an `SmLine` (re-parameterised to `ivl` when
    /// given) when `smline`, else a degree-1 `SmBSplineCurve`.
    Line {
        a: [f64; 3],
        b: [f64; 3],
        #[serde(default)]
        smline: bool,
        #[serde(default)]
        ivl: Option<[f64; 2]>,
        #[serde(default)]
        rev: bool,
    },
    /// A B-spline curve.
    Curve {
        curve: CrvIn,
        #[serde(default)]
        rev: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Query {
        v: i64,
    },
    Connected {
        v: i64,
        ty: i64,
        index: i64,
    },
    VuOfFace {
        v: i64,
        face: i64,
    },
    VusOfFace {
        v: i64,
        face: i64,
        ty: i64,
        index: i64,
    },
    VusOfEdge {
        v: i64,
        edge: i64,
        ty: i64,
        index: i64,
    },
    CommonEdges {
        v: i64,
        other: i64,
        face: i64,
    },
    ShellInRegion {
        v: i64,
        region: i64,
    },
    Tangent {
        v: i64,
        e1: i64,
        e2: i64,
        tol_deg: f64,
    },
    Topological {
        v: i64,
    },
    OnPole {
        v: i64,
    },
    Gaps {
        v: i64,
        want_array: bool,
        has_tol: bool,
        tol: f64,
    },
    Notify {
        v: i64,
        operation: u8,
    },
    SetPoint {
        v: i64,
        p: [f64; 3],
    },
    SetTolerance {
        v: i64,
        tol: f64,
        only_if_larger: bool,
        cascade: bool,
    },
    RefreshTolerance {
        v: i64,
        nested: bool,
    },
    Exact {
        v: i64,
        exact: bool,
        poly: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

// ---- encoding ---------------------------------------------------------------------------

fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let n = c
        .cpts
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("curve without points"))?;
    let m =
        c.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("curve without knots"))?;
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
            let n = b
                .net
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            let mu =
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without u knots"))?;
            let mv =
                b.v.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("surface without v knots"))?;
            v.extend([
                0.0,
                f64::from(u8::from(b.oob)),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                mu as f64,
                mv as f64,
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
fn path_of(b: &BrepIn) -> Result<String, HarnessError> {
    match b {
        BrepIn::File { file, .. } => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

/// The B-rep block.
fn brep_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match &i.brep {
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
        BrepIn::Chain { closed, segs } => {
            v.extend([4.0, flag(*closed), segs.len() as f64]);
            for sg in segs {
                match sg {
                    SegIn::Line {
                        a,
                        b,
                        smline,
                        ivl,
                        rev,
                    } => {
                        v.push(0.0);
                        v.extend(a);
                        v.extend(b);
                        let [t0, t1] = ivl.unwrap_or([0.0, 1.0]);
                        v.extend([flag(*smline), flag(ivl.is_some()), t0, t1, flag(*rev)]);
                    }
                    SegIn::Curve { curve, rev } => {
                        v.push(1.0);
                        push_curve(&mut v, curve)?;
                        v.push(flag(*rev));
                    }
                }
            }
        }
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
    let path = path_of(&i.brep)?;
    let block = brep_block(i)?;
    crate::export_memo::export("topology_vertex", &[&path], &block, || {
        ffi::eval(Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))
}

/// A build failure as an operation error.
fn build_failure(export: &[f64]) -> Output {
    Output::err(format!(
        "brep_build_status_{}",
        export.get(1).copied().unwrap_or(0.0)
    ))
}

fn ints(v: &[f64]) -> Vec<i64> {
    v.iter().map(|&x| x as i64).collect()
}

// ---- import -----------------------------------------------------------------------------

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
    /// The type of the first face surface held only as its B-spline part.
    inexact: Option<i64>,
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
        x if x >= 0 => Ok(Some(f(
            u32::try_from(x).map_err(|_| harness("index overflow"))?
        ))),
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
/// another `SmBSplineCurve` kind is an error (a UV trim curve when `uv`, else an edge
/// curve).
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
    if ty != BSPLINE_CURVE_TYPE {
        // edge curves are evaluated here: only exactly rebuilt kinds
        let what = if uv { "UV trim" } else { "edge" };
        return Err(harness(format!("export: {what} curve of type {ty}")));
    }
    Ok(Some(SmCurve::from(base)))
}

/// A face's surface block: an exact analytic head (`2 ...`,
/// [`crate::util::read_exact_surface`]) and the regular block after it, or the regular
/// block alone. A surface rebuilt exactly does not mark the B-rep inexact.
fn read_face_surface(rd: &mut Rd<'_>) -> Result<Option<Surface>, HarnessError> {
    if rd.v.get(rd.at) != Some(&2.0) {
        return read_surface(rd);
    }
    rd.at += 1;
    let exact = crate::util::read_exact_surface(&mut || rd.num())?;
    let before = rd.inexact;
    let fallback = read_surface(rd)?;
    match exact {
        Some(s) => {
            crate::util::check_exact_base(&s, fallback.as_ref())?;
            rd.inexact = before;
            Ok(Some(s))
        }
        None => Ok(fallback),
    }
}

fn read_surface(rd: &mut Rd<'_>) -> Result<Option<Surface>, HarnessError> {
    let kind = rd.num()?;
    if kind == -1.0 {
        return Ok(None);
    }
    if kind == 3.0 {
        // an SmPlane with its frame, UV scale and analytic domain
        let mut v = [0.0; 15];
        for x in &mut v {
            *x = rd.num()?;
        }
        let [ox, oy, oz, xx, xy, xz, yx, yy, yz, su, sv, u0, v0, u1, v1] = v;
        return Ok(Some(Surface::from(solidcore::surface::plane::Plane::new(
            Vec3::new(ox, oy, oz),
            Vec3::new(xx, xy, xz),
            Vec3::new(yx, yy, yz),
            solidcore::Vec2::new(su, sv),
            &solidcore::geom::extent2d::Extent2d::from_bounds(u0, v0, u1, v1),
        ))));
    }
    if kind == 4.0 {
        // a subclass held as its B-spline part: the Rust surface is a stand-in
        let ty = rd.num()? as i64;
        rd.inexact.get_or_insert(ty);
    } else if kind != 0.0 {
        return Err(harness("export: a surface is not a B-spline"));
    }
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
    let nl =
        NlSurface::new(net, p, q, u, v).map_err(|e| harness(format!("export surface: {e}")))?;
    let mut b = BSplineSurface::from_nurb(nl, false);
    *b.out_of_bounds_enabled_mut() = oob;
    let form =
        BSplineSurfaceForm::from_code(form as u64).ok_or_else(|| harness("bad surface form"))?;
    b.set_b_spline_surface_form(form);
    Ok(Some(Surface::from(b)))
}

/// Builds the Rust B-rep from the shim's export (see the module docs).
fn import(ex: &[f64]) -> Result<(Brep, Option<i64>), HarnessError> {
    let c = ffi::counts(ex).ok_or_else(|| harness("import of a failed build"))?;
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let mut rd = Rd { v: ex, at: 1, inexact: None };
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
    for _ in 0..n_f {
        let surface = read_face_surface(&mut rd)?;
        // `GetUVDomain` and `GetTolerance`
        let (u0, v0, u1, v1) = (rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        let tolerance = rd.num()?;
        // `GetUpwardFaceuse` (`m_pFU`)
        let faceuse = opt(rd.idx()?, FaceuseId::from_index)?;
        surfaces.push((surface, faceuse, solidcore::geom::extent2d::Extent2d::from_bounds(u0, v0, u1, v1), tolerance));
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
        let me =
            EdgeuseId::from_index(u32::try_from(edgeuses.len()).map_err(|_| harness("overflow"))?);
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
    for e in 0..n_e {
        // the curve's identity: two edges sharing one curve object cannot be represented
        let cid = rd.idx()?;
        if cid >= 0 && cid as usize != e {
            return Err(harness(format!(
                "export: edges {cid} and {e} share a curve"
            )));
        }
        let tolerance = rd.num()?;
        let a = rd.num()?;
        let b = rd.num()?;
        let list = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, EdgeuseId::from_index)?.ok_or_else(|| harness("null edge use")))
            .collect::<Result<Vec<_>, _>>()?;
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
        let mut r = Vertexuse {
            vertex,
            owner,
            ..Vertexuse::default()
        };
        r.vertex_edge_gap.vertexuse = opt(rd.idx()?, VertexuseId::from_index)?;
        r.vertex_edge_gap.gap_3d = rd.num()?;
        r.vertex_edge_gap.edge_t = rd.num()?;
        r.vertex_face_gap.vertexuse = opt(rd.idx()?, VertexuseId::from_index)?;
        r.vertex_face_gap.gap_3d = rd.num()?;
        r.vertex_face_gap.face_uv = Point2::new(rd.num()?, rd.num()?);
        r.uv_point = Point2::new(rd.num()?, rd.num()?);
        vertexuses.push(r);
    }
    let mut vertices = Vec::new();
    for _ in 0..n_v {
        let point = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let tolerance = rd.num()?;
        if rd.num()? != 0.0 {
            return Err(harness("export: a vertex has a poly vertex"));
        }
        let list = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, VertexuseId::from_index)?.ok_or_else(|| harness("null vertex use")))
            .collect::<Result<Vec<_>, _>>()?;
        vertices.push(Vertex {
            point,
            tolerance,
            vertexuses: list,
            poly_vertex: None,
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
    for (f, (surface, faceuse, uv_domain, tolerance)) in surfaces.into_iter().enumerate() {
        let shell = face_shells.get(f).copied().unwrap_or(ShellId::from_index(0));
        // The rectangular-trim flag is not exported (`SmFace()` default); the UV domain and
        // tolerance are (the vertex/face gaps solve against them).
        b.add_face_record(Face {
            surface,
            faceuse,
            uv_domain,
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
        b.add_faceuse_record(fu)
            .map_err(|e| harness(e.to_string()))?;
    }
    for lu in loopuses {
        b.add_loopuse_record(lu)
            .map_err(|e| harness(e.to_string()))?;
    }
    for eu in edgeuses {
        b.add_edgeuse_record(eu)
            .map_err(|e| harness(e.to_string()))?;
    }
    for vu in vertexuses {
        b.add_vertexuse_record(vu)
            .map_err(|e| harness(e.to_string()))?;
    }
    Ok((b, rd.inexact))
}

fn id<I: Copy>(o: Option<I>, f: impl Fn(I) -> u32) -> i64 {
    o.map_or(-1, |x| i64::from(f(x)))
}

fn rust_err(e: &SolidError) -> Result<Output, HarnessError> {
    match Output::unported(e) {
        Some(o) => Ok(o),
        None => Err(harness(format!("rust: {e}"))),
    }
}

fn target(ty: i64, index: i64) -> Option<EntityRef> {
    let i = u32::try_from(index).ok()?;
    Some(match ty {
        16012 => EntityRef::Region(RegionId::from_index(i)),
        16013 => EntityRef::Shell(ShellId::from_index(i)),
        16022 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        16002 => EntityRef::Face(FaceId::from_index(i)),
        16004 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        16003 => EntityRef::Loop(LoopId::from_index(i)),
        16005 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        16016 => EntityRef::Edge(EdgeId::from_index(i)),
        16006 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        16021 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

// ---- generation -------------------------------------------------------------------------

/// The upstream `.smb` files used (all B-spline geometry).
/// (`SmallBox.smp` and `Platform.smp` crash upstream's `SmBrep::ReadFromFile`; see
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
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
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
            let z = if dim == 2 {
                NOZ
            } else {
                rng.uniform(-1.0, 1.0) * scale * k
            };
            [x * k, y * k, z, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0))
    };
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
                    let (x, y) = if pinch && i == 0 {
                        (0.0, 0.0)
                    } else {
                        (i as f64, j as f64)
                    };
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
    match rng.below(13) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smp")).to_string(),
            rebuild: rng.chance(0.25),
        },
        6 => {
            let c = gen_curve(rng, 3, 1.0);
            let (a, b) = (
                c.u.first().copied().unwrap_or(0.0),
                c.u.last().copied().unwrap_or(1.0),
            );
            let (t0, t1) = if rng.chance(0.5) {
                (a, b)
            } else {
                (
                    a + (b - a) * rng.uniform(0.0, 0.3),
                    b - (b - a) * rng.uniform(0.0, 0.3),
                )
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
        9 => BrepIn::Network,
        _ => gen_chain(rng),
    }
}

fn unit3(v: [f64; 3]) -> [f64; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if l < 1e-6 {
        [1.0, 0.0, 0.0]
    } else {
        v.map(|c| c / l)
    }
}

/// `d` turned by `deg` degrees towards a random perpendicular direction.
fn turn(rng: &mut Rng, d: [f64; 3], deg: f64) -> [f64; 3] {
    let r = unit3(rng.point3(-1.0, 1.0));
    let ([d0, d1, d2], [r0, r1, r2]) = (d, r);
    let dot = d0 * r0 + d1 * r1 + d2 * r2;
    let [p0, p1, p2] = unit3([r0 - dot * d0, r1 - dot * d1, r2 - dot * d2]);
    let (c, sn) = (deg.to_radians().cos(), deg.to_radians().sin());
    unit3([c * d0 + sn * p0, c * d1 + sn * p1, c * d2 + sn * p2])
}

/// `p + len d`.
fn along(p: [f64; 3], len: f64, d: [f64; 3]) -> [f64; 3] {
    let ([p0, p1, p2], [d0, d1, d2]) = (p, d);
    [p0 + len * d0, p1 + len * d1, p2 + len * d2]
}

/// A chain of wire edges: mostly line segments (`SmLine`s, some re-parameterised so
/// that consecutive edges meet at equal parameters, or degree-1 B-splines) turning by
/// none, a little (around `SM_CONTINUITY_ANGLE`), a cusp or a random angle, and
/// B-spline curves whose end tangent the next segment continues.
fn gen_chain(rng: &mut Rng) -> BrepIn {
    let n = 1 + rng.below(4) as usize;
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let mut p = rng.point3(-1.0, 1.0).map(|c| c * scale);
    let mut d = unit3(rng.point3(-1.0, 1.0));
    let mut t = 0.0;
    let mut segs = Vec::new();
    for _ in 0..n {
        let deg = *rng
            .choose(&[
                0.0, 0.0, 0.0, 0.3, 0.49, 0.51, 0.7, 180.0, 179.8, 90.0, 37.0,
            ])
            .unwrap_or(&0.0);
        d = turn(rng, d, deg);
        let len = scale * rng.uniform(0.2, 2.0);
        let rev = rng.chance(0.2);
        if rng.chance(0.25) {
            // a B-spline whose last leg runs along d and ends at p + len d
            let mut c = gen_curve(rng, 3, scale);
            c.cpts.iter_mut().for_each(|q| q[3] = NOW);
            let k = c.cpts.len();
            let end = along(p, len, d);
            let last = c.cpts.last().copied().unwrap_or([0.0; 4]);
            let off = [end[0] - last[0], end[1] - last[1], end[2] - last[2]];
            for q in &mut c.cpts {
                for (qi, oi) in q.iter_mut().zip(off) {
                    *qi += oi;
                }
            }
            if k >= 2 {
                if let Some(q) = c.cpts.get_mut(k - 2) {
                    for ((qi, ei), di) in q.iter_mut().zip(end).zip(d) {
                        *qi = ei - 0.3 * len * di;
                    }
                }
            }
            t = c.u.last().copied().unwrap_or(1.0);
            p = end;
            segs.push(SegIn::Curve { curve: c, rev });
        } else {
            let b = along(p, len, d);
            let smline = rng.chance(0.7);
            let ivl = if smline && rng.chance(0.6) {
                // continue the previous parameter (equal parameters at the vertex)
                let t0 = if rng.chance(0.7) {
                    t
                } else {
                    rng.uniform(-3.0, 3.0)
                };
                Some([t0, t0 + rng.uniform(0.5, 3.0)])
            } else {
                None
            };
            t = ivl.map_or(1.0, |[_, t1]| t1);
            segs.push(SegIn::Line {
                a: p,
                b,
                smline,
                ivl,
                rev,
            });
            p = b;
        }
    }
    BrepIn::Chain {
        closed: n >= 2 && rng.chance(0.2),
        segs,
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

// ---- ops ---------------------------------------------------------------------------------

/// Raw adjacency read from the imported records (never the functions under test): the
/// edges of vertex `v`'s edge vertex uses and the far vertices of those edges.
fn raw_adjacent(b: &Brep, v: usize) -> (Vec<usize>, Vec<usize>) {
    let vid = VertexId::from_index(v as u32);
    let mut edges = Vec::new();
    let mut others = Vec::new();
    for (_, r) in b.vertexuses().iter() {
        if r.vertex != Some(vid) {
            continue;
        }
        let VertexuseOwner::Edgeuse(Some(eu)) = r.owner else {
            continue;
        };
        let Some(e) = b.edgeuse(eu).ok().and_then(|x| x.edge) else {
            continue;
        };
        if !edges.contains(&(e.index() as usize)) {
            edges.push(e.index() as usize);
        }
        if let Ok(rec) = b.edge(e) {
            for w in rec.vertices {
                if w != vid && !others.contains(&(w.index() as usize)) {
                    others.push(w.index() as usize);
                }
            }
        }
    }
    (edges, others)
}

/// A selector into `local` (when non-empty and `s` is even) or else into `0..n`; `-1`
/// (or an empty kind) selects null.
fn sel_local(s: i64, local: &[usize], n: usize) -> i64 {
    if s < 0 || n == 0 {
        return -1;
    }
    if !local.is_empty() && s % 2 == 0 {
        let i = ((s / 2) as u64 % local.len() as u64) as usize;
        return local.get(i).map_or(-1, |&x| x as i64);
    }
    sel(s, n)
}

fn target_count(ty: i64, c: [usize; 10]) -> usize {
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    match ty {
        16012 => n_r,
        16013 => n_s,
        16022 => n_fu,
        16002 => n_f,
        16004 => n_lu,
        16003 => n_l,
        16005 => n_eu,
        16016 => n_e,
        16006 => n_vu,
        16021 => n_v,
        _ => 0,
    }
}

/// The op with its selectors resolved against the B-rep's counts (and the vertex's raw
/// adjacency); `Err` with the output both sides report when the op has no subject.
fn resolve(op: &OpIn, c: [usize; 10], b: &Brep) -> Result<OpIn, Box<Output>> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = c;
    if n_v == 0 {
        return Err(Box::new(Output::err("no_vertex")));
    }
    let vs = |s: i64| sel(s, n_v).max(0);
    let tgt = |ty: i64, index: i64| {
        let n = target_count(ty, c);
        if n == 0 {
            index.min(-1)
        } else {
            sel(index, n)
        }
    };
    let adj = |v: i64| raw_adjacent(b, v as usize);
    Ok(match op.clone() {
        OpIn::Query { v } => OpIn::Query { v: vs(v) },
        OpIn::Connected { v, ty, index } => {
            let v = vs(v);
            let index = if ty == 16016 {
                sel_local(index, &adj(v).0, n_e)
            } else {
                tgt(ty, index)
            };
            OpIn::Connected { v, ty, index }
        }
        OpIn::VuOfFace { v, face } => OpIn::VuOfFace {
            v: vs(v),
            face: sel(face, n_f),
        },
        OpIn::VusOfFace { v, face, ty, index } => OpIn::VusOfFace {
            v: vs(v),
            face: sel(face, n_f),
            ty,
            index: tgt(ty, index),
        },
        OpIn::VusOfEdge { v, edge, ty, index } => {
            let v = vs(v);
            OpIn::VusOfEdge {
                v,
                edge: sel_local(edge, &adj(v).0, n_e),
                ty,
                index: tgt(ty, index),
            }
        }
        OpIn::CommonEdges { v, other, face } => {
            let v = vs(v);
            OpIn::CommonEdges {
                v,
                other: sel_local(other, &adj(v).1, n_v),
                face: sel(face, n_f),
            }
        }
        OpIn::ShellInRegion { v, region } => OpIn::ShellInRegion {
            v: vs(v),
            region: sel(region, n_r),
        },
        OpIn::Tangent { v, e1, e2, tol_deg } => {
            if n_e == 0 {
                return Err(Box::new(Output::err("no_edge")));
            }
            let v = vs(v);
            let (local, _) = adj(v);
            OpIn::Tangent {
                v,
                e1: sel_local(e1.max(0), &local, n_e),
                e2: sel_local(e2.max(0), &local, n_e),
                tol_deg,
            }
        }
        OpIn::Topological { v } => OpIn::Topological { v: vs(v) },
        OpIn::OnPole { v } => OpIn::OnPole { v: vs(v) },
        OpIn::Gaps {
            v,
            want_array,
            has_tol,
            tol,
        } => OpIn::Gaps {
            v: vs(v),
            want_array,
            has_tol,
            tol,
        },
        OpIn::Notify { v, operation } => OpIn::Notify {
            v: vs(v),
            operation,
        },
        OpIn::SetPoint { v, p } => OpIn::SetPoint { v: vs(v), p },
        OpIn::SetTolerance {
            v,
            tol,
            only_if_larger,
            cascade,
        } => OpIn::SetTolerance {
            v: vs(v),
            tol,
            only_if_larger,
            cascade,
        },
        OpIn::RefreshTolerance { v, nested } => OpIn::RefreshTolerance { v: vs(v), nested },
        OpIn::Exact { v, exact, poly } => OpIn::Exact {
            v: vs(v),
            exact,
            poly,
        },
    })
}

/// The op arguments after the B-rep block (resolved selectors).
fn op_args(op: &OpIn) -> (Op, Vec<f64>) {
    let f = |x: i64| x as f64;
    match op {
        OpIn::Query { v } => (Op::Query, vec![f(*v)]),
        OpIn::Connected { v, ty, index } => (Op::Connected, vec![f(*v), f(*ty), f(*index)]),
        OpIn::VuOfFace { v, face } => (Op::VuOfFace, vec![f(*v), f(*face)]),
        OpIn::VusOfFace { v, face, ty, index } => {
            (Op::VusOfFace, vec![f(*v), f(*face), f(*ty), f(*index)])
        }
        OpIn::VusOfEdge { v, edge, ty, index } => {
            (Op::VusOfEdge, vec![f(*v), f(*edge), f(*ty), f(*index)])
        }
        OpIn::CommonEdges { v, other, face } => (Op::CommonEdges, vec![f(*v), f(*other), f(*face)]),
        OpIn::ShellInRegion { v, region } => (Op::ShellInRegion, vec![f(*v), f(*region)]),
        OpIn::Tangent { v, e1, e2, tol_deg } => {
            (Op::Tangent, vec![f(*v), f(*e1), f(*e2), *tol_deg])
        }
        OpIn::Topological { v } => (Op::Topological, vec![f(*v)]),
        OpIn::OnPole { v } => (Op::OnPole, vec![f(*v)]),
        OpIn::Gaps {
            v,
            want_array,
            has_tol,
            tol,
        } => (
            Op::Gaps,
            vec![f(*v), flag(*want_array), flag(*has_tol), *tol],
        ),
        OpIn::Notify { v, operation } => (Op::Notify, vec![f(*v), f64::from(*operation)]),
        OpIn::SetPoint { v, p } => (Op::SetPoint, vec![f(*v), p[0], p[1], p[2]]),
        OpIn::SetTolerance {
            v,
            tol,
            only_if_larger,
            cascade,
        } => (
            Op::SetTolerance,
            vec![f(*v), *tol, flag(*only_if_larger), flag(*cascade)],
        ),
        OpIn::RefreshTolerance { v, nested } => (Op::RefreshTolerance, vec![f(*v), flag(*nested)]),
        OpIn::Exact { v, exact, poly } => (Op::Exact, vec![f(*v), flag(*exact), flag(*poly)]),
    }
}

/// The output key of an op (the values are compared as built by both sides).
fn output(op: &OpIn, body: &[f64]) -> Output {
    match op {
        OpIn::Query { .. }
        | OpIn::Connected { .. }
        | OpIn::VuOfFace { .. }
        | OpIn::VusOfFace { .. }
        | OpIn::VusOfEdge { .. }
        | OpIn::CommonEdges { .. }
        | OpIn::ShellInRegion { .. }
        | OpIn::Tangent { .. }
        | OpIn::Exact { .. } => Output::new().ints("q", ints(body)),
        _ => Output::new().values("state", body.to_vec()),
    }
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (b, _) = import(&ex)?;
    let op = match resolve(&i.op, c, &b) {
        Ok(op) => op,
        Err(o) => return Ok(*o),
    };
    let (code, args) = op_args(&op);
    let mut input = brep_block(i)?;
    input.extend(args);
    let path = path_of(&i.brep)?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(output(&op, body))
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

/// [`NULL_DEREF`] alone where upstream dereferences null; deferral and missing ids
/// propagate.
fn or_null<T>(r: Result<T, SolidError>) -> Result<Option<T>, SolidError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// A list `count ids...`, or [`NULL_DEREF`].
fn put_list<T>(
    q: &mut Vec<i64>,
    r: Result<Vec<T>, SolidError>,
    f: impl Fn(&T) -> i64,
) -> Result<(), SolidError> {
    match or_null(r)? {
        Some(l) => {
            q.push(l.len() as i64);
            q.extend(l.iter().map(f));
        }
        None => q.push(NULL_DEREF),
    }
    Ok(())
}

/// A flag, or [`NULL_DEREF`].
fn put_flag(q: &mut Vec<i64>, r: Result<bool, SolidError>) -> Result<(), SolidError> {
    q.push(or_null(r)?.map_or(NULL_DEREF, i64::from));
    Ok(())
}

/// The cached gap states of every vertex use, as the shim's `put_gap_states`.
fn gap_states(b: &Brep, out: &mut Vec<f64>) {
    for (_, r) in b.vertexuses().iter() {
        out.push(flag(r.vertex_edge_gap.is_init()));
        out.push(flag(r.vertex_face_gap.is_init()));
    }
}

/// A gap as the shim's `put_gap`.
fn put_gap(out: &mut Vec<f64>, g: Option<vx::HeldGap>) {
    match g {
        None => out.push(-1.0),
        Some(h) => {
            out.push(match h.gap {
                VertexGap::Edge(_) => 0.0,
                VertexGap::Face(_) => 1.0,
            });
            out.push(f64::from(h.holder.index()));
            out.push(h.gap.length());
        }
    }
}

fn notify_operation(code: u8) -> Result<NotifyOperation, HarnessError> {
    use NotifyOperation as N;
    Ok(match code {
        0 => N::AddToBrep,
        1 => N::SplitInBrep,
        2 => N::MergeInBrep,
        3 => N::TrimNoSplitInBrep,
        4 => N::Coincident,
        5 => N::RmFromBrep,
        6 => N::ChangeGeometry,
        7 => N::ChangeOwner,
        8 => N::Construction,
        9 => N::Copy,
        10 => N::PreEdit,
        11 => N::PostEdit,
        12 => N::Split,
        13 => N::Merge,
        14 => N::RegPropagation,
        15 => N::Destruction,
        16 => N::Unknown,
        x => return Err(harness(format!("bad notify operation {x}"))),
    })
}

/// The `topology.vertex` shim's build conventions (its own `build` and `walk`: the
/// vertex use lists followed after the loops' loop uses, `GetVertexEdgeGap` in the
/// `init_gaps` walk), geometry owned under the arena identities.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    vertex_edge_gaps: true,
    vertex_lists_last: true,
    ..crate::own_builder::Conventions::EDGEUSE_GRAPH
};

/// The shim's `build_chain`: per segment its curve (an `SmLine` segment, re-parameterised
/// to `ivl` when given, a `CreateLineSegment` B-spline, or the given B-spline), the
/// first segment's start vertex by `MakeShellVertex`, then `MakeWireEdgeVertex` to a
/// new end vertex, or `MakeWireEdge` back to the first vertex for the last segment of a
/// closed chain; `rev` runs the edge against its curve.
fn build_chain(
    k: &mut Kernel,
    b: &mut Brep,
    closed: bool,
    segs: &[SegIn],
    curves: &[Option<SmCurve>],
) -> Result<(), SolidError> {
    use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
    use solidcore::topology::brep::p2::ArenaNotifyState;
    use solidcore::topology::brep::{p3 as bp3, p9 as bp9};
    let mut state = ArenaNotifyState::new(b);
    let region = b.infinite_region();
    let mut first = None;
    let mut prev = None;
    let n = segs.len();
    for (i, sg) in segs.iter().enumerate() {
        let (c, rev) = match sg {
            SegIn::Line {
                a,
                b: e,
                smline,
                ivl,
                rev,
            } => {
                let (a, e) = (Vec3::from_array(*a), Vec3::from_array(*e));
                let c = if *smline {
                    let ivl = ivl.map(|[t0, t1]| Extent1d::from_min_max(t0, t1));
                    SmCurve::from(Line::create_line_segment(3, a, e, ivl.as_ref())?)
                } else {
                    crate::own_builder::line(a, e)?
                };
                (c, *rev)
            }
            SegIn::Curve { rev, .. } => (
                curves
                    .get(i)
                    .cloned()
                    .flatten()
                    .ok_or_else(|| SolidError::invalid_input("chain", "segment curve"))?,
                *rev,
            ),
        };
        let ivl = c.natural_interval();
        let p0 = c.evaluate_point(k, ivl.min())?;
        let p1 = c.evaluate_point(k, ivl.max())?;
        let o = if rev { OrientType::Opposite } else { OrientType::Same };
        let start = if let Some(v) = prev {
            v
        } else {
            let (_, v) = {
                let mut env = state.env(None);
                bp9::make_shell_vertex(b, region, if rev { p1 } else { p0 }, &mut env)?
            };
            state.seed(b);
            first = Some(v);
            v
        };
        if closed && i + 1 == n {
            let end = first.unwrap_or(start);
            let mut env = state.env(None);
            bp3::make_wire_edge(k, b, &mut env, region, start, end, c, &ivl, o)?;
        } else {
            let end_point = if rev { p0 } else { p1 };
            let (_, next) = {
                let mut env = state.env(None);
                bp3::make_wire_edge_vertex(k, b, &mut env, region, start, c, &ivl, o, end_point)?
            };
            prev = Some(next);
        }
        state.seed(b);
    }
    Ok(())
}

/// The Rust B-rep of a case, the face surface subclass held only inexactly (none for a
/// built source) and its entity counts, or the output of a failed build. Shim-built
/// sources are built by the Rust kernel ([`crate::own_builder`] under this shim's
/// [`CONVENTIONS`]; a chain by [`build_chain`]); only `.smb` file sources are still
/// imported from the C++ export ([`import`]).
fn own_brep(i: &Inputs) -> Result<crate::own_builder::RustBrep<OwnBrep>, HarnessError> {
    use crate::own_builder::{build_as, counts, failed_output, finish, in_new_brep, Built};
    let mut k = Kernel::default();
    let built = match &i.brep {
        BrepIn::Chain { closed, segs } => {
            let curves = segs
                .iter()
                .map(|sg| match sg {
                    SegIn::Curve { curve, .. } => chain_curve(curve).map(Some),
                    SegIn::Line { .. } => Ok(None),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let raw = in_new_brep(&mut k, |k, b| build_chain(k, b, *closed, segs, &curves))?;
            Some(finish(&mut k, raw, i.validate_only, i.init_gaps, &CONVENTIONS)?)
        }
        src => build_as(&mut k, src, i.validate_only, i.init_gaps, &CONVENTIONS)?,
    };
    match built {
        Some(Built::Brep(b)) => {
            let c = counts(&b);
            Ok(Ok((*b, None, c)))
        }
        Some(Built::Failed(e)) => Ok(Err(failed_output(&e))),
        None => {
            let ex = export(i)?;
            let Some(c) = ffi::counts(&ex) else {
                return Ok(Err(build_failure(&ex)));
            };
            let (b, inexact) = import(&ex)?;
            Ok(Ok((b, inexact, c)))
        }
    }
}

/// A Rust B-rep, the inexactly held face surface subclass and its entity counts.
type OwnBrep = (Brep, Option<i64>, [usize; 10]);

/// A chain segment's B-spline curve, as the shim's `read_curve` makes it.
fn chain_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let c: super::topology_edgeuse_p1::CrvIn = serde_json::to_value(c)
        .and_then(serde_json::from_value)
        .map_err(|e| harness(format!("chain curve: {e}")))?;
    Ok(SmCurve::from(super::topology_edgeuse_p1::bspline_of(&c)?))
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let (mut b, inexact, c) = match own_brep(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let op = match resolve(&i.op, c, &b) {
        Ok(op) => op,
        Err(o) => return Ok(*o),
    };
    // The gap, topological-vertex and pole queries solve against the face surfaces: a face
    // surface the export holds only as its B-spline part (a cone, sphere, torus, revolution
    // or extrusion subclass) is a stand-in whose solves differ from the analytic ones.
    if let (Some(ty), OpIn::Gaps { .. } | OpIn::Topological { .. } | OpIn::OnPole { .. }) =
        (inexact, &op)
    {
        let (unit, symbol) = match ty {
            // the DISPATCH GAP of `Surface::read_from_db` (issue #76) belongs to
            // `surface.surface_p2`, not to the analytic surface units (all green)
            15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
            15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCylinder dispatch)"),
            15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere dispatch)"),
            15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus dispatch)"),
            15030 => (
                "harness:analytic-geometry-export",
                "SmSurfOfExtrusion face surface not exported exactly",
            ),
            _ => ("topology.brep_p10", "SmBrep::MakeTopologyFromData"),
        };
        return Ok(Output::deferred(unit, symbol));
    }
    match run_rust_op(&mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// The topology records, identities and attribute store a notification runs with: the
/// vertex is key 1, vertex use `i` key `100 + i`.
struct NotifyEnv {
    records: solidcore::topology::topology::NotifyRecords,
    attributes: AttributeStore,
    this: ObjectRef,
}

impl NotifyEnv {
    fn new(b: &Brep) -> Self {
        let mut records = solidcore::topology::topology::NotifyRecords::new();
        records.insert(ObjectKey(1), Topology::new());
        for id in b.vertexuses().ids() {
            records.insert(ObjectKey(100 + u64::from(id.index())), Topology::new());
        }
        Self {
            records,
            attributes: AttributeStore::default(),
            this: ObjectRef {
                type_code: vx::TYPE,
                key: ObjectKey(1),
            },
        }
    }
}

fn vertexuse_ref(vu: VertexuseId) -> ObjectRef {
    ObjectRef {
        type_code: solidcore::topology::vertexuse::TYPE,
        key: ObjectKey(100 + u64::from(vu.index())),
    }
}

fn opt_id<I>(x: i64, f: impl Fn(u32) -> I) -> Option<I> {
    u32::try_from(x).ok().map(f)
}

fn run_rust_op(b: &mut Brep, op: &OpIn) -> Result<Output, Fail> {
    let k = Kernel::default();
    let vid = |x: i64| VertexId::from_index(x as u32);
    let body: Vec<f64> = match op {
        OpIn::Query { v } => {
            let v = vid(*v);
            let mut q = vec![vx::TYPE];
            q.extend(KIND_TYPES.iter().map(|&t| i64::from(vx::is_kind_of(t))));
            q.push(id(vx::primary_vertexuse(b, v)?, VertexuseId::index));
            put_list(&mut q, vx::vertexuses(b, v), |x| i64::from(x.index()))?;
            q.push(match or_null(vx::brep(b, v).map(|r| r.is_some()))? {
                Some(x) => i64::from(x),
                None => NULL_DEREF,
            });
            q.push(i64::from(vx::is_shell_vertex(b, v)?));
            q.push(i64::from(vx::is_loop_vertex(b, v)?));
            put_flag(&mut q, vx::is_wire_vertex(b, v))?;
            put_flag(&mut q, vx::is_lamina_vertex(b, v))?;
            put_list(&mut q, vx::edges(b, v, None), |x| i64::from(x.index()))?;
            put_list(&mut q, vx::faces(b, v), |x| i64::from(x.index()))?;
            put_list(&mut q, vx::faceuses(b, v), |x| id(*x, FaceuseId::index))?;
            put_list(&mut q, vx::shells(b, v), |x| i64::from(x.index()))?;
            put_list(&mut q, vx::regions(b, v), |x| i64::from(x.index()))?;
            return Ok(Output::new().ints("q", q));
        }
        OpIn::Connected { v, ty, index } => {
            let mut q = Vec::new();
            put_flag(&mut q, vx::is_connected_to(b, vid(*v), target(*ty, *index)))?;
            return Ok(Output::new().ints("q", q));
        }
        OpIn::VuOfFace { v, face } => {
            let f = opt_id(*face, FaceId::from_index);
            let q = match or_null(vx::vertexuse_of_face(b, vid(*v), f))? {
                Some(x) => id(x, VertexuseId::index),
                None => NULL_DEREF,
            };
            return Ok(Output::new().ints("q", vec![q]));
        }
        OpIn::VusOfFace { v, face, ty, index } => {
            let f = opt_id(*face, FaceId::from_index);
            let mut q = Vec::new();
            put_list(
                &mut q,
                vx::vertexuses_of_face(b, vid(*v), f, target(*ty, *index)),
                |x| i64::from(x.index()),
            )?;
            return Ok(Output::new().ints("q", q));
        }
        OpIn::VusOfEdge { v, edge, ty, index } => {
            let e = opt_id(*edge, EdgeId::from_index);
            let mut q = Vec::new();
            put_list(
                &mut q,
                vx::vertexuses_of_edge(b, vid(*v), e, target(*ty, *index)),
                |x| i64::from(x.index()),
            )?;
            return Ok(Output::new().ints("q", q));
        }
        OpIn::CommonEdges { v, other, face } => {
            let f = opt_id(*face, FaceId::from_index);
            let o = opt_id(*other, VertexId::from_index);
            let both = vx::edges(b, vid(*v), f)
                .and_then(|es| Ok((es, vx::common_edges(b, vid(*v), o, f)?)));
            let q = match or_null(both)? {
                Some((es, ce)) => {
                    let mut q = vec![es.len() as i64];
                    q.extend(es.iter().map(|x| i64::from(x.index())));
                    q.push(ce.len() as i64);
                    q.extend(ce.iter().map(|x| i64::from(x.index())));
                    q
                }
                None => vec![NULL_DEREF],
            };
            return Ok(Output::new().ints("q", q));
        }
        OpIn::ShellInRegion { v, region } => {
            let r = opt_id(*region, RegionId::from_index);
            let q = match or_null(vx::shell_in_region(b, vid(*v), r))? {
                Some(Some(s)) => vec![0, i64::from(s.index())],
                Some(None) => vec![i64::from(solidcore::error::SM_ERR), -1],
                None => vec![NULL_DEREF],
            };
            return Ok(Output::new().ints("q", q));
        }
        OpIn::Tangent { v, e1, e2, tol_deg } => {
            let e = |x: i64| EdgeId::from_index(x as u32);
            let mut q = Vec::new();
            put_flag(
                &mut q,
                vx::are_edges_tangent(&k, b, vid(*v), e(*e1), e(*e2), *tol_deg),
            )?;
            return Ok(Output::new().ints("q", q));
        }
        OpIn::Topological { v } => match or_null(vx::is_topological_vertex(&k, b, vid(*v)))? {
            Some(t) => {
                let mut out = vec![flag(t)];
                gap_states(b, &mut out);
                out
            }
            None => vec![NULL_DEREF as f64],
        },
        OpIn::OnPole { v } => match or_null(vx::is_on_pole(&k, b, vid(*v)))? {
            Some(t) => {
                let mut out = vec![flag(t)];
                gap_states(b, &mut out);
                out
            }
            None => vec![NULL_DEREF as f64],
        },
        OpIn::Gaps {
            v,
            want_array,
            has_tol,
            tol,
        } => {
            let mut list = Vec::new();
            let arr = want_array.then_some(&mut list);
            // SmTol3d stores the magnitude
            let t = has_tol.then_some(solidcore::geom::tol::tol3d(*tol));
            match or_null(vx::max_up_dim_gap_3d(&k, b, vid(*v), arr, t))? {
                Some(m) => {
                    let mut out = vec![0.0];
                    put_gap(&mut out, m);
                    if *want_array {
                        out.push(list.len() as f64);
                        for g in &list {
                            put_gap(&mut out, Some(*g));
                        }
                    } else {
                        out.push(-1.0);
                    }
                    gap_states(b, &mut out);
                    out
                }
                None => vec![NULL_DEREF as f64],
            }
        }
        OpIn::Notify { v, operation } => {
            let mut env = NotifyEnv::new(b);
            vx::notify(
                b,
                vid(*v),
                &mut env.records,
                &mut env.attributes,
                None,
                notify_operation(*operation)?,
                env.this,
                [None; 3],
                &vertexuse_ref,
            )?;
            let mut out = Vec::new();
            gap_states(b, &mut out);
            out
        }
        OpIn::SetPoint { v, p } => {
            let mut env = NotifyEnv::new(b);
            vx::set_point(
                b,
                vid(*v),
                Vec3::new(p[0], p[1], p[2]),
                &mut env.records,
                &mut env.attributes,
                None,
                env.this,
                &vertexuse_ref,
            )?;
            let pt = vx::point(b, vid(*v))?;
            let mut out = vec![pt.x, pt.y, pt.z];
            gap_states(b, &mut out);
            out
        }
        OpIn::SetTolerance {
            v,
            tol,
            only_if_larger,
            cascade,
        } => {
            // the call site converts the double to an SmZoneTol3d
            let t = solidcore::geom::tol::zone_tol3d(&k, *tol);
            vx::set_tolerance(b, vid(*v), t, *only_if_larger, *cascade)?;
            vec![vx::tolerance(b, vid(*v))?]
        }
        OpIn::RefreshTolerance { v, nested } => {
            match or_null(vx::refresh_tolerance(b, vid(*v), *nested))? {
                Some(()) => {
                    let mut out = vec![0.0, vx::tolerance(b, vid(*v))?];
                    for (_, e) in b.edges().iter() {
                        out.push(e.tolerance);
                    }
                    out
                }
                None => vec![NULL_DEREF as f64],
            }
        }
        OpIn::Exact { v, exact, poly } => {
            vx::set_exact(b, vid(*v), *exact)?;
            vx::set_poly_vertex(b, vid(*v), poly.then(|| PolyVertexId::from_index(0)))?;
            let q = vec![
                i64::from(vx::is_exact(b, vid(*v))?),
                i64::from(vx::poly_vertex(b, vid(*v))?.is_some()),
            ];
            vx::set_poly_vertex(b, vid(*v), None)?;
            return Ok(Output::new().ints("q", q));
        }
    };
    Ok(Output::new().values("state", body))
}

// ---- generated ops ----------------------------------------------------------------------

const TARGET_TYPES: [i64; 12] = [
    16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021, 16011, 7,
];

fn gen_target(rng: &mut Rng) -> (i64, i64) {
    let ty = *rng.choose(&TARGET_TYPES).unwrap_or(&16016);
    let index = if rng.chance(0.1) { -1 } else { gen_sel(rng) };
    (ty, index)
}

fn gen_op(rng: &mut Rng) -> OpIn {
    let v = gen_sel(rng);
    let tols = [0.0, 1e-9, 0.5, 1.0, 45.0, 90.0, 179.9, 180.0, 1e3, -1.0];
    match rng.below(34) {
        0..=3 => OpIn::Query { v },
        4..=8 => {
            let (ty, index) = gen_target(rng);
            OpIn::Connected { v, ty, index }
        }
        9..=10 => OpIn::VuOfFace {
            v,
            face: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        11..=12 => {
            let (ty, index) = if rng.chance(0.4) {
                (16016, -1)
            } else {
                gen_target(rng)
            };
            OpIn::VusOfFace {
                v,
                face: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
                ty,
                index,
            }
        }
        13..=14 => {
            let (ty, index) = if rng.chance(0.4) {
                (16016, -1)
            } else {
                gen_target(rng)
            };
            OpIn::VusOfEdge {
                v,
                edge: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
                ty,
                index,
            }
        }
        15..=16 => OpIn::CommonEdges {
            v,
            other: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
            face: if rng.chance(0.6) { -1 } else { gen_sel(rng) },
        },
        17 => OpIn::ShellInRegion {
            v,
            region: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        18..=20 => OpIn::Tangent {
            v,
            e1: gen_sel(rng),
            e2: gen_sel(rng),
            tol_deg: *rng.choose(&tols).unwrap_or(&0.5),
        },
        21..=23 => OpIn::Topological { v },
        24 => OpIn::OnPole { v },
        25..=26 => OpIn::Gaps {
            v,
            want_array: rng.chance(0.7),
            has_tol: rng.chance(0.5),
            tol: *rng
                .choose(&[0.0, 1e-12, 1e-7, 1e-5, 1e-3, 1.0, 1e10, -1e-6])
                .unwrap_or(&0.0),
        },
        27..=28 => OpIn::Notify {
            v,
            operation: rng.below(17) as u8,
        },
        29 => {
            let s = *rng.choose(&[1e-9, 1.0, 1e3, 1e9]).unwrap_or(&1.0);
            OpIn::SetPoint {
                v,
                p: rng.point3(-1.0, 1.0).map(|x| x * s),
            }
        }
        30 => OpIn::SetTolerance {
            v,
            tol: *rng
                .choose(&[
                    0.0, 1e-12, 1e-7, 1e-5, 2e-5, 1e-3, 10.0, 1e200, -1e-3, 9876543.21,
                ])
                .unwrap_or(&1e-5),
            only_if_larger: rng.chance(0.5),
            cascade: rng.chance(0.5),
        },
        31..=32 => OpIn::RefreshTolerance {
            v,
            nested: rng.chance(0.5),
        },
        _ => OpIn::Exact {
            v,
            exact: rng.chance(0.5),
            poly: rng.chance(0.5),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.vertex"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if super::topology_face_p4::level0::is_level0(case) {
            return super::topology_face_p4::level0::run_cpp(case);
        }
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if super::topology_face_p4::level0::is_level0(case) {
            return super::topology_face_p4::level0::run_rust(case);
        }
        run_rust(&case.parse::<Inputs>()?)
    }

    fn cases(&self, ctx: &crate::CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut v = crate::runner::default_cases(self, ctx)?;
        {
            use super::topology_face_p4::level0::Level0Op;
            let ops: Vec<Level0Op> = vec![Level0Op::CheckPointers];
            v.extend(super::topology_face_p4::level0::cases(self.id(), &ops, 0x1419_0101, 24)?);
        }
        Ok(v)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = gen_op(rng);
        let i = Inputs {
            brep: gen_brep(rng),
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.5),
            op,
        };
        serde_json::to_value(i).ok()
    }
}
