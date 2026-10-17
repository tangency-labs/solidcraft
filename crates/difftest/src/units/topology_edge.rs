//! Runner for unit `topology.edge`. C++: `solidcore_ffi::units::topology_edge`.
//! Rust: `solidcore::topology::edge`.
//!
//! As the `topology.vertexuse` runner, the C++ side builds a real `SmBrep` per call:
//! read from an upstream `.smb` test file (optionally rebuilding its UV trim curves), a
//! wire edge from a B-spline curve (`CreateWireEdgeFromCurve`), a face from a B-spline
//! surface or a real `SmPlane` (`CreateFaceFromSurface`), or the wire network of
//! `prog_test` `my_test_topology`. There is no Rust B-rep reader or builder yet (units
//! `topology.brep_*`), so the Rust side imports the graph the shim exports in traversal
//! order (raw members, including the cached edge/face and vertex/edge gap lengths and
//! whether each edge owns its curve) into a [`Brep`] and runs the ported functions on
//! it. Queries are compared as traversal indices, flags, statuses and values; edits
//! (tolerances, orientations, list changes, notifications, curve changes) by the
//! mutable graph state afterwards (the shim's `put_state`) and the curves they touch.
//! `init_gaps` has the C++ side compute the cached gaps before the op (their values are
//! exported, so the gap queries compare real gap lengths).
//!
//! **Inexact geometry.** The export carries plain B-spline curves and `SmLine`s exactly;
//! another `SmBSplineCurve` kind (an edge's ellipse in several upstream files) arrives as
//! its B-spline part only, as do `SmBSplineSurface` subclasses other than `SmPlane`. Ops
//! that evaluate such an edge curve (`EndTangent`, `Trimmed`, `Reverse`, `Ode`,
//! `RadialCurve`) or the surface of a face of the edge (`Seam`, `Topological`,
//! `Tangent`, `Radial`, `RadialCurve`, `Blend`) are not compared: both sides report
//! `inexact_geometry`.
//!
//! Selectors are taken modulo the entity count (`-1` selecting null where allowed); the
//! sector of `AddPair` and the edge use of `ResetPrimary` are taken from the edge's own
//! list.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::geom::solution_array::ObjectKind;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
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
use solidcore::topology::edge as ed;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId,
    Loop, LoopId, Loopuse, LoopuseId, LoopuseStart, ObjectHandle, Region, RegionId, Shell,
    ShellId, Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_edge::{self as ffi, Op};

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
const KIND_TYPES: [i64; 9] = [16016, 16005, 16001, 16010, 12060, 11001, 16013, 16021, 16999];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;

/// The Rust B-rep of an export, with whether each face's surface and each edge's curve
/// was imported exactly.
struct Imported {
    b: Brep,
    exact_faces: Vec<bool>,
    exact_curves: Vec<bool>,
}

/// Whether an exported curve was imported exactly (a B-spline or a line).
fn curve_is_exact(c: &SmCurve) -> bool {
    matches!(c.kind(), CurveKind::BSpline(_) | CurveKind::Line(_))
}

/// The identity each topology entity is attributed and notified under.
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, kind, i) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (ed::TYPE, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    ObjectRef {
        type_code,
        key: ObjectKey(1 + (kind << 32) + u64::from(i)),
    }
}

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
    /// A wire edge on a real `SmLine(p, v, [a, b], scale)` over the fractions `[f0, f1]`
    /// of its natural interval.
    Line {
        p: [f64; 3],
        v: [f64; 3],
        a: f64,
        b: f64,
        scale: f64,
        inside_out: bool,
        f0: f64,
        f1: f64,
    },
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
        BrepIn::Line {
            p,
            v: dir,
            a,
            b,
            scale,
            inside_out,
            f0,
            f1,
        } => {
            v.push(4.0);
            v.extend(p);
            v.extend(dir);
            v.extend([*a, *b, *scale, flag(*inside_out), *f0, *f1]);
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

/// Builds the Rust B-rep from the shim's export (see the module docs), with whether
/// each face's surface was imported exactly.
fn import(ex: &[f64]) -> Result<Imported, HarnessError> {
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
                    type_code: solidcore::topology::face::p2::TYPE,
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
        let ef_gap = rd.num()?;
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
            r.max_edge_face_gap.gap_3d = ef_gap;
        }
        if ccw_init {
            r.edge_ccw_edge_gap.edgeuse0 = Some(me);
            r.edge_ccw_edge_gap.edgeuse1 = ccw.or(Some(me));
        }
        edgeuses.push(r);
    }
    let mut edges = Vec::new();
    let mut exact_curves = Vec::new();
    for _ in 0..n_e {
        let tolerance = rd.num()?;
        let a = rd.num()?;
        let b = rd.num()?;
        let list = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, EdgeuseId::from_index)?.ok_or_else(|| harness("null edge use")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut curve = read_curve(&mut rd, false)?;
        let owned = rd.num()? != 0.0;
        let me = u32::try_from(edges.len()).map_err(|_| harness("overflow"))?;
        exact_curves.push(curve.as_ref().is_none_or(curve_is_exact));
        if owned {
            if let Some(c) = curve.as_mut() {
                c.set_owner(Some(key_of(EntityRef::Edge(EdgeId::from_index(me)))), None);
            }
        }
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
        let ve_gap = rd.num()?;
        let uv_point = Point2::new(rd.num()?, rd.num()?);
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
        }
        if edge_init {
            r.vertex_edge_gap.vertexuse = Some(me);
            r.vertex_edge_gap.gap_3d = ve_gap;
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
    Ok(Imported {
        b,
        exact_faces,
        exact_curves,
    })
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
    match rng.below(12) {
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
        9 => BrepIn::Network,
        _ => {
            let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
            let a = rng.uniform(-5.0, 5.0);
            let f0 = if rng.chance(0.5) { 0.0 } else { rng.uniform(0.0, 0.4) };
            let f1 = if rng.chance(0.5) { 1.0 } else { rng.uniform(0.6, 1.0) };
            BrepIn::Line {
                p: rng.point3(-2.0, 2.0).map(|x| x * scale),
                v: [rng.uniform(0.2, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
                a,
                b: a + rng.uniform(0.1, 4.0),
                scale: rng.uniform(0.5, 2.0),
                inside_out: rng.chance(0.5),
                f0,
                f1,
            }
        }
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}


// ---- ops --------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Query {
        e: i64,
        #[serde(default)]
        v: i64,
    },
    Face {
        e: i64,
        f: i64,
    },
    Connected {
        e: i64,
        ty: i64,
        index: i64,
    },
    Filtered {
        e: i64,
        attr: i64,
        #[serde(default)]
        ty: i64,
        #[serde(default)]
        index: i64,
        #[serde(default)]
        attach: Vec<[i64; 2]>,
    },
    EndTangent {
        e: i64,
        at_start: bool,
    },
    Trimmed {
        e: i64,
    },
    SetTol {
        e: i64,
        tol: f64,
        only_larger: bool,
        cascade: bool,
    },
    SetOrient {
        e: i64,
        orientation: u8,
    },
    Reverse {
        e: i64,
    },
    OrientSeq {
        edges: Vec<i64>,
    },
    ResetPrimary {
        e: i64,
        eu: i64,
    },
    AddPair {
        e: i64,
        o1: u8,
        o2: u8,
        sector: i64,
    },
    ManifoldEnd {
        e: i64,
        vu: i64,
        is_start: bool,
    },
    Gaps {
        e: i64,
        which: u8,
        want: bool,
        tol: f64,
    },
    Notify {
        e: i64,
        operation: u8,
    },
    SetCurve {
        e: i64,
        curve: Option<CrvIn>,
        delete: bool,
        notify: bool,
    },
    Radial {
        e: i64,
        eu: i64,
        region: i64,
        orientation: u8,
    },
    RadialCurve {
        e: i64,
        param: f64,
        e2: i64,
        orientation: u8,
        force: bool,
    },
    UvOfSurface {
        e: i64,
        f: i64,
    },
    Seam {
        e: i64,
        f: i64,
    },
    Topological {
        e: i64,
        gain: f64,
        want_domain: bool,
    },
    Tangent {
        e: i64,
        tol: f64,
        coincidence: bool,
    },
    Blend {
        e: i64,
    },
    Ode {
        e: i64,
        origin: [f64; 3],
        radius: f64,
        t: f64,
        n: u64,
    },
    SetInterval {
        e: i64,
        a: f64,
        b: f64,
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

/// The count of a `SmTopoTypes.h` kind in the export (`0` for other codes).
fn count_of(ty: i64, c: [usize; 10]) -> usize {
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

/// A target selector of kind `ty`: modulo the count, `-1` (null) when negative or the
/// kind is empty or unknown.
fn tsel(ty: i64, index: i64, c: [usize; 10]) -> i64 {
    sel(index, count_of(ty, c))
}

/// The op with its selectors resolved against the B-rep's counts and lists; `None` when
/// the B-rep has no edge.
fn resolve(op: &OpIn, c: [usize; 10], b: &Brep) -> Option<OpIn> {
    let [n_r, _, _, n_f, _, _, n_eu, n_e, n_vu, n_v] = c;
    if n_e == 0 {
        return None;
    }
    let es = |s: i64| sel(s, n_e).max(0);
    // an edge use of edge `e`'s own list (`-1` when the selector is negative)
    let own = |e: i64, s: i64| -> i64 {
        let list = b
            .edge(EdgeId::from_index(e as u32))
            .map(|r| r.edgeuses.clone())
            .unwrap_or_default();
        let i = sel(s, list.len());
        if i < 0 {
            -1
        } else {
            list.get(i as usize).map_or(-1, |x| i64::from(x.index()))
        }
    };
    Some(match op.clone() {
        OpIn::Query { e, v } => OpIn::Query {
            e: es(e),
            v: sel(v, n_v),
        },
        OpIn::Face { e, f } => OpIn::Face {
            e: es(e),
            f: sel(f, n_f),
        },
        OpIn::Connected { e, ty, index } => OpIn::Connected {
            e: es(e),
            ty,
            index: tsel(ty, index, c),
        },
        OpIn::Filtered {
            e,
            attr,
            ty,
            index,
            attach,
        } => OpIn::Filtered {
            e: es(e),
            attr: attr.max(-1),
            ty,
            index: tsel(ty, index, c),
            attach: attach
                .into_iter()
                .filter(|[t, _]| count_of(*t, c) > 0)
                .map(|[t, i]| [t, sel(i, count_of(t, c)).max(0)])
                .collect(),
        },
        OpIn::EndTangent { e, at_start } => OpIn::EndTangent {
            e: es(e),
            at_start,
        },
        OpIn::Trimmed { e } => OpIn::Trimmed { e: es(e) },
        OpIn::SetTol {
            e,
            tol,
            only_larger,
            cascade,
        } => OpIn::SetTol {
            e: es(e),
            tol,
            only_larger,
            cascade,
        },
        OpIn::SetOrient { e, orientation } => OpIn::SetOrient {
            e: es(e),
            orientation: orientation % 3,
        },
        OpIn::Reverse { e } => OpIn::Reverse { e: es(e) },
        OpIn::OrientSeq { edges } => OpIn::OrientSeq {
            edges: edges.into_iter().take(ffi::MAX_SEQ).map(es).collect(),
        },
        OpIn::ResetPrimary { e, eu } => {
            let e = es(e);
            OpIn::ResetPrimary {
                e,
                eu: if eu >= 1 << 30 { sel(eu, n_eu) } else { own(e, eu) },
            }
        }
        OpIn::AddPair { e, o1, o2, sector } => {
            let e = es(e);
            OpIn::AddPair {
                e,
                o1: o1 % 3,
                o2: o2 % 3,
                sector: own(e, sector),
            }
        }
        OpIn::ManifoldEnd { e, vu, is_start } => OpIn::ManifoldEnd {
            e: es(e),
            vu: sel(vu, n_vu),
            is_start,
        },
        OpIn::Gaps {
            e,
            which,
            want,
            tol,
        } => OpIn::Gaps {
            e: es(e),
            which: which % 3,
            want,
            tol,
        },
        OpIn::Notify { e, operation } => OpIn::Notify {
            e: es(e),
            operation: operation % 17,
        },
        OpIn::SetCurve {
            e,
            curve,
            delete,
            notify,
        } => OpIn::SetCurve {
            e: es(e),
            curve,
            delete,
            notify,
        },
        OpIn::Radial {
            e,
            eu,
            region,
            orientation,
        } => OpIn::Radial {
            e: es(e),
            eu: sel(eu, n_eu),
            region: sel(region, n_r),
            orientation: orientation % 3,
        },
        OpIn::RadialCurve {
            e,
            param,
            e2,
            orientation,
            force,
        } => OpIn::RadialCurve {
            e: es(e),
            param,
            e2: es(e2),
            orientation: orientation % 3,
            force,
        },
        OpIn::UvOfSurface { e, f } => OpIn::UvOfSurface {
            e: es(e),
            f: sel(f, n_f),
        },
        OpIn::Seam { e, f } => OpIn::Seam {
            e: es(e),
            f: sel(f, n_f),
        },
        OpIn::Topological {
            e,
            gain,
            want_domain,
        } => OpIn::Topological {
            e: es(e),
            gain,
            want_domain,
        },
        OpIn::Tangent {
            e,
            tol,
            coincidence,
        } => OpIn::Tangent {
            e: es(e),
            tol,
            coincidence,
        },
        OpIn::Blend { e } => OpIn::Blend { e: es(e) },
        OpIn::Ode {
            e,
            origin,
            radius,
            t,
            n,
        } => OpIn::Ode {
            e: es(e),
            origin,
            radius,
            t,
            n: n.clamp(28, ffi::MAX_ODE as u64),
        },
        OpIn::SetInterval { e, a, b } => OpIn::SetInterval { e: es(e), a, b },
    })
}

/// The op arguments after the B-rep block (resolved selectors).
fn op_args(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let f = |x: i64| x as f64;
    Ok(match op {
        OpIn::Query { e, v } => (Op::Query, vec![f(*e), f(*v)]),
        OpIn::Face { e, f: fc } => (Op::Face, vec![f(*e), f(*fc)]),
        OpIn::Connected { e, ty, index } => (Op::Connected, vec![f(*e), f(*ty), f(*index)]),
        OpIn::Filtered {
            e,
            attr,
            ty,
            index,
            attach,
        } => {
            let mut v = vec![f(*e), f(*attr), f(*ty), f(*index), attach.len() as f64];
            for [t, i] in attach {
                v.extend([f(*t), f(*i)]);
            }
            (Op::Filtered, v)
        }
        OpIn::EndTangent { e, at_start } => (Op::EndTangent, vec![f(*e), flag(*at_start)]),
        OpIn::Trimmed { e } => (Op::Trimmed, vec![f(*e)]),
        OpIn::SetTol {
            e,
            tol,
            only_larger,
            cascade,
        } => (
            Op::SetTol,
            vec![f(*e), *tol, flag(*only_larger), flag(*cascade)],
        ),
        OpIn::SetOrient { e, orientation } => {
            (Op::SetOrient, vec![f(*e), f64::from(*orientation)])
        }
        OpIn::Reverse { e } => (Op::Reverse, vec![f(*e)]),
        OpIn::OrientSeq { edges } => {
            let mut v = vec![edges.len() as f64];
            v.extend(edges.iter().map(|&x| f(x)));
            (Op::OrientSeq, v)
        }
        OpIn::ResetPrimary { e, eu } => (Op::ResetPrimary, vec![f(*e), f(*eu)]),
        OpIn::AddPair { e, o1, o2, sector } => (
            Op::AddPair,
            vec![f(*e), f64::from(*o1), f64::from(*o2), f(*sector)],
        ),
        OpIn::ManifoldEnd { e, vu, is_start } => {
            (Op::ManifoldEnd, vec![f(*e), f(*vu), flag(*is_start)])
        }
        OpIn::Gaps {
            e,
            which,
            want,
            tol,
        } => (
            Op::Gaps,
            vec![f(*e), f64::from(*which), flag(*want), tol.max(-1.0)],
        ),
        OpIn::Notify { e, operation } => (Op::Notify, vec![f(*e), f64::from(*operation)]),
        OpIn::SetCurve {
            e,
            curve,
            delete,
            notify,
        } => {
            let mut v = vec![f(*e), flag(curve.is_some())];
            if let Some(c) = curve {
                push_curve(&mut v, c)?;
            }
            v.extend([flag(*delete), flag(*notify)]);
            (Op::SetCurve, v)
        }
        OpIn::Radial {
            e,
            eu,
            region,
            orientation,
        } => (
            Op::Radial,
            vec![f(*e), f(*eu), f(*region), f64::from(*orientation)],
        ),
        OpIn::RadialCurve {
            e,
            param,
            e2,
            orientation,
            force,
        } => (
            Op::RadialCurve,
            vec![f(*e), *param, f(*e2), f64::from(*orientation), flag(*force)],
        ),
        OpIn::UvOfSurface { e, f: fc } => (Op::UvOfSurface, vec![f(*e), f(*fc)]),
        OpIn::Seam { e, f: fc } => (Op::Seam, vec![f(*e), f(*fc)]),
        OpIn::Topological {
            e,
            gain,
            want_domain,
        } => (Op::Topological, vec![f(*e), *gain, flag(*want_domain)]),
        OpIn::Tangent {
            e,
            tol,
            coincidence,
        } => (Op::Tangent, vec![f(*e), *tol, flag(*coincidence)]),
        OpIn::Blend { e } => (Op::Blend, vec![f(*e)]),
        OpIn::Ode {
            e,
            origin,
            radius,
            t,
            n,
        } => {
            let mut v = vec![f(*e)];
            v.extend(origin);
            v.extend([*radius, *t, *n as f64]);
            (Op::Ode, v)
        }
        OpIn::SetInterval { e, a, b } => (Op::SetInterval, vec![f(*e), *a, *b]),
    })
}

/// Whether the op evaluates geometry that was not imported exactly (see the module
/// docs).
fn inexact(op: &OpIn, im: &Imported) -> bool {
    let curve_inexact = |e: i64| !im.exact_curves.get(e as usize).copied().unwrap_or(true);
    let faces_inexact = |e: i64| {
        ed::faces(&im.b, EdgeId::from_index(e as u32), None)
            .unwrap_or_default()
            .iter()
            .any(|f| !im.exact_faces.get(f.index() as usize).copied().unwrap_or(true))
    };
    match op {
        OpIn::EndTangent { e, .. }
        | OpIn::Trimmed { e }
        | OpIn::Reverse { e }
        | OpIn::Ode { e, .. } => curve_inexact(*e),
        OpIn::SetCurve { e, delete, .. } => !*delete && curve_inexact(*e),
        OpIn::OrientSeq { edges } => edges.iter().any(|&e| curve_inexact(e)),
        OpIn::RadialCurve { e, e2, .. } => {
            curve_inexact(*e) || curve_inexact(*e2) || faces_inexact(*e)
        }
        OpIn::Seam { e, .. }
        | OpIn::Topological { e, .. }
        | OpIn::Tangent { e, .. }
        | OpIn::Radial { e, .. }
        | OpIn::Blend { e } => faces_inexact(*e) || curve_inexact(*e),
        _ => false,
    }
}

/// Statuses of curve evaluations (`EndTangent`, `Ode`) are compared as success or
/// failure: the Rust evaluators' error kinds do not carry upstream's status codes.
fn normalise_cpp(op: &OpIn, mut body: Vec<f64>) -> Vec<f64> {
    if matches!(op, OpIn::EndTangent { .. } | OpIn::Ode { .. }) {
        if let Some(s) = body.first_mut() {
            if *s != NULL_DEREF && *s != 0.0 {
                *s = 1.0;
            }
        }
    }
    body
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let im = import(&ex)?;
    let Some(op) = resolve(&i.op, c, &im.b) else {
        return Ok(Output::err("no_edge"));
    };
    if inexact(&op, &im) {
        return Ok(Output::err("inexact_geometry"));
    }
    let (code, args) = op_args(&op)?;
    let mut input = brep_block(i)?;
    input.extend(args);
    let path = path_of(&i.brep)?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(Output::new().values("state", normalise_cpp(&op, body.to_vec())))
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

/// A value slot: [`NULL_DEREF`] where upstream dereferences null; deferral and missing
/// ids propagate.
fn slot<T>(r: Result<T, SolidError>, f: impl FnOnce(T) -> f64) -> Result<f64, SolidError> {
    match r {
        Ok(v) => Ok(f(v)),
        Err(SolidError::InvalidInput { .. }) => Ok(NULL_DEREF),
        Err(e) => Err(e),
    }
}

/// A list slot (`len items`, `-1` for null entries), or [`NULL_DEREF`].
fn list_slot<T: Copy>(
    out: &mut Vec<f64>,
    r: Result<Vec<T>, SolidError>,
    f: impl Fn(T) -> f64,
) -> Result<(), SolidError> {
    match r {
        Ok(v) => {
            out.push(v.len() as f64);
            out.extend(v.into_iter().map(f));
            Ok(())
        }
        Err(SolidError::InvalidInput { .. }) => {
            out.push(NULL_DEREF);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn ix<I: Copy>(o: Option<I>, f: impl Fn(I) -> u32) -> f64 {
    o.map_or(-1.0, |x| f64::from(f(x)))
}

/// An upstream status: `0` for success, the `SmStatus` of an upstream error.
fn status_of(r: &Result<(), SolidError>) -> Result<Option<f64>, SolidError> {
    match r {
        Ok(()) => Ok(Some(0.0)),
        Err(SolidError::Upstream { code, .. }) => Ok(Some(f64::from(*code))),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(e.clone()),
    }
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

/// The shim's `put_state`.
fn put_state(b: &Brep, out: &mut Vec<f64>) {
    for (_, r) in b.edgeuses().iter() {
        out.push(ix(r.edge, EdgeId::index));
        out.push(orient_code(r.orientation));
        out.push(ix(r.cw, EdgeuseId::index));
        out.push(ix(r.ccw, EdgeuseId::index));
        out.push(ix(r.vertexuse, VertexuseId::index));
        out.push(flag(r.uv_trim_curve.is_some()));
        out.push(flag(r.max_edge_face_gap.is_init()));
    }
    for (_, r) in b.edges().iter() {
        out.push(r.tolerance);
        out.push(r.interval.min());
        out.push(r.interval.max());
        out.push(r.edgeuses.len() as f64);
        out.extend(r.edgeuses.iter().map(|x| f64::from(x.index())));
    }
    for (_, r) in b.vertexuses().iter() {
        out.push(ix(r.vertex, VertexId::index));
        out.push(flag(r.vertex_edge_gap.is_init()));
        out.push(flag(r.vertex_face_gap.is_init()));
    }
    for (_, r) in b.vertices().iter() {
        out.push(r.tolerance);
    }
}

/// The shim's `put_curve` block of a B-spline or line (`-1` for none).
fn put_curve(out: &mut Vec<f64>, c: Option<&SmCurve>) -> Result<(), HarnessError> {
    let Some(c) = c else {
        out.push(-1.0);
        return Ok(());
    };
    let (ty, base) = match c.kind() {
        CurveKind::BSpline(b) => (BSPLINE_CURVE_TYPE, b),
        CurveKind::Line(l) => (LINE_TYPE, l.base()),
        _ => return Err(harness("put_curve: not a B-spline or line")),
    };
    out.push(ty as f64);
    out.push(base.dim() as f64);
    match base.gw_nurb() {
        Some(n) => {
            out.push(1.0);
            out.extend([n.n() as f64, n.degree() as f64, n.m() as f64]);
            for p in n.cpts() {
                out.extend([p.x, p.y, p.z, p.w]);
            }
            out.extend(n.knots());
        }
        None => out.push(0.0),
    }
    if let CurveKind::Line(l) = c.kind() {
        let p = l.line_point();
        let v = l.line_vector();
        let d = l.step_interval();
        out.extend([p.x, p.y, p.z, v.x, v.y, v.z, d.min(), d.max(), l.line_scale()]);
        out.push(flag(l.inside_out()));
    }
    Ok(())
}

fn orient_of_code(code: u8) -> OrientType {
    match code {
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => OrientType::Unknown,
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

fn opt_id<I>(x: i64, f: impl Fn(u32) -> I) -> Option<I> {
    u32::try_from(x).ok().map(f)
}

/// A gap as `kind length` (`0` edge/face, `1` vertex/edge).
fn gap_values(g: &ed::EdgeGap) -> [f64; 2] {
    match g {
        ed::EdgeGap::EdgeFace(x) => [0.0, x.gap_3d],
        ed::EdgeGap::VertexEdge(x) => [1.0, x.gap_3d],
    }
}

fn rust_curve(c: &CrvIn) -> Result<SmCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let nl = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(SmCurve::from(BSplineCurve::from_nurb(nl, c.dim, false)))
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let mut im = import(&ex)?;
    let Some(op) = resolve(&i.op, c, &im.b) else {
        return Ok(Output::err("no_edge"));
    };
    if inexact(&op, &im) {
        return Ok(Output::err("inexact_geometry"));
    }
    match run_rust_op(&mut im.b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

fn run_rust_op(b: &mut Brep, op: &OpIn) -> Result<Output, Fail> {
    let k = Kernel::default();
    let eid = |x: i64| EdgeId::from_index(x as u32);
    let mut out: Vec<f64> = Vec::new();
    match op {
        OpIn::Query { e, v } => {
            let e = eid(*e);
            let tv = opt_id(*v, VertexId::from_index);
            let r = b.edge(e)?;
            out.push(r.edgeuses.len() as f64);
            out.push(ix(ed::primary_edgeuse(b, e)?, EdgeuseId::index));
            out.push(flag(r.curve.is_some()));
            out.push(flag(ed::is_wire(b, e)?));
            out.push(flag(ed::is_lamina(b, e)?));
            out.push(flag(ed::is_manifold(b, e)?));
            out.push(flag(ed::is_spine(b, e)?));
            out.push(slot(ed::is_closed(b, e), flag)?);
            out.push(slot(ed::vertex(b, e), |x| ix(x, VertexId::index))?);
            out.push(slot(ed::start_vertex(b, e), |x| ix(x, VertexId::index))?);
            out.push(slot(ed::end_vertex(b, e), |x| ix(x, VertexId::index))?);
            match ed::start_end_vertices(b, e) {
                Ok((s, t)) => out.extend([ix(s, VertexId::index), ix(t, VertexId::index)]),
                Err(SolidError::InvalidInput { .. }) => out.extend([NULL_DEREF, NULL_DEREF]),
                Err(err) => return Err(err.into()),
            }
            list_slot(&mut out, ed::vertices(b, e, None), |x| ix(x, VertexId::index))?;
            list_slot(&mut out, ed::vertexuses(b, e, None), |x| {
                ix(x, VertexuseId::index)
            })?;
            out.push(0.0);
            list_slot(&mut out, ed::edgeuses(b, e, None, None), |x| {
                f64::from(x.index())
            })?;
            list_slot(&mut out, ed::faces(b, e, None), |x| f64::from(x.index()))?;
            list_slot(&mut out, ed::shells(b, e, None), |x| ix(x, ShellId::index))?;
            list_slot(&mut out, ed::regions(b, e, None), |x| ix(x, RegionId::index))?;
            out.push(slot(ed::same_oriented_edgeuse(b, e), |x| {
                ix(x, EdgeuseId::index)
            })?);
            out.push(flag(ed::brep(b, e)?.is_some()));
            out.push(ix(ed::other_vertex(b, e, tv)?, VertexId::index));
            out.push(ed::TYPE as f64);
            out.extend(KIND_TYPES.iter().map(|&t| flag(ed::is_kind_of(t))));
            out.push(ed::tolerance(b, e)?);
            let ivl = ed::interval(b, e)?;
            out.extend([ivl.min(), ivl.max()]);
        }
        OpIn::Face { e, f } => {
            let e = eid(*e);
            let f = opt_id(*f, FaceId::from_index);
            let first = match ed::edgeuse_of_face(b, e, f) {
                Ok(x) => x,
                Err(SolidError::InvalidInput { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF]))
                }
                Err(err) => return Err(err.into()),
            };
            out.push(ix(first, EdgeuseId::index));
            out.push(slot(ed::upward_edgeuse_of_face(b, e, f), |x| {
                ix(x, EdgeuseId::index)
            })?);
            out.push(slot(ed::number_face_occurances(b, e, f), |x| x as f64)?);
            out.push(slot(ed::is_connected_to_face(b, e, f), flag)?);
        }
        OpIn::Connected { e, ty, index } => {
            let t = target(*ty, *index);
            out.push(slot(ed::is_connected_to(b, eid(*e), t), flag)?);
        }
        OpIn::Filtered {
            e,
            attr,
            ty,
            index,
            attach,
        } => {
            let e = eid(*e);
            let mut store = AttributeStore::default();
            if *attr >= 0 {
                for [t, i] in attach {
                    if let Some(tg) = target(*t, *i) {
                        let h = store
                            .insert(Attribute::new(*attr as u64, Behavior::Copy))
                            .map_err(|x| harness(x.to_string()))?;
                        store
                            .add_attribute(key_of(tg).key, h)
                            .map_err(|x| harness(x.to_string()))?;
                    }
                }
            }
            let key = |x: EntityRef| key_of(x).key;
            let filter = ed::AttributeFilter {
                attributes: &store,
                attribute_id: (*attr).max(0) as u64,
                key_of: &key,
            };
            let fl = (*attr >= 0).then_some(&filter);
            let conn = target(*ty, *index);
            out.push(0.0);
            list_slot(&mut out, ed::edgeuses(b, e, fl, conn), |x| f64::from(x.index()))?;
            list_slot(&mut out, ed::faces(b, e, fl), |x| f64::from(x.index()))?;
            list_slot(&mut out, ed::shells(b, e, fl), |x| ix(x, ShellId::index))?;
            list_slot(&mut out, ed::regions(b, e, fl), |x| ix(x, RegionId::index))?;
            list_slot(&mut out, ed::vertices(b, e, fl), |x| ix(x, VertexId::index))?;
            list_slot(&mut out, ed::vertexuses(b, e, fl), |x| {
                ix(x, VertexuseId::index)
            })?;
        }
        OpIn::EndTangent { e, at_start } => match ed::end_tangent(b, eid(*e), *at_start) {
            Ok(t) => out.extend([0.0, t.x, t.y, t.z]),
            Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
            Err(err) if err.is_unported() => return Err(err.into()),
            Err(_) => out.extend([1.0, 0.0, 0.0, 0.0]),
        },
        OpIn::Trimmed { e } => {
            let c = ed::create_trimmed_nurbs_curve(&k, b, eid(*e))?;
            put_curve(&mut out, c.as_ref())?;
        }
        OpIn::SetTol {
            e,
            tol,
            only_larger,
            cascade,
        } => match ed::set_tolerance(b, eid(*e), *tol, *only_larger, *cascade) {
            Ok(()) => {
                out.push(0.0);
                put_state(b, &mut out);
            }
            Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
            Err(err) => return Err(err.into()),
        },
        OpIn::SetOrient { e, orientation } => {
            ed::set_orientation(b, eid(*e), orient_of_code(*orientation))?;
            put_state(b, &mut out);
        }
        OpIn::Reverse { e } => match ed::reverse_orientation(&k, b, eid(*e)) {
            Ok(()) => {
                out.push(0.0);
                put_curve(&mut out, b.edge(eid(*e))?.curve.as_ref())?;
                put_state(b, &mut out);
            }
            Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
            Err(err) => return Err(err.into()),
        },
        OpIn::OrientSeq { edges } => {
            let es: Vec<EdgeId> = edges.iter().map(|&x| eid(x)).collect();
            let r = ed::orient_edge_sequence(&k, b, &es);
            match status_of(&r)? {
                Some(s) => {
                    out.push(s);
                    put_state(b, &mut out);
                }
                None => out.push(NULL_DEREF),
            }
        }
        OpIn::ResetPrimary { e, eu } => {
            let r = ed::reset_primary_edgeuse(b, eid(*e), opt_id(*eu, EdgeuseId::from_index));
            match status_of(&r)? {
                Some(s) => {
                    out.push(s);
                    put_state(b, &mut out);
                }
                None => out.push(NULL_DEREF),
            }
        }
        OpIn::AddPair { e, o1, o2, sector } => {
            let a = b
                .add_edgeuse_record(Edgeuse {
                    orientation: orient_of_code(*o1),
                    ..Edgeuse::default()
                })
                .map_err(|x| harness(x.to_string()))?;
            let c2 = b
                .add_edgeuse_record(Edgeuse {
                    orientation: orient_of_code(*o2),
                    ..Edgeuse::default()
                })
                .map_err(|x| harness(x.to_string()))?;
            let r = ed::add_oriented_eu_pair(
                b,
                eid(*e),
                a,
                c2,
                opt_id(*sector, EdgeuseId::from_index),
            );
            match status_of(&r)? {
                Some(s) => {
                    out.push(s);
                    put_state(b, &mut out);
                }
                None => out.push(NULL_DEREF),
            }
        }
        OpIn::ManifoldEnd { e, vu, is_start } => {
            let r = ed::insert_manifold_edge_end(
                b,
                eid(*e),
                opt_id(*vu, VertexuseId::from_index),
                *is_start,
            );
            match status_of(&r)? {
                Some(s) => {
                    out.push(s);
                    put_state(b, &mut out);
                }
                None => out.push(NULL_DEREF),
            }
        }
        OpIn::Gaps {
            e,
            which,
            want,
            tol,
        } => {
            let t = (*tol >= 0.0).then_some(*tol);
            let r = match which {
                0 => ed::max_gap_3d(&k, b, eid(*e), *want, t),
                1 => ed::max_up_dim_gap_3d(&k, b, eid(*e), *want, t),
                _ => ed::max_down_dim_gap_3d(b, eid(*e), *want, t),
            };
            match r {
                Ok(g) => {
                    out.push(0.0);
                    match g.max {
                        Some(m) => out.extend(gap_values(&m)),
                        None => out.extend([-1.0, 0.0]),
                    }
                    match g.gaps {
                        Some(gs) => {
                            out.push(gs.len() as f64);
                            for x in &gs {
                                out.extend(gap_values(x));
                            }
                        }
                        None => out.push(-1.0),
                    }
                }
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::Notify { e, operation } => {
            let mut records = std::collections::BTreeMap::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            match ed::notify(b, eid(*e), &mut env, notify_operation(*operation)?, [None; 3]) {
                Ok(()) => {
                    out.push(0.0);
                    put_state(b, &mut out);
                }
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::SetCurve {
            e,
            curve,
            delete,
            notify,
        } => {
            let e = eid(*e);
            let nc = curve.as_ref().map(rust_curve).transpose()?;
            let mut records = std::collections::BTreeMap::new();
            let mut attrs = AttributeStore::default();
            let mut env = ed::NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let opts = ed::SetCurveOptions {
                delete_old_curve: *delete,
                dbg_warn_leaks: true,
                call_notify: *notify,
            };
            let old = ed::set_curve(b, e, nc, opts, &mut env, [None, None])?;
            out.push(0.0);
            put_curve(&mut out, b.edge(e)?.curve.as_ref())?;
            out.push(flag(old.is_some()));
            out.push(flag(old.as_ref().is_some_and(|c| c.owner().is_none())));
            if let Some(c) = old.as_ref() {
                put_curve(&mut out, Some(c))?;
            }
            put_state(b, &mut out);
        }
        OpIn::Radial {
            e,
            eu,
            region,
            orientation,
        } => {
            let Some(ceu) = opt_id(*eu, EdgeuseId::from_index) else {
                return Ok(Output::new().values("state", vec![NULL_DEREF]));
            };
            match ed::find_radial_sector(
                &k,
                b,
                eid(*e),
                ceu,
                opt_id(*region, RegionId::from_index),
                orient_of_code(*orientation),
                None,
            ) {
                Ok(r) => out.extend([
                    0.0,
                    ix(r.radial_edgeuse, EdgeuseId::index),
                    ix(r.coincident_faceuse, FaceuseId::index),
                ]),
                Err(SolidError::Upstream { code, .. }) => out.extend([f64::from(code), -1.0, -1.0]),
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::RadialCurve {
            e,
            param,
            e2,
            orientation,
            force,
        } => {
            let Some(c) = b.edge(eid(*e2))?.curve.clone() else {
                return Ok(Output::new().values("state", vec![NULL_DEREF]));
            };
            let ivl = b.edge(eid(*e2))?.interval;
            match ed::find_radial_sector_of_curve(
                &k,
                b,
                eid(*e),
                *param,
                &c,
                &ivl,
                orient_of_code(*orientation),
                *force,
            ) {
                Ok(r) => out.extend([
                    0.0,
                    ix(r.radial_edgeuse, EdgeuseId::index),
                    ix(r.coincident_faceuse, FaceuseId::index),
                ]),
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::UvOfSurface { e, f } => {
            match ed::uv_trim_curve_of_surface(b, eid(*e), opt_id(*f, FaceId::from_index)) {
                Ok(h) => out.extend([flag(h.is_some()), ix(h, EdgeuseId::index)]),
                Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
                Err(err) => return Err(err.into()),
            }
        }
        OpIn::Seam { e, f } => {
            let r = match opt_id(*f, FaceId::from_index) {
                Some(f) => ed::is_seam_of_surface(b, eid(*e), f),
                None => ed::is_seam(b, eid(*e)),
            };
            out.push(slot(r, flag)?);
        }
        OpIn::Topological { e, gain, .. } => match ed::is_topological(&k, b, eid(*e), *gain) {
            Ok(t) => out.extend([
                flag(t.is_topological),
                ix(t.surf_to_keep, FaceId::index),
                t.dist_between,
            ]),
            Err(SolidError::InvalidInput { .. }) => out.push(NULL_DEREF),
            Err(err) => return Err(err.into()),
        },
        OpIn::Tangent {
            e,
            tol,
            coincidence,
        } => out.push(slot(ed::is_tangent_edge(b, eid(*e), *tol, *coincidence), flag)?),
        OpIn::Blend { e } => out.push(slot(ed::blend_edgeuse(&k, b, eid(*e)), |x| {
            ix(x, EdgeuseId::index)
        })?),
        OpIn::Ode {
            e,
            origin,
            radius,
            t,
            n,
        } => {
            let Some(c) = b.edge(eid(*e))?.curve.as_ref() else {
                return Ok(Output::new().values("state", vec![NULL_DEREF]));
            };
            let mut dydx = vec![1.0; *n as usize];
            let ode = ed::OdeEdgeLength::new(c, Vec3::new(origin[0], origin[1], origin[2]), *radius);
            match ode.evaluate(*t, &mut dydx) {
                Ok(()) => out.push(0.0),
                Err(SolidError::InvalidInput { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF]))
                }
                Err(err) if err.is_unported() => return Err(err.into()),
                Err(_) => out.push(1.0),
            }
            out.extend(dydx);
        }
        OpIn::SetInterval { e, a, b: hi } => {
            ed::set_interval(b, eid(*e), &Extent1d::from_min_max(*a, *hi))?;
            let ivl = ed::interval(b, eid(*e))?;
            out.extend([ivl.min(), ivl.max()]);
        }
    }
    Ok(Output::new().values("state", out))
}

// ---- generated ops ----------------------------------------------------------------------

fn gen_op(rng: &mut Rng) -> OpIn {
    let types = [16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021, 16011, 7];
    let e = gen_sel(rng);
    let scale = *rng.choose(&[1e-6, 1e-3, 1.0, 1.0, 1e3, 1e6]).unwrap_or(&1.0);
    let o = |rng: &mut Rng| rng.below(3) as u8;
    match rng.below(40) {
        0..=4 => OpIn::Query { e, v: if rng.chance(0.2) { -1 } else { gen_sel(rng) } },
        5..=6 => OpIn::Face {
            e,
            f: if rng.chance(0.2) { -1 } else { gen_sel(rng) },
        },
        7..=8 => OpIn::Connected {
            e,
            ty: *rng.choose(&types).unwrap_or(&16016),
            index: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        9..=10 => {
            let n = rng.below(4) as usize;
            let attach = (0..n)
                .map(|_| [*rng.choose(&types[..10]).unwrap_or(&16005), gen_sel(rng)])
                .collect();
            OpIn::Filtered {
                e,
                attr: if rng.chance(0.2) { -1 } else { 1 + rng.below(3) as i64 },
                ty: *rng.choose(&types).unwrap_or(&16016),
                index: if rng.chance(0.5) { -1 } else { gen_sel(rng) },
                attach,
            }
        }
        11 => OpIn::EndTangent {
            e,
            at_start: rng.chance(0.5),
        },
        12 => OpIn::Trimmed { e },
        13..=14 => OpIn::SetTol {
            e,
            tol: *rng.choose(&[0.0, 1e-7, 1e-6, 1e-5, 2e-5, 1e-3, 0.5]).unwrap_or(&1e-6),
            only_larger: rng.chance(0.6),
            cascade: rng.chance(0.5),
        },
        15 => OpIn::SetOrient { e, orientation: o(rng) },
        16..=17 => OpIn::Reverse { e },
        18..=19 => OpIn::OrientSeq {
            edges: (0..rng.below(5)).map(|_| gen_sel(rng)).collect(),
        },
        20 => OpIn::ResetPrimary {
            e,
            eu: match rng.below(20) {
                0..=2 => -1,
                // any edge use of the B-rep
                3..=5 => (1 << 30) + gen_sel(rng),
                _ => gen_sel(rng),
            },
        },
        21 => OpIn::AddPair {
            e,
            o1: o(rng),
            o2: o(rng),
            sector: if rng.chance(0.15) { -1 } else { gen_sel(rng) },
        },
        22 => OpIn::ManifoldEnd {
            e,
            vu: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
            is_start: rng.chance(0.5),
        },
        23..=25 => OpIn::Gaps {
            e,
            which: rng.below(3) as u8,
            want: rng.chance(0.5),
            tol: if rng.chance(0.5) { -1.0 } else { rng.uniform(0.0, 1e-4) },
        },
        26..=27 => OpIn::Notify {
            e,
            operation: rng.below(17) as u8,
        },
        28 => OpIn::SetCurve {
            e,
            curve: rng.chance(0.7).then(|| gen_curve(rng, 3, scale)),
            delete: rng.chance(0.5),
            notify: rng.chance(0.3),
        },
        29 => OpIn::Radial {
            e,
            eu: gen_sel(rng),
            region: if rng.chance(0.5) { -1 } else { gen_sel(rng) },
            orientation: o(rng),
        },
        30 => OpIn::RadialCurve {
            e,
            param: rng.uniform(-1.0, 2.0),
            e2: gen_sel(rng),
            orientation: o(rng),
            force: rng.chance(0.5),
        },
        31 => OpIn::UvOfSurface {
            e,
            f: if rng.chance(0.2) { -1 } else { gen_sel(rng) },
        },
        32 => OpIn::Seam {
            e,
            f: if rng.chance(0.5) { -1 } else { gen_sel(rng) },
        },
        33 => OpIn::Topological {
            e,
            gain: *rng.choose(&[1.0, 0.5, 10.0]).unwrap_or(&1.0),
            want_domain: rng.chance(0.5),
        },
        34 => OpIn::Tangent {
            e,
            tol: *rng.choose(&[0.5, 2.0, 30.0]).unwrap_or(&0.5),
            coincidence: rng.chance(0.5),
        },
        35 => OpIn::Blend { e },
        36..=37 => OpIn::Ode {
            e,
            origin: rng.point3(-2.0, 2.0).map(|x| x * scale),
            radius: rng.uniform(0.0, 2.0) * scale,
            t: rng.uniform(-0.2, 1.2),
            n: 28 + rng.below(8),
        },
        _ => {
            let a = rng.uniform(-10.0, 10.0) * scale;
            OpIn::SetInterval {
                e,
                a,
                b: if rng.chance(0.2) { a } else { a + rng.uniform(-1.0, 5.0) * scale },
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.edge"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
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
