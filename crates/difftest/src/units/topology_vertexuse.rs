//! Runner for unit `topology.vertexuse`. C++: `solidcore_ffi::units::topology_vertexuse`.
//! Rust: `solidcore::topology::vertexuse`.
//!
//! The C++ side builds a real `SmBrep` per call, as the `topology.edgeuse_p1` runner
//! does: read from an upstream `.smb` test file (optionally rebuilding its UV trim
//! curves), a wire edge from a B-spline curve (`CreateWireEdgeFromCurve`), a face from a
//! B-spline surface or a plane (`CreateFaceFromSurface`), or the wire network of
//! `prog_test` `my_test_topology`. There is no Rust B-rep reader or builder yet (units
//! `topology.brep_*`), so the Rust side imports the graph the shim exports in traversal
//! order (raw members, including each vertex use's cached gap states and UV point) into a
//! [`Brep`] and runs the ported functions on it; their results are compared as traversal
//! indices, flags, statuses and values. `init_gaps` has the C++ side compute the cached
//! gaps before the op, so that resets are observable.
//!
//! The vertex use selector is taken modulo the vertex use count (so generated cases need
//! no counts); target selectors of `IsConnectedTo` likewise, `-1` selecting null.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::{BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
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
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::topology::Topology;
use solidcore::topology::vertexuse as vu1;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, ObjectHandle, Region, RegionId, Shell, ShellId,
    Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_vertexuse::{self as ffi, Op};

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
    16006, 16005, 16001, 16010, 12060, 11001, 16013, 16004, 16999,
];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: i64 = -2;

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve: `dim`, control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrvIn {
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BssIn {
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default)]
    pub(crate) form: u8,
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BrepIn {
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    VuQuery {
        vu: i64,
    },
    Connected {
        vu: i64,
        ty: i64,
        index: i64,
    },
    /// `which`: 0 `GetVertexEdgeGap`, 1 `GetVertexFaceGap`, 2 `CalcVertexEdgeGap`, 3
    /// `CalcVertexFaceGap`, 4 `InitVertexFaceGap`.
    Gaps {
        vu: i64,
        which: u8,
        flag: bool,
        want_out: bool,
    },
    Notify {
        vu: i64,
        operation: u8,
    },
    UvPoint {
        vu: i64,
        ok_to_make: bool,
    },
    UvSector {
        vu: i64,
        ok_to_make: bool,
    },
    MissingSeam {
        vu: i64,
        want_u: bool,
        want_v: bool,
        uv_ok: bool,
    },
    OnPole {
        vu: i64,
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

pub(crate) fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
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

pub(crate) fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
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
            let mu = b
                .u
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("surface without u knots"))?;
            let mv = b
                .v
                .len()
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
pub(crate) fn path_of(b: &BrepIn) -> Result<String, HarnessError> {
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
    }
    v.push(flag(i.validate_only));
    v.push(flag(i.init_gaps));
    Ok(v)
}

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
pub(crate) fn sel(s: i64, n: usize) -> i64 {
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
    crate::export_memo::export("topology_vertexuse", &[&path], &block, || {
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
/// another `SmBSplineCurve` kind is an error for a UV trim curve (`uv`) and its B-spline
/// part for an edge curve (this runner compares no edge curve values; the `heal.gap`
/// runner, which reuses this importer, gives the C++ edges the same B-splines).
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

/// An exported surface block: the surface, whether it was imported exactly, and the
/// exported `SmBSplineSurface` subclass type code (`None` for a plane or no surface).
type SurfaceBlock = (Option<Surface>, bool, Option<i64>);

/// A face's surface block: an exact analytic head (`2 ...`,
/// [`crate::util::read_exact_surface`]) and the regular block after it, or the regular
/// block alone.
fn read_face_surface(rd: &mut Rd<'_>) -> Result<SurfaceBlock, HarnessError> {
    if rd.v.get(rd.at) != Some(&2.0) {
        return read_surface(rd);
    }
    rd.at += 1;
    let exact = crate::util::read_exact_surface(&mut || rd.num())?;
    let (fallback, fallback_exact, ty) = read_surface(rd)?;
    match exact {
        Some(s) => {
            crate::util::check_exact_base(&s, fallback.as_ref())?;
            Ok((Some(s), true, ty))
        }
        None => Ok((fallback, fallback_exact, ty)),
    }
}

fn read_surface(rd: &mut Rd<'_>) -> Result<SurfaceBlock, HarnessError> {
    let kind = rd.num()?;
    if kind == -1.0 {
        return Ok((None, true, None));
    }
    if kind == 1.0 {
        // an SmPlane: origin, axes, scales, analytic domain
        let o = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let x = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let y = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let sc = Vec2::new(rd.num()?, rd.num()?);
        let d = Extent2d::from_bounds(rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        return Ok((Some(Surface::from(Plane::new(o, x, y, sc, &d))), true, None));
    }
    if kind != 0.0 {
        return Err(harness("export: a surface is not a B-spline or plane"));
    }
    // an SmBSplineSurface subclass other than SmPlane: only its B-spline part is
    // imported, so its evaluations are not the upstream ones
    let ty = rd.idx()?;
    let exact = ty == B_SPLINE_SURFACE_TYPE;
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
    Ok((Some(Surface::from(b)), exact, Some(ty)))
}

/// Reads an exported region record's shell list (`SmRegion::m_pList` from its head).
pub(crate) fn region_shell_list(list: Vec<i64>) -> Result<Vec<ShellId>, HarnessError> {
    list.into_iter()
        .map(|x| opt(x, ShellId::from_index)?.ok_or_else(|| harness("null shell")))
        .collect()
}

/// An exported shell record's type and face-use list (`m_tShellType`, `m_pList`).
pub(crate) fn shell_members(
    ty: i64,
    list: Vec<i64>,
) -> Result<(ShellType, ShellMembers), HarnessError> {
    let list = list
        .into_iter()
        .map(|x| opt(x, FaceuseId::from_index)?.ok_or_else(|| harness("null face use")))
        .collect::<Result<Vec<_>, _>>()?;
    let members = if list.is_empty() {
        ShellMembers::Empty
    } else {
        ShellMembers::Faceuses(list)
    };
    Ok((ShellType::from_code(ty), members))
}

/// Reads the version-3 shell/region state block that the shims append after the vertices
/// (`sc_topology_fixture::append_shell_state`, shared with `topology.edgeuse_p1`'s
/// export): the infinite region, each region's raw void flag, each wire or vertex shell's
/// list head, each vertex's use list and each face use's `m_pNext` / `m_pLast`. Every value
/// is read from the native record; nothing is inferred. Returns the infinite region.
pub(crate) fn read_shell_state(
    ex: &[f64],
    at: &mut usize,
    regions: &mut [Region],
    shells: &mut [Shell],
    vertices: &mut [Vertex],
    faceuses: &mut [Faceuse],
) -> Result<Option<RegionId>, HarnessError> {
    let mut rd = Rd { v: ex, at: *at };
    if rd.idx()? != 3 {
        return Err(harness("export: unsupported shell extension version"));
    }
    let infinite_region = opt(rd.idx()?, RegionId::from_index)?;
    for region in regions.iter_mut() {
        region.is_void = i32::try_from(rd.idx()?)
            .map_err(|_| harness("export: void flag outside native int range"))?;
    }
    for shell in shells.iter_mut() {
        let head = rd.idx()?;
        if shell.is_wire_shell() {
            shell.members = opt(head, EdgeuseId::from_index)?
                .map_or(ShellMembers::Empty, ShellMembers::Edgeuse);
        } else if shell.is_vertex_shell() {
            shell.members = opt(head, VertexuseId::from_index)?
                .map_or(ShellMembers::Empty, ShellMembers::Vertexuse);
        } else if head != -1 {
            return Err(harness("export: unexpected shell head"));
        }
    }
    for vertex in vertices.iter_mut() {
        vertex.vertexuses = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, VertexuseId::from_index)?.ok_or_else(|| harness("null vertex use")))
            .collect::<Result<Vec<_>, _>>()?;
    }
    for faceuse in faceuses.iter_mut() {
        faceuse.sibling_links = Some((
            opt(rd.idx()?, FaceuseId::from_index)?,
            opt(rd.idx()?, FaceuseId::from_index)?,
        ));
    }
    *at = rd.at;
    Ok(infinite_region)
}

/// Builds the Rust B-rep from the shim's export (see the module docs), with whether
/// each face's surface was imported exactly.
pub(crate) fn import(ex: &[f64]) -> Result<(Brep, Vec<bool>), HarnessError> {
    import_with_types(ex).map(|(b, exact, _)| (b, exact))
}

/// An imported B-rep with, per face, whether its surface was imported exactly and its
/// exported surface subclass type code (`None` for a plane or no surface).
pub(crate) type TypedImport = (Brep, Vec<bool>, Vec<Option<i64>>);

/// [`import`], also with each face's exported surface subclass type code.
pub(crate) fn import_with_types(ex: &[f64]) -> Result<TypedImport, HarnessError> {
    let c = ffi::counts(ex).ok_or_else(|| harness("import of a failed build"))?;
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let mut rd = Rd { v: ex, at: 1 };
    let validate_only = rd.num()? != 0.0;
    rd.at = 12;

    // Region shell lists, shell types and face-use lists are the native lists from
    // their heads; the void flags, the infinite region, the wire/vertex shell heads, the
    // vertex-use lists and the face-use links follow the vertices (`read_shell_state`).
    let mut regions = Vec::new();
    for _ in 0..n_r {
        let in_brep = rd.num()? != 0.0;
        let shells = region_shell_list(rd.list()?)?;
        regions.push(Region {
            in_brep,
            shells,
            ..Region::default()
        });
    }
    let mut shells = Vec::new();
    for _ in 0..n_s {
        let region = opt(rd.idx()?, RegionId::from_index)?;
        let (shell_type, members) = shell_members(rd.idx()?, rd.list()?)?;
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
    let mut face_types = Vec::new();
    for f in 0..n_f {
        let (mut surface, exact, ty) = read_face_surface(&mut rd)?;
        exact_faces.push(exact);
        face_types.push(ty);
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
    for _ in 0..n_e {
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
        let face_init = rd.num()? != 0.0;
        let edge_init = rd.num()? != 0.0;
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
    let infinite_region = read_shell_state(
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
        let shell = face_shells
            .get(f)
            .copied()
            .unwrap_or(ShellId::from_index(0));
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
    Ok((b, exact_faces, face_types))
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
    let mut ks: Vec<f64> = (0..inner)
        .map(|_| a + (b - a) * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(ks);
    u.extend(vec![b; p + 1]);
    u
}

pub(crate) fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
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

pub(crate) fn gen_surface(rng: &mut Rng) -> SrfIn {
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

pub(crate) fn domain(s: &SrfIn) -> [f64; 4] {
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

pub(crate) fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(10) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smb")).to_string(),
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
        _ => BrepIn::Network,
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

// ---- ops ---------------------------------------------------------------------------------

/// The op with its selectors resolved against the B-rep's counts; `None` when the
/// B-rep has no vertex use.
fn resolve(op: &OpIn, c: [usize; 10]) -> Option<OpIn> {
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    if n_vu == 0 {
        return None;
    }
    let v = |s: i64| sel(s, n_vu).max(0);
    Some(match op.clone() {
        OpIn::VuQuery { vu } => OpIn::VuQuery { vu: v(vu) },
        OpIn::Connected { vu, ty, index } => {
            let n = match ty {
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
            };
            OpIn::Connected {
                vu: v(vu),
                ty,
                index: if n == 0 { index.min(-1) } else { sel(index, n) },
            }
        }
        OpIn::Gaps {
            vu,
            which,
            flag,
            want_out,
        } => OpIn::Gaps {
            vu: v(vu),
            which,
            flag,
            want_out,
        },
        OpIn::Notify { vu, operation } => OpIn::Notify {
            vu: v(vu),
            operation,
        },
        OpIn::UvPoint { vu, ok_to_make } => OpIn::UvPoint {
            vu: v(vu),
            ok_to_make,
        },
        OpIn::UvSector { vu, ok_to_make } => OpIn::UvSector {
            vu: v(vu),
            ok_to_make,
        },
        OpIn::MissingSeam {
            vu,
            want_u,
            want_v,
            uv_ok,
        } => OpIn::MissingSeam {
            vu: v(vu),
            want_u,
            want_v,
            uv_ok,
        },
        OpIn::OnPole { vu } => OpIn::OnPole { vu: v(vu) },
    })
}

/// The op arguments after the B-rep block (resolved selectors).
fn op_args(op: &OpIn) -> (Op, Vec<f64>) {
    match op {
        OpIn::VuQuery { vu } => (Op::VuQuery, vec![*vu as f64]),
        OpIn::Connected { vu, ty, index } => {
            (Op::Connected, vec![*vu as f64, *ty as f64, *index as f64])
        }
        OpIn::Gaps {
            vu,
            which,
            flag: f,
            want_out,
        } => (
            Op::Gaps,
            vec![*vu as f64, f64::from(*which), flag(*f), flag(*want_out)],
        ),
        OpIn::Notify { vu, operation } => (Op::Notify, vec![*vu as f64, f64::from(*operation)]),
        OpIn::UvPoint { vu, ok_to_make } => (Op::UvPoint, vec![*vu as f64, flag(*ok_to_make)]),
        OpIn::UvSector { vu, ok_to_make } => (Op::UvSector, vec![*vu as f64, flag(*ok_to_make)]),
        OpIn::MissingSeam {
            vu,
            want_u,
            want_v,
            uv_ok,
        } => (
            Op::MissingSeam,
            vec![*vu as f64, flag(*want_u), flag(*want_v), flag(*uv_ok)],
        ),
        OpIn::OnPole { vu } => (Op::OnPole, vec![*vu as f64]),
    }
}

/// The output key of an op (the values are compared as built by both sides).
fn output(op: &OpIn, body: &[f64]) -> Output {
    match op {
        OpIn::VuQuery { .. } | OpIn::Connected { .. } | OpIn::MissingSeam { .. } => {
            Output::new().ints("q", ints(body))
        }
        _ => Output::new().values("state", body.to_vec()),
    }
}

/// Whether the op's vertex use is on a face whose surface was not imported exactly (an
/// `SmBSplineSurface` subclass other than `SmPlane`, imported as its B-spline part):
/// the ops reaching the face's surface would evaluate a different surface, so such
/// cases are not compared (both sides report `inexact_surface`).
fn inexact(op: &OpIn, b: &Brep, exact: &[bool]) -> bool {
    let vu = match op {
        OpIn::VuQuery { vu }
        | OpIn::Connected { vu, .. }
        | OpIn::Gaps { vu, .. }
        | OpIn::Notify { vu, .. }
        | OpIn::UvPoint { vu, .. }
        | OpIn::UvSector { vu, .. }
        | OpIn::MissingSeam { vu, .. }
        | OpIn::OnPole { vu } => *vu,
    };
    if matches!(
        op,
        OpIn::VuQuery { .. } | OpIn::Connected { .. } | OpIn::Notify { .. }
    ) {
        return false;
    }
    let Ok(Some(fu)) = vu1::faceuse(b, VertexuseId::from_index(vu as u32)) else {
        return false;
    };
    let Ok(Some(f)) = b.faceuse(fu).map(|r| r.face) else {
        return false;
    };
    !exact.get(f.index() as usize).copied().unwrap_or(true)
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(Output::err("no_vertexuse"));
    };
    let (b, exact) = import(&ex)?;
    if inexact(&op, &b, &exact) {
        return Ok(Output::err("inexact_surface"));
    }
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

/// The value of a query slot: [`NULL_DEREF`] where upstream dereferences null;
/// deferral and missing ids propagate.
fn slot<T>(r: Result<T, SolidError>, f: impl FnOnce(T) -> i64) -> Result<i64, SolidError> {
    match r {
        Ok(v) => Ok(f(v)),
        Err(SolidError::InvalidInput { .. }) => Ok(NULL_DEREF),
        Err(e) => Err(e),
    }
}

/// An upstream status: `0` for success, the `SmStatus` of an upstream error.
fn status_of(e: &SolidError) -> Result<f64, SolidError> {
    match e {
        SolidError::Upstream { code, .. } => Ok(f64::from(*code)),
        other => Err(other.clone()),
    }
}

/// The cached gap states of every vertex use, as the shim's `put_gap_states`.
fn gap_states(b: &Brep, out: &mut Vec<f64>) {
    for (_, r) in b.vertexuses().iter() {
        out.push(flag(r.vertex_edge_gap.is_init()));
        out.push(flag(r.vertex_face_gap.is_init()));
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

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (mut b, exact) = import(&ex)?;
    let Some(op) = resolve(&i.op, c) else {
        return Ok(Output::err("no_vertexuse"));
    };
    if inexact(&op, &b, &exact) {
        return Ok(Output::err("inexact_surface"));
    }
    // The export says whether each vertex use's cached vertex/face gap is set, not its
    // values: refill the set ones by `CalcVertexFaceGap(NULL, TRUE)`, their only setter
    // upstream, so the ops that read the cache (`IsOnPole`: its face UV) see real values.
    if let Err(e) = refill_face_gaps(&mut b) {
        return rust_err(&e);
    }
    match run_rust_op(&mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// Recomputes, in arena order, every vertex use's cached vertex/face gap the import
/// marked as set (see [`run_rust`]). Other runners that import through
/// [`import_with_types`] and read the cache call it too.
pub(crate) fn refill_face_gaps(b: &mut Brep) -> Result<(), SolidError> {
    let k = Kernel::default();
    let set: Vec<VertexuseId> = b
        .vertexuses()
        .iter()
        .filter(|(_, r)| r.vertex_face_gap.is_init())
        .map(|(id, _)| id)
        .collect();
    for vu in set {
        vu1::calc_vertex_face_gap(&k, b, vu, false, true)?;
    }
    Ok(())
}

fn run_rust_op(b: &mut Brep, op: &OpIn) -> Result<Output, Fail> {
    let k = Kernel::default();
    let vid = |x: i64| VertexuseId::from_index(x as u32);
    let body: Vec<f64> = match op {
        OpIn::VuQuery { vu } => {
            let v = vid(*vu);
            let mut q = vec![
                vu1::vertexuse_type(b, v)?,
                id(vu1::vertex(b, v)?, VertexId::index),
                id(vu1::edgeuse(b, v)?, EdgeuseId::index),
                slot(vu1::faceuse(b, v), |x| id(x, FaceuseId::index))?,
                slot(vu1::loopuse(b, v), |x| id(x, LoopuseId::index))?,
                slot(vu1::shell(b, v), |x| id(x, ShellId::index))?,
                i64::from(vu1::is_loop_vertexuse(b, v)?),
                i64::from(vu1::is_shell_vertexuse(b, v)?),
                i64::from(vu1::is_edge_vertexuse(b, v)?),
                i64::from(vu1::has_vertex_edge_gap(b, v)?),
                i64::from(vu1::has_vertex_face_gap(b, v)?),
                slot(vu1::is_strut_vertexuse(b, v), i64::from)?,
                vu1::TYPE,
            ];
            q.extend(KIND_TYPES.iter().map(|&t| i64::from(vu1::is_kind_of(t))));
            let r = b.vertexuse(v)?;
            q.push(i64::from(r.vertex_edge_gap.is_init()));
            q.push(i64::from(r.vertex_face_gap.is_init()));
            return Ok(Output::new().ints("q", q));
        }
        OpIn::Connected { vu, ty, index } => {
            let t = target(*ty, *index);
            let q = slot(vu1::is_connected_to(b, vid(*vu), t), i64::from)?;
            return Ok(Output::new().ints("q", vec![q]));
        }
        OpIn::Gaps {
            vu,
            which,
            flag: f,
            want_out,
        } => {
            let v = vid(*vu);
            let mut out = Vec::new();
            let r = match which {
                0 => vu1::vertex_edge_gap(&k, b, v, *f).map(|g| vec![flag(g.is_some())]),
                1 => vu1::vertex_face_gap(&k, b, v, *f).map(|g| vec![flag(g.is_some())]),
                2 => vu1::calc_vertex_edge_gap(&k, b, v, *want_out, *f)
                    .map(|g| vec![0.0, g.map_or(-1.0, |g| flag(g.is_init()))]),
                3 => match vu1::calc_vertex_face_gap(&k, b, v, *want_out, *f) {
                    Ok(g) => Ok(vec![0.0, g.map_or(-1.0, |g| flag(g.is_init()))]),
                    Err(e @ SolidError::Upstream { .. }) => {
                        // the output was reset before the failing step
                        Ok(vec![status_of(&e)?, if *want_out { 0.0 } else { -1.0 }])
                    }
                    Err(e) => Err(e),
                },
                _ => vu1::init_vertex_face_gap(b, v).map(|()| vec![0.0]),
            };
            match r {
                Ok(x) => out.extend(x),
                Err(SolidError::InvalidInput { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF as f64]))
                }
                Err(e) => return Err(e.into()),
            }
            gap_states(b, &mut out);
            out
        }
        OpIn::Notify { vu, operation } => {
            let v = vid(*vu);
            let this = ObjectRef {
                type_code: vu1::TYPE,
                key: ObjectKey(1),
            };
            let mut records = std::collections::BTreeMap::new();
            records.insert(this.key, Topology::new());
            let mut attrs = AttributeStore::default();
            vu1::notify(
                b,
                v,
                &mut records,
                &mut attrs,
                None,
                notify_operation(*operation)?,
                this,
                [None; 3],
            )?;
            let mut out = Vec::new();
            gap_states(b, &mut out);
            out
        }
        OpIn::UvPoint { vu, ok_to_make } => {
            match vu1::compute_uv_point(&k, b, vid(*vu), *ok_to_make) {
                Ok(p) => vec![0.0, p.x, p.y],
                Err(SolidError::InvalidInput { .. }) => vec![NULL_DEREF as f64],
                Err(e) => vec![
                    status_of(&e)?,
                    solidcore::math::UNDEF_DOUBLE,
                    solidcore::math::UNDEF_DOUBLE,
                ],
            }
        }
        OpIn::UvSector { vu, ok_to_make } => {
            match vu1::compute_uv_sector(&k, b, vid(*vu), *ok_to_make) {
                Ok(s) => {
                    let io = &s.io;
                    let mut out = vec![
                        if s.success {
                            0.0
                        } else {
                            f64::from(solidcore::error::SM_ERR)
                        },
                        id(io.vertexuse, VertexuseId::index) as f64,
                        id(io.beg_edgeuse, EdgeuseId::index) as f64,
                        id(io.end_edgeuse, EdgeuseId::index) as f64,
                        io.beg_edge_param,
                        io.end_edge_param,
                    ];
                    for p in [
                        io.beg_edge_uv_point,
                        io.end_edge_uv_point,
                        io.beg_edge_uv_tan,
                        io.end_edge_uv_tan,
                    ] {
                        out.extend([p.x, p.y]);
                    }
                    out.push(io.sector_ang_deg);
                    out.extend(
                        [
                            io.vertex_on_closed_u,
                            io.vertex_on_closed_v,
                            io.vertex_on_pole,
                            io.vertex_on_flat_corner,
                            io.sector_missing_seam_u,
                            io.sector_missing_seam_v,
                        ]
                        .map(flag),
                    );
                    out
                }
                Err(SolidError::InvalidInput { .. }) => vec![NULL_DEREF as f64],
                Err(e) => return Err(e.into()),
            }
        }
        OpIn::MissingSeam {
            vu,
            want_u,
            want_v,
            uv_ok,
        } => {
            let q = match vu1::is_sector_missing_seam(&k, b, vid(*vu), *uv_ok) {
                Ok(m) => vec![
                    i64::from(m.missing),
                    if *want_u { i64::from(m.u) } else { -1 },
                    if *want_v { i64::from(m.v) } else { -1 },
                ],
                Err(SolidError::InvalidInput { .. }) => vec![NULL_DEREF],
                Err(e) => return Err(e.into()),
            };
            return Ok(Output::new().ints("q", q));
        }
        OpIn::OnPole { vu } => match vu1::is_on_pole(&k, b, vid(*vu)) {
            Ok(p) => {
                let mut out = vec![flag(p)];
                gap_states(b, &mut out);
                out
            }
            Err(SolidError::InvalidInput { .. }) => vec![NULL_DEREF as f64],
            Err(e) => return Err(e.into()),
        },
    };
    Ok(Output::new().values("state", body))
}

// ---- generated ops ----------------------------------------------------------------------

fn gen_op(rng: &mut Rng) -> OpIn {
    let types = [
        16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021, 16011, 7,
    ];
    let vu = gen_sel(rng);
    match rng.below(20) {
        0..=3 => OpIn::VuQuery { vu },
        4..=6 => OpIn::Connected {
            vu,
            ty: *rng.choose(&types).unwrap_or(&16006),
            index: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        7..=10 => OpIn::Gaps {
            vu,
            which: rng.below(5) as u8,
            flag: rng.chance(0.5),
            want_out: rng.chance(0.5),
        },
        11..=12 => OpIn::Notify {
            vu,
            operation: rng.below(17) as u8,
        },
        13..=14 => OpIn::UvPoint {
            vu,
            ok_to_make: rng.chance(0.5),
        },
        15..=16 => OpIn::UvSector {
            vu,
            ok_to_make: rng.chance(0.5),
        },
        17..=18 => OpIn::MissingSeam {
            vu,
            want_u: rng.chance(0.5),
            want_v: rng.chance(0.5),
            uv_ok: rng.chance(0.5),
        },
        _ => OpIn::OnPole { vu },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.vertexuse"
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
            init_gaps: rng.chance(0.4),
            op,
        };
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_encoding_rejects_empty_knots_before_writing() {
        for (u_empty, v_empty, message) in [
            (true, false, "surface without u knots"),
            (false, true, "surface without v knots"),
            (true, true, "surface without u knots"),
        ] {
            let surface = SrfIn::Bspline(BssIn {
                oob: false,
                form: 0,
                net: vec![
                    vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.0, NOW]],
                    vec![[1.0, 0.0, 0.0, NOW], [1.0, 1.0, 0.0, NOW]],
                ],
                p: 1,
                q: 1,
                u: if u_empty {
                    vec![]
                } else {
                    vec![0.0, 0.0, 1.0, 1.0]
                },
                v: if v_empty {
                    vec![]
                } else {
                    vec![0.0, 0.0, 1.0, 1.0]
                },
            });
            let mut encoded = vec![42.0];
            let error = push_surface(&mut encoded, &surface).unwrap_err();
            assert_eq!(error.to_string(), message);
            assert_eq!(encoded, vec![42.0]);
        }
    }
}
