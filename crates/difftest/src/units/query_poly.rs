//! Runner for unit `query.poly`: `SmPolyBrep` and its poly objects (traversals,
//! bounding boxes, normals, bounding spheres, point containment, degenerate edges,
//! `Transform`, setters and destructors, `IsKindOf`).
//!
//! C++: `solidcore_ffi::units::query_poly` (upstream `source/SMLib/src/SmPoly.cpp`).
//! Rust: `solidcore::query::poly`.
//!
//! A case is a poly B-rep description (`brep`) and an operation (`op`). Both sides
//! build the same poly B-rep (the shim with upstream's constructors, the Rust side with
//! the mirrors in `query::poly`), run the operation and dump the whole structure
//! (lists, rings, stored tolerances, normals and spheres) as keyed sections; integer
//! sections are compared exactly, float sections to `1e-9` relative. Statuses are `0`
//! (`SM_SUCCESS`) or the `SmStatus`.
//!
//! Rejected cases: `IsDegenerate` of an edge whose face has a label for an original
//! face while the back pointers are valid. Upstream then calls `SmFace::GetSurface`
//! on that face (to map UV points), and a label is not an `SmFace`: the call is
//! undefined in C++ (AGENTS.md section 4, step 5.6). The shim decides this from the
//! built model (label plus back pointers), before any call, and returns
//! `STATUS_UNDEFINED_INPUT`; `run_cpp` maps that status to `rejected`, no C++ result is
//! inspected, and the Rust side, which would defer it at `SmFace::GetSurface`
//! (`topology.face_p2`) for lack of the face, is not run.
//! `owner_degenerate` is a separate opt-in operation that does compare these calls:
//! explicit face records resolve only their own labels, and it compares real native
//! `IsDegenerate` calls and independent surface samples.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::a_object::CacheStore;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::query::poly::{
    PolyAuxId, PolyBrep, PolyEdgeId, PolyFaceId, PolyLoopId, PolyNotifyEnv, PolyObjectKind,
    PolyRegionId, PolyShellId, PolyVertexId,
};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::topology::topology::Topology;
use solidcore::topology::{Brep, Face, FaceId, Shell};
use solidcore::SolidError;
use solidcore_ffi::units::query_poly as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `query.poly` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopIn {
    mode: u8,
    edge_tol: f64,
    verts: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FaceIn {
    shell: usize,
    tol: f64,
    #[serde(default)]
    normal: Option<[f64; 3]>,
    loops: Vec<LoopIn>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct BrepIn {
    tol: f64,
    regions: Vec<usize>,
    vertices: Vec<[f64; 4]>,
    faces: Vec<FaceIn>,
    #[serde(default)]
    radials: Vec<Vec<usize>>,
    #[serde(default)]
    aux: Vec<[f64; 5]>,
    #[serde(default)]
    edge_aux: Vec<(usize, usize)>,
    #[serde(default)]
    vertex_normals: Vec<(usize, [f64; 3])>,
    #[serde(default)]
    cpolys: Vec<Vec<usize>>,
    #[serde(default)]
    orig_faces: Vec<(usize, i64)>,
    #[serde(default)]
    orig_edgeuses: Vec<(usize, i64)>,
    #[serde(default)]
    ok_back_ptrs: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ContainsIn {
    face: bool,
    id: usize,
    point: [f64; 3],
    tol: f64,
    work_in_3d: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum MutIn {
    SetPoint { v: usize, p: [f64; 3] },
    SetVertexTolerance { v: usize, t: f64 },
    DeleteEdge { e: usize },
    DeleteFace { f: usize },
    CpolyRemove { c: usize, f: usize },
    SetNormalUninit { f: usize },
    SetEdgeTolerance { e: usize, t: f64 },
    SetFaceTolerance { f: usize, t: f64 },
    SetOriginalFace { f: usize, label: i64 },
    ClearAux { e: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OpIn {
    Dump,
    Bbox {
        pseudo: bool,
        points: Option<Vec<[f64; 3]>>,
        centroid: bool,
        normal: Option<[f64; 3]>,
    },
    Normal {
        calls: Vec<(usize, bool, bool)>,
    },
    Sphere {
        calls: Vec<(usize, bool)>,
    },
    Contains {
        queries: Vec<ContainsIn>,
    },
    Degenerate {
        calls: Vec<(usize, Option<f64>)>,
    },
    OwnerDegenerate {
        owners: Vec<OwnerIn>,
        calls: Vec<(usize, Option<f64>)>,
    },
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Mutate {
        cmds: Vec<MutIn>,
    },
    Kinds {
        codes: Vec<i64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    op: OpIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OwnerIn {
    label: i64,
    surface: Option<OwnerSurfaceIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OwnerSurfaceIn {
    Plane {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    Bezier {
        net: Vec<Vec<[f64; 3]>>,
    },
}

fn owner_to_ffi(owner: &OwnerIn) -> ffi::OwnerFace {
    ffi::OwnerFace {
        label: owner.label,
        surface: owner.surface.as_ref().map(|s| match s {
            OwnerSurfaceIn::Plane {
                origin,
                x,
                y,
                scale,
                domain,
            } => ffi::OwnerSurface::Plane {
                origin: *origin,
                x: *x,
                y: *y,
                scale: *scale,
                domain: *domain,
            },
            OwnerSurfaceIn::Bezier { net } => ffi::OwnerSurface::Bezier { net: net.clone() },
        }),
    }
}

fn to_ffi(b: &BrepIn) -> ffi::PolyDesc {
    ffi::PolyDesc {
        tol: b.tol,
        regions: b.regions.clone(),
        vertices: b.vertices.clone(),
        faces: b
            .faces
            .iter()
            .map(|f| ffi::FaceDesc {
                shell: f.shell,
                tol: f.tol,
                normal: f.normal,
                loops: f
                    .loops
                    .iter()
                    .map(|l| ffi::LoopDesc {
                        mode: l.mode,
                        edge_tol: l.edge_tol,
                        verts: l.verts.clone(),
                    })
                    .collect(),
            })
            .collect(),
        radials: b.radials.clone(),
        aux: b.aux.clone(),
        edge_aux: b.edge_aux.clone(),
        vertex_normals: b.vertex_normals.clone(),
        cpolys: b.cpolys.clone(),
        orig_faces: b.orig_faces.clone(),
        orig_edgeuses: b.orig_edgeuses.clone(),
        ok_back_ptrs: b.ok_back_ptrs,
    }
}

fn op_to_ffi(op: &OpIn) -> ffi::Op {
    match op {
        OpIn::Dump => ffi::Op::Dump,
        OpIn::Bbox {
            pseudo,
            points,
            centroid,
            normal,
        } => ffi::Op::BBox {
            pseudo: *pseudo,
            points: points.clone(),
            centroid: *centroid,
            normal: *normal,
        },
        OpIn::Normal { calls } => ffi::Op::Normal(calls.clone()),
        OpIn::Sphere { calls } => ffi::Op::Sphere(calls.clone()),
        OpIn::Contains { queries } => ffi::Op::Contains(
            queries
                .iter()
                .map(|q| ffi::ContainsQuery {
                    face: q.face,
                    id: q.id,
                    point: q.point,
                    tol: q.tol,
                    work_in_3d: q.work_in_3d,
                })
                .collect(),
        ),
        OpIn::Degenerate { calls } => ffi::Op::Degenerate(calls.clone()),
        OpIn::OwnerDegenerate { owners, calls } => ffi::Op::OwnerDegenerate {
            owners: owners.iter().map(owner_to_ffi).collect(),
            calls: calls.clone(),
        },
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => ffi::Op::Transform {
            origin: *origin,
            x: *x,
            y: *y,
            scale: *scale,
        },
        OpIn::Mutate { cmds } => ffi::Op::Mutate(
            cmds.iter()
                .map(|c| match *c {
                    MutIn::SetPoint { v, p } => ffi::Mutation::SetPoint(v, p),
                    MutIn::SetVertexTolerance { v, t } => ffi::Mutation::SetVertexTolerance(v, t),
                    MutIn::DeleteEdge { e } => ffi::Mutation::DeleteEdge(e),
                    MutIn::DeleteFace { f } => ffi::Mutation::DeleteFace(f),
                    MutIn::CpolyRemove { c, f } => ffi::Mutation::CPolyRemove(c, f),
                    MutIn::SetNormalUninit { f } => ffi::Mutation::SetNormalUninit(f),
                    MutIn::SetEdgeTolerance { e, t } => ffi::Mutation::SetEdgeTolerance(e, t),
                    MutIn::SetFaceTolerance { f, t } => ffi::Mutation::SetFaceTolerance(f, t),
                    MutIn::SetOriginalFace { f, label } => ffi::Mutation::SetOriginalFace(f, label),
                    MutIn::ClearAux { e } => ffi::Mutation::ClearAux(e),
                })
                .collect(),
        ),
        OpIn::Kinds { codes } => ffi::Op::Kinds(codes.clone()),
    }
}

// ---- sections ---------------------------------------------------------------------

const K_STATUS: i32 = 0;
const K_REGIONS: i32 = 1;
const K_SHELLS: i32 = 2;
const K_FACES: i32 = 3;
const K_FACES_COMMON: i32 = 4;
const K_FIRST_FACE: i32 = 5;
const K_FACE_TOPO: i32 = 6;
const K_FACE_GEOM: i32 = 7;
const K_LOOP_TOPO: i32 = 8;
const K_EDGE_TOPO: i32 = 9;
const K_EDGE_GEOM: i32 = 10;
const K_VERTEX_TOPO: i32 = 11;
const K_VERTEX_GEOM: i32 = 12;
const K_AUX_TOPO: i32 = 13;
const K_AUX_GEOM: i32 = 14;
const K_BREP_GEOM: i32 = 15;
const K_CPOLY: i32 = 16;
const K_BREP_EDGES: i32 = 17;
const K_BREP_TOPO: i32 = 18;
const K_BBOX: i32 = 20;
const K_BBOX_POINTS: i32 = 21;
const K_OBJ_BOXES: i32 = 22;
const K_NORMALS: i32 = 23;
const K_SPHERES: i32 = 25;
const K_RESULTS: i32 = 26;
const K_OWNER_POINTS: i32 = 27;
const K_OWNER_TOPO: i32 = 28;

fn key_name(key: i32) -> &'static str {
    match key {
        K_STATUS => "status",
        K_REGIONS => "regions",
        K_SHELLS => "shells",
        K_FACES => "faces",
        K_FACES_COMMON => "faces_common",
        K_FIRST_FACE => "first_face",
        K_FACE_TOPO => "face_topo",
        K_FACE_GEOM => "face_geom",
        K_LOOP_TOPO => "loop_topo",
        K_EDGE_TOPO => "edge_topo",
        K_EDGE_GEOM => "edge_geom",
        K_VERTEX_TOPO => "vertex_topo",
        K_VERTEX_GEOM => "vertex_geom",
        K_AUX_TOPO => "aux_topo",
        K_AUX_GEOM => "aux_geom",
        K_BREP_GEOM => "brep_geom",
        K_CPOLY => "cpoly",
        K_BREP_EDGES => "brep_edges",
        K_BREP_TOPO => "brep_topo",
        K_BBOX => "bbox",
        K_BBOX_POINTS => "bbox_points",
        K_OBJ_BOXES => "obj_boxes",
        K_NORMALS => "normals",
        K_SPHERES => "spheres",
        K_RESULTS => "results",
        K_OWNER_POINTS => "owner_points",
        K_OWNER_TOPO => "owner_topo",
        _ => "unknown",
    }
}

fn to_output(secs: &[ffi::Section]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    for s in secs {
        let name = key_name(s.key);
        if name == "unknown" {
            return Err(harness(format!("unknown section {}", s.key)));
        }
        out = if s.ints {
            out.ints(
                name,
                s.values.iter().map(|&x| x as i64).collect::<Vec<i64>>(),
            )
        } else {
            out.values(name, s.values.clone())
        };
    }
    Ok(out)
}

#[derive(Default)]
struct Secs(Vec<ffi::Section>);

impl Secs {
    fn floats(&mut self, key: i32, v: Vec<f64>) {
        self.0.push(ffi::Section {
            key,
            ints: false,
            values: v,
        });
    }
    fn ints(&mut self, key: i32, v: Vec<f64>) {
        self.0.push(ffi::Section {
            key,
            ints: true,
            values: v,
        });
    }
}

fn put(o: &mut Vec<f64>, v: Vec3) {
    o.extend_from_slice(&[v.x, v.y, v.z]);
}

fn put_box(o: &mut Vec<f64>, b: &Extent3d) {
    put(o, b.min());
    put(o, b.max());
}

fn opt<T: Copy>(x: Option<T>, f: impl Fn(T) -> u32) -> f64 {
    x.map_or(-1.0, |v| f64::from(f(v)))
}

/// The `SmStatus` of a Rust error (`SM_ERR_INVALID_INPUT` for invalid input).
fn code_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => 1007.0,
    }
}

/// Unported errors abort the case (deferred); others become a status.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

fn rs<T>(r: solidcore::Result<T>) -> Result<T, Fail> {
    r.map_err(|e| match Output::unported(&e) {
        Some(o) => Fail::Deferred(Box::new(o)),
        None => Fail::Harness(harness(format!("rust error: {e}"))),
    })
}

// ---- Rust side ----------------------------------------------------------------------

struct Built {
    brep: PolyBrep,
    n_plain_faces: usize,
    n_shells: usize,
    n_faces: usize,
    n_loops: usize,
    n_edges: usize,
    n_vertices: usize,
}

fn build(k: &Kernel, b: &BrepIn) -> Result<Built, Fail> {
    let mut p = PolyBrep::new(k, b.tol);
    let mut regions = Vec::new();
    for _ in &b.regions {
        regions.push(rs(p.add_region())?);
    }
    let mut shells: Vec<PolyShellId> = Vec::new();
    for (r, &n) in regions.iter().zip(&b.regions) {
        // The FFI rejects larger counts; mirror it so a fuzzed count cannot make the
        // runner build billions of shells.
        if n > ffi::MAX_SHELLS_PER_REGION {
            return Err(Fail::Harness(harness("too many shells in a region")));
        }
        for _ in 0..n {
            shells.push(rs(p.add_shell(*r))?);
        }
    }
    let mut verts = Vec::new();
    for v in &b.vertices {
        verts.push(rs(p.add_vertex(k, Vec3::new(v[0], v[1], v[2]), v[3]))?);
    }
    let mut n_loops = 0;
    let mut edges: Vec<PolyEdgeId> = Vec::new();
    let pick = |i: usize| -> Result<PolyVertexId, Fail> {
        verts
            .get(i)
            .copied()
            .ok_or(Fail::Harness(harness("vertex index")))
    };
    for f in &b.faces {
        let shell = *shells
            .get(f.shell)
            .ok_or(Fail::Harness(harness("shell index")))?;
        let face = rs(p.add_face(k, shell, f.tol, f.normal.map(v3)))?;
        for l in &f.loops {
            let lp = rs(p.start_poly_edge_loop(face))?;
            n_loops += 1;
            let kk = l.verts.len();
            // the FFI's loop check ("loop vertex count / mode")
            if kk == 0 || (l.mode == 2 && kk < 2) || l.mode > 2 {
                return Err(Fail::Harness(harness("loop vertex count / mode")));
            }
            let ne = if l.mode == 2 { kk - 1 } else { kk };
            for j in 0..ne {
                let v0 = pick(*l.verts.get(j).ok_or(Fail::Harness(harness("loop")))?)?;
                let v1 = pick(
                    *l.verts
                        .get((j + 1) % kk)
                        .ok_or(Fail::Harness(harness("loop")))?,
                )?;
                let p0 = rs(p.vertex_point(v0))?;
                let p1 = rs(p.vertex_point(v1))?;
                edges.push(rs(p.add_poly_edge(k, lp, l.edge_tol, p0, p1, v0))?);
            }
            if l.mode == 1 {
                // the status is ignored by the shim
                let _ = p.finish_poly_edge_loop(lp);
            } else if l.mode == 2 {
                let last = pick(*l.verts.last().ok_or(Fail::Harness(harness("loop")))?)?;
                rs(p.set_last_end_poly_vertex(lp, Some(last)))?;
            }
        }
    }
    let edge = |i: usize| -> Result<PolyEdgeId, Fail> {
        edges
            .get(i)
            .copied()
            .ok_or(Fail::Harness(harness("edge index")))
    };
    for g in &b.radials {
        let ring = g.iter().map(|&i| edge(i)).collect::<Result<Vec<_>, _>>()?;
        rs(p.link_radials(&ring))?;
    }
    let mut aux = Vec::new();
    for a in &b.aux {
        aux.push(rs(p.add_aux_data(
            Vec3::new(a[0], a[1], a[2]),
            Vec2::new(a[3], a[4]),
        ))?);
    }
    for &(e, a) in &b.edge_aux {
        let a = *aux.get(a).ok_or(Fail::Harness(harness("aux index")))?;
        rs(p.edge_set_aux_data(edge(e)?, Some(a)))?;
    }
    for &(v, n) in &b.vertex_normals {
        rs(p.vertex_add_face_normal(pick(v)?, v3(n)))?;
    }
    let nf = b.faces.len();
    for c in &b.cpolys {
        let mut cp = None;
        for &f in c {
            let face = PolyFaceId::from_index(f as u32);
            match cp {
                None => cp = Some(rs(p.add_cpoly_face(face))?),
                Some(x) => rs(p.cpoly_add_poly_face(x, face, false))?,
            }
        }
    }
    for &(f, l) in &b.orig_faces {
        rs(p.face_set_original_face(
            PolyFaceId::from_index(f as u32),
            Some(ObjectKey(l as u64)),
            false,
        ))?;
    }
    for &(e, l) in &b.orig_edgeuses {
        rs(p.edge_set_original_edgeuse(edge(e)?, Some(ObjectKey(l as u64))))?;
    }
    p.set_ok_back_ptrs(b.ok_back_ptrs);
    Ok(Built {
        brep: p,
        n_plain_faces: nf,
        n_shells: shells.len(),
        n_faces: nf + b.cpolys.len(),
        n_loops,
        n_edges: edges.len(),
        n_vertices: verts.len(),
    })
}

fn fid(i: usize) -> PolyFaceId {
    PolyFaceId::from_index(i as u32)
}

fn eid(i: usize) -> PolyEdgeId {
    PolyEdgeId::from_index(i as u32)
}

fn lid(i: usize) -> PolyLoopId {
    PolyLoopId::from_index(i as u32)
}

fn vid(i: usize) -> PolyVertexId {
    PolyVertexId::from_index(i as u32)
}

/// Real owner faces in their own arena, independent of the poly B-rep's labels.
struct OwnerWorld {
    brep: Brep,
    faces: std::collections::BTreeMap<ObjectKey, FaceId>,
}

fn build_owners(owners: &[OwnerIn]) -> Result<OwnerWorld, Fail> {
    let descs: Vec<_> = owners.iter().map(owner_to_ffi).collect();
    ffi::check_owner_faces(&descs).map_err(|e| Fail::Harness(harness(e.to_string())))?;
    let mut brep = Brep::new();
    let shell = rs(brep.add_shell_record(Shell::default()))?;
    let mut faces = std::collections::BTreeMap::new();
    for owner in owners {
        let mut face = Face::new(shell);
        face.surface = match &owner.surface {
            None => None,
            Some(OwnerSurfaceIn::Plane {
                origin,
                x,
                y,
                scale,
                domain,
            }) => Some(Surface::from(Plane::new(
                v3(*origin),
                v3(*x),
                v3(*y),
                Vec2::from(*scale),
                &Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]),
            ))),
            Some(OwnerSurfaceIn::Bezier { net }) => {
                // Dimensions were checked above; each knot vector is at most 8 entries.
                let nu = net.len();
                let nv = net
                    .first()
                    .ok_or_else(|| Fail::Harness(harness("empty owner Bezier net")))?
                    .len();
                let mut u = vec![0.0; nu];
                u.extend(vec![1.0; nu]);
                let mut v = vec![0.0; nv];
                v.extend(vec![1.0; nv]);
                let net = net
                    .iter()
                    .map(|row| {
                        row.iter()
                            .map(|p| CPoint::new(p[0], p[1], p[2], NOW))
                            .collect()
                    })
                    .collect();
                Some(Surface::from(BSplineSurface::from_nurb(
                    rs(NurbSurface::new(net, nu - 1, nv - 1, u, v))?,
                    false,
                )))
            }
        };
        if let Some(s) = &face.surface {
            // (runner helper without a kernel in scope)
            face.uv_domain = s.natural_uv_domain_if_built();
        }
        let id = rs(brep.add_face_record(face))?;
        rs(brep.shell_mut(shell))?.faces.push(id);
        if let Some(surface) = rs(brep.face_mut(id))?.surface.as_mut() {
            surface.set_owner(Some(ObjectRef {
                type_code: solidcore::topology::face::p2::TYPE,
                key: ObjectKey(owner.label as u64),
            }));
            surface.set_owner_zone_tol_3d(Some(0.0)); // SmFace() default
        }
        faces.insert(ObjectKey(owner.label as u64), id);
    }
    Ok(OwnerWorld { brep, faces })
}

/// Independent native-visible samples, not a substitute for `IsDegenerate`.
/// Per call: 10 integers (edge, label, resolved, surface, back-pointers, mapping,
/// four classifications); raw endpoints, then domain and three mapped samples
/// only when mapping applies. The midpoint is sampled even if the native
/// degeneracy implementation's seam branch would not need it.
fn owner_samples(
    k: &Kernel,
    p: &PolyBrep,
    e: PolyEdgeId,
    world: &OwnerWorld,
    ints: &mut Vec<f64>,
    points: &mut Vec<f64>,
) -> Result<(), Fail> {
    let label = match rs(p.edge_poly_face(e))? {
        Some(f) => rs(p.face_original_face(f))?,
        None => None,
    };
    let face = label.and_then(|key| world.faces.get(&key).copied());
    let surface = match face {
        Some(f) => rs(solidcore::topology::face::p2::surface(&world.brep, f))?,
        None => None,
    };
    let back = rs(p.edge_ok_back_ptrs(e))?;
    let mapping = back && surface.is_some() && rs(p.edge_is_2d_or_zero(e))?;
    ints.extend([
        f64::from(e.index()),
        label.map_or(0.0, |key| key.0 as f64),
        f64::from(u8::from(face.is_some())),
        f64::from(u8::from(surface.is_some())),
        f64::from(u8::from(back)),
        f64::from(u8::from(mapping)),
    ]);
    let start = rs(p.edge_start_point(e))?;
    let end = rs(p.edge_end_point(e))?;
    put(points, start);
    put(points, end);
    if let Some(surface) = surface.filter(|_| mapping) {
        let domain = rs(surface.natural_uv_domain_with_nurb(k))?;
        let a = Vec2::new(start.x, start.y);
        let b = Vec2::new(end.x, end.y);
        let (su, sv) = domain.classify_point(a, solidcore::math::EFF_ZERO_PARAM, None);
        let (eu, ev) = domain.classify_point(b, solidcore::math::EFF_ZERO_PARAM, None);
        ints.extend([su, sv, eu, ev].map(|pos| f64::from(pos.code())));
        points.extend([
            domain.min().x,
            domain.min().y,
            domain.max().x,
            domain.max().y,
        ]);
        for uv in [a, b, (a + b) / 2.0] {
            put(points, rs(surface.evaluate_point(k, uv))?);
        }
    } else {
        ints.extend([-1.0; 4]);
    }
    Ok(())
}

fn dump(b: &mut Built, out: &mut Secs) -> Result<(), Fail> {
    let p = &mut b.brep;
    let mut o = Vec::new();
    for r in p.poly_regions() {
        let sh = rs(p.region_poly_shells(r))?;
        o.push(f64::from(r.index()));
        o.push(sh.len() as f64);
        o.extend(sh.iter().map(|s| f64::from(s.index())));
    }
    out.ints(K_REGIONS, std::mem::take(&mut o));

    for s in rs(p.poly_shells())? {
        let fs = rs(p.shell_poly_faces(s))?;
        o.push(f64::from(s.index()));
        o.push(opt(rs(p.shell_poly_region(s))?, PolyRegionId::index));
        o.push(fs.len() as f64);
        o.extend(fs.iter().map(|f| f64::from(f.index())));
    }
    out.ints(K_SHELLS, std::mem::take(&mut o));

    o.extend(rs(p.poly_faces(None))?.iter().map(|f| f64::from(f.index())));
    out.ints(K_FACES, std::mem::take(&mut o));

    for label in 1..=3u64 {
        let fs = rs(p.poly_faces(Some(ObjectKey(label))))?;
        o.push(fs.len() as f64);
        o.extend(fs.iter().map(|f| f64::from(f.index())));
    }
    out.ints(K_FACES_COMMON, std::mem::take(&mut o));

    o.push(opt(rs(p.first_poly_face())?, PolyFaceId::index));
    out.ints(K_FIRST_FACE, std::mem::take(&mut o));

    let mut g = Vec::new();
    for i in 0..b.n_faces {
        let f = fid(i);
        if !p.face_is_live(f) {
            continue;
        }
        let ls = rs(p.face_poly_loops(f))?;
        let es = rs(p.face_poly_edges(f))?;
        let cp = rs(p.face_cpoly_face(f))?;
        o.push(i as f64);
        o.push(opt(cp, PolyFaceId::index));
        o.push(if cp == Some(f) {
            -1.0
        } else {
            opt(rs(p.face_poly_shell(f))?, PolyShellId::index)
        });
        o.push(if rs(p.face_ok_back_ptrs(f))? {
            1.0
        } else {
            0.0
        });
        o.push(rs(p.face_original_face(f))?.map_or(0.0, |k| k.0 as f64));
        o.push(opt(rs(p.face_outer_poly_loop(f))?, PolyLoopId::index));
        o.push(ls.len() as f64);
        o.extend(ls.iter().map(|l| f64::from(l.index())));
        o.push(es.len() as f64);
        o.extend(es.iter().map(|e| f64::from(e.index())));
        if cp == Some(f) {
            let mem = rs(p.cpoly_faces_of_composite(f))?;
            o.push(mem.len() as f64);
            o.extend(mem.iter().map(|m| f64::from(m.index())));
        }
        g.push(rs(p.face_tolerance(f))?);
        put(&mut g, rs(p.face_stored_normal(f))?);
        g.push(rs(p.face_max_deviation(f))?);
        let (c, r) = rs(p.face_stored_sphere(f))?;
        put(&mut g, c);
        g.push(r);
    }
    out.ints(K_FACE_TOPO, std::mem::take(&mut o));
    out.floats(K_FACE_GEOM, std::mem::take(&mut g));

    for i in 0..b.n_loops {
        let l = lid(i);
        let es = rs(p.loop_poly_edges(l))?;
        o.push(i as f64);
        o.push(opt(rs(p.loop_poly_face(l))?, PolyFaceId::index));
        o.push(if rs(p.loop_is_closed(l))? { 1.0 } else { 0.0 });
        o.push(f64::from(rs(p.loop_orientation(l))? as i32));
        o.push(opt(rs(p.loop_first_poly_edge(l))?, PolyEdgeId::index));
        o.push(es.len() as f64);
        o.extend(es.iter().map(|e| f64::from(e.index())));
    }
    out.ints(K_LOOP_TOPO, std::mem::take(&mut o));

    for i in 0..b.n_edges {
        let e = eid(i);
        if !p.edge_is_live(e) {
            continue;
        }
        let rads = rs(p.edge_all_radials(e))?;
        let ev = rs(p.edge_end_poly_vertex(e))?;
        o.push(i as f64);
        o.push(opt(rs(p.edge_poly_loop(e))?, PolyLoopId::index));
        o.push(opt(rs(p.edge_poly_face(e))?, PolyFaceId::index));
        o.push(opt(rs(p.edge_start_poly_vertex(e))?, PolyVertexId::index));
        o.push(opt(ev, PolyVertexId::index));
        o.push(opt(rs(p.edge_cw_poly_edge(e))?, PolyEdgeId::index));
        o.push(opt(rs(p.edge_ccw_poly_edge(e))?, PolyEdgeId::index));
        o.push(opt(rs(p.edge_radial(e))?, PolyEdgeId::index));
        o.push(rads.len() as f64);
        o.extend(rads.iter().map(|r| f64::from(r.index())));
        o.push(opt(rs(p.edge_aux_data(e))?, PolyAuxId::index));
        o.push(if rs(p.edge_is_2d_or_zero(e))? {
            1.0
        } else {
            0.0
        });
        o.push(if rs(p.edge_ok_back_ptrs(e))? {
            1.0
        } else {
            0.0
        });
        o.push(rs(p.edge_original_edgeuse(e))?.map_or(0.0, |k| k.0 as f64));
        g.push(rs(p.edge_tolerance(e))?);
        put(&mut g, rs(p.edge_start_point(e))?);
        if ev.is_some() {
            put(&mut g, rs(p.edge_end_point(e))?);
        }
    }
    out.ints(K_EDGE_TOPO, std::mem::take(&mut o));
    out.floats(K_EDGE_GEOM, std::mem::take(&mut g));

    let verts = p.poly_vertices();
    for &v in &verts {
        let se = rs(p.vertex_starting_poly_edges(v))?;
        let pe = rs(p.vertex_poly_edges(v))?;
        let pf = rs(p.vertex_poly_faces(v))?;
        o.push(f64::from(v.index()));
        o.push(se.len() as f64);
        o.extend(se.iter().map(|e| f64::from(e.index())));
        o.push(pe.len() as f64);
        o.extend(pe.iter().map(|e| f64::from(e.index())));
        o.push(pf.len() as f64);
        o.extend(pf.iter().map(|f| opt(*f, PolyFaceId::index)));
        o.push(if rs(p.vertex_is_2d_or_zero(v))? {
            1.0
        } else {
            0.0
        });
        put(&mut g, rs(p.vertex_point(v))?);
        g.push(rs(p.vertex_tolerance(v))?);
        let ns = rs(p.vertex_normals(v))?;
        g.push(ns.len() as f64);
        for n in ns {
            put(&mut g, *n);
        }
    }
    out.ints(K_VERTEX_TOPO, std::mem::take(&mut o));
    out.floats(K_VERTEX_GEOM, std::mem::take(&mut g));

    let al = rs(p.aux_data_list())?;
    let mut seen: Vec<PolyAuxId> = Vec::new();
    for &a in &al {
        o.push(f64::from(a.index()));
        if !seen.contains(&a) {
            seen.push(a);
        }
    }
    o.push(-3.0);
    for &a in &seen {
        let d = rs(p.aux_data(a))?;
        o.push(f64::from(a.index()));
        o.push(d.num_users() as f64);
        put(&mut g, d.normal);
        g.push(d.uv.x);
        g.push(d.uv.y);
    }
    out.ints(K_AUX_TOPO, std::mem::take(&mut o));
    out.floats(K_AUX_GEOM, std::mem::take(&mut g));

    g.push(p.tolerance());
    for q in rs(p.poly_points())? {
        put(&mut g, q);
    }
    out.floats(K_BREP_GEOM, std::mem::take(&mut g));

    o.extend(p.cpoly_faces().iter().map(|f| f64::from(f.index())));
    out.ints(K_CPOLY, std::mem::take(&mut o));

    o.extend(rs(p.poly_edges(None))?.iter().map(|e| f64::from(e.index())));
    out.ints(K_BREP_EDGES, std::mem::take(&mut o));

    o.push(if p.ok_back_ptrs() { 1.0 } else { 0.0 });
    o.push(verts.len() as f64);
    out.ints(K_BREP_TOPO, std::mem::take(&mut o));
    Ok(())
}

fn run_op(k: &Kernel, b: &mut Built, op: &OpIn, out: &mut Secs) -> Result<(), Fail> {
    let p = &mut b.brep;
    match op {
        OpIn::Dump => {}
        OpIn::Bbox {
            pseudo,
            points,
            centroid,
            normal,
        } => {
            let mut pts: Option<Vec<Vec3>> =
                points.as_ref().map(|v| v.iter().map(|&q| v3(q)).collect());
            let mut nrm = normal.map(v3);
            let r = rs(p.calculate_bounding_box(*pseudo, pts.as_mut(), *centroid, nrm.as_mut()))?;
            out.ints(K_STATUS, vec![0.0]);
            let mut g = Vec::new();
            put_box(&mut g, &r.normal_box);
            if let Some(pb) = r.pseudo_box {
                for i in 0..3 {
                    put(&mut g, pb.basis_vector(i).unwrap_or(Vec3::UNDEF));
                }
                for i in 0..3 {
                    let ivl = pb.interval(i).unwrap_or_default();
                    g.push(ivl.min());
                    g.push(ivl.max());
                }
            }
            if let Some(c) = r.centroid {
                put(&mut g, c);
            }
            if let Some(n) = nrm {
                put(&mut g, n);
            }
            out.floats(K_BBOX, g);
            let mut g = Vec::new();
            for q in pts.unwrap_or_default() {
                put(&mut g, q);
            }
            out.floats(K_BBOX_POINTS, g);
            let mut g = Vec::new();
            for i in 0..b.n_faces {
                put_box(&mut g, &rs(p.face_bounding_box(fid(i)))?);
            }
            for i in 0..b.n_edges {
                put_box(&mut g, &rs(p.edge_bounding_box(eid(i)))?);
            }
            for i in 0..b.n_vertices {
                put_box(&mut g, &rs(p.vertex_bounding_box(vid(i)))?);
            }
            out.floats(K_OBJ_BOXES, g);
        }
        OpIn::Normal { calls } => {
            let (mut o, mut g) = (Vec::new(), Vec::new());
            for &(f, force, upd) in calls {
                let (n, deg) = rs(p.face_normal(k, fid(f), force, upd))?;
                o.push(if deg { 1.0 } else { 0.0 });
                put(&mut g, n);
            }
            out.ints(K_RESULTS, o);
            out.floats(K_NORMALS, g);
        }
        OpIn::Sphere { calls } => {
            let (mut o, mut g) = (Vec::new(), Vec::new());
            for &(f, force) in calls {
                let (c, r) = rs(p.face_compute_sphere_bound(fid(f), force))?;
                o.push(0.0);
                put(&mut g, c);
                g.push(r);
            }
            out.ints(K_STATUS, o);
            out.floats(K_SPHERES, g);
        }
        OpIn::Contains { queries } => {
            let mut o = Vec::new();
            for q in queries {
                let r = if q.face {
                    p.face_point_in_polygon(k, fid(q.id), v3(q.point), q.work_in_3d)
                        .map(|b| if b { 1.0 } else { 0.0 })
                } else {
                    p.loop_contains_point(k, lid(q.id), v3(q.point), q.tol, q.work_in_3d)
                        .map(|c| f64::from(c as i32))
                };
                match r {
                    Ok(x) => {
                        o.push(0.0);
                        o.push(x);
                    }
                    Err(e) => {
                        if let Some(d) = Output::unported(&e) {
                            return Err(Fail::Deferred(Box::new(d)));
                        }
                        o.push(code_of(&e));
                        o.push(-1.0);
                    }
                }
            }
            out.ints(K_RESULTS, o);
        }
        OpIn::Degenerate { calls } => {
            let mut o = Vec::new();
            for &(e, t) in calls {
                let xs = t.map(|t| tol::xsect_tol3d(k, t));
                let d = rs(p.edge_is_degenerate(k, eid(e), xs))?;
                o.push(if d { 1.0 } else { 0.0 });
            }
            out.ints(K_RESULTS, o);
        }
        OpIn::OwnerDegenerate { owners, calls } => {
            let world = build_owners(owners)?;
            let (mut results, mut topo, mut points) = (Vec::new(), Vec::new(), Vec::new());
            for &(e, t) in calls {
                let e = eid(e);
                let d = rs(p.edge_is_degenerate_with_owner(
                    k,
                    e,
                    t.map(|t| tol::xsect_tol3d(k, t)),
                    |key| world.faces.get(&key).map(|&f| (&world.brep, f)),
                ))?;
                results.push(f64::from(u8::from(d)));
                owner_samples(k, p, e, &world, &mut topo, &mut points)?;
            }
            out.ints(K_RESULTS, results);
            out.ints(K_OWNER_TOPO, topo);
            out.floats(K_OWNER_POINTS, points);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let placement = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            let mut records: std::collections::BTreeMap<ObjectKey, Topology> =
                std::collections::BTreeMap::new();
            let mut attributes = AttributeStore::new();
            let mut caches: CacheStore<()> = CacheStore::new();
            let mut env = PolyNotifyEnv {
                records: &mut records,
                attributes: &mut attributes,
                caches: &mut caches,
                context: None,
                this: ObjectRef {
                    type_code: PolyObjectKind::Brep.type_code(),
                    key: ObjectKey(0),
                },
            };
            let st = match p.transform(k, &mut env, &placement, scale.map(v3)) {
                Ok(()) => 0.0,
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Err(Fail::Deferred(Box::new(d)));
                    }
                    code_of(&e)
                }
            };
            out.ints(K_STATUS, vec![st]);
        }
        OpIn::Mutate { cmds } => {
            let mut o = Vec::new();
            for c in cmds {
                match *c {
                    MutIn::SetPoint { v, p: q } => rs(p.vertex_set_point(vid(v), v3(q)))?,
                    MutIn::SetVertexTolerance { v, t } => {
                        rs(p.vertex_set_tolerance(vid(v), tol::zone_tol3d(k, t), true))?;
                    }
                    MutIn::DeleteEdge { e } => rs(p.delete_edge(eid(e)))?,
                    MutIn::DeleteFace { f } => rs(p.delete_face(fid(f)))?,
                    MutIn::CpolyRemove { c, f } => {
                        rs(p.cpoly_remove_poly_face(fid(c), fid(f)))?;
                        o.push(0.0);
                    }
                    MutIn::SetNormalUninit { f } => rs(p.face_set_normal_uninit(fid(f)))?,
                    MutIn::SetEdgeTolerance { e, t } => {
                        rs(p.edge_set_tolerance(eid(e), tol::zone_tol3d(k, t)))?;
                    }
                    MutIn::SetFaceTolerance { f, t } => {
                        rs(p.face_set_tolerance(fid(f), tol::zone_tol3d(k, t)))?;
                    }
                    MutIn::SetOriginalFace { f, label } => {
                        rs(p.face_set_original_face(fid(f), Some(ObjectKey(label as u64)), false))?;
                    }
                    MutIn::ClearAux { e } => rs(p.edge_set_aux_data(eid(e), None))?,
                }
            }
            out.ints(K_STATUS, o);
        }
        OpIn::Kinds { codes } => {
            let mut kinds = vec![PolyObjectKind::Brep];
            if !p.poly_regions().is_empty() {
                kinds.push(PolyObjectKind::Region);
            }
            if b.n_shells > 0 {
                kinds.push(PolyObjectKind::Shell);
            }
            for i in 0..b.n_faces {
                kinds.push(if i < b.n_plain_faces {
                    PolyObjectKind::Face
                } else {
                    PolyObjectKind::CPolyFace
                });
            }
            if b.n_loops > 0 {
                kinds.push(PolyObjectKind::Loop);
            }
            if b.n_edges > 0 {
                kinds.push(PolyObjectKind::Edge);
            }
            if b.n_vertices > 0 {
                kinds.push(PolyObjectKind::Vertex);
            }
            let mut o = Vec::new();
            for kd in kinds {
                o.push(kd.type_code() as f64);
                for &c in codes {
                    o.push(if kd.is_kind_of(c) { 1.0 } else { 0.0 });
                }
            }
            out.ints(K_RESULTS, o);
        }
    }
    Ok(())
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.poly"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        match ffi::run(&to_ffi(&i.brep), &op_to_ffi(&i.op)) {
            Ok(secs) => to_output(&secs),
            // The shim declined the input before calling anything (decided from the built
            // model, not from a result): upstream would call `SmFace::GetSurface` on a
            // label that is not an `SmFace`.
            Err(solidcore_ffi::FfiError::Status(ffi::STATUS_UNDEFINED_INPUT)) => {
                Ok(Output::rejected(
                    "SmPolyEdge::IsDegenerate would call GetSurface on a labelled fake SmFace \
                     (undefined in C++; real original faces: owner_degenerate)",
                ))
            }
            Err(solidcore_ffi::FfiError::Status(s)) => Ok(Output::err(format!("status_{s}"))),
            Err(e) => Err(harness(e.to_string())),
        }
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let k = Kernel::default();
        let res = (|| -> Result<Secs, Fail> {
            let mut b = build(&k, &i.brep)?;
            let mut out = Secs::default();
            run_op(&k, &mut b, &i.op, &mut out)?;
            dump(&mut b, &mut out)?;
            Ok(out)
        })();
        match res {
            Ok(s) => to_output(&s.0),
            Err(Fail::Deferred(o)) => Ok(*o),
            Err(Fail::Harness(h)) => Err(h),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen::case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut c = ctx.clone();
        c.seeds = c.seeds.max(MIN_GENERATED);
        default_cases(self, &c)
    }
}

#[cfg(test)]
mod owner_tests {
    use super::*;
    use crate::case::{repo_root, CaseSource};

    fn owner_cases() -> Vec<Case> {
        let file: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/query.poly/real_owners.json"
        ))
        .unwrap();
        file["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| Case {
                unit: "query.poly".into(),
                name: c["name"].as_str().unwrap().into(),
                source: CaseSource::Inline,
                inputs: c["inputs"].clone(),
            })
            .collect()
    }

    #[test]
    fn original_corpus_is_retained_and_opaque_owner_cases_are_rejected() {
        let cases = Runner
            .cases(&CaseContext {
                root: repo_root(),
                seeds: 16,
            })
            .unwrap();
        assert_eq!(cases.len(), 939 + 16);
        assert_eq!(
            cases
                .iter()
                .filter(|c| !c.name.starts_with("owner_"))
                .count(),
            939
        );
        assert_eq!(
            cases
                .iter()
                .filter(|c| matches!(c.source, CaseSource::Generated { .. }))
                .count(),
            600
        );
        for (name, seed) in [
            ("gen0378", 13_165_552_566_343_800_279),
            ("gen0496", 13_165_552_566_343_800_397),
        ] {
            let case = cases.iter().find(|c| c.name == name).unwrap();
            assert_eq!(case.source, CaseSource::Generated { seed });
            assert_eq!(case.inputs, Runner.generate(&mut Rng::new(seed)).unwrap());
            let rust = Runner.run_rust(case).unwrap();
            let deferred = rust.deferred.unwrap();
            assert_eq!(deferred.unit, "topology.face_p2");
            assert_eq!(deferred.symbol, "SmFace::GetSurface");
            // The labelled original face is not an `SmFace`: upstream would dereference
            // it, so the C++ side rejects the case and nothing is compared.
            let cpp = Runner.run_cpp(case).unwrap();
            assert!(cpp.error.is_none());
            assert!(cpp.rejected.as_deref().unwrap().contains("fake SmFace"));
            assert!(cpp.ints.is_empty() && cpp.values.is_empty());
        }
    }

    #[test]
    fn real_owner_native_mapping_poles_seams_and_controls() {
        for case in owner_cases() {
            let cpp = Runner.run_cpp(&case).unwrap();
            let rust = Runner.run_rust(&case).unwrap();
            assert!(cpp.error.is_none() && cpp.rejected.is_none() && cpp.deferred.is_none());
            assert!(rust.error.is_none() && rust.rejected.is_none() && rust.deferred.is_none());
            let diff = crate::compare::compare(&cpp, &rust, &[]);
            assert!(diff.pass(), "{}: {diff:?}", case.name);
            assert!(diff.values_compared > 0);
            assert!(cpp.values["owner_points"]
                .iter()
                .any(|v| v.is_finite() && *v != 0.0));
            let expected: &[i64] = match case.name.as_str() {
                "owner_plane_stretch" => &[0, 0],
                "owner_plane_compress" | "owner_face_without_surface" => &[0, 1, 0],
                "owner_u_pole" | "owner_shared_face_identity" => &[1, 1],
                "owner_u_seam_midpoint" => &[0, 0, 1],
                "owner_partial_pole" | "owner_v_pole" => &[1],
                "owner_v_seam_midpoint" => &[0, 1],
                "owner_distinct_face_identity" => &[1, 0],
                _ => &[0],
            };
            assert_eq!(cpp.ints["results"], expected, "{}", case.name);
            if case.name == "owner_u_seam_midpoint" {
                assert_eq!(
                    &cpp.values["owner_points"][10..19],
                    &[1.0, 0.0, 3.0, 1.0, 0.0, 3.0, 0.0, 0.0, 3.0]
                );
            }
            if case.name == "owner_plane_compress" {
                assert_eq!(
                    &cpp.values["owner_points"][10..19],
                    &[8.0625, -1.0, 1.0, 8.125, -1.0, 1.0, 8.09375, -1.0, 1.0]
                );
            }
            if case.name == "owner_face_without_surface" {
                assert_eq!(&cpp.ints["owner_topo"][1..6], &[3, 1, 0, 1, 0]);
                // The same valid original face label, without its explicit record,
                // must defer instead of becoming another surface-less face.
                let mut unresolved = case.clone();
                unresolved.inputs["op"]["owners"] = serde_json::json!([]);
                assert!(Runner.run_rust(&unresolved).unwrap().deferred.is_some());
                assert!(Runner
                    .run_cpp(&unresolved)
                    .unwrap()
                    .rejected
                    .as_deref()
                    .unwrap()
                    .contains("fake SmFace"));
            }
        }
    }
}

// ---- generator ----------------------------------------------------------------------

mod gen {
    use super::{BrepIn, ContainsIn, FaceIn, Inputs, LoopIn, MutIn, OpIn};
    use crate::Rng;

    /// `SM_USE_DEFAULT`.
    const USE_DEFAULT: f64 = 9876543.21;

    fn tol(rng: &mut Rng) -> f64 {
        match rng.below(8) {
            0 => USE_DEFAULT,
            1 => 0.0,
            2 => 1.0e-9,
            3 => 1.0e-3,
            4 => -1.0e-5,
            _ => 1.0e-5,
        }
    }

    struct Frame {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        s: f64,
    }

    fn frame(rng: &mut Rng) -> Frame {
        let s = match rng.below(6) {
            0 => 1.0e-6,
            1 => 1.0e6,
            2 => 1.0e-3,
            _ => 1.0,
        };
        if rng.chance(0.3) {
            return Frame {
                o: [0.0; 3],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                s,
            };
        }
        let a = rng.uniform(0.0, 6.0);
        let b = rng.uniform(-1.4, 1.4);
        let x = [a.cos() * b.cos(), a.sin() * b.cos(), b.sin()];
        let t = rng.point3(-1.0, 1.0);
        let d = t[0] * x[0] + t[1] * x[1] + t[2] * x[2];
        let mut y = [t[0] - d * x[0], t[1] - d * x[1], t[2] - d * x[2]];
        let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
        if l < 1e-3 {
            y = [-x[1], x[0], 0.0];
        }
        let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt().max(1e-12);
        y = [y[0] / l, y[1] / l, y[2] / l];
        Frame {
            o: rng.point3(-5.0, 5.0),
            x,
            y,
            s,
        }
    }

    fn at(f: &Frame, u: f64, v: f64, w: f64) -> [f64; 3] {
        let z = [
            f.x[1] * f.y[2] - f.x[2] * f.y[1],
            f.x[2] * f.y[0] - f.x[0] * f.y[2],
            f.x[0] * f.y[1] - f.x[1] * f.y[0],
        ];
        [
            f.o[0] * f.s + f.s * (u * f.x[0] + v * f.y[0] + w * z[0]),
            f.o[1] * f.s + f.s * (u * f.x[1] + v * f.y[1] + w * z[1]),
            f.o[2] * f.s + f.s * (u * f.x[2] + v * f.y[2] + w * z[2]),
        ]
    }

    struct Gen {
        b: BrepIn,
        /// `(start, end)` vertex of every edge, creation order.
        edges: Vec<(usize, usize)>,
        /// `(face, loop index)` of every loop.
        loops: usize,
        frame: Frame,
    }

    impl Gen {
        fn vert(&mut self, rng: &mut Rng, p: [f64; 3]) -> usize {
            let t = if rng.chance(0.8) { 1.0e-5 } else { tol(rng) };
            self.b.vertices.push([p[0], p[1], p[2], t]);
            self.b.vertices.len() - 1
        }
        fn face(&mut self, rng: &mut Rng, shell: usize, loops: Vec<(u8, Vec<usize>)>) {
            let normal = if rng.chance(0.25) {
                Some(if rng.chance(0.2) {
                    [0.0, 0.0, 0.0]
                } else {
                    rng.point3(-1.0, 1.0)
                })
            } else {
                None
            };
            let mut ls = Vec::new();
            for (mode, verts) in loops {
                let k = verts.len();
                let ne = if mode == 2 { k - 1 } else { k };
                for j in 0..ne {
                    if let (Some(&a), Some(&b)) = (verts.get(j), verts.get((j + 1) % k)) {
                        self.edges.push((a, b));
                    }
                }
                self.loops += 1;
                let edge_tol = if rng.chance(0.85) { 1.0e-5 } else { tol(rng) };
                ls.push(LoopIn {
                    mode,
                    edge_tol,
                    verts,
                });
            }
            let t = if rng.chance(0.85) { 1.0e-5 } else { tol(rng) };
            self.b.faces.push(FaceIn {
                shell,
                tol: t,
                normal,
                loops: ls,
            });
        }
        /// Radial rings: edges joining the same two vertices.
        fn stitch(&mut self, rng: &mut Rng) {
            let mut used = vec![false; self.edges.len()];
            for (i, &(a, b)) in self.edges.iter().enumerate() {
                if used.get(i).copied().unwrap_or(true) {
                    continue;
                }
                let mut ring = vec![i];
                for (j, &(c, d)) in self.edges.iter().enumerate().skip(i + 1) {
                    if !used.get(j).copied().unwrap_or(true)
                        && ((c == a && d == b) || (c == b && d == a))
                    {
                        ring.push(j);
                    }
                }
                if ring.len() >= 2 && rng.chance(0.9) {
                    for &r in &ring {
                        if let Some(u) = used.get_mut(r) {
                            *u = true;
                        }
                    }
                    if rng.chance(0.2) {
                        ring.reverse();
                    }
                    self.b.radials.push(ring);
                }
            }
        }
    }

    fn polygon(rng: &mut Rng, n: usize, kind: u64) -> Vec<(f64, f64, f64)> {
        let mut pts = Vec::new();
        for i in 0..n {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            let r = match kind {
                1 => {
                    if i % 2 == 0 {
                        1.0
                    } else {
                        0.4
                    }
                }
                2 => rng.uniform(0.3, 1.2),
                _ => 1.0,
            };
            let w = match kind {
                3 => rng.uniform(-0.2, 0.2),
                4 => rng.uniform(-1.0e-7, 1.0e-7),
                _ => 0.0,
            };
            pts.push((r * a.cos(), r * a.sin(), w));
        }
        pts
    }

    fn build(rng: &mut Rng) -> Gen {
        let mut s = Gen {
            b: BrepIn {
                tol: if rng.chance(0.8) { 1.0e-5 } else { tol(rng) },
                ..BrepIn::default()
            },
            edges: Vec::new(),
            loops: 0,
            frame: frame(rng),
        };
        let shape = rng.below(9);
        match shape {
            // closed mesh: tetrahedron / cube / octahedron-like
            0 | 1 => {
                let cube = shape == 1;
                s.b.regions = vec![1];
                let pts: Vec<[f64; 3]> = if cube {
                    vec![
                        [0.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [1.0, 1.0, 0.0],
                        [0.0, 1.0, 0.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 1.0],
                        [1.0, 1.0, 1.0],
                        [0.0, 1.0, 1.0],
                    ]
                } else {
                    vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                };
                let vs: Vec<usize> = pts
                    .iter()
                    .map(|p| {
                        let q = at(&s.frame, p[0], p[1], p[2]);
                        s.vert(rng, q)
                    })
                    .collect();
                let faces: Vec<Vec<usize>> = if cube {
                    vec![
                        vec![0, 3, 2, 1],
                        vec![4, 5, 6, 7],
                        vec![0, 1, 5, 4],
                        vec![1, 2, 6, 5],
                        vec![2, 3, 7, 6],
                        vec![3, 0, 4, 7],
                    ]
                } else {
                    vec![vec![0, 2, 1], vec![0, 1, 3], vec![1, 2, 3], vec![2, 0, 3]]
                };
                for f in faces {
                    let loop_v: Vec<usize> = f.iter().filter_map(|&i| vs.get(i).copied()).collect();
                    s.face(rng, 0, vec![(1, loop_v)]);
                }
            }
            // single polygon, maybe with holes, in random plane
            2..=4 => {
                s.b.regions = vec![1];
                let n = 3 + rng.below(8) as usize;
                let kind = rng.below(5);
                let outer: Vec<usize> = polygon(rng, n, kind)
                    .into_iter()
                    .map(|(u, v, w)| {
                        let q = at(&s.frame, u, v, w);
                        s.vert(rng, q)
                    })
                    .collect();
                let mut loops = vec![(1u8, outer)];
                if rng.chance(0.4) {
                    let m = 3 + rng.below(3) as usize;
                    let hole: Vec<usize> = polygon(rng, m, 0)
                        .into_iter()
                        .rev()
                        .map(|(u, v, _)| {
                            let q = at(&s.frame, 0.2 * u, 0.2 * v, 0.0);
                            s.vert(rng, q)
                        })
                        .collect();
                    loops.push((1u8, hole));
                }
                s.face(rng, 0, loops);
            }
            // degenerate polygons
            5 => {
                s.b.regions = vec![1];
                let which = rng.below(8);
                let pts: Vec<(f64, f64, f64)> = match which {
                    0 => vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (2.0, 0.0, 0.0), (3.0, 0.0, 0.0)],
                    1 => vec![(0.0, 0.0, 0.0), (1.0, 1.0, 0.0), (1.0, 0.0, 0.0), (0.0, 1.0, 0.0)],
                    2 => vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)],
                    3 => vec![(0.0, 0.0, 0.0)],
                    4 => vec![(0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (0.0, 0.0, 0.0)],
                    5 => vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1e-13, 0.0)],
                    6 => vec![
                        (0.0, 0.0, 0.0),
                        (1.0, 0.0, 0.0),
                        (1.0, 1.0, 0.0),
                        (1.0, 1.0, 0.0),
                        (0.0, 1.0, 0.0),
                    ],
                    _ => vec![(0.0, 0.0, 0.0), (1.0, 0.0, 0.0), (1.0, 1.0, 0.0), (0.0, 1.0, 0.0), (0.5, 0.5, 0.0)],
                };
                let raw = rng.chance(0.5);
                let vs: Vec<usize> = pts
                    .into_iter()
                    .map(|(u, v, w)| {
                        let q = if raw { [u, v, w] } else { at(&s.frame, u, v, w) };
                        s.vert(rng, q)
                    })
                    .collect();
                let mode = match rng.below(4) {
                    0 => 0,
                    1 if vs.len() >= 2 => 2,
                    _ => 1,
                };
                s.face(rng, 0, vec![(mode, vs)]);
            }
            // isthmus / strut: loop going there and back, and a spine (3 faces sharing an edge)
            6 => {
                s.b.regions = vec![1];
                let raw = rng.chance(0.5);
                let v = |rng: &mut Rng, s: &mut Gen, u: f64, w: f64| {
                    let q = if raw { [u, w, 0.0] } else { at(&s.frame, u, w, 0.0) };
                    s.vert(rng, q)
                };
                let a = v(rng, &mut s, 0.0, 0.0);
                let b = v(rng, &mut s, 2.0, 0.0);
                let c = v(rng, &mut s, 2.0, 2.0);
                let d = v(rng, &mut s, 0.0, 2.0);
                let e = v(rng, &mut s, 1.0, 1.0);
                let f = v(rng, &mut s, 3.0, 1.0);
                // square with a strut a -> e -> a
                s.face(rng, 0, vec![(1, vec![a, e, a, b, c, d])]);
                // spine: three triangles on edge a-b
                s.face(rng, 0, vec![(1, vec![a, b, f])]);
                s.face(rng, 0, vec![(1, vec![b, a, f])]);
            }
            // several regions / shells / open loops
            _ => {
                let nr = 1 + rng.below(3) as usize;
                s.b.regions = (0..nr).map(|_| rng.below(3) as usize).collect();
                let n_shells: usize = s.b.regions.iter().sum();
                if n_shells == 0 {
                    if let Some(r) = s.b.regions.first_mut() {
                        *r = 1;
                    }
                }
                let n_shells: usize = s.b.regions.iter().sum();
                let nf = 1 + rng.below(4) as usize;
                for _ in 0..nf {
                    let n = 3 + rng.below(4) as usize;
                    let kind = rng.below(5);
                    let off = rng.uniform(-3.0, 3.0);
                    let vs: Vec<usize> = polygon(rng, n, kind)
                        .into_iter()
                        .map(|(u, v, w)| {
                            let q = at(&s.frame, u + off, v, w);
                            s.vert(rng, q)
                        })
                        .collect();
                    let mode = rng.below(3) as u8;
                    let shell = rng.below(n_shells as u64) as usize;
                    s.face(rng, shell, vec![(mode, vs)]);
                }
            }
        }
        // z = 0 points (2D) sometimes
        if rng.chance(0.15) {
            for v in &mut s.b.vertices {
                v[2] = 0.0;
            }
        }
        s.stitch(rng);
        let ne = s.edges.len();
        let nf = s.b.faces.len();
        // aux data
        if rng.chance(0.4) && ne > 0 {
            let na = 1 + rng.below(3) as usize;
            for _ in 0..na {
                let n = rng.point3(-1.0, 1.0);
                s.b.aux.push([n[0], n[1], n[2], rng.uniform(0.0, 1.0), rng.uniform(0.0, 1.0)]);
            }
            for e in 0..ne {
                if rng.chance(0.5) {
                    let a = rng.below(na as u64) as usize;
                    s.b.edge_aux.push((e, a));
                }
            }
        }
        // vertex normals
        if rng.chance(0.4) {
            for v in 0..s.b.vertices.len() {
                for _ in 0..rng.below(3) {
                    let n = if rng.chance(0.1) {
                        [0.0, 0.0, 0.0]
                    } else {
                        rng.point3(-1.0, 1.0)
                    };
                    s.b.vertex_normals.push((v, n));
                }
            }
        }
        // composite faces
        if rng.chance(0.3) && nf > 0 {
            let mut members: Vec<usize> = (0..nf).filter(|_| rng.chance(0.6)).collect();
            if members.is_empty() {
                members.push(0);
            }
            s.b.cpolys.push(members);
        }
        // labels
        if rng.chance(0.3) {
            for f in 0..nf {
                if rng.chance(0.5) {
                    s.b.orig_faces.push((f, 1 + rng.below(3) as i64));
                }
            }
            for e in 0..ne {
                if rng.chance(0.3) {
                    s.b.orig_edgeuses.push((e, 1 + rng.below(3) as i64));
                }
            }
        }
        s.b.ok_back_ptrs = rng.chance(0.2);
        s
    }

    pub(super) fn case(rng: &mut Rng) -> Inputs {
        let s = build(rng);
        let nv = s.b.vertices.len();
        let nf = s.b.faces.len() + s.b.cpolys.len();
        let n_plain = s.b.faces.len();
        let ne = s.edges.len();
        let nl = s.loops;
        let pick = |rng: &mut Rng, n: usize| rng.below(n.max(1) as u64) as usize;
        let op = match rng.below(9) {
            0 => OpIn::Dump,
            1 => {
                let pts = match rng.below(3) {
                    0 => None,
                    1 => Some(Vec::new()),
                    _ => Some((0..=rng.below(6)).map(|_| rng.point3(-10.0, 10.0)).collect()),
                };
                OpIn::Bbox {
                    pseudo: pts.is_some() && rng.chance(0.5),
                    points: pts,
                    centroid: rng.chance(0.5),
                    normal: if rng.chance(0.4) {
                        Some(rng.point3(-2.0, 2.0))
                    } else {
                        None
                    },
                }
            }
            2 => OpIn::Normal {
                calls: (0..=rng.below(4))
                    .map(|_| (pick(rng, nf), rng.chance(0.5), rng.chance(0.7)))
                    .collect(),
            },
            3 => OpIn::Sphere {
                calls: (0..=rng.below(3))
                    .map(|_| (pick(rng, nf), rng.chance(0.5)))
                    .collect(),
            },
            4 => {
                let mut queries = Vec::new();
                for _ in 0..=rng.below(6) {
                    let face = nl == 0 || rng.chance(0.4);
                    let id = if face { pick(rng, nf) } else { pick(rng, nl) };
                    // near a vertex, an edge midpoint, inside, or far
                    let point = match rng.below(4) {
                        0 if nv > 0 => {
                            let v = s.b.vertices.get(pick(rng, nv)).copied().unwrap_or_default();
                            [v[0], v[1], v[2]]
                        }
                        1 if ne > 0 => {
                            let (a, b) = s.edges.get(pick(rng, ne)).copied().unwrap_or_default();
                            let pa = s.b.vertices.get(a).copied().unwrap_or_default();
                            let pb = s.b.vertices.get(b).copied().unwrap_or_default();
                            [(pa[0] + pb[0]) / 2.0, (pa[1] + pb[1]) / 2.0, (pa[2] + pb[2]) / 2.0]
                        }
                        2 => {
                            let u = rng.uniform(-1.2, 1.2);
                            let v = rng.uniform(-1.2, 1.2);
                            at(&s.frame, u, v, rng.uniform(-0.01, 0.01))
                        }
                        _ => rng.point3(-20.0, 20.0),
                    };
                    let tol = match rng.below(4) {
                        0 => 1.0e-12,
                        1 => 1.0e-3,
                        2 => 0.0,
                        _ => 1.0e-5,
                    };
                    queries.push(ContainsIn {
                        face,
                        id,
                        point,
                        tol,
                        work_in_3d: rng.chance(0.6),
                    });
                }
                OpIn::Contains { queries }
            }
            5 => OpIn::Degenerate {
                calls: (0..=rng.below(6))
                    .map(|_| {
                        let t = match rng.below(4) {
                            0 => None,
                            1 => Some(1.0e-3),
                            2 => Some(10.0),
                            _ => Some(9876543.21),
                        };
                        (pick(rng, ne), t)
                    })
                    .collect(),
            },
            6 => {
                let f = frame(rng);
                let scale = match rng.below(9) {
                    0 => None,
                    1 => Some([1.0, 1.0, 1.0]),
                    2 => Some([2.0, 2.0, 2.0]),
                    3 => Some([-1.0, 2.0, 0.5]),
                    4 => Some([0.0, 2.0, 3.0]),
                    5 => Some([0.0, 0.0, 3.0]),
                    6 => Some([1.0e-200, 1.0e200, 1.0]),
                    7 => Some([2000.0, 1.0, 1.0]),
                    _ => Some([1.0e19, 1.0, 1.0]),
                };
                let ident = rng.chance(0.15);
                OpIn::Transform {
                    origin: if ident { [0.0; 3] } else { f.o },
                    x: if ident { [1.0, 0.0, 0.0] } else { f.x },
                    y: if ident { [0.0, 1.0, 0.0] } else { f.y },
                    scale,
                }
            }
            7 => {
                let mut cmds = Vec::new();
                for _ in 0..rng.below(4) {
                    let c = match rng.below(6) {
                        0 if nv > 0 => MutIn::SetPoint {
                            v: pick(rng, nv),
                            p: rng.point3(-3.0, 3.0),
                        },
                        1 if nv > 0 => MutIn::SetVertexTolerance {
                            v: pick(rng, nv),
                            t: if rng.chance(0.5) { 1.0e-4 } else { 1.0 },
                        },
                        2 if ne > 0 => MutIn::SetEdgeTolerance {
                            e: pick(rng, ne),
                            t: 1.0e-3,
                        },
                        3 => MutIn::SetFaceTolerance {
                            f: pick(rng, nf),
                            t: 2.0e-3,
                        },
                        4 => MutIn::SetOriginalFace {
                            f: pick(rng, nf),
                            label: 1 + rng.below(3) as i64,
                        },
                        _ => MutIn::SetNormalUninit { f: pick(rng, nf) },
                    };
                    cmds.push(c);
                }
                // removals last
                if nf > n_plain && rng.chance(0.5) {
                    cmds.push(MutIn::CpolyRemove {
                        c: n_plain + pick(rng, nf - n_plain),
                        f: pick(rng, nf),
                    });
                }
                if ne > 0 && rng.chance(0.4) {
                    cmds.push(MutIn::ClearAux { e: pick(rng, ne) });
                }
                if ne > 0 && rng.chance(0.6) {
                    let e = pick(rng, ne);
                    cmds.push(MutIn::DeleteEdge { e });
                    if rng.chance(0.3) {
                        let e2 = pick(rng, ne);
                        if e2 != e {
                            cmds.push(MutIn::DeleteEdge { e: e2 });
                        }
                    }
                }
                if nf > 0 && rng.chance(0.5) {
                    cmds.push(MutIn::DeleteFace { f: pick(rng, nf) });
                }
                OpIn::Mutate { cmds }
            }
            _ => OpIn::Kinds {
                codes: vec![
                    0, 1, 1001, 12001, 16000, 16001, 16010, 16050, 18000, 18001, 18002, 18003,
                    18004, 18005, 18006, 18007, 18008, 18009,
                ],
            },
        };
        // operations on faces need faces
        let op = match op {
            _ if nf == 0 => OpIn::Dump,
            OpIn::Degenerate { .. } if ne == 0 => OpIn::Dump,
            o => o,
        };
        Inputs { brep: s.b, op }
    }
}
