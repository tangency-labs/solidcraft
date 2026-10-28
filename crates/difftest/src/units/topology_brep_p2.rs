//! Runner for unit `topology.brep_p2`. C++: `solidcore_ffi::units::topology_brep_p2`.
//! Rust: `solidcore::topology::brep::p2`.
//!
//! The C++ side builds a real `SmBrep` per call with the shared topology fixture (an
//! upstream `.smb` file, a wire edge, a one-face B-rep or the `my_test_topology` wire
//! network), replaces analytic geometry by plain B-spline copies, and runs one
//! `SmBrep` member. The Rust side builds a wire edge, face or network source itself
//! ([`crate::own_builder`] under this shim's conventions: lists registered first, its
//! own owner identities). For an `.smb` file it imports the graph the shim exports (the
//! `topology.vertexuse` importer), restores what the import cannot carry (the B-rep
//! tolerance, infinite region, region list head, editing flag and each vertex's vertex
//! use order, from the shim's `Extras`), runs the port and writes the same output
//! layout (`out`).
//!
//! Entity selectors are taken modulo the matching entity count (`-1` selects null);
//! an op that needs an entity the B-rep lacks is skipped on both sides
//! (`no target`). The B-rep cache does not exist on either side (fresh B-reps), so
//! `Notify` is compared without it. `DeleteStaleCaches` first seeds trimmed-surface
//! caches by face-index masks on both sides (C++: `SmCacheMgr`; Rust: the caller-held
//! map) and compares which survive.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::geom::tol;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::point_class::{PointClassType, PointClassification};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::SurfaceCacheOptions;
use solidcore::surface::trim_srf_cache::TrimSrfCache;
use solidcore::topology::brep::p1 as bp1;
use solidcore::topology::brep::p2 as bp2;
use solidcore::topology::brep_cache::CacheObject;
use solidcore::topology::edge::NotifyEnv;
use solidcore::topology::topology_traverser::Marks;
use solidcore::topology::{
    Brep, BrepId, EdgeId, EdgeuseId, FaceId, FaceuseId, LoopId, LoopuseId, RegionId, ShellId,
    VertexId, VertexuseId,
};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_brep_p2::{self as ffi, Op};
use solidcore_ffi::units::topology_vertexuse::counts;

use super::topology_vertexuse::{
    gen_brep, gen_curve, gen_surface, import, path_of, push_curve, push_surface, BrepIn, CrvIn,
    SrfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmTopoTypes.h` codes, in the export's slot order.
const TYPES: [i64; 10] = [
    16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021,
];
const BREP_TYPE: i64 = 16011;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    /// `0` `GetEdgeuses`, `1` `GetShellVertices`, `2` `IsManifoldSolid`, `3` `IsSheet`,
    /// `4` `IsOrientedSheet`, `5` `GetMinEdgeBBoxSize`.
    Query {
        which: u8,
    },
    SetTol {
        tol: f64,
        only_larger: bool,
        cascade: bool,
    },
    Rebuild,
    /// `0` `MakeShellVertexTopology`, `1` `MakeEdgeVertexTopology`, `2` `MakeEdgeTopology`.
    Make {
        which: u8,
        region: i64,
        v1: i64,
        v2: i64,
    },
    /// `class`: an `SmPointClassificationType` code (`102` face, `103` edge, `104`
    /// vertex: the object's kind; any other with a null (`object < 0`) or vertex
    /// object); `orient` an `SmOrientType` code (`0` unknown, `1` same, `2` opposite).
    Sector {
        curve: CrvIn,
        t0: f64,
        t1: f64,
        orient: u8,
        class: i64,
        object: i64,
        t: f64,
        uv: [f64; 2],
    },
    Merge {
        face: i64,
        tol: f64,
        curves: Vec<CrvIn>,
    },
    /// `MergeCurvesOnSurface` with optional UV curves (one per curve) and an optional
    /// intersection surface (`pOptIntersectionSurface`), which enables
    /// `sm_RefineCCIToSurfaces`'s surface/surface/surface refinement.
    MergeSsi {
        face: i64,
        tol: f64,
        curves: Vec<CrvIn>,
        #[serde(default)]
        uv_curves: Option<Vec<CrvIn>>,
        #[serde(default)]
        surface: Option<SrfIn>,
    },
    Mate {
        edge: i64,
        surface: SrfIn,
        ext: f64,
        uv_ext: f64,
    },
    /// `data`: `[type, selector]` per data object (`[-1, -1]` null, type `16011` the
    /// B-rep).
    Notify {
        operation: u8,
        data: [[i64; 2]; 3],
    },
    /// Face `i` gets a trimmed-surface cache when bit `i % 8` of `create` is set,
    /// marked modified when that bit of `modified` is.
    Stale {
        #[serde(default)]
        create: u8,
        #[serde(default)]
        modified: u8,
    },
    Swap,
    /// `keep`: `0` delete, `1` deleted list, `2` keep.
    Replace {
        face: i64,
        surface: SrfIn,
        keep: u8,
        create: bool,
    },
    RayFire {
        p: [f64; 3],
        v: [f64; 3],
        tol: f64,
        limit: Option<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    /// `-1` keep the built B-rep's `m_bEditingEnabled`, else set it.
    #[serde(default = "keep")]
    editing: i64,
    #[serde(flatten)]
    op: OpIn,
}

const fn keep() -> i64 {
    -1
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn head(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut v = Vec::new();
    match &i.brep {
        BrepIn::File { rebuild, .. } => v.extend([0.0, flag(*rebuild)]),
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
    v.push(i.editing as f64);
    Ok(v)
}

/// Selector `s` modulo `n`; `None` for a negative selector or an empty kind.
fn pick(s: i64, n: usize) -> Option<usize> {
    if s < 0 || n == 0 {
        return None;
    }
    Some((s as u64 % n as u64) as usize)
}

/// The op's arguments after the head, with selectors resolved against the counts;
/// `None` when the op needs an entity the B-rep lacks.
fn op_args(op: &OpIn, c: &[usize; 10]) -> Result<Option<(Op, Vec<f64>)>, HarnessError> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = *c;
    let idx = |x: Option<usize>| x.map_or(-1.0, |i| i as f64);
    Ok(Some(match op {
        OpIn::Query { which } => (Op::Query, vec![f64::from(*which)]),
        OpIn::SetTol {
            tol,
            only_larger,
            cascade,
        } => (Op::SetTol, vec![*tol, flag(*only_larger), flag(*cascade)]),
        OpIn::Rebuild => (Op::Rebuild, vec![]),
        OpIn::Make {
            which,
            region,
            v1,
            v2,
        } => {
            let (v1, v2) = (pick(*v1, n_v), pick(*v2, n_v));
            if *which >= 1 && v1.is_none() || *which == 2 && v2.is_none() {
                return Ok(None);
            }
            (
                Op::Make,
                vec![f64::from(*which), idx(pick(*region, n_r)), idx(v1), idx(v2)],
            )
        }
        OpIn::Sector {
            curve,
            t0,
            t1,
            orient,
            class,
            object,
            t,
            uv,
        } => {
            let (ty, n) = match class {
                102 => (16002, n_f),
                103 => (16016, n_e),
                104 => (16021, n_v),
                // another class: upstream never reads the object (null for `-1`, else
                // a vertex)
                _ => (if *object < 0 { -1 } else { 16021 }, n_v),
            };
            let o = match pick(*object, n) {
                Some(o) => o as f64,
                None if ty == -1 => -1.0,
                None => return Ok(None),
            };
            let mut v = Vec::new();
            push_curve(&mut v, curve)?;
            v.extend([
                *t0,
                *t1,
                f64::from(*orient),
                *class as f64,
                f64::from(ty),
                o,
            ]);
            v.extend([*t, uv[0], uv[1]]);
            (Op::Sector, v)
        }
        OpIn::Merge { face, tol, curves } => {
            let Some(f) = pick(*face, n_f) else {
                return Ok(None);
            };
            let mut v = vec![f as f64, *tol, curves.len() as f64];
            for c in curves {
                push_curve(&mut v, c)?;
            }
            (Op::Merge, v)
        }
        OpIn::MergeSsi {
            face,
            tol,
            curves,
            uv_curves,
            surface,
        } => {
            let Some(f) = pick(*face, n_f) else {
                return Ok(None);
            };
            let mut v = vec![f as f64, *tol, curves.len() as f64];
            for c in curves {
                push_curve(&mut v, c)?;
            }
            v.push(flag(uv_curves.is_some()));
            if let Some(uv) = uv_curves {
                if uv.len() != curves.len() {
                    return Err(harness("one UV curve per curve"));
                }
                for c in uv {
                    push_curve(&mut v, c)?;
                }
            }
            v.push(flag(surface.is_some()));
            if let Some(srf) = surface {
                push_surface(&mut v, srf)?;
            }
            (Op::MergeSsi, v)
        }
        OpIn::Mate {
            edge,
            surface,
            ext,
            uv_ext,
        } => {
            let Some(e) = pick(*edge, n_e) else {
                return Ok(None);
            };
            let mut v = vec![e as f64];
            push_surface(&mut v, surface)?;
            v.extend([*ext, *uv_ext]);
            (Op::Mate, v)
        }
        OpIn::Notify { operation, data } => {
            let mut v = vec![f64::from(*operation)];
            for [ty, s] in data {
                if *ty == BREP_TYPE {
                    v.extend([BREP_TYPE as f64, 0.0]);
                    continue;
                }
                let slot = TYPES.iter().position(|t| t == ty);
                match slot.and_then(|k| pick(*s, c.get(k).copied().unwrap_or(0))) {
                    Some(i) => v.extend([*ty as f64, i as f64]),
                    None => v.extend([-1.0, -1.0]),
                }
            }
            (Op::Notify, v)
        }
        OpIn::Stale { create, modified } => {
            (Op::Stale, vec![f64::from(*create), f64::from(*modified)])
        }
        OpIn::Swap => (Op::Swap, vec![]),
        OpIn::Replace {
            face,
            surface,
            keep,
            create,
        } => {
            let Some(f) = pick(*face, n_f) else {
                return Ok(None);
            };
            let mut v = vec![f as f64];
            push_surface(&mut v, surface)?;
            v.extend([f64::from(*keep), flag(*create)]);
            (Op::Replace, v)
        }
        OpIn::RayFire { p, v, tol, limit } => {
            let mut a = Vec::new();
            a.extend(p);
            a.extend(v);
            a.extend([*tol, flag(limit.is_some()), limit.unwrap_or(0.0)]);
            (Op::RayFire, a)
        }
    }))
}

fn ffi_err(e: &solidcore_ffi::FfiError) -> HarnessError {
    harness(e.to_string())
}

/// `(path, head, counts)` of a built B-rep, or the build failure's output.
type Prepared = Result<(String, Vec<f64>, [usize; 10]), Output>;

fn prepare(i: &Inputs) -> Result<Prepared, HarnessError> {
    let path = path_of(&i.brep)?;
    let h = head(i)?;
    let export = ffi::eval(Op::Export, &path, &h).map_err(|e| ffi_err(&e))?;
    match counts(&export) {
        Some(c) => Ok(Ok((path, h, c))),
        None => Ok(Err(Output::new().values("out", export))),
    }
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (path, mut input, c) = match prepare(i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some((op, args)) = op_args(&i.op, &c)? else {
        return Ok(Output::err("no target"));
    };
    input.extend(args);
    let out = ffi::eval(op, &path, &input).map_err(|e| ffi_err(&e))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side ----------------------------------------------------------------------------

/// The identity each entity is notified and attributed under: faces as the importer
/// keys their surfaces' owners (`1 + index`), every other type `type << 32 | 1 + index`
/// (so records of different types never share a key).
fn key_of(e: EntityRef) -> ObjectRef {
    let (type_code, i) = match e {
        EntityRef::Region(x) => (16012, x.index()),
        EntityRef::Shell(x) => (16013, x.index()),
        EntityRef::Faceuse(x) => (16022, x.index()),
        EntityRef::Face(x) => (16002, x.index()),
        EntityRef::Loopuse(x) => (16004, x.index()),
        EntityRef::Loop(x) => (16003, x.index()),
        EntityRef::Edgeuse(x) => (16005, x.index()),
        EntityRef::Edge(x) => (16016, x.index()),
        EntityRef::Vertexuse(x) => (16006, x.index()),
        EntityRef::Vertex(x) => (16021, x.index()),
        EntityRef::Brep(_) => (16999, 0),
    };
    // faces keep `1 + index` (the importer's surface owners); every other type gets its
    // own key range, so region 0, face 0 and edge 0 are distinct records
    let key = 1 + u64::from(i);
    ObjectRef {
        type_code,
        key: ObjectKey(if type_code == 16002 {
            key
        } else {
            (type_code.unsigned_abs() << 32) | key
        }),
    }
}

fn entity(ty: f64, i: f64) -> Option<EntityRef> {
    let i = i as u32;
    Some(match ty as i64 {
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

/// The Rust B-rep of the case (with the shim's extras restored).
fn rust_brep(path: &str, h: &[f64], c: &[usize; 10]) -> Result<Brep, HarnessError> {
    let export = ffi::eval(Op::Export, path, h).map_err(|e| ffi_err(&e))?;
    let (mut b, exact) = import(&export)?;
    if exact.iter().any(|e| !e) {
        return Err(harness("a face surface was not imported exactly"));
    }
    let ex = ffi::eval(Op::Extras, path, h).map_err(|e| ffi_err(&e))?;
    let mut at = 1usize;
    let mut next = || -> Result<f64, HarnessError> {
        let v = ex
            .get(at)
            .copied()
            .ok_or_else(|| harness("extras too short"))?;
        at += 1;
        Ok(v)
    };
    let tol = next()?;
    let inf = next()?;
    let head_r = next()?;
    let editing = next()? != 0.0;
    b.set_tolerance(tol);
    b.set_infinite_region((inf >= 0.0).then(|| RegionId::from_index(inf as u32)));
    b.set_region_list_head((head_r >= 0.0).then(|| RegionId::from_index(head_r as u32)));
    b.set_editing_enabled(editing);
    for v in 0..c[9] {
        let n = next()? as usize;
        let mut list = Vec::new();
        for _ in 0..n {
            let i = next()?;
            if i < 0.0 {
                return Err(harness("unregistered vertex use in a vertex list"));
            }
            list.push(VertexuseId::from_index(i as u32));
        }
        b.vertex_mut(VertexId::from_index(v as u32))
            .map_err(|e| harness(e.to_string()))?
            .vertexuses = list;
    }
    Ok(b)
}

fn rust_curve(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
        .collect();
    let nl = NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(BSplineCurve::from_nurb(nl, c.dim, false))
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
                .map(|r| {
                    r.iter()
                        .map(|p| CPoint::new(p[0], p[1], p[2], p[3]))
                        .collect()
                })
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

/// What a failed Rust op reports: the upstream status, or the deferral.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

/// The status slot of a Rust result (`0` success, else the upstream code).
fn status<T>(r: Result<T, SolidError>) -> Result<(f64, Option<T>), Fail> {
    match r {
        Ok(v) => Ok((0.0, Some(v))),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => match e {
                SolidError::Upstream { code, .. } => Ok((f64::from(code), None)),
                other => Err(Fail::Harness(harness(format!("rust: {other}")))),
            },
        },
    }
}

fn must<T>(r: Result<T, SolidError>) -> Result<T, Fail> {
    match status(r)? {
        (_, Some(v)) => Ok(v),
        (s, None) => Err(Fail::Harness(harness(format!("rust: status {s}")))),
    }
}

fn idx<I>(o: Option<I>, f: impl Fn(I) -> u32) -> f64 {
    o.map_or(-1.0, |x| f64::from(f(x)))
}

/// The shim's `topology_summary` of the B-rep after a Make op: per region (`GetRegions`)
/// its shells (`GetShells`) as `kind ref`, per vertex (`GetVertices`) its vertex-use
/// count, per edge (`GetEdges`) the vertices and shells of its primary edge use and
/// that use's mate (shells by position in the region-major list, `-1` none).
fn topology_summary(b: &Brep, o: &mut Vec<f64>) -> Result<(), Fail> {
    let h = |e: SolidError| Fail::Harness(harness(e.to_string()));
    let regions = bp1::regions(b, None).map_err(h)?;
    let vertices = bp1::vertices(b, None).map_err(h)?;
    let edges = bp1::edges(b, None).map_err(h)?;
    let pos = |v: &[u32], x: Option<u32>| {
        x.and_then(|x| v.iter().position(|&y| y == x))
            .map_or(-1.0, |i| i as f64)
    };
    let vidx: Vec<u32> = vertices.iter().map(|v| v.index()).collect();
    let eidx: Vec<u32> = edges.iter().map(|e| e.index()).collect();
    let mut all: Vec<u32> = Vec::new();
    o.push(regions.len() as f64);
    for r in regions {
        let shells = b
            .region(r)
            .map_err(h)?
            .get_shells(None, |_, _| Ok(true))
            .map_err(h)?;
        o.push(shells.len() as f64);
        for s in shells {
            all.push(s.index());
            let shell = b.shell(s).map_err(h)?;
            if shell.is_vertex_shell() {
                let v = shell.vertex(b).map_err(h)?;
                o.extend([1.0, pos(&vidx, v.map(VertexId::index))]);
            } else if shell.is_wire_shell() {
                let e = shell.wire_edge(b).map_err(h)?;
                o.extend([2.0, pos(&eidx, e.map(EdgeId::index))]);
            } else {
                o.extend([if shell.is_faceuse_shell() { 3.0 } else { 0.0 }, -1.0]);
            }
        }
    }
    o.push(vertices.len() as f64);
    for v in vertices {
        o.push(b.vertex(v).map_err(h)?.vertexuses.len() as f64);
    }
    o.push(edges.len() as f64);
    for e in edges {
        let p = solidcore::topology::edgeuse::p1::primary_edgeuse(b, e).map_err(h)?;
        let m = match p {
            Some(p) => solidcore::topology::edgeuse::p1::mate(b, p).map_err(h)?,
            None => None,
        };
        for eu in [p, m] {
            let v = match eu {
                Some(eu) => match b.edgeuse(eu).map_err(h)?.vertexuse {
                    Some(vu) => b.vertexuse(vu).map_err(h)?.vertex.map(VertexId::index),
                    None => None,
                },
                None => None,
            };
            o.push(pos(&vidx, v));
        }
        for eu in [p, m] {
            let s = match eu {
                Some(eu) => solidcore::topology::edgeuse::p1::shell(b, eu).map_err(h)?,
                None => None,
            };
            o.push(pos(&all, s.map(ShellId::index)));
        }
    }
    Ok(())
}

fn run_rust_op(k: &mut Kernel, b: &mut Brep, op: Op, a: &[f64]) -> Result<Vec<f64>, Fail> {
    let mut out = vec![0.0];
    let arg = |i: usize| -> Result<f64, Fail> {
        a.get(i)
            .copied()
            .ok_or_else(|| Fail::Harness(harness("args too short")))
    };
    match op {
        Op::Query => match arg(0)? as u8 {
            0 => {
                let l = must(bp2::edgeuses(b, None))?;
                out.push(l.len() as f64);
                out.extend(l.iter().map(|x| f64::from(x.index())));
            }
            1 => {
                let l = must(bp2::shell_vertices(b, None))?;
                out.push(l.len() as f64);
                out.extend(l.iter().map(|x| f64::from(x.index())));
            }
            2 => out.push(flag(must(bp2::is_manifold_solid(b))?)),
            3 => out.push(flag(must(bp2::is_sheet(k, b))?)),
            4 => out.push(flag(must(bp2::is_oriented_sheet(k, b))?)),
            _ => {
                let (size, e) = must(bp2::min_edge_bbox_size(k, b))?;
                out.push(size);
                out.push(idx(e, EdgeId::index));
            }
        },
        Op::SetTol => {
            let tol = tol::zone_tol3d(k, arg(0)?);
            must(bp2::set_tolerance(
                k,
                b,
                tol,
                arg(1)? != 0.0,
                arg(2)? != 0.0,
            ))?;
            out.push(b.tolerance());
            out.extend(b.faces().iter().map(|(_, f)| f.tolerance));
            out.extend(b.edges().iter().map(|(_, e)| e.tolerance));
            out.extend(b.vertices().iter().map(|(_, v)| v.tolerance));
        }
        Op::Rebuild => {
            let (s, _) = status(bp2::rebuild_uv_trim_curves(k, b))?;
            out.push(s);
            out.extend(
                b.edgeuses()
                    .iter()
                    .map(|(_, e)| flag(e.uv_trim_curve.is_some())),
            );
            out.extend(b.edges().iter().map(|(_, e)| e.tolerance));
        }
        Op::Make => {
            let region =
                (arg(1)? >= 0.0).then(|| RegionId::from_index(arg(1).unwrap_or(0.0) as u32));
            let v1 = VertexId::from_index(arg(2)?.max(0.0) as u32);
            let v2 = VertexId::from_index(arg(3)?.max(0.0) as u32);
            let s = match arg(0)? as u8 {
                0 => status(bp2::make_shell_vertex_topology(b, region))?.0,
                1 => status(bp2::make_edge_vertex_topology(b, region, v1))?.0,
                _ => {
                    let mut records = solidcore::topology::topology::NotifyRecords::new();
                    let mut attrs = AttributeStore::default();
                    let mut env = NotifyEnv {
                        records: &mut records,
                        attributes: &mut attrs,
                        context: None,
                        key_of: &key_of,
                    };
                    status(bp2::make_edge_topology(k, b, region, v1, v2, &mut env))?.0
                }
            };
            out.push(s);
            topology_summary(b, &mut out)?;
        }
        Op::Sector => {
            // the curve block: dim n p m cpts(4 * (n + 1)) knots(m + 1)
            let n = arg(1)? as usize;
            let p = arg(2)? as usize;
            let m = arg(3)? as usize;
            let cpts: Vec<[f64; 4]> = (0..=n)
                .map(|j| -> Result<[f64; 4], Fail> {
                    Ok([
                        arg(4 + 4 * j)?,
                        arg(5 + 4 * j)?,
                        arg(6 + 4 * j)?,
                        arg(7 + 4 * j)?,
                    ])
                })
                .collect::<Result<_, _>>()?;
            let at = 4 + 4 * (n + 1);
            let u: Vec<f64> = (0..=m).map(|j| arg(at + j)).collect::<Result<_, _>>()?;
            let crv = SmCurve::from(rust_curve(&CrvIn {
                dim: arg(0)? as u64,
                cpts,
                p,
                u,
            })?);
            let at = at + m + 1;
            // the shim's SmExtent1d(t0, t1), which swaps an inverted pair
            let ivl = Extent1d::from_min_max(arg(at)?, arg(at + 1)?);
            // `SmOrientType`: exactly the three codes (`SM_OT_UNKNOWN` is neither
            // `SAME` nor `OPPOSITE`; upstream tests both per site)
            let orient = match arg(at + 2)? {
                0.0 => OrientType::Unknown,
                1.0 => OrientType::Same,
                2.0 => OrientType::Opposite,
                _ => return Err(Fail::Harness(harness("bad orientation"))),
            };
            let class = PointClassType::from_code(arg(at + 3)? as i32)
                .ok_or_else(|| Fail::Harness(harness("bad point class")))?;
            let obj = entity(arg(at + 4)?, arg(at + 5)?);
            let mut pc = PointClassification::new(tol::zone_tol3d(k, tol::USE_DEFAULT));
            pc.set_class_object(class, obj);
            pc.set_t_param(arg(at + 6)?, false);
            pc.set_uv_param(Point2::new(arg(at + 7)?, arg(at + 8)?));
            let (s, r) = status(bp2::local_curve_sector(k, b, &crv, &ivl, orient, &pc))?;
            out.push(s);
            out.push(idx(r.flatten(), RegionId::index));
        }
        Op::Merge | Op::MergeSsi => {
            return Err(Fail::Harness(harness("merge runs in merge_rust")))
        }
        Op::Mate => {
            let e = EdgeId::from_index(arg(0)? as u32);
            let surf = surface_arg(a, 1)?;
            let ext = arg(a.len() - 2)?;
            let uv = arg(a.len() - 1)?;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let kk: &Kernel = k;
            let (s, _) = status(bp2::mate_lamina_edge_to_surface(
                kk, b, &mut env, e, &surf, ext, uv,
            ))?;
            out.push(s);
        }
        Op::Notify => {
            let operation = NotifyOperation::from_code(arg(0)? as i32)
                .ok_or_else(|| Fail::Harness(harness("bad notify operation")))?;
            let mut data = [None; 3];
            for (j, d) in data.iter_mut().enumerate() {
                let ty = arg(1 + 2 * j)?;
                let i = arg(2 + 2 * j)?;
                *d = if ty as i64 == BREP_TYPE {
                    Some(bp2::NotifyArg::Object(bp2::brep_ref(b)))
                } else {
                    entity(ty, i).map(bp2::NotifyArg::Entity)
                };
            }
            // every notified object, the B-rep included (it is a topology object
            // upstream: `this` and possibly a data object), has its default record
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            records.insert(
                bp2::brep_ref(b).key,
                solidcore::topology::topology::Topology::default(),
            );
            for d in data.iter().flatten() {
                if let bp2::NotifyArg::Entity(e) = d {
                    records.insert(
                        key_of(*e).key,
                        solidcore::topology::topology::Topology::default(),
                    );
                }
            }
            let mut attrs = AttributeStore::default();
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            must(bp2::notify(b, &mut env, None, operation, data))?;
            out.push(0.0);
        }
        Op::Stale => {
            let (create, modified) = (arg(0)? as u32, arg(1)? as u32);
            let faces = must(bp1::faces(b, None))?;
            let mut caches = BTreeMap::new();
            for (i, &f) in faces.iter().enumerate() {
                let bit = (i % 8) as u32;
                let Some(surface) = must(b.face(f))?.surface.clone() else {
                    continue;
                };
                if (create >> bit) & 1 == 0 {
                    continue;
                }
                let mut c =
                    TrimSrfCache::<CacheObject>::new(surface, SurfaceCacheOptions::default());
                c.base_mut().face_was_modified = (modified >> bit) & 1 == 1;
                caches.insert(f, c);
            }
            let (s, _) = status(bp2::delete_stale_caches::<CacheObject>(b, &mut caches))?;
            out.push(s);
            for f in faces {
                out.push(if caches.contains_key(&f) { 1.0 } else { 0.0 });
            }
        }
        Op::Swap => {
            let mut marks = Marks::default();
            let (s, _) = status(bp2::swap_uv(k, b, &mut marks))?;
            out.push(s);
        }
        Op::Replace => {
            let f = FaceId::from_index(arg(0)? as u32);
            let surf = surface_arg(a, 1)?;
            let keep = match arg(a.len() - 2)? as u8 {
                0 => bp2::OldSurface::Delete,
                1 => bp2::OldSurface::AddToDeletedList,
                _ => bp2::OldSurface::Keep,
            };
            let create = arg(a.len() - 1)? != 0.0;
            let mut records = solidcore::topology::topology::NotifyRecords::new();
            let mut attrs = AttributeStore::default();
            let mut env = NotifyEnv {
                records: &mut records,
                attributes: &mut attrs,
                context: None,
                key_of: &key_of,
            };
            let kk: &Kernel = k;
            let (s, r) = status(bp2::replace_surface(
                kk,
                b,
                &mut env,
                f,
                surf,
                key_of(EntityRef::Face(f)),
                keep,
                create,
            ))?;
            out.push(s);
            out.push(r.map_or(0.0, |(_, m)| m.len() as f64));
        }
        Op::RayFire => {
            let start = Vec3::new(arg(0)?, arg(1)?, arg(2)?);
            let dir = Vec3::new(arg(3)?, arg(4)?, arg(5)?);
            let tol = tol::zone_tol3d(k, arg(6)?);
            let limit = (arg(7)? != 0.0).then_some(arg(8)?);
            // the cache SmBrep::RayFire's solve builds (CacheMakeOrValidate), over the
            // geometry owners of a native B-rep
            crate::util::own_arena_geometry(b);
            let kk: &Kernel = k;
            let cache = must(solidcore::topology::brep_cache::make_brep_cache(
                kk,
                solidcore::topology::brep_cache::CacheSource::Brep(&*b),
                &solidcore::topology::brep_cache::arena_identity,
            ))?;
            let keys = |o: CacheObject| solidcore::topology::brep_cache::arena_identity(o);
            let owners = |_: ObjectRef| None;
            let (s, r) = status(bp2::ray_fire(
                kk,
                b,
                BrepId::from_index(0),
                &cache,
                &keys,
                &owners,
                start,
                dir,
                tol,
                limit,
            ))?;
            out.push(s);
            out.push(idx(r.as_ref().and_then(|x| x.region), RegionId::index));
            out.push(r.map_or(f64::from(PointClassType::Unknown as i32), |x| {
                f64::from(x.intersection.point_class() as i32)
            }));
        }
        Op::Export | Op::Extras | Op::MateProbe => return Err(Fail::Harness(harness("not an op"))),
    }
    Ok(out)
}

/// The surface block of `a` starting at `at` (`push_surface`'s layout).
fn surface_arg(a: &[f64], at: usize) -> Result<Surface, Fail> {
    let get = |i: usize| -> Result<f64, Fail> {
        a.get(i)
            .copied()
            .ok_or_else(|| Fail::Harness(harness("surface block too short")))
    };
    if get(at)? == 1.0 {
        let s = SrfIn::Plane {
            o: [get(at + 1)?, get(at + 2)?, get(at + 3)?],
            n: [get(at + 4)?, get(at + 5)?, get(at + 6)?],
        };
        return Ok(rust_surface(&s)?);
    }
    let oob = get(at + 1)? != 0.0;
    let form = get(at + 2)? as u8;
    let n = get(at + 3)? as usize;
    let m = get(at + 4)? as usize;
    let p = get(at + 5)? as usize;
    let q = get(at + 6)? as usize;
    let r = get(at + 7)? as usize;
    let s = get(at + 8)? as usize;
    let mut i = at + 9;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            row.push([get(i)?, get(i + 1)?, get(i + 2)?, get(i + 3)?]);
            i += 4;
        }
        net.push(row);
    }
    let u: Vec<f64> = (0..=r).map(|j| get(i + j)).collect::<Result<_, _>>()?;
    i += r + 1;
    let v: Vec<f64> = (0..=s).map(|j| get(i + j)).collect::<Result<_, _>>()?;
    let b = super::topology_vertexuse::BssIn {
        oob,
        form,
        net,
        p,
        q,
        u,
        v,
    };
    Ok(rust_surface(&SrfIn::Bspline(b))?)
}

/// The surface and curve owners of the `topology.brep_p2` Rust B-rep: this runner's
/// identities ([`key_of`]).
fn face_owner(f: FaceId) -> ObjectRef {
    key_of(EntityRef::Face(f))
}

fn edge_owner(e: EdgeId) -> (ObjectRef, ObjectRef) {
    (
        key_of(EntityRef::Edge(e)),
        key_of(EntityRef::Brep(BrepId::from_index(0))),
    )
}

/// The shim's build conventions (`sc_topology_fixture.h`, its `run`): the face, edge
/// and vertex lists registered before the traversal, `GetVertexEdgeGap` in the
/// `init_gaps` walk.
const CONVENTIONS: crate::own_builder::Conventions<'static> = crate::own_builder::Conventions {
    prefix_lists: true,
    vertex_edge_gaps: true,
    vertex_lists_last: false,
    face_owner: &face_owner,
    edge_owner: &edge_owner,
};

/// The Rust B-rep of the case and its entity counts, or the build failure's output:
/// shim-built sources are built by the Rust kernel ([`crate::own_builder`], the
/// shim's `editing` flag applied after); `.smb` files are imported from the shim's
/// export ([`rust_brep`]).
fn own_brep(k: &mut Kernel, i: &Inputs) -> Result<Result<(Brep, [usize; 10]), Output>, HarnessError> {
    use crate::own_builder::{build_with, build_status, counts, Built};
    let src: super::topology_edgeuse_p1::BrepIn = serde_json::to_value(&i.brep)
        .and_then(serde_json::from_value)
        .map_err(|e| harness(format!("B-rep source: {e}")))?;
    match build_with(k, &src, i.validate_only, i.init_gaps, &CONVENTIONS)? {
        Some(Built::Brep(b)) => {
            let mut b = *b;
            if i.editing >= 0 {
                b.set_editing_enabled(i.editing != 0);
            }
            let c = counts(&b);
            Ok(Ok((b, c)))
        }
        Some(Built::Failed(e)) => match Output::unported(&e) {
            Some(o) => Ok(Err(o)),
            None => Ok(Err(Output::new().values("out", vec![-1.0, build_status(&e)]))),
        },
        None => {
            let (path, h, c) = match prepare(i)? {
                Ok(x) => x,
                Err(o) => return Ok(Err(o)),
            };
            Ok(Ok((rust_brep(&path, &h, &c)?, c)))
        }
    }
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let (mut b, c) = match own_brep(&mut k, i)? {
        Ok(x) => x,
        Err(o) => return Ok(o),
    };
    let Some((op, args)) = op_args(&i.op, &c)? else {
        return Ok(Output::err("no target"));
    };
    if let OpIn::Merge { curves, tol, .. } = &i.op {
        let face = FaceId::from_index(args.first().copied().unwrap_or(0.0) as u32);
        return merge_rust(&mut k, &mut b, face, curves, *tol);
    }
    if let OpIn::MergeSsi {
        curves,
        tol,
        uv_curves,
        surface,
        ..
    } = &i.op
    {
        let face = FaceId::from_index(args.first().copied().unwrap_or(0.0) as u32);
        return merge_ssi_rust(
            &mut k,
            &mut b,
            face,
            curves,
            uv_curves.as_deref(),
            surface.as_ref(),
            *tol,
        );
    }
    match run_rust_op(&mut k, &mut b, op, &args) {
        Ok(out) => Ok(Output::new().values("out", out)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

/// `MergeCurvesOnSurface` with editing enabled over the case's B-rep
/// (`topology::brep::p10::merge_curves_on_surface`, no UV curves): every curve shorter
/// than `tol` is skipped (no new faces or edges); a longer one is classified against the
/// face and merged into it, its edits notified to default records under the arena
/// identities.
fn merge_rust(
    k: &mut Kernel,
    b: &mut Brep,
    face: FaceId,
    curves: &[CrvIn],
    tol: f64,
) -> Result<Output, HarnessError> {
    let crvs: Vec<SmCurve> = curves
        .iter()
        .map(|c| rust_curve(c).map(SmCurve::from))
        .collect::<Result<_, _>>()?;
    let mut state = bp2::ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let curve_ref = |e: EdgeId, _type_code: i64| key_of(EntityRef::Edge(e));
    let r = solidcore::topology::brep::p10::merge_curves_on_surface(
        k, b, &mut env, &curve_ref, face, tol, &crvs, None, false,
    );
    match r {
        Ok((new_faces, edges)) => Ok(Output::new().values(
            "out",
            vec![0.0, 0.0, new_faces.len() as f64, edges.len() as f64],
        )),
        Err(e) => match Output::unported(&e) {
            Some(o) => Ok(o),
            None => match e {
                SolidError::Upstream { code, .. } => {
                    Ok(Output::new().values("out", vec![0.0, f64::from(code), 0.0, 0.0]))
                }
                other => Err(harness(format!("rust: {other}"))),
            },
        },
    }
}

/// `n` points of `c` spread evenly over `[t0, t1]` (the shim's `put_curve_points`).
fn curve_points(k: &Kernel, c: &SmCurve, t0: f64, t1: f64, n: u32, o: &mut Vec<f64>) {
    for i in 0..n {
        let t = t0 + (t1 - t0) * f64::from(i) / f64::from(n - 1);
        let p = c.evaluate_point(k, t).unwrap_or(Vec3::UNDEF);
        o.extend([p.x, p.y, p.z]);
    }
}

/// `MergeCurvesOnSurface` with UV curves and an intersection surface over the case's
/// B-rep (`topology::brep::p10::merge_curves_on_surface_through_surface`), then the
/// shim's read-back: the (refined) curves, every edge (`GetEdges`) and vertex
/// (`GetVertices`).
#[allow(clippy::too_many_arguments)]
fn merge_ssi_rust(
    k: &mut Kernel,
    b: &mut Brep,
    face: FaceId,
    curves: &[CrvIn],
    uv_curves: Option<&[CrvIn]>,
    surface: Option<&SrfIn>,
    tol: f64,
) -> Result<Output, HarnessError> {
    let mut crvs: Vec<SmCurve> = curves
        .iter()
        .map(|c| rust_curve(c).map(SmCurve::from))
        .collect::<Result<_, _>>()?;
    let uv: Option<Vec<SmCurve>> = uv_curves
        .map(|uv| {
            uv.iter()
                .map(|c| rust_curve(c).map(SmCurve::from))
                .collect::<Result<_, _>>()
        })
        .transpose()?;
    let srf = surface.map(rust_surface).transpose()?;
    let mut state = bp2::ArenaNotifyState::new(b);
    let mut env = state.env(None);
    let key_of = env.key_of;
    let curve_ref = |e: EdgeId, _type_code: i64| key_of(EntityRef::Edge(e));
    let r = solidcore::topology::brep::p10::merge_curves_on_surface_through_surface(
        k,
        b,
        &mut env,
        &curve_ref,
        face,
        tol,
        &mut crvs,
        uv.as_deref(),
        srf,
    );
    let (s, nf, ne) = match r {
        Ok((new_faces, edges)) => (0.0, new_faces.len(), edges.len()),
        Err(e) => match Output::unported(&e) {
            Some(o) => return Ok(o),
            None => match e {
                SolidError::Upstream { code, .. } => (f64::from(code), 0, 0),
                other => return Err(harness(format!("rust: {other}"))),
            },
        },
    };
    let mut out = vec![0.0, s, nf as f64, ne as f64];
    for c in &crvs {
        let ivl = c.natural_interval();
        out.push(
            c.number_control_points()
                .map_err(|e| harness(format!("rust: {e}")))? as f64,
        );
        curve_points(k, c, ivl.min(), ivl.max(), 9, &mut out);
    }
    let h = |e: SolidError| harness(format!("rust: {e}"));
    let edges = bp1::edges(b, None).map_err(h)?;
    let vertices = bp1::vertices(b, None).map_err(h)?;
    out.push(edges.len() as f64);
    for e in edges {
        let edge = b.edge(e).map_err(h)?;
        out.push(solidcore::topology::edge::tolerance(b, e).map_err(h)?);
        let Some(c) = edge.curve.as_ref() else {
            out.push(-1.0);
            continue;
        };
        let ivl = solidcore::topology::edge::interval(b, e).map_err(h)?;
        out.extend([ivl.min(), ivl.max()]);
        curve_points(k, c, ivl.min(), ivl.max(), 3, &mut out);
    }
    out.push(vertices.len() as f64);
    for v in vertices {
        let p = b.vertex(v).map_err(h)?.point;
        let t = solidcore::topology::vertex::tolerance(b, v).map_err(h)?;
        out.extend([p.x, p.y, p.z, t]);
    }
    Ok(Output::new().values("out", out))
}

// ---- generation ---------------------------------------------------------------------------

fn tiny_curve(rng: &mut Rng) -> CrvIn {
    let mut c = gen_curve(rng, 3, 1.0);
    for p in &mut c.cpts {
        let w = if p[3] == -f64::MAX { 1.0 } else { p[3] };
        for x in p.iter_mut().take(3) {
            *x *= 1e-7 * w.abs().max(1e-3);
        }
    }
    c
}

fn gen_op(rng: &mut Rng) -> (OpIn, i64) {
    let sel = |rng: &mut Rng| rng.below(1 << 20) as i64;
    let editing = if rng.chance(0.5) {
        -1
    } else {
        rng.below(2) as i64
    };
    match rng.below(14) {
        0..=2 => (
            OpIn::Query {
                which: rng.below(6) as u8,
            },
            editing,
        ),
        3 => (
            OpIn::SetTol {
                tol: match rng.below(4) {
                    0 => 0.0,
                    1 => -1e-4,
                    2 => 10f64.powf(rng.uniform(-12.0, 3.0)),
                    _ => 1e-5,
                },
                only_larger: rng.chance(0.7),
                cascade: rng.chance(0.5),
            },
            editing,
        ),
        4 => (OpIn::Rebuild, editing),
        5 => (
            OpIn::Make {
                which: rng.below(3) as u8,
                region: if rng.chance(0.5) { -1 } else { sel(rng) },
                v1: sel(rng),
                v2: sel(rng),
            },
            editing,
        ),
        6 | 7 => {
            let scale = *rng.choose(&[1.0, 1.0, 1e-6, 1e6]).unwrap_or(&1.0);
            let curve = gen_curve(rng, 3, scale);
            let (a, b) = (
                curve.u.first().copied().unwrap_or(0.0),
                curve.u.last().copied().unwrap_or(1.0),
            );
            let class = *rng
                .choose(&[102i64, 103, 104, 102, 103, 104, 100, 105])
                .unwrap_or(&103);
            let object = if class < 102 || class > 104 && rng.chance(0.5) {
                -1
            } else {
                sel(rng)
            };
            (
                OpIn::Sector {
                    curve,
                    t0: a,
                    t1: b,
                    orient: rng.below(3) as u8,
                    class,
                    object,
                    t: rng.uniform(0.0, 1.0),
                    uv: [rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)],
                },
                editing,
            )
        }
        8 => {
            let n = rng.below(3) as usize;
            let curves = (0..n).map(|_| tiny_curve(rng)).collect();
            (
                OpIn::Merge {
                    face: sel(rng),
                    tol: 1e-3,
                    curves,
                },
                editing,
            )
        }
        9 => (
            OpIn::Mate {
                edge: sel(rng),
                surface: gen_surface(rng),
                ext: 1.5,
                uv_ext: 10.0,
            },
            // the full algorithm only runs on chosen fixture inputs
            0,
        ),
        10 => {
            let op = *rng.choose(&[0u8, 5, 6, 10, 11, 15, 16]).unwrap_or(&10);
            let mut data = [[-1i64, -1i64]; 3];
            for d in &mut data {
                if rng.chance(0.6) {
                    *d = [
                        *rng.choose(&[16002i64, 16016, 16021, BREP_TYPE])
                            .unwrap_or(&16016),
                        sel(rng),
                    ];
                }
            }
            (
                OpIn::Notify {
                    operation: op,
                    data,
                },
                editing,
            )
        }
        11 => (
            OpIn::Stale {
                create: rng.below(256) as u8,
                modified: rng.below(256) as u8,
            },
            editing,
        ),
        12 => (OpIn::Swap, editing),
        _ => (
            OpIn::RayFire {
                p: rng.point3(-5.0, 5.0),
                v: {
                    let mut v = rng.point3(-1.0, 1.0);
                    if v.iter().all(|x| x.abs() < 1e-3) {
                        v = [0.0, 0.0, 1.0];
                    }
                    v
                },
                tol: 1e-5,
                limit: rng.chance(0.3).then(|| rng.uniform(0.1, 20.0)),
            },
            editing,
        ),
    }
}

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.brep_p2"
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let brep = gen_brep(rng);
        // a wire edge or the wire network has no face: draw again rather than
        // generate a case that compares nothing (`no target` on both sides)
        let faceless = matches!(brep, BrepIn::Wire { .. } | BrepIn::Network);
        let (op, editing) = loop {
            let (op, editing) = gen_op(rng);
            let needs_face = matches!(
                op,
                OpIn::Merge { .. } | OpIn::Replace { .. } | OpIn::Sector { class: 102, .. }
            );
            if !(faceless && needs_face) {
                break (op, editing);
            }
        };
        serde_json::to_value(Inputs {
            brep,
            validate_only: rng.chance(0.2),
            init_gaps: rng.chance(0.2),
            editing,
            op,
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A negative `MateLaminaEdgeToSurface` extension factor hangs the C++
    /// (`N_SrfExtendByDist`, HANG-18): the FFI must reject it before calling in, and
    /// still accept `0` and the upstream `1.5`.
    #[test]
    fn mate_rejects_a_negative_extension_factor() {
        let case = |ext: f64| -> Inputs {
            let v = serde_json::json!({
                "brep": {"kind": "face", "surface": {"plane": {"o": [0, 0, 0], "n": [0, 0, 1]}},
                         "dom": [0, 0, 10, 10]},
                "editing": 1, "op": "mate", "edge": 0,
                "surface": {"plane": {"o": [0, 0, 0.5], "n": [0, 0, 1]}},
                "ext": ext, "uv_ext": 10.0
            });
            serde_json::from_value(v).unwrap()
        };
        let err = run_cpp(&case(-1.0)).unwrap_err().to_string();
        assert!(err.contains("negative surface extension factor"), "{err}");
        assert!(run_cpp(&case(-1e-300)).is_err());
        for ext in [0.0, 1.5] {
            assert!(run_cpp(&case(ext)).is_ok(), "ext {ext}");
        }
    }

    /// The fixture's gap initialisation slows down from a corner at `1e155` and stops
    /// returning around `1e156` on a u-degree-2, three-span face (HANG-28, `--fuzz` case
    /// fuzz00497): the FFI refuses degree >= 2 faces with a corner past `1e154` under
    /// `init_gaps`, and nothing else. The compared control at the bound returns in about
    /// 0.01 s. A rational corner is bounded at `1e151`, stored and Cartesian (stored / w).
    #[test]
    fn gap_initialisation_refuses_a_huge_curved_corner() {
        let now = -f64::MAX;
        let case = |p: u8, z: f64, gaps: bool| -> Inputs {
            let n: u32 = if p == 2 { 5 } else { 4 };
            let u: Vec<f64> = if p == 2 {
                vec![0.0, 0.0, 0.0, 0.33, 0.66, 1.0, 1.0, 1.0]
            } else {
                vec![0.0, 0.0, 0.33, 0.66, 1.0, 1.0]
            };
            let mut net: Vec<Vec<[f64; 4]>> = (0..n)
                .map(|a| {
                    (0..2)
                        .map(|b| [f64::from(a), f64::from(b), 0.0, now])
                        .collect()
                })
                .collect();
            net.last_mut().unwrap()[1][2] = z;
            let v = serde_json::json!({
                "brep": {"kind": "face", "dom": [0, 0, 1, 1], "surface": {"bspline": {
                    "form": 0, "oob": false, "p": p, "q": 1, "u": u, "v": [0, 0, 1, 1],
                    "net": net}}},
                "init_gaps": gaps, "op": "query", "which": 5
            });
            serde_json::from_value(v).unwrap()
        };
        for z in [2e154, 1e156, 5e156, 1e300] {
            let err = run_cpp(&case(2, z, true)).unwrap_err().to_string();
            assert!(
                err.contains("face too large for the gap initialisation"),
                "{z}: {err}"
            );
        }
        for (p, z, gaps) in [(2, 1e154, true), (2, 1e300, false), (1, 1e300, true)] {
            let c = run_cpp(&case(p, z, gaps)).unwrap();
            let r = run_rust(&case(p, z, gaps)).unwrap();
            assert_eq!(format!("{c:?}"), format!("{r:?}"), "{p} {z} {gaps}");
        }
        // a rational net is homogeneous (w x, w y, w z, w), bounded at 1e151 on both the
        // stored and the Cartesian corner: the stored x of 1e154 with weight 1e-3 is a
        // vertex at x = 1e157 (no return) and a stored 1e152 with weight 1e3 is past the
        // stored bound, both refused; a stored 1e140 with weight 1e3 is compared
        let rational = |x: f64, w: f64| -> Inputs {
            let mut v = serde_json::to_value(case(2, 0.0, true)).unwrap();
            let net = v["brep"]["surface"]["bspline"]["net"]
                .as_array_mut()
                .unwrap();
            for row in net.iter_mut() {
                for pt in row.as_array_mut().unwrap() {
                    pt[3] = serde_json::json!(1.0);
                }
            }
            let corner = &mut net.last_mut().unwrap()[1];
            corner[0] = serde_json::json!(x);
            // homogeneous: the stored y is w * 1, so the corner's Cartesian y stays 1
            corner[1] = serde_json::json!(w);
            corner[3] = serde_json::json!(w);
            serde_json::from_value(v).unwrap()
        };
        for (x, w) in [(1e154, 1e-3), (1e152, 1e3), (2e151, 1.0)] {
            let err = run_cpp(&rational(x, w)).unwrap_err().to_string();
            assert!(
                err.contains("face too large for the gap initialisation"),
                "{x} {w}: {err}"
            );
        }
        // compared inside the bound and exactly at it (inclusive): stored 1e151 with
        // w = 1, stored 1e148 with w = 1e-3 (a vertex at 1e151)
        for (x, w) in [(1e140, 1e3), (1e151, 1.0), (1e148, 1e-3)] {
            let c = run_cpp(&rational(x, w)).unwrap();
            let r = run_rust(&rational(x, w)).unwrap();
            assert_eq!(format!("{c:?}"), format!("{r:?}"), "{x} {w}");
        }
    }

    /// A mating surface so far from the lamina edge that the squared distance overflows
    /// makes upstream's `dMaxDist` infinite, and `CreateExtendedSurface` then halves an
    /// infinite distance forever (HANG-9; `--fuzz` cases fuzz00745 / 01418 / 01659): the
    /// FFI's `MateProbe` refuses exactly the non-finite extension distances.
    #[test]
    fn mate_rejects_a_non_finite_extension_distance() {
        let case_ed = |z: f64, ext: f64, editing: i64| -> Inputs {
            let v = serde_json::json!({
                "brep": {"kind": "face", "surface": {"plane": {"o": [0, 0, 0], "n": [0, 0, 1]}},
                         "dom": [0, 0, 10, 10]},
                "editing": editing, "op": "mate", "edge": 0,
                "surface": {"plane": {"o": [0, 0, z], "n": [0, 0, 1]}},
                "ext": ext, "uv_ext": 10.0
            });
            serde_json::from_value(v).unwrap()
        };
        let case = |z: f64, ext: f64| case_ed(z, ext, 1);
        for (z, ext) in [
            (1e300, 1.5),
            (-1e300, 1.5),
            (1e160, 1.5),
            (1e160, 0.0),
            (1e154, 1e300),
        ] {
            let err = run_cpp(&case(z, ext)).unwrap_err().to_string();
            assert!(
                err.contains("non-finite surface extension distance"),
                "{z} {ext}: {err}"
            );
        }
        for (z, ext) in [
            (0.5, 1.5),
            (1e10, 1.5),
            (1e154, 1.5),
            (1e154, 0.0),
            (0.0, 1e300),
        ] {
            assert!(run_cpp(&case(z, ext)).is_ok(), "{z} {ext}");
        }
        // Mate returns before the extension when editing is off (SmBrep.cpp:22166):
        // the guard must not refuse it, and the early status is compared.
        for z in [1e300, -1e300, 1e160] {
            let c = run_cpp(&case_ed(z, 1.5, 0)).unwrap();
            let r = run_rust(&case_ed(z, 1.5, 0)).unwrap();
            assert_eq!(format!("{c:?}"), format!("{r:?}"), "{z}");
        }
    }
}
